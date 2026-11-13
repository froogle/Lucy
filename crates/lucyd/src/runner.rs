//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use lucy_config::Tier;
use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::adapters::{Adapters, SessionModel, ThinkingEvent};
use crate::mcp::{Caller, Relays};
use crate::session::{Input, Output, Session};
use crate::status::StatusBus;

/// A request from the public socket or the MCP server.
pub enum Command {
    Start(oneshot::Sender<Result<SessionId, StartError>>),
    /// End now, from any state.
    End(oneshot::Sender<Result<(), EndError>>),
    /// `end_session`: end once the reply has been spoken.
    EndAfterSpeech(oneshot::Sender<Result<(), EndError>>),
}

/// Names one Session to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(u64);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub struct StartError;
pub struct EndError;

pub struct Runner {
    commands: mpsc::UnboundedReceiver<Command>,
    status: StatusBus,
    adapters: Adapters,
    relays: Relays,
    config_path: PathBuf,
    /// False once the speech source has gone away.
    speech_open: bool,
    sessions_started: u64,
}

impl Runner {
    pub fn new(
        commands: mpsc::UnboundedReceiver<Command>,
        status: StatusBus,
        adapters: Adapters,
        relays: Relays,
        config_path: PathBuf,
    ) -> Self {
        Runner {
            commands,
            status,
            adapters,
            relays,
            config_path,
            speech_open: true,
            sessions_started: 0,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.commands.recv() => match command {
                    Some(Command::Start(reply)) => {
                        self.sessions_started += 1;
                        let id = SessionId(self.sessions_started);
                        self.run_session(id, reply).await;
                    }
                    Some(Command::End(reply) | Command::EndAfterSpeech(reply)) => {
                        let _ = reply.send(Err(EndError));
                    }
                    None => return,
                },
                // Speech outside a Session is ignored.
                speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = speech.is_some();
                }
            }
        }
    }

    /// Runs one Session until it ends. Status changes are published before
    /// replying, so a client sees them as soon as its request returns.
    async fn run_session(
        &mut self,
        id: SessionId,
        started: oneshot::Sender<Result<SessionId, StartError>>,
    ) {
        let (events_tx, mut events) = mpsc::unbounded_channel();
        // Revoked when the Session ends.
        let (relay, _token) = self.relays.issue(Caller::SessionModel);
        // Edits to the file apply from the next Session on.
        let config = lucy_config::load(&self.config_path);
        let provider = match &config {
            Ok(config) => config.provider(Tier::Fast),
            Err(e) => Err(format!(
                "I {}. Fix the file and wake me again.",
                e.to_string().lines().next().unwrap_or_default()
            )),
        };
        let mut model: Box<dyn SessionModel> = match provider {
            Ok(provider) => self.adapters.thinking.open(provider, events_tx, relay),
            Err(reason) => {
                eprintln!("lucyd: {reason}");
                let _ = events_tx.send(ThinkingEvent::Unavailable(reason));
                Box::new(Unopened { _events: events_tx })
            }
        };
        let silence_timeout = config.unwrap_or_default().session.silence_timeout_secs;
        let (played_tx, mut played) = mpsc::unbounded_channel();
        let mut speaker = self.adapters.tts.open(played_tx);
        let (transcripts_tx, mut transcripts) = mpsc::unbounded_channel();
        let mut session = Session::default();
        let mut silence = SilenceTimer::new(Duration::from_secs_f64(silence_timeout));
        silence.set_running(session.silence_timer_runs());

        self.publish(session.state(), Some(id));
        let _ = started.send(Ok(id));
        loop {
            let mut segment = None;
            let input = tokio::select! {
                command = self.commands.recv() => match command {
                    Some(Command::Start(reply)) => {
                        let _ = reply.send(Err(StartError));
                        continue;
                    }
                    Some(Command::End(reply)) => {
                        self.publish(State::Idle, None);
                        let _ = reply.send(Ok(()));
                        return;
                    }
                    Some(Command::EndAfterSpeech(reply)) => {
                        let _ = reply.send(Ok(()));
                        Input::EndRequested
                    }
                    None => return,
                },
                speech = self.adapters.speech.recv(), if self.speech_open => match speech {
                    Some(speech) => {
                        segment = Some(speech);
                        Input::Speech
                    }
                    None => {
                        self.speech_open = false;
                        continue;
                    }
                },
                Some(text) = transcripts.recv() => Input::Transcript(text),
                Some(event) = events.recv() => Input::Thinking(event),
                Some(()) = played.recv() => Input::Played,
                () = tokio::time::sleep_until(silence.deadline()), if silence.is_running() => {
                    self.publish(State::Idle, None);
                    return;
                }
            };
            for output in session.handle(input) {
                match output {
                    Output::Transcribe => {
                        let segment = segment.take().expect("Transcribe follows Speech");
                        let stt = self.adapters.stt.clone();
                        let transcripts = transcripts_tx.clone();
                        tokio::task::spawn_blocking(move || {
                            let _ = transcripts.send(stt.transcribe(segment));
                        });
                    }
                    Output::SendTurn(text) => model.send_turn(text),
                    Output::Speak(sentence) => speaker.speak(sentence),
                    Output::ResetSilenceTimer => silence.reset(),
                    Output::End => {
                        self.publish(State::Idle, None);
                        return;
                    }
                }
            }
            silence.set_running(session.silence_timer_runs());
            self.publish(session.state(), Some(id));
        }
    }

    fn publish(&self, state: State, session_id: Option<SessionId>) {
        self.status.publish(Status {
            state,
            session_id: session_id.map(|id| id.to_string()),
        });
    }
}

/// Stands in for the model when no Provider could be opened.
struct Unopened {
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl SessionModel for Unopened {
    fn send_turn(&mut self, _text: String) {}
}

/// A countdown that can be paused and resumed.
struct SilenceTimer {
    timeout: Duration,
    remaining: Duration,
    /// When it last resumed, while running.
    resumed: Option<Instant>,
}

impl SilenceTimer {
    fn new(timeout: Duration) -> Self {
        SilenceTimer {
            timeout,
            remaining: timeout,
            resumed: None,
        }
    }

    fn is_running(&self) -> bool {
        self.resumed.is_some()
    }

    fn set_running(&mut self, running: bool) {
        match (running, self.resumed) {
            (true, None) => self.resumed = Some(Instant::now()),
            (false, Some(resumed)) => {
                self.remaining = self.remaining.saturating_sub(resumed.elapsed());
                self.resumed = None;
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.remaining = self.timeout;
        if self.is_running() {
            self.resumed = Some(Instant::now());
        }
    }

    fn deadline(&self) -> Instant {
        self.resumed.unwrap_or_else(Instant::now) + self.remaining
    }
}
