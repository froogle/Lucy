use std::collections::{BTreeMap, VecDeque};

use lucy_ipc::State;

use crate::adapters::ThinkingEvent;
use crate::authoring::Building;
use crate::hand_off::Working;
use crate::input::Dialog;
use crate::model_text::INTERRUPTED;

pub enum Input {
    Opening(String),
    Speaking(bool),
    Speech,
    Transcript(String),
    TranscribeFailed,
    Thinking(ThinkingEvent),
    Played,
    EndRequested,
    WakeWord,
    BargeIn,
    Ask(Question),
    AnswerTimedOut,
    Withdrawn,
    Say(String),
    Building(Building),
    Working(Working),
    Typed(Answer),
    Unshown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub prompt: String,
    pub expects: Expects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expects {
    Yes,
    Words,
    YesOrWords,
    Typed(Dialog),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Said(String),
    Silence,
    Typed(BTreeMap<String, String>),
    Cancelled,
    TimedOut,
}

pub const DECLINED: &str = "Okay, I won't.";

#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    Transcribe,
    SendTurn(String),
    Speak(String),
    ResetSilenceTimer,
    Error,
    End,
    Answered(Answer),
    StopSpeaking,
    Interrupt,
    CancelBuild,
    CancelHandOff,
    AskWhichToCancel,
}

const DIDNT_CATCH_THAT: &str = "Sorry, I didn't catch that.";

const CANCEL_IT: &str = "cancel it";

pub const BUILD_STOPPED: &str = "Okay, I've stopped the build.";

pub const HAND_OFF_STOPPED: &str = "Okay, I've stopped Claude.";

pub const BOTH_STOPPED: &str = "Okay, I've stopped them both.";

pub const BOTH_RUNNING: &str = "Okay, I'll leave them both running.";

pub const WHICH_TO_CANCEL: &str = "Do you mean the Plugin build, or Claude?";

#[derive(Debug, PartialEq, Eq)]
pub enum Cancel {
    Build,
    HandOff,
    Both,
    Neither,
}

pub const KEEP_WORKING: &str = "I'll keep working on the Plugin, and let you know when it's ready.";

#[derive(Default)]
pub struct Session {
    transcribing: bool,
    speaking: bool,
    turn_in_progress: bool,
    queued: VecDeque<String>,
    heard: String,
    interrupted: Option<String>,
    speech_barges_in: bool,
    sentence: String,
    closing: bool,
    stt_failed: bool,
    turn: String,
    asking: Option<Expects>,
    yes_phrases: Vec<String>,
    cancel_phrases: Vec<String>,
    building: Building,
    working: Working,
}

impl Session {
    pub fn new(yes_phrases: Vec<String>) -> Self {
        Session {
            yes_phrases,
            ..Session::default()
        }
    }

    pub fn barge_in_by_speech(&mut self, on: bool) {
        self.speech_barges_in = on;
    }

    pub fn hear_cancel_phrases(&mut self, phrases: Vec<String>) {
        self.cancel_phrases = phrases;
    }

    pub fn state(&self) -> State {
        if matches!(self.asking, Some(Expects::Typed(_))) {
            State::Typing
        } else if self.asking.is_some() {
            State::Confirming
        } else if !self.queued.is_empty() {
            State::Speaking
        } else if self.turn_in_progress {
            State::Thinking
        } else {
            State::Listening
        }
    }

    pub fn pending_turn(&self) -> Option<&str> {
        self.turn_in_progress.then_some(self.turn.as_str())
    }

    pub fn silence_timer_runs(&self) -> bool {
        self.state() == State::Listening
            && !self.speaking
            && self.building != Building::Working
            && self.working == Working::No
    }

    pub fn quiet(&self) -> bool {
        self.state() == State::Listening && !self.speaking && !self.transcribing
    }

    pub fn box_line_said(&self) -> bool {
        self.state() == State::Typing && self.queued.is_empty()
    }

    pub fn answer_timer_runs(&self) -> bool {
        let said = self.asking.is_some() && self.queued.is_empty();
        let typed = self.state() == State::Typing;
        said && (typed || !self.transcribing && !self.speaking)
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
                if speaking && self.speech_barges_in && self.state() == State::Thinking {
                    return self.barge_in();
                }
                vec![]
            }
            Input::Speech
                if matches!(
                    self.state(),
                    State::Listening | State::Confirming | State::Typing
                ) && !self.transcribing =>
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
            Input::Speech => vec![],
            Input::Transcript(text) => {
                self.transcribing = false;
                self.stt_failed = false;
                let text = text.trim();
                let nothing_recognised = text.is_empty();
                if let Some(expects) = &self.asking {
                    if nothing_recognised {
                        return vec![];
                    }
                    if let Expects::Typed(_) = expects {
                        if is_one_of(text, &self.cancel_phrases) {
                            return self.answered(Answer::Cancelled);
                        }
                        return vec![];
                    }
                    let yes = is_one_of(text, &self.yes_phrases);
                    let answer = match expects {
                        Expects::Yes | Expects::YesOrWords if yes => Answer::Yes,
                        Expects::Yes => Answer::No,
                        Expects::Words | Expects::YesOrWords => Answer::Said(text.to_owned()),
                        Expects::Typed(_) => unreachable!("handled above"),
                    };
                    return self.answered(answer);
                }
                if nothing_recognised {
                    return vec![];
                }
                if is_phrase(text, CANCEL_IT) {
                    if let Some(outputs) = self.cancel() {
                        return outputs;
                    }
                }
                self.turn_in_progress = true;
                self.heard.clear();
                self.turn = match self.interrupted.take() {
                    Some(heard) if heard.is_empty() => format!("{INTERRUPTED}]\n{text}"),
                    Some(heard) => {
                        format!("{INTERRUPTED}; the user heard up to: \"{heard}\"]\n{text}")
                    }
                    None => text.to_owned(),
                };
                vec![Output::SendTurn(self.turn.clone())]
            }
            Input::TranscribeFailed => {
                self.transcribing = false;
                if self.state() == State::Typing {
                    return vec![];
                }
                if let Some(answer) = self.unheard() {
                    let mut outputs = vec![Output::Error];
                    outputs.extend(self.answered(answer));
                    return outputs;
                }
                let failed_twice_running = std::mem::replace(&mut self.stt_failed, true);
                self.closing |= failed_twice_running;
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
            Input::Thinking(
                ThinkingEvent::RateLimit(_)
                | ThinkingEvent::Final(_)
                | ThinkingEvent::Conversation(_),
            ) => vec![],
            Input::Thinking(ThinkingEvent::Died(_) | ThinkingEvent::Failed(_)) => vec![],
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
            Input::WakeWord if matches!(self.state(), State::Speaking | State::Thinking) => {
                self.barge_in()
            }
            Input::WakeWord => vec![],
            Input::BargeIn if matches!(self.state(), State::Speaking | State::Thinking) => {
                self.barge_in()
            }
            Input::BargeIn => vec![],
            Input::Ask(question) => {
                let reply_so_far = self.speak_sentence();
                let mut outputs: Vec<Output> = reply_so_far.into_iter().collect();
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
            Input::Working(working) => {
                self.working = working;
                vec![]
            }
            Input::Typed(answer) if self.state() == State::Typing => self.answered(answer),
            Input::Typed(_) => vec![],
            Input::Unshown(why) if self.state() == State::Typing => {
                self.asking = None;
                self.sentence = why;
                let mut outputs: Vec<Output> = self.speak_sentence().into_iter().collect();
                outputs.push(Output::Answered(Answer::Cancelled));
                outputs
            }
            Input::Unshown(_) => vec![],
        }
    }

    fn cancel(&mut self) -> Option<Vec<Output>> {
        let (cancel, stopped) = match (self.building != Building::No, self.working) {
            (true, Working::Yes) => return Some(vec![Output::AskWhichToCancel]),
            (true, Working::No) => (Output::CancelBuild, BUILD_STOPPED),
            (false, Working::Yes) => (Output::CancelHandOff, HAND_OFF_STOPPED),
            (false, Working::No) => return None,
        };
        self.sentence = stopped.to_owned();
        let mut outputs = vec![cancel];
        outputs.extend(self.speak_sentence());
        Some(outputs)
    }

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

    fn unheard(&self) -> Option<Answer> {
        match self.asking {
            Some(Expects::Yes | Expects::Typed(_)) => Some(Answer::TimedOut),
            Some(Expects::Words | Expects::YesOrWords) => Some(Answer::Silence),
            None => None,
        }
    }

    fn answered(&mut self, answer: Answer) -> Vec<Output> {
        let asked = self.asking.take();
        let mut outputs = vec![];
        let unheard_yes = matches!(asked, Some(Expects::Yes)) && answer == Answer::TimedOut;
        if answer == Answer::No || unheard_yes {
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

    fn turn_finished(&self) -> Option<Output> {
        match self.state() {
            State::Listening if self.closing => Some(Output::End),
            State::Listening => Some(Output::ResetSilenceTimer),
            _ => None,
        }
    }
}

fn push_sentence(heard: &mut String, sentence: &str) {
    if !heard.is_empty() {
        heard.push(' ');
    }
    heard.push_str(sentence);
}

pub fn which_to_cancel(said: &str) -> Cancel {
    let said = words(said);
    let named = |names: &[&str]| said.iter().any(|word| names.contains(&word.as_str()));
    let both = named(&["both", "all"]);
    let build = both || named(&["build", "plugin"]);
    let hand_off = both || named(&["claude", "hand", "handoff"]);
    match (build, hand_off) {
        (true, true) => Cancel::Both,
        (true, false) => Cancel::Build,
        (false, true) => Cancel::HandOff,
        (false, false) => Cancel::Neither,
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn is_one_of(reply: &str, phrases: &[String]) -> bool {
    phrases.iter().any(|phrase| is_phrase(reply, phrase))
}

fn is_phrase(utterance: &str, phrase: &str) -> bool {
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
    fn the_answer_to_which_one_names_the_build_claude_or_both() {
        assert_eq!(which_to_cancel("The Plugin build."), Cancel::Build);
        assert_eq!(which_to_cancel("claude"), Cancel::HandOff);
        assert_eq!(which_to_cancel("the hand-off"), Cancel::HandOff);
        assert_eq!(which_to_cancel("both of them"), Cancel::Both);
        assert_eq!(which_to_cancel("neither"), Cancel::Neither);
    }

    #[test]
    fn a_yes_phrase_counts_whatever_its_case_and_punctuation() {
        assert!(is_one_of("Yes.", &yes_phrases()));
        assert!(is_one_of("  Go ahead!", &yes_phrases()));
        assert!(is_one_of("yep", &yes_phrases()));
    }

    #[test]
    fn anything_but_a_whole_yes_phrase_is_not_a_yes() {
        assert!(!is_one_of("yes but not to Bob", &yes_phrases()));
        assert!(!is_one_of("no", &yes_phrases()));
        assert!(!is_one_of("yesterday", &yes_phrases()));
        assert!(!is_one_of("...", &yes_phrases()));
    }
}
