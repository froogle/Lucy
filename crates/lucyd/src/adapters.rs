//! The seams between the Session and the outside world. Real adapters
//! (microphone, Whisper, `claude`, Piper) and the harness fakes both sit
//! behind these.
//!
//! An adapter keeps the sender it was handed alive for as long as the handle
//! it returned (`SessionModel`, `Speaker`).

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

pub use lucy_config::credentials::CredentialStore;
use lucy_config::Provider;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::audio::Hearing;

/// One utterance, cut from the microphone by the VAD: 16 kHz mono.
pub struct SpeechSegment {
    pub samples: Vec<f32>,
}

/// Speech-to-text. Blocking; lucyd calls it off the async runtime.
pub trait Stt: Send + Sync {
    /// The words heard, or an empty string if no speech was recognised.
    ///
    /// # Errors
    ///
    /// When the STT Provider fails.
    fn transcribe(&self, segment: SpeechSegment) -> Result<String, SttError>;
}

/// Why STT couldn't transcribe an utterance, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SttError(pub String);

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
    /// The model process ended while the SessionModel was still open. Lucy
    /// opens a new one once and resends the Turn in progress.
    Died,
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

/// The command a model process runs as its one MCP server, to reach Lucy's tools.
/// It is `lucy mcp-relay` with a token that names this caller.
#[derive(Debug, Clone)]
pub struct Relay {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// A Thinking Provider.
pub trait Thinking: Send + Sync {
    /// Starts the model process for a new Session with the settings in `provider`.
    /// It is configured with `relay` as its MCP server and follows
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

    /// Checks without a model call that the CLI is installed and signed in.
    /// It checks as `provider` requires. Blocking.
    ///
    /// # Errors
    ///
    /// A reason ready to speak, with the fix.
    fn health(&self, provider: &Provider) -> Result<(), String>;

    /// A real "Reply OK" round trip, with the subscription guard's checks
    /// on it. Blocking.
    ///
    /// # Errors
    ///
    /// A reason ready to speak, with the fix.
    fn check(&self, provider: &Provider) -> Result<(), String>;

    /// Starts a Delegate's model process, as `open` starts a Session's.
    /// It has the CLI's own file and shell tools, works in `workspace.dir`,
    /// and is confined by Landlock to `workspace` plus the CLI's own program
    /// and config. A Provider that can't run Delegates sends `Unavailable`.
    fn open_delegate(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
        workspace: Workspace,
    ) -> Box<dyn SessionModel> {
        let _ = (provider, relay, instructions, workspace);
        let reason = "This Provider can't build Plugins.".to_owned();
        let _ = events.send(ThinkingEvent::Unavailable(reason));
        Box::new(InertSessionModel::new(events))
    }

    /// lucyd is stopping: ends every model process still open.
    /// It also ends what they keep on disk, as dropping each SessionModel would.
    fn shutdown(&self) {}
}

/// Where a Delegate works, and the only other places it may touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The build staging dir: its working directory, read-write.
    pub dir: PathBuf,
    /// Its `TMPDIR`, read-write.
    pub tmp: PathBuf,
    /// Other paths it may read and write, such as toolchain caches.
    pub writable: Vec<PathBuf>,
    /// Other paths it may only read and run, such as toolchains and the
    /// relay.
    pub readable: Vec<PathBuf>,
}

pub(crate) struct Role {
    pub relay: Relay,
    pub instructions: String,
    pub workspace: Option<Workspace>,
}

/// The model process of one Session.
pub trait SessionModel: Send {
    /// Starts a Turn with what the user said. If a Turn is still running,
    /// this one starts once it has ended.
    fn send_turn(&mut self, text: String);

    /// Stops the Turn in progress, for a Barge-in. The interrupted Turn
    /// sends no more events, not even `TurnEnd`. Does nothing between Turns.
    fn interrupt(&mut self);
}

/// A TTS Provider together with playback.
pub trait Tts: Send + Sync {
    /// Opens playback on `speaker` in `voice`, for a new Session or a Voice preview.
    /// `speaker` is `"default"` or a pinned node name. A voice that isn't
    /// there yet is fetched first. Each sentence sends one `Played` or
    /// `Failed` on `events`, in order, and `Level` about 30 times a second
    /// while it plays. If the device goes away it sends `DeviceLost`.
    /// Dropping the Speaker stops playback.
    fn open(
        &self,
        speaker: &str,
        voice: &str,
        events: mpsc::UnboundedSender<SpeakerEvent>,
    ) -> Box<dyn Speaker>;
}

pub trait Speaker: Send {
    fn speak(&mut self, sentence: String);

    fn sound(&mut self, sound: Sound);

    fn duck(&mut self, ducked: bool);

    fn stop(&mut self);
}

/// The cues Lucy plays around a Session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// A Session has opened.
    Start,
    /// A Session ended on its own, after silence.
    Closing,
}

/// What a Speaker reports back.
#[derive(Debug, Clone, PartialEq)]
pub enum SpeakerEvent {
    /// The oldest queued sentence finished playing.
    Played,
    /// The oldest queued sentence couldn't be synthesised or played.
    Failed { sentence: String, reason: String },
    /// RMS of the samples just played, from 0.0 to 1.0.
    Level(f32),
    /// The output device went away; the Session ends.
    DeviceLost,
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
    Status,
    Reply,
    Problem,
    Details,
    Build,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationAction {
    EndSession,
}

/// Time, as lucyd's timers count it: a Session's and an Authoring build's.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// A timer, not yet set.
    fn timer(&self) -> Box<dyn Timer>;
}

/// Goes off at the deadline it was last set to.
pub trait Timer: Send {
    /// `None` stops it.
    fn set(&mut self, deadline: Option<Instant>);
    /// Resolves once the clock reaches the deadline, as it is set while
    /// this waits.
    fn fired(&self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn timer(&self) -> Box<dyn Timer> {
        Box::new(SystemTimer(watch::Sender::new(None)))
    }
}

struct SystemTimer(watch::Sender<Option<Instant>>);

impl Timer for SystemTimer {
    fn set(&mut self, deadline: Option<Instant>) {
        self.0.send_replace(deadline);
    }

    fn fired(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let mut deadline = self.0.subscribe();
        Box::pin(async move {
            loop {
                let set = *deadline.borrow_and_update();
                match set {
                    Some(at) if at <= Instant::now() => return,
                    Some(at) => tokio::select! {
                        () = tokio::time::sleep_until(at) => {}
                        _ = deadline.changed() => {}
                    },
                    None => {
                        if deadline.changed().await.is_err() {
                            std::future::pending::<()>().await;
                        }
                    }
                }
            }
        })
    }
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
    ///
    /// # Errors
    ///
    /// When the opener can't be started.
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// Opens the file manager with `paths` highlighted.
    ///
    /// # Errors
    ///
    /// When no file manager can be reached or started.
    fn show_items(&self, paths: &[PathBuf]) -> io::Result<()>;
    /// Opens the file at `path` in its default app.
    ///
    /// # Errors
    ///
    /// When the opener can't be started.
    fn open_file(&self, path: &Path) -> io::Result<()>;
    /// Starts the app whose desktop entry is at `entry`.
    /// It goes through `launcher` (`[desktop] launcher`, with the entry's path
    /// appended) if it is set.
    ///
    /// # Errors
    ///
    /// When the app can't be started.
    fn launch(&self, entry: &Path, launcher: Option<&[String]>) -> io::Result<()>;
    /// Runs `command` in a new window of the user's terminal.
    ///
    /// # Errors
    ///
    /// When no terminal can be started.
    fn open_terminal(&self, command: &[String]) -> io::Result<()>;
}

pub struct Adapters {
    /// Utterances from the microphone. Ignored while no Session is open.
    pub speech: mpsc::UnboundedReceiver<SpeechSegment>,
    /// One message each time the Wake word is heard.
    pub wake_word: mpsc::UnboundedReceiver<()>,
    /// Whether the user is in the middle of an utterance. It turns false
    /// once the utterance is on `speech`, or dropped.
    pub speaking: watch::Receiver<bool>,
    /// What `speech` comes from, if it is the microphone.
    /// It takes `[audio]` and `[endpointing]` at each Session start.
    pub hearing: Option<Hearing>,
    /// The `whisper` STT Provider.
    pub stt: Arc<dyn Stt>,
    /// The `superwhisper` STT Provider, used when `[stt]` chooses it.
    pub superwhisper: Arc<dyn Stt>,
    pub thinking: Arc<dyn Thinking>,
    pub tts: Box<dyn Tts>,
    pub notifier: Arc<dyn Notifier>,
    pub launcher: Arc<dyn Launcher>,
    /// Where Plugin credentials are kept: the Secret Service.
    pub credentials: Arc<dyn CredentialStore>,
    /// What lucyd's timers count time on.
    pub clock: Arc<dyn Clock>,
    /// Where to look for desktop entries, most important first.
    pub applications: Vec<PathBuf>,
    /// The user's home, which `~` means in `[files] roots`.
    pub home: PathBuf,
}

impl Adapters {
    /// Adapters that hear nothing and say nothing, until the real ones land.
    pub fn inert() -> Adapters {
        let (_, speech) = mpsc::unbounded_channel();
        let (_, wake_word) = mpsc::unbounded_channel();
        let (_, speaking) = watch::channel(false);
        Adapters {
            speech,
            wake_word,
            speaking,
            hearing: None,
            stt: Arc::new(Inert),
            superwhisper: Arc::new(Inert),
            thinking: Arc::new(Inert),
            tts: Box::new(Inert),
            notifier: Arc::new(Inert),
            launcher: Arc::new(Inert),
            credentials: Arc::new(Inert),
            clock: Arc::new(SystemClock),
            applications: vec![],
            home: PathBuf::new(),
        }
    }
}

struct Inert;

impl CredentialStore for Inert {
    fn get(&self, _plugin: &str, _name: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn set(&self, _plugin: &str, _name: &str, _value: &str) -> Result<(), String> {
        Err("this lucyd keeps no credentials".into())
    }
}

impl Stt for Inert {
    fn transcribe(&self, _segment: SpeechSegment) -> Result<String, SttError> {
        Ok(String::new())
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
        Box::new(InertSessionModel::new(events))
    }

    fn health(&self, _provider: &Provider) -> Result<(), String> {
        Err(INERT_THINKING.to_owned())
    }

    fn check(&self, _provider: &Provider) -> Result<(), String> {
        Err(INERT_THINKING.to_owned())
    }
}

const INERT_THINKING: &str = "This lucyd has no Thinking Provider.";

pub(crate) struct InertSessionModel {
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl InertSessionModel {
    pub(crate) fn new(events: mpsc::UnboundedSender<ThinkingEvent>) -> Self {
        InertSessionModel { _events: events }
    }
}

impl SessionModel for InertSessionModel {
    fn send_turn(&mut self, _text: String) {}

    fn interrupt(&mut self) {}
}

impl Tts for Inert {
    fn open(
        &self,
        _speaker: &str,
        _voice: &str,
        events: mpsc::UnboundedSender<SpeakerEvent>,
    ) -> Box<dyn Speaker> {
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

    fn sound(&mut self, _sound: Sound) {}

    fn duck(&mut self, _ducked: bool) {}

    fn stop(&mut self) {}
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

    fn show_items(&self, _paths: &[PathBuf]) -> io::Result<()> {
        Ok(())
    }

    fn open_file(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn launch(&self, _entry: &Path, _launcher: Option<&[String]>) -> io::Result<()> {
        Ok(())
    }

    fn open_terminal(&self, _command: &[String]) -> io::Result<()> {
        Ok(())
    }
}
