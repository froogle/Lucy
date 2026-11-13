//! The seams between the Session and the outside world. Real adapters
//! (microphone, Whisper, `claude`, Piper) and the harness fakes both sit
//! behind these.
//!
//! An adapter keeps the sender it was handed alive for as long as the handle
//! it returned (`Conversation`, `Speaker`).

use std::sync::Arc;

use tokio::sync::mpsc;

/// One utterance, cut from the microphone by the VAD: 16 kHz mono.
pub struct SpeechSegment {
    pub samples: Vec<f32>,
}

/// Speech-to-text. Blocking; lucyd calls it off the async runtime.
pub trait Stt: Send + Sync {
    /// The words heard, or an empty string if no speech was recognised.
    fn transcribe(&self, segment: SpeechSegment) -> String;
}

/// What the model streams back during a Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThinkingEvent {
    /// More of the reply's text.
    Delta(String),
    /// The text so far completes a sentence, ready to speak.
    SentenceEnd,
    /// The model has finished the Turn.
    TurnEnd,
}

/// A Thinking Provider.
pub trait Thinking: Send + Sync {
    /// Starts the model process for a new Session. The process streams its
    /// replies into `events`. Dropping the Conversation ends the process.
    fn open(&self, events: mpsc::UnboundedSender<ThinkingEvent>) -> Box<dyn Conversation>;
}

/// The model process of one Session.
pub trait Conversation: Send {
    /// Starts a Turn with what the user said.
    fn send_turn(&mut self, text: String);
}

/// A TTS Provider together with playback.
pub trait Tts: Send + Sync {
    /// Opens playback for a new Session. Each sentence sends one `()` on
    /// `played` once it has finished playing. Dropping the Speaker stops
    /// playback.
    fn open(&self, played: mpsc::UnboundedSender<()>) -> Box<dyn Speaker>;
}

pub trait Speaker: Send {
    /// Queues a sentence to be synthesised and played.
    fn speak(&mut self, sentence: String);
}

pub struct Adapters {
    /// Utterances from the microphone. Ignored while no Session is open.
    pub speech: mpsc::UnboundedReceiver<SpeechSegment>,
    pub stt: Arc<dyn Stt>,
    pub thinking: Box<dyn Thinking>,
    pub tts: Box<dyn Tts>,
}

impl Adapters {
    /// Adapters that hear nothing and say nothing, until the real ones land.
    pub fn inert() -> Adapters {
        let (_, speech) = mpsc::unbounded_channel();
        Adapters {
            speech,
            stt: Arc::new(Inert),
            thinking: Box::new(Inert),
            tts: Box::new(Inert),
        }
    }
}

struct Inert;

impl Stt for Inert {
    fn transcribe(&self, _segment: SpeechSegment) -> String {
        String::new()
    }
}

impl Thinking for Inert {
    fn open(&self, events: mpsc::UnboundedSender<ThinkingEvent>) -> Box<dyn Conversation> {
        Box::new(InertConversation { _events: events })
    }
}

struct InertConversation {
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl Conversation for InertConversation {
    fn send_turn(&mut self, _text: String) {}
}

impl Tts for Inert {
    fn open(&self, played: mpsc::UnboundedSender<()>) -> Box<dyn Speaker> {
        Box::new(InertSpeaker { played })
    }
}

struct InertSpeaker {
    played: mpsc::UnboundedSender<()>,
}

impl Speaker for InertSpeaker {
    fn speak(&mut self, _sentence: String) {
        let _ = self.played.send(());
    }
}
