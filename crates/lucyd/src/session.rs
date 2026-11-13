//! The Session state machine: pure and I/O-free. The runner feeds it inputs
//! and carries out the outputs it returns.

use lucy_ipc::State;

use crate::adapters::ThinkingEvent;

pub enum Input {
    /// Lucy speaks first: start a Turn with these instructions.
    Opening(String),
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
    /// A tool call waits on the user: say the question, then hear one
    /// utterance (Confirming).
    Ask(Question),
    /// The question went unanswered for too long.
    AnswerTimedOut,
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
}

const DIDNT_CATCH_THAT: &str = "Sorry, I didn't catch that.";

/// An open Session.
#[derive(Default)]
pub struct Session {
    transcribing: bool,
    turn_in_progress: bool,
    /// Sentences handed to TTS that haven't finished playing.
    queued: usize,
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
}

impl Session {
    /// A Session that hears `yes_phrases` as a yes.
    pub fn new(yes_phrases: Vec<String>) -> Self {
        Session {
            yes_phrases,
            ..Session::default()
        }
    }

    pub fn state(&self) -> State {
        if self.asking.is_some() {
            State::Confirming
        } else if self.queued > 0 {
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

    /// The silence timer counts only while Lucy is listening, including while
    /// an utterance is being transcribed.
    pub fn silence_timer_runs(&self) -> bool {
        self.state() == State::Listening
    }

    /// The wait for an answer counts once the question has been said, and
    /// not while the user's reply is being transcribed.
    pub fn answer_timer_runs(&self) -> bool {
        self.asking.is_some() && self.queued == 0 && !self.transcribing
    }

    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Opening(text) => {
                self.turn_in_progress = true;
                self.turn.clone_from(&text);
                vec![Output::SendTurn(text)]
            }
            // While Confirming, a reply spoken over the question counts.
            Input::Speech
                if matches!(self.state(), State::Listening | State::Confirming)
                    && !self.transcribing =>
            {
                self.transcribing = true;
                vec![Output::Transcribe]
            }
            // Speech while Lucy is busy is Barge-in, which isn't handled yet.
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
                    let answer = match expects {
                        Expects::Yes if is_yes(text, &self.yes_phrases) => Answer::Yes,
                        Expects::Yes => Answer::No,
                        Expects::Words => Answer::Said(text.to_owned()),
                    };
                    return self.answered(answer);
                }
                if text.is_empty() {
                    // No speech recognised: keep listening, timer not reset.
                    return vec![];
                }
                self.turn_in_progress = true;
                self.turn = text.to_owned();
                vec![Output::SendTurn(text.to_owned())]
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
                self.queued = self.queued.saturating_sub(1);
                self.turn_finished().into_iter().collect()
            }
            Input::EndRequested => {
                self.closing = true;
                self.turn_finished().into_iter().collect()
            }
            // Ordinary speech: its utterance arrives as `Speech`. During
            // Speaking it will be an instant Barge-in (ticket 09).
            Input::WakeWord => vec![],
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
        }
    }

    /// The answer to the question being asked, if any, when no answer was
    /// heard.
    fn unheard(&self) -> Option<Answer> {
        match self.asking {
            Some(Expects::Yes) => Some(Answer::No),
            Some(Expects::Words) => Some(Answer::Silence),
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
        self.queued += 1;
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

/// Whether `reply` is a clear yes: one of `phrases`, ignoring case,
/// punctuation and spacing.
fn is_yes(reply: &str, phrases: &[String]) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let reply = words(reply);
    !reply.is_empty() && phrases.iter().any(|phrase| words(phrase) == reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes_phrases() -> Vec<String> {
        lucy_config::Confirmation::default().yes_phrases
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
