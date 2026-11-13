//! The public socket: newline-delimited JSON-RPC 2.0 (ADR 0006).

use std::path::PathBuf;
use std::sync::Arc;

use lucy_config::{Config, Tier};
use lucy_ipc::{
    error_code, Event, Hello, HelloResult, LevelEvent, PluginsChanged, ProviderCheck,
    ProviderCheckResult, ProviderStatus, ProvidersStatus, RevokeApproval, Role, SessionStarted,
    SetPluginEnabled, Status, Subscribe, Topic, PROTOCOL,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::adapters::Thinking;
use crate::notifications::{IndicatorClaim, Indicators};
use crate::plugins::{self, Host};
use crate::runner::Command;
use crate::status::Buses;

/// What `providers.status` and `provider.check` need.
#[derive(Clone)]
pub struct Providers {
    pub thinking: Arc<dyn Thinking>,
    /// `config.toml`, read afresh for each request.
    pub config_path: PathBuf,
}

pub async fn serve(
    listener: UnixListener,
    buses: Buses,
    indicators: Indicators,
    sessions: mpsc::UnboundedSender<Command>,
    providers: Providers,
    plugins: Arc<Host>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(
                    stream,
                    buses.clone(),
                    indicators.clone(),
                    sessions.clone(),
                    providers.clone(),
                    plugins.clone(),
                ));
            }
            Err(e) => eprintln!("lucyd: accept failed: {e}"),
        }
    }
}

struct Connection {
    writer: OwnedWriteHalf,
    buses: Buses,
    indicators: Indicators,
    sessions: mpsc::UnboundedSender<Command>,
    providers: Providers,
    plugins: Arc<Host>,
    greeted: bool,
    /// Held while this client holds the `indicator` role.
    indicator: Option<IndicatorClaim>,
    state_subscribed: bool,
    /// Every state change since subscribing, once subscribed.
    state_events: Option<mpsc::UnboundedReceiver<Status>>,
    /// Playback levels, once subscribed.
    level_events: Option<broadcast::Receiver<f32>>,
    /// A queue of `plugins.changed`, once subscribed.
    plugin_events: Option<mpsc::UnboundedReceiver<()>>,
}

async fn connection(
    stream: UnixStream,
    buses: Buses,
    indicators: Indicators,
    sessions: mpsc::UnboundedSender<Command>,
    providers: Providers,
    plugins: Arc<Host>,
) {
    let (reader, writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut conn = Connection {
        writer,
        buses,
        indicators,
        sessions,
        providers,
        plugins,
        greeted: false,
        indicator: None,
        state_subscribed: false,
        state_events: None,
        level_events: None,
        plugin_events: None,
    };
    loop {
        let sent = tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => conn.handle_line(&line).await,
                _ => return,
            },
            Some(status) = next_state(&mut conn.state_events) => conn.send_state(status).await,
            Some(rms) = next_level(&mut conn.level_events) => {
                conn.send_event(Event::Level(LevelEvent { rms })).await
            }
            Some(()) = next_change(&mut conn.plugin_events) => {
                conn.send_event(Event::PluginsChanged(PluginsChanged {})).await
            }
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

/// The next playback level, or never if not subscribed. Levels a slow
/// subscriber missed are skipped.
async fn next_level(level_events: &mut Option<broadcast::Receiver<f32>>) -> Option<f32> {
    let Some(level_events) = level_events else {
        return std::future::pending().await;
    };
    loop {
        match level_events.recv().await {
            Ok(rms) => return Some(rms),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

/// The next change, or never if not subscribed.
async fn next_change(changes: &mut Option<mpsc::UnboundedReceiver<()>>) -> Option<()> {
    match changes {
        Some(changes) => changes.recv().await,
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
            let (status, state_events) = self.buses.status.subscribe();
            self.state_events = Some(state_events);
            self.send_state(status).await?;
        }
        Ok(())
    }

    async fn send_state(&mut self, status: Status) -> std::io::Result<()> {
        self.send_event(Event::State(status)).await
    }

    async fn send_event(&mut self, event: Event) -> std::io::Result<()> {
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
            "status" => Ok(json!(self.buses.status.snapshot())),
            "subscribe" => {
                let subscribe: Subscribe = parse_params(params)?;
                if subscribe.topics.contains(&Topic::State) {
                    self.state_subscribed = true;
                }
                if subscribe.topics.contains(&Topic::Level) && self.level_events.is_none() {
                    self.level_events = Some(self.buses.levels.subscribe());
                }
                if subscribe.topics.contains(&Topic::Plugins) && self.plugin_events.is_none() {
                    self.plugin_events = Some(self.buses.plugins.subscribe());
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
            "providers.status" => Ok(json!(self.providers_status().await?)),
            "provider.check" => Ok(json!(self.check_provider(parse_params(params)?).await?)),
            "plugins.list" => Ok(json!(self.plugins.list())),
            "plugins.set_enabled" => {
                let SetPluginEnabled { name, enabled } = parse_params(params)?;
                self.plugins
                    .set_enabled(&name, enabled)
                    .await
                    .map_err(plugin_error)?;
                Ok(json!({}))
            }
            "approvals.list" => Ok(json!(self.plugins.approval_list().map_err(plugin_error)?)),
            "approvals.revoke" => {
                let RevokeApproval { plugin } = parse_params(params)?;
                self.plugins.revoke(&plugin).await.map_err(plugin_error)?;
                Ok(json!({}))
            }
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

    /// Health checks for every Provider in config, with no model call.
    async fn providers_status(&self) -> Result<ProvidersStatus, RpcError> {
        let config = self.config()?;
        let thinking = self.providers.thinking.clone();
        let providers = blocking(move || {
            let tiers = |name: &str| {
                [Tier::Fast, Tier::Advanced]
                    .into_iter()
                    .filter(|tier| config.tiers.name(*tier) == name)
                    .map(Into::into)
                    .collect()
            };
            config
                .providers
                .iter()
                .map(|(name, provider)| ProviderStatus {
                    name: name.clone(),
                    kind: provider.kind.into(),
                    model: provider.model.clone(),
                    tiers: tiers(name),
                    problem: thinking.health(provider).err(),
                })
                .collect()
        })
        .await?;
        Ok(ProvidersStatus { providers })
    }

    /// A real "Reply OK" round trip through one Provider.
    async fn check_provider(&self, check: ProviderCheck) -> Result<ProviderCheckResult, RpcError> {
        let config = self.config()?;
        let Some(provider) = config.providers.get(&check.name).cloned() else {
            return Err(RpcError::new(
                error_code::UNKNOWN_PROVIDER,
                format!("config.toml has no Provider called {:?}", check.name),
            ));
        };
        let thinking = self.providers.thinking.clone();
        let problem = blocking(move || thinking.check(&provider).err()).await?;
        Ok(ProviderCheckResult { problem })
    }

    fn config(&self) -> Result<Config, RpcError> {
        lucy_config::load(&self.providers.config_path)
            .map_err(|e| RpcError::new(error_code::INVALID_CONFIG, e.to_string()))
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
        self.indicator = match hello.role {
            Some(Role::Indicator) => Some(self.indicators.claim()),
            None => None,
        };
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

/// Runs a blocking Provider call off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, RpcError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        RpcError::new(
            error_code::INTERNAL_ERROR,
            format!("Provider check failed: {e}"),
        )
    })
}

fn plugin_error(e: plugins::Refusal) -> RpcError {
    let code = match e {
        plugins::Refusal::NoSuchPlugin(_) => error_code::UNKNOWN_PLUGIN,
        plugins::Refusal::NotApproved(_) => error_code::NOT_APPROVED,
        plugins::Refusal::Other(_) => error_code::INTERNAL_ERROR,
    };
    RpcError::new(code, e.to_string())
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}
