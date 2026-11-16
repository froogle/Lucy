mod preview;
mod setup;
mod start;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lucy_config::SttKind;
use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::adapters::{
    Adapters, Clock, Notification, RateLimitStatus, Sound, SpeakerEvent, SttError, ThinkingEvent,
    Timer,
};
use crate::audio::{BargeInEvent, Direction, Heard, Hearing};
use crate::authoring::{Authoring, Building};
use crate::input::{Dialog, OpenDialog, Typed};
use crate::limits;
use crate::mcp::Relays;
use crate::notifications;
use crate::persona;
use crate::plugins;
use crate::session::{Answer, Expects, Input, Output, Question, Session};
use crate::status::Buses;
use crate::wake;
pub use preview::{PreviewError, VoicePreview};
pub use setup::{SetupTest, SetupTestError, WakeWordHeard};
pub use start::SessionFiles;

pub enum Command {
    Start(oneshot::Sender<Result<SessionId, StartError>>),
    End(oneshot::Sender<Result<(), EndError>>),
    EndAfterSpeech(oneshot::Sender<Result<(), EndError>>),
    Ask(Ask),
    Say(String),
    TurnOrNextSession(String, Notification),
    VoicePreview(VoicePreview),
    SetupTest(SetupTest),
}

pub struct Ask {
    pub question: Question,
    pub shown: Option<Notification>,
    pub reply: oneshot::Sender<Answer>,
    pub when: When,
}

pub enum When {
    Now,
    QuietOrNextSession(Notification),
    Kept,
}

impl When {
    pub fn keeps(&self) -> bool {
        matches!(self, When::QuietOrNextSession(_) | When::Kept)
    }
}

pub async fn ask(
    commands: &mpsc::UnboundedSender<Command>,
    question: Question,
    shown: Option<Notification>,
) -> Option<Answer> {
    ask_at(commands, question, shown, When::Now).await
}

pub fn ask_when_quiet_or_next_session(
    commands: &mpsc::UnboundedSender<Command>,
    question: Question,
    shown: Option<Notification>,
    kept: Notification,
) -> impl std::future::Future<Output = Option<Answer>> {
    ask_at(commands, question, shown, When::QuietOrNextSession(kept))
}

pub fn ask_at(
    commands: &mpsc::UnboundedSender<Command>,
    question: Question,
    shown: Option<Notification>,
    when: When,
) -> impl std::future::Future<Output = Option<Answer>> {
    let (reply, answer) = oneshot::channel();
    let _ = commands.send(Command::Ask(Ask {
        question,
        shown,
        reply,
        when,
    }));
    async move { answer.await.ok() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(u64);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

const FALSE_WAKE_TIMEOUT: Duration = Duration::from_secs(8);

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
    speech_open: bool,
    wake_open: bool,
    speaking_open: bool,
    sessions_started: u64,
    building: watch::Receiver<Building>,
    authoring: Arc<Authoring>,
    parked: VecDeque<Parked>,
}

enum Parked {
    Ask(Ask),
    Turn(String, When),
}

impl Parked {
    fn when(&mut self) -> &mut When {
        match self {
            Parked::Ask(ask) => &mut ask.when,
            Parked::Turn(_, when) => when,
        }
    }
}

impl Runner {
    pub fn new(
        commands: mpsc::UnboundedReceiver<Command>,
        buses: Buses,
        adapters: Adapters,
        relays: Relays,
        plugin_host: Arc<plugins::Host>,
        files: SessionFiles,
        authoring: Arc<Authoring>,
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
            building: authoring.building(),
            authoring,
            parked: VecDeque::new(),
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                biased;
                command = self.commands.recv() => match command.map(|c| self.outside_session(c)) {
                    Some(Some(Command::Start(reply))) => self.run_session(Some(reply)).await,
                    Some(Some(Command::VoicePreview(preview))) => self.preview(preview).await,
                    Some(Some(Command::SetupTest(test))) => self.setup_test(test).await,
                    Some(_) => {}
                    None => return,
                },
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => self.run_session(None).await,
                    None => self.wake_open = false,
                },
                ignored_speech = self.adapters.speech.recv(), if self.speech_open => {
                    self.speech_open = ignored_speech.is_some();
                }
                Ok(()) = self.building.changed() => self.show_building_mark(),
                () = mic_lost_for(&mut self.adapters.hearing, REOPEN_MIC_EVERY) => self.reopen_mic(),
            }
        }
    }

    fn outside_session(&mut self, command: Command) -> Option<Command> {
        match command {
            Command::End(reply) | Command::EndAfterSpeech(reply) => {
                let _ = reply.send(Err(EndError));
            }
            Command::Ask(ask) if ask.when.keeps() => self.keep(Parked::Ask(ask)),
            Command::Ask(unanswerable) => {
                self.notify(notifications::unasked(&unanswerable.question.prompt));
            }
            Command::TurnOrNextSession(text, kept) => {
                self.keep(Parked::Turn(text, When::QuietOrNextSession(kept)));
            }
            Command::Say(_aside_without_a_turn) => {}
            command => return Some(command),
        }
        None
    }

    fn while_busy(
        &mut self,
        command: Command,
    ) -> Option<oneshot::Sender<Result<SessionId, StartError>>> {
        match self.outside_session(command)? {
            Command::Start(started) => return Some(started),
            Command::VoicePreview(preview) => {
                let _ = preview.reply.send(Err(PreviewError::Busy));
            }
            Command::SetupTest(test) => test.refuse(SetupTestError::Busy),
            _ => {}
        }
        None
    }

    async fn run_session(
        &mut self,
        started: Option<oneshot::Sender<Result<SessionId, StartError>>>,
    ) {
        self.sessions_started += 1;
        let id = SessionId(self.sessions_started);
        let plugin_tools = self.plugin_host.open_session().await;
        self.session_loop(id, started, plugin_tools.clone()).await;
        self.keep_unsaid_for_next_session();
        if let Some(hearing) = &mut self.adapters.hearing {
            hearing.lucy_speaking(false);
        }
        plugin_tools.stop().await;
    }

    fn keep_unsaid_for_next_session(&mut self) {
        for mut parked in std::mem::take(&mut self.parked) {
            if parked.when().keeps() {
                self.keep(parked);
            }
        }
    }

    fn show_building_mark(&self) {
        self.publish(State::Idle, None);
    }

    fn reopen_mic(&mut self) {
        if let Some(hearing) = &mut self.adapters.hearing {
            hearing.reopen();
        }
    }

    async fn session_loop(
        &mut self,
        id: SessionId,
        started: Option<oneshot::Sender<Result<SessionId, StartError>>>,
        plugin_tools: Arc<plugins::SessionTools>,
    ) {
        let woken = started.is_none();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let start::Started {
            mut model,
            token: _revoked_when_the_session_ends,
            settings,
            first_run,
            opened,
            devices,
        } = self.start(plugin_tools.clone(), events_tx.clone());
        let provider_name = opened.as_ref().map_or("the Thinking Provider", |opened| {
            opened.provider.kind.name()
        });
        let mut respawned_since_turn_end = false;
        let stt = match settings.stt.kind {
            SttKind::Whisper => self.adapters.stt.clone(),
            SttKind::Superwhisper => self.adapters.superwhisper.clone(),
        };
        let silence_timeout = settings.session.silence_timeout_secs;
        let wake_word = settings.wake_word.word();
        let confirmation = settings.confirmation;
        let typing_timeout = Duration::from_secs_f64(settings.input.timeout_secs);
        let (speaker_tx, mut speaker_events) = mpsc::unbounded_channel();
        let mut speaker =
            self.adapters
                .tts
                .open(&devices.speaker, &settings.piper.voice, speaker_tx);
        speaker.sound(Sound::Start);
        let (transcripts_tx, mut transcripts) =
            mpsc::unbounded_channel::<Result<String, SttError>>();
        let mut session = Session::new(confirmation.yes_phrases);
        session.hear_cancel_phrases(settings.input.cancel_phrases);
        session.barge_in_by_speech(devices.speech_barge_in);
        let speaking_before_open = *self.adapters.speaking.borrow_and_update();
        if speaking_before_open {
            session.handle(Input::Speaking(true));
        }
        session.handle(Input::Building(*self.building.borrow_and_update()));
        let silence_timeout = Duration::from_secs_f64(silence_timeout);
        let clock = &self.adapters.clock;
        let mut silence = SilenceTimer::new(silence_timeout, clock.clone());
        if woken {
            silence.first_timeout(FALSE_WAKE_TIMEOUT.min(silence_timeout));
        }
        silence.set_running(session.silence_timer_runs());
        let mut questions: VecDeque<Ask> = VecDeque::new();
        let mut answer_to = None;
        let answer_timeout = Duration::from_secs_f64(confirmation.timeout_secs);
        let mut answer_timer = SilenceTimer::new(answer_timeout, clock.clone());
        let mut dialog: Option<OpenDialog> = None;
        let mut due: Option<Dialog> = None;
        let mut unshown: Option<String> = None;

        let mut introducing = first_run;
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
            } else if let Some(why) = unshown.take() {
                Input::Unshown(why)
            } else if let Some(next) = answer_to
                .is_none()
                .then(|| {
                    let parked = &mut self.parked;
                    let when_quiet = || session.quiet().then(|| parked.pop_front()).flatten();
                    questions.pop_front().map(Parked::Ask).or_else(when_quiet)
                })
                .flatten()
            {
                match next {
                    Parked::Ask(abandoned) if abandoned.reply.is_closed() => continue,
                    Parked::Ask(ask) => {
                        if let Some(shown) = ask.shown {
                            self.notify(shown);
                        }
                        let timeout = match &ask.question.expects {
                            Expects::Typed(typed) => {
                                due = Some(typed.clone());
                                typing_timeout
                            }
                            _ => answer_timeout,
                        };
                        answer_to = Some(ask.reply);
                        answer_timer.reset_to(timeout);
                        Input::Ask(ask.question)
                    }
                    Parked::Turn(text, _) => Input::Opening(text),
                }
            } else {
                tokio::select! {
                biased;
                Some(event) = events.recv() => Input::Thinking(event),
                Some(event) = speaker_events.recv() => match event {
                    SpeakerEvent::Played => Input::Played,
                    SpeakerEvent::Failed { sentence, .. } => {
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
                        self.authoring.show_still_building();
                        self.publish(State::Idle, None);
                        let _ = reply.send(Ok(()));
                        return;
                    }
                    Some(Command::EndAfterSpeech(reply)) => {
                        let _ = reply.send(Ok(()));
                        Input::EndRequested
                    }
                    Some(Command::Ask(ask)) if !matches!(ask.when, When::Now) => {
                        self.parked.push_back(Parked::Ask(ask));
                        continue;
                    }
                    Some(Command::Ask(ask)) => {
                        questions.push_back(ask);
                        continue;
                    }
                    Some(Command::Say(text)) => Input::Say(text),
                    Some(Command::TurnOrNextSession(text, kept)) => {
                        let when = When::QuietOrNextSession(kept);
                        self.parked.push_back(Parked::Turn(text, when));
                        continue;
                    }
                    Some(Command::VoicePreview(preview)) => {
                        let _ = preview.reply.send(Err(PreviewError::SessionOpen));
                        continue;
                    }
                    Some(Command::SetupTest(test)) => {
                        test.refuse(SetupTestError::SessionOpen);
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
                changed = self.adapters.speaking.changed(), if self.speaking_open => match changed {
                    Ok(()) => Input::Speaking(*self.adapters.speaking.borrow_and_update()),
                    Err(_) => {
                        self.speaking_open = false;
                        continue;
                    }
                },
                Ok(()) = self.building.changed() => {
                    Input::Building(*self.building.borrow_and_update())
                }
                wake = self.adapters.wake_word.recv(), if self.wake_open => match wake {
                    Some(()) => {
                        log::debug!("wake word in {:?}", session.state());
                        Input::WakeWord
                    }
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
                        log::debug!("gate: ducking Lucy in {:?}", session.state());
                        speaker.duck(true);
                        continue;
                    }
                    Heard::Gate(BargeInEvent::Unduck) => {
                        log::debug!("gate: unducking Lucy in {:?}", session.state());
                        speaker.duck(false);
                        continue;
                    }
                    Heard::Gate(BargeInEvent::BargeIn) => {
                        log::debug!("gate: Barge-in in {:?}", session.state());
                        Input::BargeIn
                    }
                },
                () = silence.fired() => {
                    speaker.sound(Sound::Closing);
                    self.publish(State::Idle, None);
                    return;
                }
                () = answer_timer.fired() => {
                    Input::AnswerTimedOut
                }
                typed = typed(&mut dialog) => Input::Typed(typed),
                }
            };
            let input = match input {
                Input::Thinking(ThinkingEvent::Died) => match opened.as_ref() {
                    Some(opened) if !respawned_since_turn_end => {
                        eprintln!("lucyd: the model process died; opening it again");
                        respawned_since_turn_end = true;
                        model = self.open_model(opened, events_tx.clone());
                        if let Some(turn) = session.pending_turn() {
                            model.send_turn(turn.to_owned());
                        }
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
                respawned_since_turn_end = false;
            }
            if introducing && matches!(input, Input::Thinking(ThinkingEvent::TurnEnd)) {
                introducing = false;
                if let Err(e) = self.files.persona.introduced() {
                    let path = self.files.persona.path().display();
                    eprintln!("lucyd: can't save {path}: {e}");
                }
            }
            let outputs = session.handle(input);
            let reply_over = session.state() == State::Listening;
            if reply_over && !unspoken.is_empty() {
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
                        let mut finished_after_barge_in = plugin_tools.take_finished();
                        finished_after_barge_in.push(text);
                        model.send_turn(finished_after_barge_in.join("\n"));
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
                    Output::StopSpeaking => {
                        log::debug!("stopping Lucy's speech");
                        speaker.stop();
                    }
                    Output::CancelBuild => self.authoring.cancel(),
                    Output::Interrupt => {
                        model.interrupt();
                        plugin_tools.barge_in();
                    }
                }
            }
            if session.state() != State::Typing {
                dialog = None;
                due = None;
            } else if session.box_line_said() {
                if let Some(typed) = due.take() {
                    match self.open_dialog(typed) {
                        Ok(opened) => dialog = Some(opened),
                        Err(why) => unshown = Some(why),
                    }
                }
            }
            silence.set_running(session.silence_timer_runs());
            answer_timer.set_running(session.answer_timer_runs());
            if let Some(hearing) = &mut self.adapters.hearing {
                hearing.lucy_speaking(session.state() == State::Speaking);
            }
            self.publish(session.state(), Some(id));
        }
    }

    fn open_dialog(&self, typed: Dialog) -> Result<OpenDialog, String> {
        let (opened, held) = self.buses.dialogs.open(typed);
        if held {
            return Ok(opened);
        }
        match self
            .adapters
            .launcher
            .open_terminal(self.buses.dialogs.page())
        {
            Ok(()) => Ok(opened),
            Err(e) => {
                eprintln!("lucyd: can't open lucy input in a terminal: {e}");
                Err(NO_TERMINAL.to_owned())
            }
        }
    }

    fn end_on_lost_device(&self, direction: Direction, id: SessionId) {
        self.notify(notifications::device_lost(direction));
        self.publish(State::Error, Some(id));
        self.publish(State::Idle, None);
    }

    fn keep(&mut self, mut parked: Parked) {
        if matches!(&parked, Parked::Ask(ask) if ask.reply.is_closed()) {
            return;
        }
        if let When::QuietOrNextSession(kept) = std::mem::replace(parked.when(), When::Kept) {
            self.notify(kept);
        }
        self.parked.push_back(parked);
    }

    fn notify(&self, notification: Notification) {
        let (actions, _) = mpsc::unbounded_channel();
        self.adapters.notifier.notify(notification, actions);
    }

    fn publish(&self, state: State, session_id: Option<SessionId>) {
        self.buses.status.publish(Status {
            state,
            session_id: session_id.map(|id| id.to_string()),
            building: *self.building.borrow() == Building::Working,
        });
    }
}

const NO_TERMINAL: &str =
    "I couldn't put a box on screen: no terminal opened, and nothing else can show one.";

async fn typed(dialog: &mut Option<OpenDialog>) -> Typed {
    match dialog {
        Some(dialog) => dialog.typed().await,
        None => std::future::pending().await,
    }
}

async fn heard(hearing: &mut Option<Hearing>) -> Heard {
    match hearing {
        Some(hearing) => hearing.next().await,
        None => std::future::pending().await,
    }
}

async fn mic_lost(hearing: &mut Option<Hearing>) {
    match hearing {
        Some(hearing) => hearing.lost().await,
        None => std::future::pending().await,
    }
}

async fn mic_lost_for(hearing: &mut Option<Hearing>, wait: Duration) {
    mic_lost(hearing).await;
    tokio::time::sleep(wait).await;
}

struct SilenceTimer {
    timeout: Duration,
    remaining: Duration,
    resumed: Option<Instant>,
    clock: Arc<dyn Clock>,
    timer: Box<dyn Timer>,
}

impl SilenceTimer {
    fn new(timeout: Duration, clock: Arc<dyn Clock>) -> Self {
        SilenceTimer {
            timeout,
            remaining: timeout,
            resumed: None,
            timer: clock.timer(),
            clock,
        }
    }

    fn is_running(&self) -> bool {
        self.resumed.is_some()
    }

    fn set_running(&mut self, running: bool) {
        let now = self.clock.now();
        match (running, self.resumed) {
            (true, None) => self.resumed = Some(now),
            (false, Some(resumed)) => {
                let ran = now.saturating_duration_since(resumed);
                self.remaining = self.remaining.saturating_sub(ran);
                self.resumed = None;
            }
            _ => return,
        }
        self.timer.set(self.deadline());
    }

    fn first_timeout(&mut self, timeout: Duration) {
        self.remaining = timeout;
        self.timer.set(self.deadline());
    }

    fn reset_to(&mut self, timeout: Duration) {
        self.timeout = timeout;
        self.reset();
    }

    fn reset(&mut self) {
        self.remaining = self.timeout;
        if self.is_running() {
            self.resumed = Some(self.clock.now());
        }
        self.timer.set(self.deadline());
    }

    fn deadline(&self) -> Option<Instant> {
        self.resumed.map(|resumed| resumed + self.remaining)
    }

    fn fired(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        self.timer.fired()
    }
}
