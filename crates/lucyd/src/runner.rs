//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

use std::time::Duration;

use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::adapters::Adapters;
use crate::mcp::{Caller, Relays};
use crate::session::{Input, Output, Session};

/// A request from the public socket or the MCP server.
pub enum Command {
    Start(oneshot::Sender<Result<String, StartError>>),
    /// End now, from any state.
    End(oneshot::Sender<Result<(), EndError>>),
    /// `end_session`: end once the reply has been spoken.
    EndAfterSpeech(oneshot::Sender<Result<(), EndError>>),
}

pub struct StartError;
pub struct EndError;

pub struct Runner {
    commands: mpsc::UnboundedReceiver<Command>,
    status: watch::Sender<Status>,
    adapters: Adapters,
    relays: Relays,
    silence_timeout: Duration,
    /// False once the speech source has gone away.
    speech_open: bool,
    sessions_started: u64,
}

impl Runner {
    pub fn new(
        commands: mpsc::UnboundedReceiver<Command>,
        status: watch::Sender<Status>,
        adapters: Adapters,
        relays: Relays,
        silence_timeout: Duration,
    ) -> Self {
        Runner {
            commands,
            status,
            adapters,
            relays,
            silence_timeout,
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
                        let id = self.sessions_started.to_string();
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
        id: String,
        started: oneshot::Sender<Result<String, StartError>>,
    ) {
        let (events_tx, mut events) = mpsc::unbounded_channel();
        // Revoked when the Session ends.
        let (relay, _token) = self.relays.issue(Caller::SessionModel);
        let mut conversation = self.adapters.thinking.open(events_tx, relay);
        let (played_tx, mut played) = mpsc::unbounded_channel();
        let mut speaker = self.adapters.tts.open(played_tx);
        let (transcripts_tx, mut transcripts) = mpsc::unbounded_channel();
        let mut session = Session::default();
        let mut silence = SilenceTimer::new(self.silence_timeout);
        silence.set_running(session.silence_timer_runs());

        self.publish(session.state(), Some(&id));
        let _ = started.send(Ok(id.clone()));
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
                    Output::SendTurn(text) => conversation.send_turn(text),
                    Output::Speak(sentence) => speaker.speak(sentence),
                    Output::ResetSilenceTimer => silence.reset(),
                    Output::End => {
                        self.publish(State::Idle, None);
                        return;
                    }
                }
            }
            silence.set_running(session.silence_timer_runs());
            self.publish(session.state(), Some(&id));
        }
    }

    fn publish(&self, state: State, session_id: Option<&str>) {
        self.status.send_if_modified(|status| {
            let new = Status {
                state,
                session_id: session_id.map(str::to_owned),
            };
            let changed = *status != new;
            *status = new;
            changed
        });
    }
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
