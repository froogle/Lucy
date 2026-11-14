//! lucyd's public IPC contract (ADR 0006).
//!
//! A Unix socket at `$XDG_RUNTIME_DIR/lucy/lucyd.sock` carrying
//! newline-delimited JSON-RPC 2.0. Every client starts with `hello`. Within a
//! major protocol version, fields are only ever added.
//!
//! Never carried: transcripts, spoken text, tool arguments.

mod client;
mod schema;

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
    /// An Authoring build is running, with or without a Session: the
    /// Indicator adds a "building" mark.
    #[serde(default)]
    pub building: bool,
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
    /// Something went wrong. Transient: the next state follows straight after.
    Error,
}

/// Result of `session.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionStarted {
    pub session_id: String,
}

/// Result of `providers.status`: each Thinking Provider in `config.toml`,
/// with health checks that make no model call.
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
/// Provider, with the subscription guard's checks on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCheckResult {
    /// What's wrong and how to fix it, or `None` if the check passed.
    pub problem: Option<String>,
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

/// Params of the `state` event: the new status.
pub type StateEvent = Status;

/// Params of the `level` event: how loud Lucy's voice is right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LevelEvent {
    /// RMS of the samples just played, from 0.0 (silent) to 1.0.
    pub rms: f32,
}

/// Result of `plugins.list`: every installed Plugin, a user one overriding
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

/// Params of `test.voice_preview`: Lucy speaks `text` in her running voice
/// on the configured speaker, outside any Session, between the Start and
/// Closing sounds. It replies once playback has finished. Refused while a
/// Session is open or another Voice preview plays; a Session that opens
/// cuts it short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePreview {
    /// The sentence to say. Not empty.
    pub text: String,
    /// A Piper voice to say it in, such as `en_GB-cori-medium`, in place of
    /// the running one. lucyd downloads it first if it must.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

/// Result of `test.mic_level`. lucyd first switches to the microphone
/// `config.toml` names now. Refused while a Session is open or another
/// setup test runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MicLevel {
    /// The loudest RMS the microphone heard since the last `test.mic_level`,
    /// from 0.0 (silent) to 1.0.
    pub rms: f32,
}

/// Params of `test.wake_word`: lucyd listens for the Wake word, outside any
/// Session, and replies once it is heard or `timeout_secs` have passed.
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
}
