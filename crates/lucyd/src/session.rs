//! The Session state machine: pure and I/O-free. The runner feeds it inputs
//! and carries out the outputs it returns.

use std::collections::VecDeque;

use lucy_ipc::State;

use crate::adapters::ThinkingEvent;
use crate::authoring::Building;

pub enum Input {
    /// Lucy speaks first: start a Turn with these instructions.
    Opening(String),
    /// The user started an utterance (true), or it was cut or dropped
    /// (false).
    Speaking(bool),
    /// The VAD cut an utterance.
    Speech,
    /// STT finished the utterance it was given.
    Transcript(String),
    /// STT failed on the utterance it was given.
    TranscribeFailed,
    Thinking(ThinkingEvent),
    /// TTS finished playing one sentence.
    Played,
    /// The model called `end_session`.
    EndRequested,
    /// The Wake word was heard during the Session.
    WakeWord,
    /// The user has kept talking over Lucy long enough to cut her off.
    BargeIn,
    /// A tool call waits on the user: say the question, then hear one
    /// utterance (Confirming).
    Ask(Question),
    /// The question went unanswered for too long.
    AnswerTimedOut,
    /// The question is no longer asked; nothing is said.
    Withdrawn,
    /// Lucy says this herself, after any reply text so far.
    Say(String),
    /// What Authoring is doing now. While a build works, the Session stays
    /// open in Listening.
    Building(Building),
}

/// Something Lucy asks the user in the middle of a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// What she says.
    pub prompt: String,
    pub expects: Expects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expects {
    /// A yes: one of the yes-phrases. Anything else is a no.
    Yes,
    /// Any words: a mid-call question from a Plugin.
    Words,
    /// A yes, as one of the yes-phrases, or else any words.
    YesOrWords,
}

/// How the user answered a `Question`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    /// What they said, to a question that expects words.
    Said(String),
    /// Nothing, to a question that expects words.
    Silence,
}

/// Said after any answer to a Confirmation but a yes.
pub const DECLINED: &str = "Okay, I won't.";

#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// Transcribe the utterance that was just cut.
    Transcribe,
    SendTurn(String),
    Speak(String),
    /// Start the silence timer over.
    ResetSilenceTimer,
    /// Something went wrong: flash the transient error state.
    Error,
    /// End the Session.
    End,
    /// The question asked is answered.
    Answered(Answer),
    /// Stop playback and drop everything queued.
    StopSpeaking,
    /// Stop the model's Turn, and the tool calls it made that the user
    /// hasn't confirmed.
    Interrupt,
    /// The user said "cancel it": stop the Authoring build.
    CancelBuild,
}

const DIDNT_CATCH_THAT: &str = "Sorry, I didn't catch that.";

/// What stops an Authoring build, as the whole utterance.
const CANCEL_IT: &str = "cancel it";

/// Said when the user stops a build.
pub const BUILD_STOPPED: &str = "Okay, I've stopped the build.";

/// Said when the Session ends by `end_session` while a build runs.
pub const KEEP_WORKING: &str = "I'll keep working on the Plugin, and let you know when it's ready.";

/// An open Session.
#[derive(Default)]
pub struct Session {
    transcribing: bool,
    /// The user is in the middle of an utterance.
    speaking: bool,
    turn_in_progress: bool,
    /// Sentences handed to TTS that haven't finished playing, oldest first.
    queued: VecDeque<String>,
    /// The sentences of the current reply that have finished playing.
    heard: String,
    /// What the user heard of the reply they cut off, until the next Turn
    /// tells the model.
    interrupted: Option<String>,
    /// Speech, not only the Wake word, can barge in.
    speech_barges_in: bool,
    /// Reply text not yet handed to TTS.
    sentence: String,
    /// `end_session` was called; end once back in Listening.
    closing: bool,
    /// STT failed on the last utterance.
    stt_failed: bool,
    /// What started the current or last Turn.
    turn: String,
    /// What the question being asked expects, while Confirming.
    asking: Option<Expects>,
    yes_phrases: Vec<String>,
    /// What Authoring is doing.
    building: Building,
}

impl Session {
    /// A Session that hears `yes_phrases` as a yes.
    pub fn new(yes_phrases: Vec<String>) -> Self {
        Session {
            yes_phrases,
            ..Session::default()
        }
    }

    /// Lets speech, not only the Wake word, barge in.
    pub fn barge_in_by_speech(&mut self, on: bool) {
        self.speech_barges_in = on;
    }

    pub fn state(&self) -> State {
        if self.asking.is_some() {
            State::Confirming
        } else if !self.queued.is_empty() {
            State::Speaking
        } else if self.turn_in_progress {
            State::Thinking
        } else {
            State::Listening
        }
    }

    /// What started the Turn in progress, if there is one.
    pub fn pending_turn(&self) -> Option<&str> {
        self.turn_in_progress.then_some(self.turn.as_str())
    }

    /// The silence timer counts only while Lucy is listening and the user
    /// isn't speaking, including while an utterance is being transcribed,
    /// and no Authoring build is running.
    pub fn silence_timer_runs(&self) -> bool {
        self.state() == State::Listening && !self.speaking && self.building != Building::Working
    }

    /// Neither Lucy nor the user is speaking, and nothing they said is
    /// still being heard or answered.
    pub fn quiet(&self) -> bool {
        self.state() == State::Listening && !self.speaking && !self.transcribing
    }

    /// The wait for an answer counts once the question has been said, and
    /// not while the user is speaking or their reply is being transcribed.
    pub fn answer_timer_runs(&self) -> bool {
        self.asking.is_some() && self.queued.is_empty() && !self.transcribing && !self.speaking
    }

    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Opening(text) => {
                self.turn_in_progress = true;
                self.heard.clear();
                self.turn.clone_from(&text);
                vec![Output::SendTurn(text)]
            }
            Input::Speaking(speaking) => {
                self.speaking = speaking;
                // The rest of the utterance arrives as `Speech`.
                if speaking && self.speech_barges_in && self.state() == State::Thinking {
                    return self.barge_in();
                }
                vec![]
            }
            // While Confirming, a reply spoken over the question counts.
            Input::Speech
                if matches!(self.state(), State::Listening | State::Confirming)
                    && !self.transcribing =>
            {
                self.transcribing = true;
                vec![Output::Transcribe]
            }
            Input::Speech
                if self.speech_barges_in
                    && self.state() == State::Thinking
                    && !self.transcribing =>
            {
                let mut outputs = self.barge_in();
                self.transcribing = true;
                outputs.push(Output::Transcribe);
                outputs
            }
            // Lucy's own echo, or the user talking over her without cutting
            // her off.
            Input::Speech => vec![],
            Input::Transcript(text) => {
                self.transcribing = false;
                self.stt_failed = false;
                let text = text.trim();
                if let Some(expects) = &self.asking {
                    // No speech recognised: keep waiting for the answer.
                    if text.is_empty() {
                        return vec![];
                    }
                    let yes = is_yes(text, &self.yes_phrases);
                    let answer = match expects {
                        Expects::Yes | Expects::YesOrWords if yes => Answer::Yes,
                        Expects::Yes => Answer::No,
                        Expects::Words | Expects::YesOrWords => Answer::Said(text.to_owned()),
                    };
                    return self.answered(answer);
                }
                if text.is_empty() {
                    // No speech recognised: keep listening, timer not reset.
                    return vec![];
                }
                if self.building != Building::No && is_phrase(text, CANCEL_IT) {
                    self.sentence = BUILD_STOPPED.to_owned();
                    let mut outputs = vec![Output::CancelBuild];
                    outputs.extend(self.speak_sentence());
                    return outputs;
                }
                self.turn_in_progress = true;
                self.heard.clear();
                self.turn = match self.interrupted.take() {
                    // Cut off before anything played.
                    Some(heard) if heard.is_empty() => format!("[You were interrupted]\n{text}"),
                    Some(heard) => {
                        format!("[You were interrupted; the user heard up to: \"{heard}\"]\n{text}")
                    }
                    None => text.to_owned(),
                };
                vec![Output::SendTurn(self.turn.clone())]
            }
            // Say sorry; a second failure in a row ends the Session.
            Input::TranscribeFailed => {
                self.transcribing = false;
                // An answer that couldn't be heard isn't a yes.
                if let Some(answer) = self.unheard() {
                    let mut outputs = vec![Output::Error];
                    outputs.extend(self.answered(answer));
                    return outputs;
                }
                self.closing |= std::mem::replace(&mut self.stt_failed, true);
                self.sentence = DIDNT_CATCH_THAT.to_owned();
                let mut outputs = vec![Output::Error];
                outputs.extend(self.speak_sentence());
                outputs
            }
            Input::Thinking(ThinkingEvent::Delta(text)) => {
                self.sentence.push_str(&text);
                vec![]
            }
            Input::Thinking(ThinkingEvent::SentenceEnd) => {
                self.speak_sentence().into_iter().collect()
            }
            Input::Thinking(ThinkingEvent::TurnEnd) => {
                let mut outputs: Vec<Output> = self.speak_sentence().into_iter().collect();
                self.turn_in_progress = false;
                outputs.extend(self.turn_finished());
                outputs
            }
            // A limit that has been hit arrives as `Unavailable`.
            Input::Thinking(ThinkingEvent::RateLimit(_)) => vec![],
            // The runner opens the model again, or says it's lost.
            Input::Thinking(ThinkingEvent::Died) => vec![],
            // Say why, instead of any reply in progress, then end.
            Input::Thinking(ThinkingEvent::Unavailable(reason)) => {
                self.sentence = reason;
                self.turn_in_progress = false;
                self.closing = true;
                let mut outputs = vec![Output::Error];
                outputs.extend(self.speak_sentence());
                outputs
            }
            Input::Played => {
                if let Some(sentence) = self.queued.pop_front() {
                    push_sentence(&mut self.heard, &sentence);
                }
                self.turn_finished().into_iter().collect()
            }
            Input::EndRequested => {
                self.closing = true;
                let mut outputs: Vec<Output> = vec![];
                if self.building != Building::No {
                    outputs.extend(self.speak_sentence());
                    self.sentence = KEEP_WORKING.to_owned();
                    outputs.extend(self.speak_sentence());
                }
                outputs.extend(self.turn_finished());
                outputs
            }
            // Cuts Lucy off at once, whatever `barge_in` says.
            Input::WakeWord if matches!(self.state(), State::Speaking | State::Thinking) => {
                self.barge_in()
            }
            // Ordinary speech: its utterance arrives as `Speech`.
            Input::WakeWord => vec![],
            // The gate also runs a little past the end of Lucy's speech.
            Input::BargeIn if matches!(self.state(), State::Speaking | State::Thinking) => {
                self.barge_in()
            }
            Input::BargeIn => vec![],
            Input::Ask(question) => {
                // Any reply text so far is said before the question.
                let mut outputs: Vec<Output> = self.speak_sentence().into_iter().collect();
                self.sentence = question.prompt;
                outputs.extend(self.speak_sentence());
                self.asking = Some(question.expects);
                outputs
            }
            Input::AnswerTimedOut => match self.unheard() {
                Some(answer) => self.answered(answer),
                None => vec![],
            },
            Input::Withdrawn => {
                self.asking = None;
                self.turn_finished().into_iter().collect()
            }
            Input::Say(text) => {
                let mut outputs: Vec<Output> = self.speak_sentence().into_iter().collect();
                self.sentence = text;
                outputs.extend(self.speak_sentence());
                outputs
            }
            Input::Building(building) => {
                self.building = building;
                vec![]
            }
        }
    }

    /// The user cuts in: Lucy stops speaking and the model stops its Turn,
    /// and she listens. The next Turn says what the user heard: the reply's
    /// sentences up to and including the one cut off.
    fn barge_in(&mut self) -> Vec<Output> {
        let mut outputs = vec![];
        if let Some(playing) = self.queued.front() {
            push_sentence(&mut self.heard, playing);
            self.queued.clear();
            outputs.push(Output::StopSpeaking);
        }
        if self.turn_in_progress {
            self.turn_in_progress = false;
            outputs.push(Output::Interrupt);
        }
        self.sentence.clear();
        self.interrupted = Some(std::mem::take(&mut self.heard));
        outputs.push(Output::ResetSilenceTimer);
        outputs
    }

    /// The answer to the question being asked, if any, when no answer was
    /// heard.
    fn unheard(&self) -> Option<Answer> {
        match self.asking {
            Some(Expects::Yes) => Some(Answer::No),
            Some(Expects::Words | Expects::YesOrWords) => Some(Answer::Silence),
            None => None,
        }
    }

    /// The question is answered: a Confirmation that isn't a yes is
    /// declined out loud.
    fn answered(&mut self, answer: Answer) -> Vec<Output> {
        self.asking = None;
        let mut outputs = vec![];
        if answer == Answer::No {
            self.sentence = DECLINED.to_owned();
            outputs.extend(self.speak_sentence());
        }
        outputs.push(Output::Answered(answer));
        outputs.extend(self.turn_finished());
        outputs
    }

    fn speak_sentence(&mut self) -> Option<Output> {
        let sentence = std::mem::take(&mut self.sentence);
        let sentence = sentence.trim();
        if sentence.is_empty() {
            return None;
        }
        self.queued.push_back(sentence.to_owned());
        Some(Output::Speak(sentence.to_owned()))
    }

    /// Back to Listening once the Turn is over and its speech has played,
    /// unless the Session is closing.
    fn turn_finished(&self) -> Option<Output> {
        match self.state() {
            State::Listening if self.closing => Some(Output::End),
            State::Listening => Some(Output::ResetSilenceTimer),
            _ => None,
        }
    }
}

/// Adds `sentence` to the reply text in `heard`.
fn push_sentence(heard: &mut String, sentence: &str) {
    if !heard.is_empty() {
        heard.push(' ');
    }
    heard.push_str(sentence);
}

/// Whether `reply` is a clear yes: one of `phrases`.
fn is_yes(reply: &str, phrases: &[String]) -> bool {
    phrases.iter().any(|phrase| is_phrase(reply, phrase))
}

/// Whether the whole of `utterance` is `phrase`, ignoring case,
/// punctuation and spacing.
fn is_phrase(utterance: &str, phrase: &str) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let utterance = words(utterance);
    !utterance.is_empty() && words(phrase) == utterance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes_phrases() -> Vec<String> {
        lucy_config::Confirmation::default().yes_phrases
    }

    #[test]
    fn after_a_barge_in_nothing_heard_says_nothing_and_the_next_words_say_what_was_heard() {
        let mut session = Session::new(yes_phrases());
        session.handle(Input::Speech);
        session.handle(Input::Transcript("count".into()));
        session.handle(Input::Thinking(ThinkingEvent::Delta("One.".into())));
        session.handle(Input::Thinking(ThinkingEvent::SentenceEnd));
        session.handle(Input::WakeWord);

        session.handle(Input::Speech);
        assert_eq!(session.handle(Input::Transcript(String::new())), []);
        assert_eq!(session.state(), State::Listening);

        session.handle(Input::Speech);
        assert_eq!(
            session.handle(Input::Transcript("go on".into())),
            [Output::SendTurn(
                "[You were interrupted; the user heard up to: \"One.\"]\ngo on".into()
            )]
        );
    }

    #[test]
    fn a_barge_in_before_anything_played_says_only_that_lucy_was_interrupted() {
        let mut session = Session::new(yes_phrases());
        session.barge_in_by_speech(true);
        session.handle(Input::Speech);
        session.handle(Input::Transcript("count".into()));
        session.handle(Input::Thinking(ThinkingEvent::Delta("One".into())));
        session.handle(Input::Speech);

        assert_eq!(
            session.handle(Input::Transcript("stop".into())),
            [Output::SendTurn("[You were interrupted]\nstop".into())]
        );
    }

    #[test]
    fn a_yes_phrase_counts_whatever_its_case_and_punctuation() {
        assert!(is_yes("Yes.", &yes_phrases()));
        assert!(is_yes("  Go ahead!", &yes_phrases()));
        assert!(is_yes("yep", &yes_phrases()));
    }

    #[test]
    fn anything_but_a_whole_yes_phrase_is_not_a_yes() {
        assert!(!is_yes("yes but not to Bob", &yes_phrases()));
        assert!(!is_yes("no", &yes_phrases()));
        assert!(!is_yes("yesterday", &yes_phrases()));
        assert!(!is_yes("...", &yes_phrases()));
    }
}
