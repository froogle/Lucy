//! The public socket: newline-delimited JSON-RPC 2.0 (ADR 0006).

use lucy_ipc::{
    error_code, Event, Hello, HelloResult, SessionStarted, Status, Subscribe, Topic, PROTOCOL,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::runner::Command;
use crate::status::StatusBus;

pub async fn serve(
    listener: UnixListener,
    status: StatusBus,
    sessions: mpsc::UnboundedSender<Command>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(stream, status.clone(), sessions.clone()));
            }
            Err(e) => eprintln!("lucyd: accept failed: {e}"),
        }
    }
}

struct Connection {
    writer: OwnedWriteHalf,
    status: StatusBus,
    sessions: mpsc::UnboundedSender<Command>,
    greeted: bool,
    state_subscribed: bool,
    /// Every state change since subscribing, once subscribed.
    state_events: Option<mpsc::UnboundedReceiver<Status>>,
}

async fn connection(
    stream: UnixStream,
    status: StatusBus,
    sessions: mpsc::UnboundedSender<Command>,
) {
    let (reader, writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut conn = Connection {
        writer,
        status,
        sessions,
        greeted: false,
        state_subscribed: false,
        state_events: None,
    };
    loop {
        let sent = tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => conn.handle_line(&line).await,
                _ => return,
            },
            Some(status) = next_state(&mut conn.state_events) => conn.send_state(status).await,
        };
        if sent.is_err() {
            return;
        }
    }
}

/// The next state change, or never if not subscribed.
async fn next_state(state_events: &mut Option<mpsc::UnboundedReceiver<Status>>) -> Option<Status> {
    match state_events {
        Some(state_events) => state_events.recv().await,
        None => std::future::pending().await,
    }
}

/// A JSON-RPC error to send back.
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
        }
    }
}

impl Connection {
    async fn handle_line(&mut self, line: &str) -> std::io::Result<()> {
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let error = RpcError::new(error_code::PARSE_ERROR, format!("parse error: {e}"));
                return self.respond(Value::Null, Err(error)).await;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            let error = RpcError::new(error_code::INVALID_REQUEST, "request has no method");
            return self.respond(id, Err(error)).await;
        };
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let was_subscribed = self.state_subscribed;
        let outcome = self.dispatch(method, params).await;
        self.respond(id, outcome).await?;
        if self.state_subscribed && !was_subscribed {
            let (status, state_events) = self.status.subscribe();
            self.state_events = Some(state_events);
            self.send_state(status).await?;
        }
        Ok(())
    }

    async fn send_state(&mut self, status: Status) -> std::io::Result<()> {
        let event = Event::State(status);
        let mut message = json!(event);
        message["jsonrpc"] = json!("2.0");
        self.send(message).await
    }

    async fn dispatch(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        if method == "hello" {
            return self.hello(params);
        }
        if !self.greeted {
            return Err(RpcError::new(
                error_code::HELLO_REQUIRED,
                "send hello before any other request",
            ));
        }
        match method {
            "status" => Ok(json!(self.status.snapshot())),
            "subscribe" => {
                let subscribe: Subscribe = parse_params(params)?;
                if subscribe.topics.contains(&Topic::State) {
                    self.state_subscribed = true;
                }
                Ok(json!({}))
            }
            "session.start" => match self.ask(Command::Start).await? {
                Ok(session_id) => Ok(json!(SessionStarted {
                    session_id: session_id.to_string()
                })),
                Err(_) => Err(RpcError::new(
                    error_code::SESSION_OPEN,
                    "a Session is already open",
                )),
            },
            "session.end" => match self.ask(Command::End).await? {
                Ok(()) => Ok(json!({})),
                Err(_) => Err(RpcError::new(error_code::NO_SESSION, "no Session is open")),
            },
            _ => Err(RpcError::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {method:?}"),
            )),
        }
    }

    /// Sends a command to the Session runner and waits for its reply.
    async fn ask<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, RpcError> {
        let (reply, answer) = oneshot::channel();
        let gone = || RpcError::new(error_code::INTERNAL_ERROR, "lucyd's Session runner stopped");
        self.sessions.send(command(reply)).map_err(|_| gone())?;
        answer.await.map_err(|_| gone())
    }

    fn hello(&mut self, params: Value) -> Result<Value, RpcError> {
        let hello: Hello = parse_params(params)?;
        if hello.protocol != PROTOCOL {
            return Err(RpcError::new(
                error_code::PROTOCOL_MISMATCH,
                format!(
                    "protocol mismatch: lucyd speaks protocol {PROTOCOL} but {} speaks protocol {}. \
                     If Lucy was just upgraded, restart lucyd: systemctl --user restart lucyd",
                    hello.client, hello.protocol
                ),
            ));
        }
        self.greeted = true;
        Ok(json!(HelloResult {
            protocol: PROTOCOL,
            server: format!("lucyd {}", env!("CARGO_PKG_VERSION")),
        }))
    }

    async fn respond(
        &mut self,
        id: Value,
        outcome: Result<Value, RpcError>,
    ) -> std::io::Result<()> {
        let message = match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": e.code, "message": e.message},
            }),
        };
        self.send(message).await
    }

    async fn send(&mut self, message: Value) -> std::io::Result<()> {
        let mut line = message.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}
