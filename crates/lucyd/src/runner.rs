//! Runs Sessions: owns the published status and does the I/O that the
//! `session` state machine asks for.

mod start;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lucy_config::SttKind;
use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::adapters::{
    Adapters, Notification, RateLimitStatus, Sound, SpeakerEvent, SttError, ThinkingEvent,
};
use crate::audio::{BargeInEvent, Direction, Heard, Hearing};
use crate::limits;
use crate::mcp::Relays;
use crate::notifications;
use crate::persona;
use crate::plugins;
use crate::session::{Answer, Input, Output, Question, Session};
use crate::status::Buses;
use crate::wake;
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
    /// Session ends or the model process dies first, `reply` is dropped
    /// unanswered.
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

/// How long a Session opened by the Wake word waits for the first Turn.
const WAKE_TIMEOUT: Duration = Duration::from_secs(8);

/// How often lucyd tries to open the microphone again while it is lost
/// outside a Session.
const REOPEN_MIC_EVERY: Duration = Duration::from_secs(1);

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
    /// False once the wake-word source has gone away.
    wake_open: bool,
    /// False once the source of whether the user is speaking has gone away.
    speaking_open: bool,
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
            wake_open: true,
            speaking_open: true,
            sessions_started: 0,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                // The Wake word before speech: the utterance it starts
                // belongs to the Session it opens.
                biased;
                command = self.commands.recv() => match command {
                    Some(Command::Start(reply)) => self.run_session(Some(reply)).await,
                    Some(Command::End(reply) | Command::EndAfterSpeech(reply)) => {
                        let _ = reply.send(Err(EndError));
                    }
                    // Dropped unanswered: there is no one to ask.
                    Some(Command::Ask(_)) => {}
                    None => return,
                },
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => self.run_session(None).await,
                    None => self.wake_open = false,
                },
                // Speech outside a Session is ignored.
                speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = speech.is_some();
                }
                // So the Wake word is heard again once a microphone is back.
                () = mic_lost_for(&mut self.adapters.hearing, REOPEN_MIC_EVERY) => {
                    if let Some(hearing) = &mut self.adapters.hearing {
                        hearing.reopen();
                    }
                }
            }
        }
    }

    /// Runs one Session until it ends, then stops the Plugins it started.
    /// `started` hears that it opened; it is None when the Wake word opened
    /// it.
    async fn run_session(
        &mut self,
        started: Option<oneshot::Sender<Result<SessionId, StartError>>>,
    ) {
        self.sessions_started += 1;
        let id = SessionId(self.sessions_started);
        let plugin_tools = self.plugin_host.open_session().await;
        self.session_loop(id, started, plugin_tools.clone()).await;
        if let Some(hearing) = &mut self.adapters.hearing {
            hearing.lucy_speaking(false);
        }
        plugin_tools.stop().await;
    }

    /// Status changes are published before replying, so a client sees them
    /// as soon as its request returns.
    async fn session_loop(
        &mut self,
        id: SessionId,
        started: Option<oneshot::Sender<Result<SessionId, StartError>>>,
        plugin_tools: Arc<plugins::SessionTools>,
    ) {
        let woken = started.is_none();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        // The token is revoked when the Session ends.
        let start::Started {
            mut model,
            token: _token,
            settings,
            first_run,
            opened,
            devices,
        } = self.start(plugin_tools.clone(), events_tx.clone());
        let provider_name = opened.as_ref().map_or("the Thinking Provider", |opened| {
            opened.provider.kind.name()
        });
        // The model has died and been opened again since the last Turn ended.
        let mut respawned = false;
        let stt = match settings.stt.kind {
            SttKind::Whisper => self.adapters.stt.clone(),
            SttKind::Superwhisper => self.adapters.superwhisper.clone(),
        };
        let silence_timeout = settings.session.silence_timeout_secs;
        let wake_word = wake::word(&settings.wake_word);
        let confirmation = settings.confirmation;
        let (speaker_tx, mut speaker_events) = mpsc::unbounded_channel();
        let mut speaker = self.adapters.tts.open(&devices.speaker, speaker_tx);
        speaker.sound(Sound::Start);
        let (transcripts_tx, mut transcripts) =
            mpsc::unbounded_channel::<Result<String, SttError>>();
        let mut session = Session::new(confirmation.yes_phrases);
        session.barge_in_by_speech(devices.speech_barge_in);
        // An utterance may have started before the Session opened, as one
        // with the Wake word in it does.
        if *self.adapters.speaking.borrow_and_update() {
            session.handle(Input::Speaking(true));
        }
        let silence_timeout = Duration::from_secs_f64(silence_timeout);
        let mut silence = SilenceTimer::new(silence_timeout);
        if woken {
            // A false wake clears quickly.
            silence.first_timeout(WAKE_TIMEOUT.min(silence_timeout));
        }
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
        if let Some(started) = started {
            let _ = started.send(Ok(id));
        }
        loop {
            let mut segment = None;
            let input = if let Some(input) = opening.take() {
                input
            } else if let Some(ask) = answer_to.is_none().then(|| questions.pop_front()).flatten() {
                if let Some(shown) = ask.shown {
                    self.notify(shown);
                }
                answer_to = Some(ask.reply);
                answer_timer.reset();
                Input::Ask(ask.question)
            } else {
                tokio::select! {
                // What the model and the speaker report first: a Turn that
                // ended, or a sentence that finished playing, before a
                // request or a reply came in is seen to have ended first.
                biased;
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
                    SpeakerEvent::DeviceLost => {
                        self.end_on_lost_device(Direction::Output, id);
                        return;
                    }
                },
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
                // After `speech`: an utterance is cut before the user stops
                // speaking.
                changed = self.adapters.speaking.changed(), if self.speaking_open => match changed {
                    Ok(()) => Input::Speaking(*self.adapters.speaking.borrow_and_update()),
                    Err(_) => {
                        self.speaking_open = false;
                        continue;
                    }
                },
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => Input::WakeWord,
                    None => {
                        self.wake_open = false;
                        continue;
                    }
                },
                Some(transcript) = transcripts.recv() => match transcript {
                    Ok(text) => Input::Transcript(wake::strip(&text, &wake_word).to_owned()),
                    Err(SttError(e)) => {
                        eprintln!("lucyd: {e}");
                        Input::TranscribeFailed
                    }
                },
                heard = heard(&mut self.adapters.hearing) => match heard {
                    Heard::Lost => {
                        self.end_on_lost_device(Direction::Input, id);
                        return;
                    }
                    Heard::Gate(BargeInEvent::Duck) => {
                        speaker.duck(true);
                        continue;
                    }
                    Heard::Gate(BargeInEvent::Unduck) => {
                        speaker.duck(false);
                        continue;
                    }
                    Heard::Gate(BargeInEvent::BargeIn) => Input::BargeIn,
                },
                () = tokio::time::sleep_until(silence.deadline()), if silence.is_running() => {
                    speaker.sound(Sound::Closing);
                    self.publish(State::Idle, None);
                    return;
                }
                () = tokio::time::sleep_until(answer_timer.deadline()), if answer_timer.is_running() => {
                    Input::AnswerTimedOut
                }
                }
            };
            let input = match input {
                Input::Thinking(ThinkingEvent::Died) => match opened.as_ref() {
                    Some(opened) if !respawned => {
                        eprintln!("lucyd: the model process died; opening it again");
                        respawned = true;
                        model = self.open_model(opened, events_tx.clone());
                        if let Some(turn) = session.pending_turn() {
                            model.send_turn(turn.to_owned());
                        }
                        // Questions go unanswered rather than linger: the
                        // resent Turn asks again if it needs to.
                        questions.clear();
                        if answer_to.take().is_none() {
                            continue;
                        }
                        Input::Withdrawn
                    }
                    _ => {
                        eprintln!("lucyd: the model process died again");
                        self.notify(notifications::thinking_lost(provider_name));
                        let lost = format!("I've lost my connection to {provider_name}.");
                        Input::Thinking(ThinkingEvent::Unavailable(lost))
                    }
                },
                Input::Thinking(ThinkingEvent::RateLimit(limit))
                    if limit.status == RateLimitStatus::Rejected =>
                {
                    let said = limits::spoken(&limit, provider_name);
                    Input::Thinking(ThinkingEvent::Unavailable(said))
                }
                input => input,
            };
            if matches!(input, Input::Thinking(ThinkingEvent::TurnEnd)) {
                respawned = false;
            }
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
                self.notify(notifications::unspoken(&std::mem::take(&mut unspoken)));
            }
            for output in outputs {
                match output {
                    Output::Transcribe => {
                        let segment = segment.take().expect("Transcribe follows Speech");
                        let stt = stt.clone();
                        let transcripts = transcripts_tx.clone();
                        tokio::task::spawn_blocking(move || {
                            let _ = transcripts.send(stt.transcribe(segment));
                        });
                    }
                    Output::SendTurn(text) => {
                        // Confirmed calls that finished after a Barge-in
                        // cut the model off.
                        let mut finished = plugin_tools.take_finished();
                        finished.push(text);
                        model.send_turn(finished.join("\n"));
                    }
                    Output::Speak(sentence) => speaker.speak(sentence),
                    Output::ResetSilenceTimer => silence.reset(),
                    Output::Error => self.publish(State::Error, Some(id)),
                    Output::End => {
                        self.publish(State::Idle, None);
                        return;
                    }
                    Output::Answered(answer) => {
                        if let Some(reply) = answer_to.take() {
                            let _ = reply.send(answer);
                        }
                    }
                    Output::StopSpeaking => speaker.stop(),
                    Output::Interrupt => {
                        model.interrupt();
                        plugin_tools.barge_in();
                    }
                }
            }
            silence.set_running(session.silence_timer_runs());
            answer_timer.set_running(session.answer_timer_runs());
            // Before it is published, so the gate is open by the time anyone
            // sees Lucy speaking.
            if let Some(hearing) = &mut self.adapters.hearing {
                hearing.lucy_speaking(session.state() == State::Speaking);
            }
            self.publish(session.state(), Some(id));
        }
    }

    /// Ends the Session because a device went away.
    fn end_on_lost_device(&self, direction: Direction, id: SessionId) {
        self.notify(notifications::device_lost(direction));
        self.publish(State::Error, Some(id));
        self.publish(State::Idle, None);
    }

    /// Shows a notification that offers no action.
    fn notify(&self, notification: Notification) {
        let (actions, _) = mpsc::unbounded_channel();
        self.adapters.notifier.notify(notification, actions);
    }

    fn publish(&self, state: State, session_id: Option<SessionId>) {
        self.buses.status.publish(Status {
            state,
            session_id: session_id.map(|id| id.to_string()),
        });
    }
}

/// What the microphone reports next, if Lucy hears through one.
async fn heard(hearing: &mut Option<Hearing>) -> Heard {
    match hearing {
        Some(hearing) => hearing.next().await,
        None => std::future::pending().await,
    }
}

/// Resolves once the microphone has gone away, if Lucy hears through one.
async fn mic_lost(hearing: &mut Option<Hearing>) {
    match hearing {
        Some(hearing) => hearing.lost().await,
        None => std::future::pending().await,
    }
}

/// Resolves once the microphone has been gone for `wait`.
async fn mic_lost_for(hearing: &mut Option<Hearing>, wait: Duration) {
    mic_lost(hearing).await;
    tokio::time::sleep(wait).await;
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

    /// Counts down from `timeout` until the next reset, instead of the
    /// usual timeout.
    fn first_timeout(&mut self, timeout: Duration) {
        self.remaining = timeout;
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
