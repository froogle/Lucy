//! The Session state machine: pure and I/O-free. The runner feeds it inputs
//! and carries out the outputs it returns.

use lucy_ipc::State;

use crate::adapters::ThinkingEvent;

pub enum Input {
    /// The VAD cut an utterance.
    Speech,
    /// STT finished the utterance it was given.
    Transcript(String),
    Thinking(ThinkingEvent),
    /// TTS finished playing one sentence.
    Played,
    /// The model called `end_session`.
    EndRequested,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// Transcribe the utterance that was just cut.
    Transcribe,
    SendTurn(String),
    Speak(String),
    /// Start the silence timer over.
    ResetSilenceTimer,
    /// End the Session.
    End,
}

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
}

impl Session {
    pub fn state(&self) -> State {
        if self.queued > 0 {
            State::Speaking
        } else if self.turn_in_progress {
            State::Thinking
        } else {
            State::Listening
        }
    }

    /// The silence timer counts only while Lucy is listening and no
    /// utterance is being transcribed.
    pub fn silence_timer_runs(&self) -> bool {
        self.state() == State::Listening && !self.transcribing
    }

    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Speech if self.state() == State::Listening && !self.transcribing => {
                self.transcribing = true;
                vec![Output::Transcribe]
            }
            // Speech while Lucy is busy is Barge-in, which isn't handled yet.
            Input::Speech => vec![],
            Input::Transcript(text) => {
                self.transcribing = false;
                let text = text.trim();
                if text.is_empty() {
                    // No speech recognised: keep listening, timer not reset.
                    return vec![];
                }
                self.turn_in_progress = true;
                vec![Output::SendTurn(text.to_owned())]
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
            Input::Played => {
                self.queued = self.queued.saturating_sub(1);
                self.turn_finished().into_iter().collect()
            }
            Input::EndRequested => {
                self.closing = true;
                self.turn_finished().into_iter().collect()
            }
        }
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
