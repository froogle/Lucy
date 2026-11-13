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
}

/// Result of `session.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionStarted {
    pub session_id: String,
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
}

/// Params of the `state` event: the new status.
pub type StateEvent = Status;

/// Params of the `level` event: how loud Lucy's voice is right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoudnessEvent {
    /// RMS of the samples just played, from 0.0 (silent) to 1.0.
    pub rms: f32,
}

/// An event: a JSON-RPC notification from lucyd.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Event {
    State(StateEvent),
    Level(LoudnessEvent),
}
