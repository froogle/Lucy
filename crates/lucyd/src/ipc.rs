use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lucy_config::{Config, Tier};
use lucy_ipc::{
    error_code, Event, Hello, HelloResult, InputAnswer, InputCancel, LevelEvent, MicLevel,
    PluginsChanged, ProviderCheck, ProviderCheckResult, ProviderStatus, ProvidersStatus,
    RevokeApproval, Role, SessionStarted, SetPluginEnabled, Status, Subscribe, Topic, VoicePreview,
    WakeWordTest, WakeWordTestResult, PROTOCOL,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::adapters::Thinking;
use crate::input::{self, InputClaim};
use crate::notifications::{IndicatorClaim, Indicators};
use crate::plugins::{self, Host};
use crate::runner::{self, Command, PreviewError, SetupTest, SetupTestError, WakeWordHeard};
use crate::status::Buses;
use crate::sync::or_pending;

#[derive(Clone)]
pub struct Providers {
    pub thinking: Arc<dyn Thinking>,
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
    indicator: Option<IndicatorClaim>,
    input: Option<InputClaim>,
    state_subscribed: bool,
    state_events: Option<mpsc::UnboundedReceiver<Status>>,
    level_events: Option<broadcast::Receiver<f32>>,
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
        input: None,
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
            Some(()) = or_pending(conn.plugin_events.as_mut().map(|changes| changes.recv())) => {
                conn.send_event(Event::PluginsChanged(PluginsChanged {})).await
            }
            Some(event) = or_pending(conn.input.as_mut().map(|input| input.events.recv())) => conn.send_event(event).await,
        };
        if sent.is_err() {
            return;
        }
    }
}

async fn next_state(state_events: &mut Option<mpsc::UnboundedReceiver<Status>>) -> Option<Status> {
    match state_events {
        Some(state_events) => state_events.recv().await,
        None => std::future::pending().await,
    }
}

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
            "test.voice_preview" => {
                let VoicePreview { text, kind, voice } = parse_params(params)?;
                let kind = kind.map(|kind| match kind {
                    lucy_ipc::TtsKind::Piper => lucy_config::TtsKind::Piper,
                    lucy_ipc::TtsKind::Kokoro => lucy_config::TtsKind::Kokoro,
                });
                self.voice_preview(text, kind, voice).await?;
                Ok(json!({}))
            }
            "test.mic_level" => {
                let rms = self.setup_test(SetupTest::MicLevel).await?;
                let quiet = crate::audio::too_quiet(rms);
                Ok(json!(MicLevel { rms, quiet }))
            }
            "test.wake_word" => {
                let WakeWordTest { timeout_secs } = parse_params(params)?;
                let timeout = Duration::try_from_secs_f64(timeout_secs).map_err(|_| {
                    RpcError::new(error_code::INVALID_PARAMS, "timeout_secs must be 0 or more")
                })?;
                let WakeWordHeard { heard, score } = self
                    .setup_test(|reply| SetupTest::WakeWord { timeout, reply })
                    .await?;
                Ok(json!(WakeWordTestResult { heard, score }))
            }
            "input.answer" => {
                let InputAnswer { id, values } = parse_params(params)?;
                self.input_holder()?
                    .answer(&id, values)
                    .map_err(dialog_error)?;
                Ok(json!({}))
            }
            "input.cancel" => {
                let InputCancel { id } = parse_params(params)?;
                self.input_holder()?.cancel(&id).map_err(dialog_error)?;
                Ok(json!({}))
            }
            _ => Err(RpcError::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {method:?}"),
            )),
        }
    }

    fn input_holder(&self) -> Result<&InputClaim, RpcError> {
        self.input.as_ref().ok_or_else(|| {
            RpcError::new(
                error_code::NOT_INPUT_HOLDER,
                "only the client holding the input role answers the Input dialog",
            )
        })
    }

    async fn ask<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, RpcError> {
        let (reply, answer) = oneshot::channel();
        let gone = || RpcError::new(error_code::INTERNAL_ERROR, "lucyd's Session runner stopped");
        self.sessions.send(command(reply)).map_err(|_| gone())?;
        answer.await.map_err(|_| gone())
    }

    async fn voice_preview(
        &self,
        text: String,
        kind: Option<lucy_config::TtsKind>,
        voice: Option<String>,
    ) -> Result<(), RpcError> {
        if text.trim().is_empty() {
            return Err(RpcError::new(
                error_code::INVALID_PARAMS,
                "there is nothing to say: the text is empty",
            ));
        }
        let outcome = self
            .ask(|reply| {
                Command::VoicePreview(runner::VoicePreview {
                    text,
                    kind,
                    voice,
                    reply,
                })
            })
            .await?;
        outcome.map_err(|e| match e {
            PreviewError::SessionOpen => RpcError::new(
                error_code::SESSION_OPEN,
                "a Session is open; end it before a Voice preview",
            ),
            PreviewError::Busy => {
                RpcError::new(error_code::BUSY, "another Voice preview is playing")
            }
            PreviewError::Interrupted => RpcError::new(
                error_code::INTERRUPTED,
                "interrupted: a Session opened and cut the Voice preview short",
            ),
            PreviewError::Failed(reason) => RpcError::new(
                error_code::TTS_FAILED,
                format!("Lucy couldn't say it: {reason}"),
            ),
        })
    }

    async fn setup_test<T>(
        &self,
        test: impl FnOnce(oneshot::Sender<Result<T, SetupTestError>>) -> SetupTest,
    ) -> Result<T, RpcError> {
        let outcome = self.ask(|reply| Command::SetupTest(test(reply))).await?;
        outcome.map_err(|e| match e {
            SetupTestError::SessionOpen => RpcError::new(
                error_code::SESSION_OPEN,
                "a Session is open; end it before testing",
            ),
            SetupTestError::Busy => RpcError::new(
                error_code::BUSY,
                "another setup test or Voice preview is running",
            ),
            SetupTestError::NoMicrophone => RpcError::new(
                error_code::NO_MICROPHONE,
                "lucyd has no microphone open, so Lucy can't hear; its log says why",
            ),
            SetupTestError::WakeWordOff => RpcError::new(
                error_code::WAKE_WORD_OFF,
                "the Wake word is off: lucyd couldn't load its model; its log says why",
            ),
            SetupTestError::Interrupted => RpcError::new(
                error_code::INTERRUPTED,
                "interrupted: a Session opened and cut the test short",
            ),
        })
    }

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

    async fn check_provider(&self, check: ProviderCheck) -> Result<ProviderCheckResult, RpcError> {
        let config = self.config()?;
        let Some(provider) = config.providers.get(&check.name).cloned() else {
            return Err(RpcError::new(
                error_code::UNKNOWN_PROVIDER,
                format!("config.toml has no Provider called {:?}", check.name),
            ));
        };
        let thinking = self.providers.thinking.clone();
        let (problem, program) = blocking(move || {
            let program = thinking.executable(&provider);
            (thinking.check(&provider).err(), program)
        })
        .await?;
        let program = program.map(|program| program.display().to_string());
        Ok(ProviderCheckResult { problem, program })
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
        match hello.role {
            Some(Role::Indicator) => self.indicator = Some(self.indicators.claim()),
            Some(Role::Input) => {
                let claim = self.buses.dialogs.claim().ok_or_else(|| {
                    RpcError::new(
                        error_code::ROLE_TAKEN,
                        "another client holds the input role and draws the Input dialog",
                    )
                })?;
                self.input = Some(claim);
            }
            None => {}
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
        _ => error_code::INTERNAL_ERROR,
    };
    RpcError::new(code, e.to_string())
}

fn dialog_error(e: input::Refusal) -> RpcError {
    match e {
        input::Refusal::NoDialog => RpcError::new(
            error_code::NO_DIALOG,
            "no Input dialog with that id is open; Lucy may have closed it",
        ),
        input::Refusal::WrongFields => RpcError::new(
            error_code::INVALID_PARAMS,
            "values must hold a value for each of the dialog's fields, and nothing else",
        ),
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}
