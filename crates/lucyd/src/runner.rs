//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

mod start;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::adapters::{Adapters, Notification, SpeakerEvent, ThinkingEvent};
use crate::mcp::Relays;
use crate::notifications;
use crate::persona;
use crate::plugins;
use crate::session::{Answer, Input, Output, Question, Session};
use crate::status::Buses;
pub use start::SessionFiles;

/// A request from the public socket or the MCP server.
pub enum Command {
    Start(oneshot::Sender<Result<SessionId, StartError>>),
    /// End now, from any state.
    End(oneshot::Sender<Result<(), EndError>>),
    /// `end_session`: end once the reply has been spoken.
    EndAfterSpeech(oneshot::Sender<Result<(), EndError>>),
    /// Ask the user something and hear their answer (Confirming). Questions
    /// are asked one at a time, in order. With no open Session, or if the
    /// Session ends first, `reply` is dropped unanswered.
    Ask(Ask),
}

pub struct Ask {
    pub question: Question,
    /// Shown when the question is asked: details too long to say.
    pub shown: Option<Notification>,
    pub reply: oneshot::Sender<Answer>,
}

/// Asks the user `question` in the open Session, showing `shown` as it is
/// asked. `None` if it went unanswered because no Session was open or it
/// ended first.
pub async fn ask(
    commands: &mpsc::UnboundedSender<Command>,
    question: Question,
    shown: Option<Notification>,
) -> Option<Answer> {
    let (reply, answer) = oneshot::channel();
    let _ = commands.send(Command::Ask(Ask {
        question,
        shown,
        reply,
    }));
    answer.await.ok()
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
    buses: Buses,
    adapters: Adapters,
    relays: Relays,
    plugin_host: Arc<plugins::Host>,
    files: SessionFiles,
    /// False once the speech source has gone away.
    speech_open: bool,
    sessions_started: u64,
}

impl Runner {
    pub fn new(
        commands: mpsc::UnboundedReceiver<Command>,
        buses: Buses,
        adapters: Adapters,
        relays: Relays,
        plugin_host: Arc<plugins::Host>,
        files: SessionFiles,
    ) -> Self {
        Runner {
            commands,
            buses,
            adapters,
            relays,
            plugin_host,
            files,
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
                    // Dropped unanswered: there is no one to ask.
                    Some(Command::Ask(_)) => {}
                    None => return,
                },
                // Speech outside a Session is ignored.
                speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = speech.is_some();
                }
            }
        }
    }

    /// Runs one Session until it ends, then stops the Plugins it started.
    async fn run_session(
        &mut self,
        id: SessionId,
        started: oneshot::Sender<Result<SessionId, StartError>>,
    ) {
        let plugin_tools = self.plugin_host.open_session().await;
        self.session_loop(id, started, plugin_tools.clone()).await;
        plugin_tools.stop().await;
    }

    /// Status changes are published before replying, so a client sees them
    /// as soon as its request returns.
    async fn session_loop(
        &mut self,
        id: SessionId,
        started: oneshot::Sender<Result<SessionId, StartError>>,
        plugin_tools: Arc<plugins::SessionTools>,
    ) {
        let (events_tx, mut events) = mpsc::unbounded_channel();
        // The token is revoked when the Session ends.
        let start::Started {
            mut model,
            token: _token,
            settings,
            first_run,
        } = self.start(plugin_tools, events_tx);
        let silence_timeout = settings.session.silence_timeout_secs;
        let confirmation = settings.confirmation;
        let (speaker_tx, mut speaker_events) = mpsc::unbounded_channel();
        let mut speaker = self.adapters.tts.open(speaker_tx);
        let (transcripts_tx, mut transcripts) = mpsc::unbounded_channel();
        let mut session = Session::new(confirmation.yes_phrases);
        let mut silence = SilenceTimer::new(Duration::from_secs_f64(silence_timeout));
        silence.set_running(session.silence_timer_runs());
        // Questions waiting to be asked, and where the answer to the one
        // being asked goes.
        let mut questions: VecDeque<Ask> = VecDeque::new();
        let mut answer_to = None;
        let mut answer_timer =
            SilenceTimer::new(Duration::from_secs_f64(confirmation.timeout_secs));

        // Sent before anything the user says. The first run lasts until the
        // introduction's Turn has ended, so a Provider that can't be used
        // doesn't use it up.
        let mut introducing = first_run;
        // Sentences of the current reply that TTS couldn't say.
        let mut unspoken = Vec::new();
        let mut opening = introducing.then(|| Input::Opening(persona::INTRODUCTION.to_owned()));

        self.publish(session.state(), Some(id));
        let _ = started.send(Ok(id));
        loop {
            let mut segment = None;
            let input = if let Some(input) = opening.take() {
                input
            } else if let Some(ask) = answer_to.is_none().then(|| questions.pop_front()).flatten() {
                if let Some(shown) = ask.shown {
                    // It offers no action, so nothing listens for one.
                    let (actions, _) = mpsc::unbounded_channel();
                    self.adapters.notifier.notify(shown, actions);
                }
                answer_to = Some(ask.reply);
                answer_timer.reset();
                Input::Ask(ask.question)
            } else {
                tokio::select! {
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
                    Some(Command::Ask(ask)) => {
                        questions.push_back(ask);
                        continue;
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
                Some(event) = speaker_events.recv() => match event {
                    SpeakerEvent::Played => Input::Played,
                    // Lucy can't say it, so she shows it.
                    SpeakerEvent::Failed(sentence) => {
                        unspoken.push(sentence);
                        Input::Played
                    }
                    SpeakerEvent::Level(rms) => {
                        self.buses.levels.publish(rms);
                        continue;
                    }
                },
                () = tokio::time::sleep_until(silence.deadline()), if silence.is_running() => {
                    self.publish(State::Idle, None);
                    return;
                }
                () = tokio::time::sleep_until(answer_timer.deadline()), if answer_timer.is_running() => {
                    Input::AnswerTimedOut
                }
                }
            };
            if introducing && matches!(input, Input::Thinking(ThinkingEvent::TurnEnd)) {
                introducing = false;
                if let Err(e) = self.files.persona.introduced() {
                    let path = self.files.persona.path().display();
                    eprintln!("lucyd: can't save {path}: {e}");
                }
            }
            let outputs = session.handle(input);
            // Once the reply is over, one notification shows what of it
            // couldn't be said.
            if !unspoken.is_empty() && session.state() == State::Listening {
                let notification = notifications::unspoken(&std::mem::take(&mut unspoken));
                // It offers no action, so nothing listens for one.
                let (actions, _) = mpsc::unbounded_channel();
                self.adapters.notifier.notify(notification, actions);
            }
            for output in outputs {
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
                    Output::Answered(answer) => {
                        if let Some(reply) = answer_to.take() {
                            let _ = reply.send(answer);
                        }
                    }
                }
            }
            silence.set_running(session.silence_timer_runs());
            answer_timer.set_running(session.answer_timer_runs());
            self.publish(session.state(), Some(id));
        }
    }

    fn publish(&self, state: State, session_id: Option<SessionId>) {
        self.buses.status.publish(Status {
            state,
            session_id: session_id.map(|id| id.to_string()),
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
