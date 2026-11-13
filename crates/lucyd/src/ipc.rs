//! The public socket: newline-delimited JSON-RPC 2.0 (ADR 0006).

use lucy_ipc::{
    error_code, Event, Hello, HelloResult, StateEvent, Status, Subscribe, Topic, PROTOCOL,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

pub async fn serve(listener: UnixListener, status: watch::Receiver<Status>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(stream, status.clone()));
            }
            Err(e) => eprintln!("lucyd: accept failed: {e}"),
        }
    }
}

struct Connection {
    writer: OwnedWriteHalf,
    status: watch::Receiver<Status>,
    greeted: bool,
    state_subscribed: bool,
}

async fn connection(stream: UnixStream, status: watch::Receiver<Status>) {
    let (reader, writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut conn = Connection {
        writer,
        status,
        greeted: false,
        state_subscribed: false,
    };
    loop {
        let sent = tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => conn.handle_line(&line).await,
                _ => return,
            },
            changed = conn.status.changed(), if conn.state_subscribed => match changed {
                Ok(()) => conn.send_state().await,
                Err(_) => return,
            },
        };
        if sent.is_err() {
            return;
        }
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
        let outcome = self.dispatch(method, params);
        self.respond(id, outcome).await?;
        if self.state_subscribed && !was_subscribed {
            self.send_state().await?;
        }
        Ok(())
    }

    /// Sends the current state as a `state` event.
    async fn send_state(&mut self) -> std::io::Result<()> {
        let status = self.status.borrow_and_update().clone();
        let event = Event::State(StateEvent {
            state: status.state,
            session_id: status.session_id,
        });
        let mut message = json!(event);
        message["jsonrpc"] = json!("2.0");
        self.send(message).await
    }

    fn dispatch(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
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
            "status" => Ok(json!(*self.status.borrow())),
            "subscribe" => {
                let subscribe: Subscribe = parse_params(params)?;
                if subscribe.topics.contains(&Topic::State) {
                    self.state_subscribed = true;
                }
                Ok(json!({}))
            }
            _ => Err(RpcError::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {method:?}"),
            )),
        }
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
