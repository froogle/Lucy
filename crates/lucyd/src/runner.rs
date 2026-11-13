//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot, watch};

use crate::adapters::Adapters;
use crate::session::{Input, Output, Session};

/// A request from the public socket.
pub enum Command {
    Start(oneshot::Sender<Result<String, StartError>>),
    End(oneshot::Sender<Result<(), EndError>>),
}

pub struct StartError;
pub struct EndError;

pub struct Runner {
    commands: mpsc::UnboundedReceiver<Command>,
    status: watch::Sender<Status>,
    adapters: Adapters,
    /// False once the speech source has gone away.
    speech_open: bool,
    sessions_started: u64,
}

impl Runner {
    pub fn new(
        commands: mpsc::UnboundedReceiver<Command>,
        status: watch::Sender<Status>,
        adapters: Adapters,
    ) -> Self {
        Runner {
            commands,
            status,
            adapters,
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
                    Some(Command::End(reply)) => {
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
        let mut conversation = self.adapters.thinking.open(events_tx);
        let (played_tx, mut played) = mpsc::unbounded_channel();
        let mut speaker = self.adapters.tts.open(played_tx);
        let (transcripts_tx, mut transcripts) = mpsc::unbounded_channel();
        let mut session = Session::default();

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
                    Output::ResetSilenceTimer => {}
                }
            }
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
