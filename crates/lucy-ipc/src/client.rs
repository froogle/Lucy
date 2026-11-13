//! A small blocking client for lucyd's public socket.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::{
    ApprovalsList, Event, Hello, HelloResult, MicLevel, PluginsList, ProviderCheck,
    ProviderCheckResult, ProvidersStatus, RevokeApproval, Role, SessionStarted, SetPluginEnabled,
    Status, Subscribe, Topic, VoicePreview, WakeWordTest, WakeWordTestResult, PROTOCOL,
};

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// Notifications that arrived while waiting for a response.
    pending: VecDeque<String>,
}

impl Client {
    /// Connects and completes the `hello` handshake.
    pub fn connect(path: &Path, client: &str, role: Option<Role>) -> Result<Client, Error> {
        let stream = UnixStream::connect(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => Error::NotRunning,
            _ => Error::Io(e),
        })?;
        let mut this = Client {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next_id: 1,
            pending: VecDeque::new(),
        };
        let hello = Hello {
            protocol: PROTOCOL,
            client: client.to_owned(),
            role,
        };
        let _: HelloResult = this.call("hello", json!(hello))?;
        Ok(this)
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.writer.set_read_timeout(timeout)
    }

    pub fn status(&mut self) -> Result<Status, Error> {
        self.call("status", Value::Null)
    }

    pub fn subscribe(&mut self, topics: &[Topic]) -> Result<(), Error> {
        let _: Value = self.call(
            "subscribe",
            json!(Subscribe {
                topics: topics.to_vec()
            }),
        )?;
        Ok(())
    }

    /// Opens a Session and returns its id.
    pub fn start_session(&mut self) -> Result<String, Error> {
        let started: SessionStarted = self.call("session.start", Value::Null)?;
        Ok(started.session_id)
    }

    pub fn end_session(&mut self) -> Result<(), Error> {
        let _: Value = self.call("session.end", Value::Null)?;
        Ok(())
    }

    /// Each Thinking Provider in config, with its health.
    pub fn providers_status(&mut self) -> Result<ProvidersStatus, Error> {
        self.call("providers.status", Value::Null)
    }

    /// Runs a real "Reply OK" round trip through the Provider `name`.
    pub fn check_provider(&mut self, name: &str) -> Result<ProviderCheckResult, Error> {
        let params = ProviderCheck {
            name: name.to_owned(),
        };
        self.call("provider.check", json!(params))
    }

    /// Every installed Plugin.
    pub fn plugins(&mut self) -> Result<PluginsList, Error> {
        self.call("plugins.list", Value::Null)
    }

    /// Enables or disables the Plugin `name`, straight away.
    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) -> Result<(), Error> {
        let params = SetPluginEnabled {
            name: name.to_owned(),
            enabled,
        };
        let _: Value = self.call("plugins.set_enabled", json!(params))?;
        Ok(())
    }

    /// Every Approval Lucy holds.
    pub fn approvals(&mut self) -> Result<ApprovalsList, Error> {
        self.call("approvals.list", Value::Null)
    }

    /// Revokes the Approval of the Plugin `plugin`.
    pub fn revoke_approval(&mut self, plugin: &str) -> Result<(), Error> {
        let params = RevokeApproval {
            plugin: plugin.to_owned(),
        };
        let _: Value = self.call("approvals.revoke", json!(params))?;
        Ok(())
    }

    /// Has Lucy say `text` outside any Session, and waits until she has.
    pub fn voice_preview(&mut self, text: &str) -> Result<(), Error> {
        self.voice_preview_in(text, None)
    }

    /// Has Lucy say `text` in the Piper voice `voice` (or her running voice
    /// if `None`), and waits until she has.
    pub fn voice_preview_in(&mut self, text: &str, voice: Option<&str>) -> Result<(), Error> {
        let params = VoicePreview {
            text: text.to_owned(),
            voice: voice.map(str::to_owned),
        };
        let _: Value = self.call("test.voice_preview", json!(params))?;
        Ok(())
    }

    /// How loud the microphone `config.toml` names has been since last asked.
    pub fn mic_level(&mut self) -> Result<MicLevel, Error> {
        self.call("test.mic_level", Value::Null)
    }

    /// Listens for the Wake word for up to `timeout_secs`, opening no Session.
    pub fn test_wake_word(&mut self, timeout_secs: f64) -> Result<WakeWordTestResult, Error> {
        self.call("test.wake_word", json!(WakeWordTest { timeout_secs }))
    }

    /// The next event, exactly as lucyd sent it (one NDJSON line, no newline).
    pub fn next_event_line(&mut self) -> Result<String, Error> {
        match self.pending.pop_front() {
            Some(line) => Ok(line),
            None => self.read_line(),
        }
    }

    pub fn next_event(&mut self) -> Result<Event, Error> {
        let line = self.next_event_line()?;
        serde_json::from_str(&line).map_err(|e| Error::Protocol(format!("bad event: {e}")))
    }

    fn call<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if !params.is_null() {
            request["params"] = params;
        }
        let mut line = request.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;

        loop {
            let line = self.read_line()?;
            let message: Value = serde_json::from_str(&line)
                .map_err(|e| Error::Protocol(format!("bad message: {e}")))?;
            if message.get("id") != Some(&json!(id)) {
                self.pending.push_back(line);
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(Error::Rpc {
                    code: error["code"].as_i64().unwrap_or(0),
                    message: error["message"].as_str().unwrap_or("").to_owned(),
                });
            }
            let result = message.get("result").cloned().unwrap_or(Value::Null);
            return serde_json::from_value(result)
                .map_err(|e| Error::Protocol(format!("bad {method} result: {e}")));
        }
    }

    fn read_line(&mut self) -> Result<String, Error> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(Error::Closed);
        }
        Ok(line.trim_end_matches('\n').to_owned())
    }
}

#[derive(Debug)]
pub enum Error {
    /// Nothing is listening on the socket.
    NotRunning,
    /// lucyd closed the connection.
    Closed,
    /// lucyd answered with a JSON-RPC error.
    Rpc {
        code: i64,
        message: String,
    },
    /// lucyd sent something this client doesn't understand.
    Protocol(String),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotRunning => write!(f, "lucyd isn't running"),
            Error::Closed => write!(f, "lucyd closed the connection"),
            Error::Rpc { message, .. } => write!(f, "{message}"),
            Error::Protocol(message) => write!(f, "unexpected reply from lucyd: {message}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}
