//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot, watch};

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
    sessions_started: u64,
}

impl Runner {
    pub fn new(commands: mpsc::UnboundedReceiver<Command>, status: watch::Sender<Status>) -> Self {
        Runner {
            commands,
            status,
            sessions_started: 0,
        }
    }

    pub async fn run(mut self) {
        while let Some(command) = self.commands.recv().await {
            match command {
                Command::Start(reply) => {
                    self.sessions_started += 1;
                    let id = self.sessions_started.to_string();
                    self.run_session(id, reply).await;
                }
                Command::End(reply) => {
                    let _ = reply.send(Err(EndError));
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
        self.publish(State::Listening, Some(&id));
        let _ = started.send(Ok(id.clone()));
        while let Some(command) = self.commands.recv().await {
            match command {
                Command::Start(reply) => {
                    let _ = reply.send(Err(StartError));
                }
                Command::End(reply) => {
                    self.publish(State::Idle, None);
                    let _ = reply.send(Ok(()));
                    return;
                }
            }
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
