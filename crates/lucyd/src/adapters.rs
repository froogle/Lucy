//! The seams between the Session and the outside world. Real adapters
//! (microphone, Whisper, `claude`, Piper) and the harness fakes both sit
//! behind these.
//!
//! An adapter keeps the sender it was handed alive for as long as the handle
//! it returned (`SessionModel`, `Speaker`).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lucy_config::Provider;
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
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingEvent {
    /// More of the reply's text.
    Delta(String),
    /// The text so far completes a sentence, ready to speak.
    SentenceEnd,
    /// The model has finished the Turn.
    TurnEnd,
    /// Where the subscription stands against its usage limits.
    RateLimit(RateLimit),
    /// The Provider can't be used. The text says why and how to fix it,
    /// ready to speak; the Session says it and ends.
    Unavailable(String),
}

/// A `rate_limit_event` from `claude`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub status: RateLimitStatus,
    /// Which window this is about, e.g. `five_hour` or `seven_day`.
    pub rate_limit_type: Option<String>,
    /// Unix seconds.
    pub resets_at: Option<u64>,
    /// Fraction of the window used, when reported.
    pub utilization: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
    #[serde(other)]
    Unknown,
}

/// The command a model process runs as its one MCP server, to reach Lucy's
/// tools: `lucy mcp-relay` with a token that names this caller.
#[derive(Debug, Clone)]
pub struct Relay {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// A Thinking Provider.
pub trait Thinking: Send + Sync {
    /// Starts the model process for a new Session with the settings in
    /// `provider`, configured with `relay` as its MCP server and following
    /// `instructions` (the Persona) all Session. The process streams its
    /// replies into `events`. Dropping the SessionModel ends the process. If
    /// the Provider can't be used, it sends `Unavailable`.
    fn open(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
    ) -> Box<dyn SessionModel>;

    /// Checks without a model call that the CLI is installed and signed in
    /// as `provider` requires. Blocking. The error is a reason ready to
    /// speak, with the fix.
    fn health(&self, provider: &Provider) -> Result<(), String>;

    /// A real "Reply OK" round trip, with the subscription guard's checks
    /// on it. Blocking. The error is a reason ready to speak, with the fix.
    fn check(&self, provider: &Provider) -> Result<(), String>;
}

/// The model process of one Session.
pub trait SessionModel: Send {
    /// Starts a Turn with what the user said.
    fn send_turn(&mut self, text: String);
}

/// A TTS Provider together with playback.
pub trait Tts: Send + Sync {
    /// Opens playback for a new Session. Each sentence sends one `Played` or
    /// `Failed` on `events`, in order, and `Level` about 30 times a second
    /// while it plays. Dropping the Speaker stops playback.
    fn open(&self, events: mpsc::UnboundedSender<SpeakerEvent>) -> Box<dyn Speaker>;
}

pub trait Speaker: Send {
    /// Queues a sentence to be synthesised and played.
    fn speak(&mut self, sentence: String);
}

/// What a Speaker reports back.
#[derive(Debug, Clone, PartialEq)]
pub enum SpeakerEvent {
    /// The oldest queued sentence finished playing.
    Played,
    /// The oldest queued sentence couldn't be synthesised or played. Carries
    /// its text.
    Failed(String),
    /// RMS of the samples just played, from 0.0 to 1.0.
    Level(f32),
}

/// A desktop notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub summary: String,
    pub body: String,
    /// An action button, if the notification offers one.
    pub action: Option<NotificationAction>,
    pub kind: NotificationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// Session start, Confirming or end, from the notification fallback.
    /// Each replaces the last one that offered an action, so a stale End
    /// button doesn't linger.
    Status,
    /// A reply Lucy couldn't speak. It replaces nothing, so the End button
    /// stays.
    Reply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationAction {
    /// "End": ends the Session.
    EndSession,
}

/// Desktop notifications (`org.freedesktop.Notifications`).
pub trait Notifier: Send + Sync {
    /// Shows `notification` without blocking. If the user picks its action,
    /// the action is sent on `actions`.
    fn notify(
        &self,
        notification: Notification,
        actions: mpsc::UnboundedSender<NotificationAction>,
    );
}

/// Starts things on the user's desktop. Blocking; lucyd calls it off the
/// async runtime.
pub trait Launcher: Send + Sync {
    /// Opens an http(s) URL in the user's browser.
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// Starts the app whose desktop entry is at `entry`.
    fn launch(&self, entry: &Path) -> io::Result<()>;
}

pub struct Adapters {
    /// Utterances from the microphone. Ignored while no Session is open.
    pub speech: mpsc::UnboundedReceiver<SpeechSegment>,
    pub stt: Arc<dyn Stt>,
    pub thinking: Arc<dyn Thinking>,
    pub tts: Box<dyn Tts>,
    pub notifier: Arc<dyn Notifier>,
    pub launcher: Arc<dyn Launcher>,
    /// Where to look for desktop entries, most important first.
    pub applications: Vec<PathBuf>,
}

impl Adapters {
    /// Adapters that hear nothing and say nothing, until the real ones land.
    pub fn inert() -> Adapters {
        let (_, speech) = mpsc::unbounded_channel();
        Adapters {
            speech,
            stt: Arc::new(Inert),
            thinking: Arc::new(Inert),
            tts: Box::new(Inert),
            notifier: Arc::new(Inert),
            launcher: Arc::new(Inert),
            applications: vec![],
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
    fn open(
        &self,
        _provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        _relay: Relay,
        _instructions: String,
    ) -> Box<dyn SessionModel> {
        Box::new(InertSessionModel { _events: events })
    }

    fn health(&self, _provider: &Provider) -> Result<(), String> {
        Err(INERT_THINKING.to_owned())
    }

    fn check(&self, _provider: &Provider) -> Result<(), String> {
        Err(INERT_THINKING.to_owned())
    }
}

const INERT_THINKING: &str = "This lucyd has no Thinking Provider.";

struct InertSessionModel {
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl SessionModel for InertSessionModel {
    fn send_turn(&mut self, _text: String) {}
}

impl Tts for Inert {
    fn open(&self, events: mpsc::UnboundedSender<SpeakerEvent>) -> Box<dyn Speaker> {
        Box::new(InertSpeaker { events })
    }
}

struct InertSpeaker {
    events: mpsc::UnboundedSender<SpeakerEvent>,
}

impl Speaker for InertSpeaker {
    fn speak(&mut self, _sentence: String) {
        let _ = self.events.send(SpeakerEvent::Played);
    }
}

impl Notifier for Inert {
    fn notify(
        &self,
        _notification: Notification,
        _actions: mpsc::UnboundedSender<NotificationAction>,
    ) {
    }
}

impl Launcher for Inert {
    fn open_url(&self, _url: &str) -> io::Result<()> {
        Ok(())
    }

    fn launch(&self, _entry: &Path) -> io::Result<()> {
        Ok(())
    }
}
