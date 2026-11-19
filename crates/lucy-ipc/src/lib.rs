//! lucyd's public IPC contract (ADR 0006).
//!
//! A Unix socket at `$XDG_RUNTIME_DIR/lucy/lucyd.sock` carrying
//! newline-delimited JSON-RPC 2.0. Every client starts with `hello`. Within a
//! major protocol version, fields are only ever added.
//!
//! Never carried: transcripts, spoken text, tool arguments.

mod client;
mod schema;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use client::{Client, Error};
pub use schema::schema;

/// The major protocol version this crate speaks.
pub const PROTOCOL: u32 = 1;

/// JSON-RPC error codes. The negative ones below -32000 are lucyd's own.
pub mod error_code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    /// `hello` asked for a major protocol version lucyd doesn't speak.
    pub const PROTOCOL_MISMATCH: i64 = -32001;
    /// A request arrived before `hello`.
    pub const HELLO_REQUIRED: i64 = -32002;
    /// `session.start` while a Session is already open.
    pub const SESSION_OPEN: i64 = -32003;
    /// `session.end` while no Session is open.
    pub const NO_SESSION: i64 = -32004;
    /// `provider.check` named a Provider that isn't in `config.toml`.
    pub const UNKNOWN_PROVIDER: i64 = -32005;
    /// `config.toml` can't be read or parsed. The message says why.
    pub const INVALID_CONFIG: i64 = -32006;
    /// `plugins.set_enabled` named a Plugin that isn't installed.
    pub const UNKNOWN_PLUGIN: i64 = -32007;
    /// `approvals.revoke` named a Plugin that has no Approval.
    pub const NOT_APPROVED: i64 = -32008;
    /// `test.voice_preview` while another Voice preview plays.
    pub const BUSY: i64 = -32009;
    /// A Session opened partway through the Voice preview and cut it short.
    pub const INTERRUPTED: i64 = -32010;
    /// The Voice preview couldn't be spoken. The message says why.
    pub const TTS_FAILED: i64 = -32011;
    /// `test.mic_level` or `test.wake_word`, but lucyd has no microphone.
    pub const NO_MICROPHONE: i64 = -32012;
    /// `test.wake_word`, but the wake-word engine is off.
    pub const WAKE_WORD_OFF: i64 = -32013;
    /// `hello` claimed a role another client holds.
    pub const ROLE_TAKEN: i64 = -32014;
    /// `input.answer` or `input.cancel` from a client without the `input`
    /// role.
    pub const NOT_INPUT_HOLDER: i64 = -32015;
    /// `input.answer` or `input.cancel` named no open Input dialog.
    pub const NO_DIALOG: i64 = -32016;
}

/// `$XDG_RUNTIME_DIR/lucy/lucyd.sock`, or `None` if `XDG_RUNTIME_DIR` is unset.
pub fn default_socket_path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(runtime).join("lucy").join("lucyd.sock"))
}

/// lucyd's private MCP socket, beside the public socket at `socket_path`.
pub fn mcp_socket_path(socket_path: &Path) -> PathBuf {
    socket_path.with_file_name("mcp.sock")
}

/// `$XDG_RUNTIME_DIR/lucy/mcp.sock`, or `None` if `XDG_RUNTIME_DIR` is unset.
pub fn default_mcp_socket_path() -> Option<PathBuf> {
    default_socket_path().map(|path| mcp_socket_path(&path))
}

/// Params of `hello`, the first request on every connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hello {
    /// Major protocol version the client speaks.
    pub protocol: u32,
    /// Free-form client name, for logs.
    pub client: String,
    /// A role the client claims, if any.
    #[serde(default)]
    pub role: Option<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Draws the Indicator. While no client holds it, lucyd uses notifications.
    Indicator,
    /// Draws the Input dialog. One client holds it, the first to claim it;
    /// while none does, lucyd opens `lucy input` in a terminal.
    Input,
}

/// Result of `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HelloResult {
    /// Major protocol version lucyd speaks.
    pub protocol: u32,
    /// lucyd's name and version.
    pub server: String,
}

/// Result of `status`: a snapshot of lucyd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    pub state: State,
    /// The current Session, if one is open.
    pub session_id: Option<String>,
    /// An Authoring build is running, with or without a Session. The
    /// Indicator adds a "building" mark.
    #[serde(default)]
    pub building: bool,
    /// A screen tool is capturing the screen for the model. The Indicator
    /// adds a "looking" mark.
    #[serde(default)]
    pub looking: bool,
    /// A Hand-off is running, with or without a Session. The Indicator adds
    /// a "working" mark.
    #[serde(default)]
    pub working: bool,
}

/// Lucy's Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No Session is open.
    Idle,
    /// A Session is open and waiting for the user to speak.
    Listening,
    /// A Turn is in progress and nothing is playing.
    Thinking,
    /// Lucy's speech is queued or playing.
    Speaking,
    /// A tool call waits for the user's spoken yes.
    Confirming,
    /// The Input dialog is open: Lucy waits for the user to type.
    Typing,
    /// Something went wrong. Transient: the next state follows straight after.
    Error,
}

/// Result of `session.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionStarted {
    pub session_id: String,
}

/// Result of `providers.status`: each Thinking Provider in `config.toml`.
/// Its health checks make no model call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProvidersStatus {
    /// Sorted by name.
    pub providers: Vec<ProviderStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderStatus {
    /// Its `[providers.<name>]` name.
    pub name: String,
    /// The CLI it drives.
    pub kind: ProviderKind,
    /// The model passed to the CLI; `None` means the CLI's default.
    pub model: Option<String>,
    /// The Model tiers that use it.
    pub tiers: Vec<Tier>,
    /// What's wrong and how to fix it, or `None` if it is healthy.
    pub problem: Option<String>,
}

/// Which CLI a Thinking Provider drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Claude,
    Codex,
}

/// A Model tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Fast,
    Advanced,
}

/// Params of `provider.check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCheck {
    /// The `[providers.<name>]` name.
    pub name: String,
}

/// Result of `provider.check`: a real "Reply OK" round trip through the
/// Provider. The subscription guard's checks run on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCheckResult {
    /// What's wrong and how to fix it, or `None` if the check passed.
    pub problem: Option<String>,
    /// The executable a Delegate of it runs, past any mise shim; `None` if not found.
    #[serde(default)]
    pub program: Option<String>,
}

/// Params of `subscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subscribe {
    pub topics: Vec<Topic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// `state` events. The current state is sent straight after subscribing.
    State,
    /// `level` events, about 30 a second while Lucy is speaking.
    Level,
    /// `plugins.changed` events.
    Plugins,
}

/// Params of the `input.open` event, sent to the `input` holder only: show
/// the Input dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputOpen {
    /// Names this dialog in `input.answer`, `input.cancel` and `input.close`.
    pub id: String,
    /// The question, written by the tool or Plugin that asks it.
    pub message: String,
    pub fields: Vec<Field>,
}

/// One box in the Input dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Field {
    /// Its key in `input.answer`'s `values`.
    pub name: String,
    /// What to type in it, if the question alone doesn't say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub kind: FieldKind,
}

/// What a field takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Url,
    Number,
    /// One of `choices`.
    Choice {
        choices: Choices,
    },
    /// A password or key: hidden as it is typed.
    Secret,
}

/// The options of a choice field: at least one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "Vec<String>")]
#[schemars(extend("minItems" = 1))]
pub struct Choices(Vec<String>);

impl TryFrom<Vec<String>> for Choices {
    type Error = NoChoices;

    fn try_from(choices: Vec<String>) -> Result<Choices, NoChoices> {
        match choices.is_empty() {
            true => Err(NoChoices),
            false => Ok(Choices(choices)),
        }
    }
}

impl Choices {
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// A choice field was given nothing to pick from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoChoices;

impl std::fmt::Display for NoChoices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a choice needs at least one option")
    }
}

impl std::error::Error for NoChoices {}

/// Params of the `input.close` event: take the Input dialog away, unanswered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputClose {
    pub id: String,
}

/// Params of `input.answer`, from the `input` holder only: what the user
/// typed. It never goes out on the socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputAnswer {
    pub id: String,
    /// Each field's value, by its name.
    pub values: BTreeMap<String, String>,
}

/// Params of `input.cancel`: the user closed the Input dialog unanswered.
/// Only the `input` holder sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputCancel {
    pub id: String,
}

/// Params of the `state` event: the new status.
pub type StateEvent = Status;

/// Params of the `level` event: how loud Lucy's voice is right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LevelEvent {
    /// RMS of the samples just played, from 0.0 (silent) to 1.0.
    pub rms: f32,
}

/// Result of `plugins.list`: every installed Plugin. A user Plugin overrides
/// a shipped one of the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginsList {
    /// Sorted by name.
    pub plugins: Vec<PluginInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInfo {
    /// Its folder's name.
    pub name: String,
    /// From its Manifest, if that can be read.
    pub version: Option<String>,
    pub author: Option<PluginAuthor>,
    pub summary: Option<String>,
    /// Shipped with Lucy: it can be disabled but not removed.
    pub shipped: bool,
    /// False once disabled. A disabled Plugin keeps its files and Approval.
    pub enabled: bool,
    pub status: PluginStatus,
    /// Why it is broken, when it is.
    pub problem: Option<String>,
    /// The names of the credentials its Manifest declares, entered with
    /// `lucy credentials <name>`. Never their values.
    #[serde(default)]
    pub credentials: Vec<String>,
}

/// Who wrote a Plugin. Lucy changes only her own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginAuthor {
    Lucy,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    /// Its exact content is approved.
    Approved,
    /// New or changed since its last Approval. Only the user's spoken yes
    /// approves it.
    AwaitingApproval,
    /// It can't be used; `problem` says why.
    Broken,
    /// Its Manifest's `requires` isn't met here, so it is left out quietly.
    NotForThisDesktop,
}

/// Params of `plugins.set_enabled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SetPluginEnabled {
    /// The Plugin's name.
    pub name: String,
    pub enabled: bool,
}

/// Result of `approvals.list`: every Approval Lucy holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalsList {
    /// Sorted by Plugin name.
    pub approvals: Vec<ApprovalInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalInfo {
    pub plugin: String,
    /// The approved hash of its Manifest plus executable.
    pub hash: String,
    /// The previous approved hash, which it can be rolled back to with no
    /// new Approval.
    pub previous_hash: Option<String>,
}

/// Params of `approvals.revoke`. The Plugin's tools go at once, even
/// mid-Session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RevokeApproval {
    pub plugin: String,
}

/// Params of `test.voice_preview`: Lucy speaks `text` outside any Session.
/// She uses her configured voice on the configured speaker, between the Start
/// and Closing sounds. It replies once playback has finished. Refused while a
/// Session is open or another Voice preview plays; a Session that opens
/// cuts it short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePreview {
    /// The sentence to say. Not empty.
    pub text: String,
    /// The voice to say it in. Unset means the configured one. lucyd
    /// fetches it first if it must.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<Voice>,
}

/// A voice of one TTS Provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Voice {
    pub kind: TtsKind,
    /// The voice's name in that Provider, such as `en_GB-cori-medium` in
    /// Piper or `bf_emma` in Kokoro.
    pub name: String,
}

/// A TTS Provider, as `[tts] kind` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TtsKind {
    Piper,
    Kokoro,
}

/// Result of `test.mic_level`. lucyd first switches to the microphone
/// `config.toml` names now. Refused while a Session is open or another
/// setup test runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MicLevel {
    /// The loudest RMS the microphone heard since the last `test.mic_level`,
    /// from 0.0 (silent) to 1.0.
    pub rms: f32,
    /// `rms` is under the level Lucy wants, even after her +24 dB quiet-mic
    /// boost. Barge-in by speech may not work then. Silence is quiet too.
    pub quiet: bool,
}

/// Params of `test.wake_word`: lucyd listens for the Wake word outside any
/// Session. It replies once it is heard or `timeout_secs` have passed.
/// Hearing it opens no Session. Refused while a Session is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WakeWordTest {
    pub timeout_secs: f64,
}

/// Result of `test.wake_word`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WakeWordTestResult {
    /// Whether the Wake word was heard.
    pub heard: bool,
    /// The best score the wake-word engine gave, from 0 to 1. `[wake_word]
    /// threshold` is where it counts as heard.
    pub score: f32,
}

/// Params of the `plugins.changed` event: something in `plugins.list` or
/// `approvals.list` changed. Fetch them again to see what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginsChanged {}

/// An event: a JSON-RPC notification from lucyd.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Event {
    State(StateEvent),
    Level(LevelEvent),
    #[serde(rename = "plugins.changed")]
    PluginsChanged(PluginsChanged),
    #[serde(rename = "input.open")]
    InputOpen(InputOpen),
    #[serde(rename = "input.close")]
    InputClose(InputClose),
}
