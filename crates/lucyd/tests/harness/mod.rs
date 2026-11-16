#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Condvar, Mutex};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

use lucy_config::credentials::CredentialStore;
use lucy_config::{Config, Provider};
use lucy_ipc::{Client, Event, StateEvent, Topic};
use lucyd::adapters::{
    Adapters, Clock, Launcher, Notification, NotificationAction, Notifier, RateLimit, Relay,
    SessionModel, Sound, Speaker, SpeakerEvent, SpeechSegment, Stt, SttError, Thinking,
    ThinkingEvent, Timer, Tts, Workspace,
};
use lucyd::plugins::Environment;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use tokio::sync::{mpsc, watch};

const WAIT: Duration = Duration::from_secs(5);

pub struct Harness {
    daemon: lucyd::Daemon,
    _no_model_text: NoModelText,
    speech: Option<mpsc::UnboundedSender<SpeechSegment>>,
    wake_word: Option<mpsc::UnboundedSender<()>>,
    speaking: Option<watch::Sender<bool>>,
    pub mic: FakeMic,
    pub echo: FakeEchoCanceller,
    pub stt: FakeStt,
    pub thinking: FakeThinking,
    pub tts: FakeTts,
    pub notifier: FakeNotifier,
    pub launcher: FakeLauncher,
    pub credentials: FakeCredentials,
    pub clock: FakeClock,
    applications: [PathBuf; 2],
    bus: Arc<Mutex<Vec<String>>>,
    typed: Mutex<Vec<String>>,
    config_path: PathBuf,
    plugin_paths: lucyd::plugins::Paths,
    persona_path: PathBuf,
    _dir: tempfile::TempDir,
}

const PERSONA: &str = "You are Lucy.";

impl Harness {
    pub fn start() -> Harness {
        Harness::launch(None, Some(PERSONA), None, None, None)
    }

    pub fn with_env(vars: &[(&str, &str)]) -> Harness {
        let env = Environment::new(vars.iter().copied());
        Harness::launch_in(env, None, Some(PERSONA), None, None, None)
    }

    pub fn with_config(config: &str) -> Harness {
        Harness::launch(Some(config), Some(PERSONA), None, None, None)
    }

    pub fn first_run() -> Harness {
        Harness::launch(None, None, None, None, None)
    }

    pub fn with_thinking(config: Option<&str>, thinking: Arc<dyn Thinking>) -> Harness {
        Harness::launch(config, Some(PERSONA), Some(thinking), None, None)
    }

    pub fn with_superwhisper(config: &str, superwhisper: Arc<dyn Stt>) -> Harness {
        Harness::launch(Some(config), Some(PERSONA), None, None, Some(superwhisper))
    }

    pub fn with_superwhisper_and_thinking(
        config: &str,
        superwhisper: Arc<dyn Stt>,
        thinking: Arc<dyn Thinking>,
    ) -> Harness {
        Harness::launch(
            Some(config),
            Some(PERSONA),
            Some(thinking),
            None,
            Some(superwhisper),
        )
    }

    pub fn hearing(config: &str) -> Harness {
        let parsed: Config = toml::from_str(config).expect("config parses");
        let mic = FakeMic::default();
        let echo = FakeEchoCanceller::default();
        let ears = lucyd::audio::listen(
            Box::new(mic.clone()),
            &parsed.audio,
            Box::new(FakeVad(mic.judged.clone())),
            Some(Box::new(FakeWakeWord::default())),
            Some(Box::new(echo.clone())),
            &parsed.endpointing,
        )
        .expect("the fake microphone opens");
        let ears = Some((ears, mic, echo));
        Harness::launch(Some(config), Some(PERSONA), None, ears, None)
    }

    fn launch(
        config: Option<&str>,
        persona: Option<&str>,
        real_thinking: Option<Arc<dyn Thinking>>,
        ears: Option<(lucyd::audio::Ears, FakeMic, FakeEchoCanceller)>,
        superwhisper: Option<Arc<dyn Stt>>,
    ) -> Harness {
        let env = Environment::from_process();
        Harness::launch_in(env, config, persona, real_thinking, ears, superwhisper)
    }

    fn launch_in(
        env: Environment,
        config: Option<&str>,
        persona: Option<&str>,
        real_thinking: Option<Arc<dyn Thinking>>,
        ears: Option<(lucyd::audio::Ears, FakeMic, FakeEchoCanceller)>,
        superwhisper: Option<Arc<dyn Stt>>,
    ) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("lucy").join("lucyd.sock");
        let config_path = dir.path().join("config").join("lucy").join("config.toml");
        let persona_path = dir.path().join("config").join("lucy").join("persona.md");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        if let Some(config) = config {
            std::fs::write(&config_path, config).unwrap();
        }
        if let Some(persona) = persona {
            std::fs::write(&persona_path, persona).unwrap();
        }
        let (ears, echo) = match ears {
            Some((ears, mic, echo)) => (Some((ears, mic)), echo),
            None => (None, FakeEchoCanceller::default()),
        };
        let (speech, speech_rx, wake_word, wake_word_rx, speaking, speaking_rx, hearing, mic) =
            match ears {
                Some((ears, mic)) => (
                    None,
                    ears.speech,
                    None,
                    ears.wake_word,
                    None,
                    ears.speaking,
                    Some(ears.hearing),
                    mic,
                ),
                None => {
                    let (speech, speech_rx) = mpsc::unbounded_channel();
                    let (wake_word, wake_word_rx) = mpsc::unbounded_channel();
                    let (speaking, speaking_rx) = watch::channel(false);
                    (
                        Some(speech),
                        speech_rx,
                        Some(wake_word),
                        wake_word_rx,
                        Some(speaking),
                        speaking_rx,
                        None,
                        FakeMic::default(),
                    )
                }
            };
        let stt = FakeStt::default();
        let thinking = FakeThinking::new();
        let tts = FakeTts::new();
        let notifier = FakeNotifier::new();
        let plugin_paths = lucyd::plugins::Paths {
            user: dir.path().join("data/lucy/plugins"),
            shipped: dir.path().join("usr/share/lucy/plugins"),
            state: dir.path().join("state/lucy"),
        };
        let launcher = FakeLauncher::default();
        let credentials = FakeCredentials::default();
        let clock = FakeClock::new();
        let applications = ["applications", "system-applications"].map(|d| dir.path().join(d));
        for dir in &applications {
            std::fs::create_dir(dir).unwrap();
        }
        let daemon = lucyd::spawn(lucyd::Options {
            socket_path,
            config_path: config_path.clone(),
            persona_path: persona_path.clone(),
            lucy_program: lucy_binary(),
            plugin_paths: plugin_paths.clone(),
            plugin_env: env,
            adapters: Adapters {
                speech: speech_rx,
                wake_word: wake_word_rx,
                speaking: speaking_rx,
                hearing,
                stt: Arc::new(stt.clone()),
                superwhisper: superwhisper.unwrap_or_else(|| Arc::new(FakeStt::default())),
                thinking: real_thinking.unwrap_or_else(|| Arc::new(thinking.clone())),
                tts: Box::new(tts.clone()),
                notifier: Arc::new(notifier.clone()),
                launcher: Arc::new(launcher.clone()),
                credentials: Arc::new(credentials.clone()),
                clock: Arc::new(clock.clone()),
                applications: applications.to_vec(),
            },
        })
        .expect("lucyd starts");
        let harness = Harness {
            daemon,
            _no_model_text: NoModelText(notifier.clone()),
            speech,
            wake_word,
            speaking,
            mic,
            echo,
            stt,
            thinking,
            tts,
            notifier,
            launcher,
            credentials,
            clock,
            applications,
            bus: Arc::default(),
            typed: Mutex::default(),
            config_path,
            plugin_paths,
            persona_path,
            _dir: dir,
        };
        harness.record_bus();
        harness
    }

    pub fn socket_path(&self) -> &Path {
        self.daemon.socket_path()
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn mcp_socket_path(&self) -> &Path {
        self.daemon.mcp_socket_path()
    }

    pub fn scratch(&self) -> PathBuf {
        let scratch = self._dir.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        scratch
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugin_paths.user
    }

    pub fn state_dir(&self) -> &Path {
        &self.plugin_paths.state
    }

    pub fn install_plugin(&self, name: &str, manifest: &str) -> PathBuf {
        install(&self.plugin_paths.user.join(name), manifest)
    }

    pub fn install_shipped_plugin(&self, name: &str, manifest: &str) -> PathBuf {
        install(&self.plugin_paths.shipped.join(name), manifest)
    }

    pub fn approve(&self, name: &str) {
        let user = self.plugin_paths.user.join(name);
        let dir = if user.exists() {
            user
        } else {
            self.plugin_paths.shipped.join(name)
        };
        let manifest = std::fs::read_to_string(dir.join("manifest.toml")).unwrap();
        let parsed: toml::Table = toml::from_str(&manifest).unwrap();
        let command = parsed["command"].as_array().unwrap()[0].as_str().unwrap();
        let executable = std::fs::read(dir.join(command)).unwrap();
        let mut schemas = toml::Table::new();
        for tool in parsed["tools"].as_array().unwrap() {
            let tool = tool["name"].as_str().unwrap();
            let schema = serde_json::json!({
                "type": "object",
                "description": format!("approved {tool}"),
                "properties": {"text": {"type": "string"}, "path": {"type": "string"}},
            });
            schemas.insert(tool.into(), schema.to_string().into());
        }
        let mut approval = toml::Table::new();
        approval.insert(
            "hash".into(),
            content_hash(manifest.as_bytes(), &executable).into(),
        );
        approval.insert("schemas".into(), schemas.into());

        let path = self.plugin_paths.state.join("approvals.toml");
        let mut approvals: toml::Table = std::fs::read_to_string(&path)
            .map(|text| toml::from_str(&text).unwrap())
            .unwrap_or_default();
        approvals
            .entry("plugins")
            .or_insert_with(|| toml::Table::new().into())
            .as_table_mut()
            .unwrap()
            .insert(name.into(), approval.into());
        std::fs::create_dir_all(&self.plugin_paths.state).unwrap();
        std::fs::write(path, toml::to_string(&approvals).unwrap()).unwrap();
    }

    pub fn request_approval(&self, name: &str) -> ApprovalRequest {
        let answer = self.daemon.request_approval(name);
        let (tx, rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            if let Ok(outcome) = answer.blocking_recv() {
                let _ = tx.send(outcome);
            }
        });
        ApprovalRequest(Mutex::new(rx))
    }

    pub fn persona_path(&self) -> &Path {
        &self.persona_path
    }

    pub fn relay(&self, args: &[&str]) -> Output {
        Command::new(lucy_binary())
            .arg("mcp-relay")
            .arg("--socket")
            .arg(self.mcp_socket_path())
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    pub fn desktop_entry(&self, file_name: &str, keys: &str) -> PathBuf {
        write_desktop_entry(&self.applications[0], file_name, keys)
    }

    pub fn system_desktop_entry(&self, file_name: &str, keys: &str) -> PathBuf {
        write_desktop_entry(&self.applications[1], file_name, keys)
    }

    pub fn client(&self) -> Client {
        let client = Client::connect(self.socket_path(), "harness", None).expect("hello succeeds");
        client.set_read_timeout(Some(WAIT)).unwrap();
        client
    }

    pub fn indicator(&self, name: &str) -> Client {
        let client = Client::connect(self.socket_path(), name, Some(lucy_ipc::Role::Indicator))
            .expect("hello succeeds");
        client.set_read_timeout(Some(WAIT)).unwrap();
        client
    }

    pub fn input(&self, name: &str) -> Client {
        let client = Client::connect(self.socket_path(), name, Some(lucy_ipc::Role::Input))
            .expect("hello succeeds");
        client.set_read_timeout(Some(WAIT)).unwrap();
        client
    }

    pub fn answer_input(&self, holder: &mut Client, id: &str, value: &str) {
        self.answer_fields(holder, id, &[("answer", value)]);
    }

    pub fn answer_fields(&self, holder: &mut Client, id: &str, fields: &[(&str, &str)]) {
        let mut typed = self.typed.lock().unwrap();
        typed.extend(fields.iter().map(|(_, value)| (*value).to_owned()));
        let values = fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        holder
            .answer_input(id, values)
            .expect("the answer is taken");
    }

    pub fn state_client(&self) -> Client {
        let mut client = self.client();
        client.subscribe(&[Topic::State]).unwrap();
        next_state(&mut client);
        client
    }

    pub fn barrier(&self) {
        assert!(self.client().start_session().is_err());
    }

    pub fn start_speaking(&self) {
        let speaking = self.speaking.as_ref().expect("lucyd hears through `mic`");
        speaking.send_replace(true);
    }

    pub fn say(&self, transcript: &str) {
        self.stt.script(transcript);
        self.end_utterance();
    }

    pub fn stop_speaking(&self) {
        let speaking = self.speaking.as_ref().expect("lucyd hears through `mic`");
        speaking.send_replace(false);
    }

    pub fn say_and_go_on(&self, transcript: &str) {
        self.start_speaking();
        self.stt.script(transcript);
        let speech = self.speech.as_ref().expect("lucyd hears through `mic`");
        speech.send(SpeechSegment { samples: vec![] }).unwrap();
    }

    pub fn mumble_handled(&self) {
        let given = self.stt.segments().len();
        let heard = self.stt.heard().len();
        self.mumble();
        wait_until("the mumble transcribed", || {
            (self.stt.heard().len() > heard).then_some(())
        });
        self.stt.hold(true);
        self.stt.script("");
        let speech = self.speech.as_ref().expect("lucyd hears through `mic`");
        wait_until("lucyd to take another utterance", || {
            if self.stt.segments().len() > given + 1 {
                return Some(());
            }
            speech.send(SpeechSegment { samples: vec![] }).unwrap();
            None
        });
        self.stt.hold(false);
    }

    pub fn say_but_stt_fails(&self) {
        self.stt.script_failure();
        self.end_utterance();
    }

    fn end_utterance(&self) {
        let speech = self.speech.as_ref().expect("lucyd hears through `mic`");
        speech.send(SpeechSegment { samples: vec![] }).unwrap();
        self.speaking.as_ref().unwrap().send_replace(false);
    }

    pub fn wake(&self) {
        let wake_word = self.wake_word.as_ref().expect("lucyd hears through `mic`");
        wake_word.send(()).unwrap();
    }

    pub fn mumble(&self) {
        self.say("");
    }

    pub fn raw_exchange(&self, line: &str) -> serde_json::Value {
        use std::io::{BufRead, BufReader, Write};
        let mut stream = UnixStream::connect(self.socket_path()).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        writeln!(stream, "{line}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn record_bus(&self) {
        let mut client = self.client();
        client.set_read_timeout(None).unwrap();
        client.subscribe(&[Topic::State]).unwrap();
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            while let Ok(line) = client.next_event_line() {
                bus.lock().unwrap().push(line);
            }
        });
    }

    pub fn assert_nothing_private_on_bus(&self) {
        let deadline = Instant::now() + WAIT;
        while !self.bus_is_idle() {
            assert!(Instant::now() < deadline, "the bus never returned to idle");
            // sleep-ok: poll loop with a deadline
            std::thread::sleep(Duration::from_millis(10));
        }
        let lines = self.bus.lock().unwrap().clone();
        let private: Vec<String> = [
            self.stt.heard(),
            self.thinking.streamed(),
            self.tts.sentences(),
            self.credentials.values(),
            self.typed.lock().unwrap().clone(),
        ]
        .concat()
        .into_iter()
        .filter(|text| !text.trim().is_empty())
        .collect();
        assert!(
            !private.is_empty(),
            "nothing private was said; the check is vacuous"
        );
        for line in &lines {
            for text in &private {
                assert!(
                    !line.contains(text.trim()),
                    "{text:?} appeared on the bus: {line}"
                );
            }
        }
    }

    pub fn assert_no_level_on_bus(&self) {
        let bus = self.bus.lock().unwrap();
        let level = bus.iter().find(|line| line.contains(r#""method":"level""#));
        assert!(level.is_none(), "a state-only subscriber got {level:?}");
    }

    fn bus_is_idle(&self) -> bool {
        let bus = self.bus.lock().unwrap();
        let idle = |line: &String| line.contains(r#""state":"idle""#);
        bus.len() > 1 && bus.last().is_some_and(idle)
    }
}

pub struct ApprovalRequest(Mutex<std_mpsc::Receiver<Result<bool, String>>>);

impl ApprovalRequest {
    pub fn outcome(&self) -> Result<bool, String> {
        recv(&self.0, "the Approval to end")
    }
}

pub struct ToolWatch {
    changed: Mutex<std_mpsc::Receiver<()>>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl ToolWatch {
    pub fn wait_for_change(&self) {
        recv(&self.changed, "tools/list_changed");
    }
}

#[derive(Clone)]
struct ChangeCounter(std_mpsc::Sender<()>);

impl rmcp::ClientHandler for ChangeCounter {
    async fn on_tool_list_changed(&self, _context: rmcp::service::NotificationContext<RoleClient>) {
        let _ = self.0.send(());
    }
}

fn write_desktop_entry(dir: &Path, file_name: &str, keys: &str) -> PathBuf {
    let path = dir.join(file_name);
    std::fs::write(
        &path,
        format!("[Desktop Entry]\nType=Application\n{keys}\n"),
    )
    .unwrap();
    path
}

pub fn wait_until<T>(what: &str, mut found: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(Instant::now() < deadline, "{what}");
        // sleep-ok: poll loop with a deadline
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn next_state(client: &mut Client) -> StateEvent {
    match client.next_event().expect("a state event") {
        Event::State(state) => state,
        other => panic!("expected a state event, got {other:?}"),
    }
}

fn in_profile_dir(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .skip(1)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| exe.parent().unwrap().join(name))
}

fn lucy_binary() -> PathBuf {
    let lucy = in_profile_dir("lucy");
    assert!(
        lucy.exists(),
        "{} not found; run `cargo build -p lucy` or `cargo test --workspace`",
        lucy.display()
    );
    lucy
}

fn fixture_plugin() -> PathBuf {
    let fixture = in_profile_dir("examples/fixture-plugin");
    assert!(
        fixture.exists(),
        "{} not found; run `cargo build -p lucyd --examples`",
        fixture.display()
    );
    fixture
}

fn install(dir: &Path, manifest: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    copy_in_a_child(&fixture_plugin(), &dir.join("fixture-plugin"));
    dir.to_owned()
}

fn copy_in_a_child(from: &Path, to: &Path) {
    let copied = Command::new("cp").arg(from).arg(to).status().unwrap();
    assert!(copied.success());
}

fn content_hash(manifest: &[u8], executable: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update((manifest.len() as u64).to_le_bytes());
    hasher.update(manifest);
    hasher.update(executable);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn recv<T>(receiver: &Mutex<std_mpsc::Receiver<T>>, what: &str) -> T {
    receiver
        .lock()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[derive(Clone, Default)]
pub struct FakeStt {
    scripted: Arc<Mutex<VecDeque<Result<String, SttError>>>>,
    heard: Arc<Mutex<Vec<String>>>,
    segments: Arc<Mutex<Vec<Duration>>>,
    delay: Arc<Mutex<Duration>>,
    held: Arc<(Mutex<bool>, Condvar)>,
}

impl FakeStt {
    pub fn take(&self, delay: Duration) {
        *self.delay.lock().unwrap() = delay;
    }

    pub fn hold(&self, held: bool) {
        let (lock, released) = &*self.held;
        *lock.lock().unwrap() = held;
        released.notify_all();
    }

    pub fn script(&self, transcript: &str) {
        self.scripted
            .lock()
            .unwrap()
            .push_back(Ok(transcript.to_owned()));
    }

    pub fn script_failure(&self) {
        let failure = SttError("the fake STT failed".to_owned());
        self.scripted.lock().unwrap().push_back(Err(failure));
    }

    pub fn heard(&self) -> Vec<String> {
        self.heard.lock().unwrap().clone()
    }

    pub fn segments(&self) -> Vec<Duration> {
        self.segments.lock().unwrap().clone()
    }
}

impl Stt for FakeStt {
    fn transcribe(&self, segment: SpeechSegment) -> Result<String, SttError> {
        let length = Duration::from_secs_f64(segment.samples.len() as f64 / 16_000.0);
        self.segments.lock().unwrap().push(length);
        let (lock, released) = &*self.held;
        drop(
            released
                .wait_while(lock.lock().unwrap(), |held| *held)
                .unwrap(),
        );
        // sleep-ok: the fake's scripted transcription time
        std::thread::sleep(*self.delay.lock().unwrap());
        let transcript = self
            .scripted
            .lock()
            .unwrap()
            .pop_front()
            .expect("a scripted transcript")?;
        self.heard.lock().unwrap().push(transcript.clone());
        Ok(transcript)
    }
}

#[derive(Clone)]
pub struct FakeThinking {
    turns_tx: std_mpsc::Sender<String>,
    turns: Arc<Mutex<std_mpsc::Receiver<String>>>,
    events: Arc<Mutex<Option<mpsc::UnboundedSender<ThinkingEvent>>>>,
    relay: Arc<Mutex<Option<Relay>>>,
    instructions: Arc<Mutex<Option<String>>>,
    streamed: Arc<Mutex<Vec<String>>>,
    opened: Arc<Mutex<Vec<Provider>>>,
    interrupts: Arc<Mutex<usize>>,
    unhealthy: Arc<Mutex<HashMap<String, String>>>,
    failing_checks: Arc<Mutex<HashMap<String, String>>>,
    pub delegate: FakeDelegate,
}

impl FakeThinking {
    fn new() -> Self {
        let (turns_tx, turns) = std_mpsc::channel();
        FakeThinking {
            delegate: FakeDelegate::new(),
            turns_tx,
            turns: Arc::new(Mutex::new(turns)),
            events: Arc::default(),
            relay: Arc::default(),
            instructions: Arc::default(),
            streamed: Arc::default(),
            opened: Arc::default(),
            interrupts: Arc::default(),
            unhealthy: Arc::default(),
            failing_checks: Arc::default(),
        }
    }

    pub fn unhealthy(&self, model: &str, problem: &str) {
        let mut unhealthy = self.unhealthy.lock().unwrap();
        unhealthy.insert(model.to_owned(), problem.to_owned());
    }

    pub fn fails_check(&self, model: &str, problem: &str) {
        let mut failing = self.failing_checks.lock().unwrap();
        failing.insert(model.to_owned(), problem.to_owned());
    }

    pub fn opened(&self) -> Vec<Provider> {
        self.opened.lock().unwrap().clone()
    }

    pub fn interrupts(&self) -> usize {
        *self.interrupts.lock().unwrap()
    }

    pub fn next_turn(&self) -> String {
        recv(&self.turns, "a Turn sent to Thinking")
    }

    pub fn assert_no_turn(&self) {
        let turn = self.turns.lock().unwrap().try_recv();
        assert!(turn.is_err(), "unexpected Turn sent to Thinking: {turn:?}");
    }

    pub fn delta(&self, text: &str) {
        self.streamed.lock().unwrap().push(text.to_owned());
        self.send(ThinkingEvent::Delta(text.to_owned()));
    }

    pub fn sentence_end(&self) {
        self.send(ThinkingEvent::SentenceEnd);
    }

    pub fn turn_end(&self) {
        self.send(ThinkingEvent::TurnEnd);
    }

    pub fn dies(&self) {
        self.send(ThinkingEvent::Died);
    }

    pub fn rate_limit(&self, limit: RateLimit) {
        self.send(ThinkingEvent::RateLimit(limit));
    }

    pub fn unavailable(&self, reason: &str) {
        self.send(ThinkingEvent::Unavailable(reason.to_owned()));
    }

    pub fn instructions(&self) -> String {
        self.instructions
            .lock()
            .unwrap()
            .clone()
            .expect("a SessionModel was opened")
    }

    pub fn call_tool(&self, name: &str) -> Result<CallToolResult, String> {
        self.call_tool_with(name, serde_json::json!({}))
    }

    pub fn call_tool_with(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, String> {
        let serde_json::Value::Object(arguments) = arguments else {
            panic!("tool arguments are an object");
        };
        self.with_mcp(|client| async move {
            client
                .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
                .await
                .map_err(|e| e.to_string())
        })
    }

    pub fn tools(&self) -> Vec<Tool> {
        self.with_mcp(
            |client| async move { client.list_all_tools().await.map_err(|e| e.to_string()) },
        )
        .unwrap()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.with_mcp(|client| async move {
            let tools = client.list_all_tools().await.map_err(|e| e.to_string())?;
            Ok(tools
                .into_iter()
                .map(|tool| tool.name.into_owned())
                .collect())
        })
        .unwrap()
    }

    pub fn watch_tools(&self) -> ToolWatch {
        let relay = self
            .relay
            .lock()
            .unwrap()
            .clone()
            .expect("a SessionModel was opened");
        let (changed_tx, changed) = std_mpsc::channel();
        let (connected_tx, connected) = std_mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let mut command = tokio::process::Command::new(&relay.program);
                command.args(&relay.args);
                let transport = TokioChildProcess::new(command).unwrap();
                let client = ChangeCounter(changed_tx).serve(transport).await.unwrap();
                let _ = connected_tx.send(());
                let _ = stopped.await;
                let _ = client.cancel().await;
            });
        });
        recv(&Mutex::new(connected), "the watching relay to connect");
        ToolWatch {
            changed: Mutex::new(changed),
            _stop: stop,
        }
    }

    fn with_mcp<F, Fut, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(RunningService<RoleClient, ()>) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let relay = self
            .relay
            .lock()
            .unwrap()
            .clone()
            .expect("a SessionModel was opened");
        with_mcp(&relay, f)
    }

    fn send(&self, event: ThinkingEvent) {
        let events = self.events.lock().unwrap();
        let events = events.as_ref().expect("a SessionModel is open");
        events
            .send(event)
            .expect("lucyd is reading the SessionModel");
    }

    fn streamed(&self) -> Vec<String> {
        self.streamed.lock().unwrap().clone()
    }
}

impl Thinking for FakeThinking {
    fn open(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
    ) -> Box<dyn SessionModel> {
        self.opened.lock().unwrap().push(provider.clone());
        *self.events.lock().unwrap() = Some(events);
        *self.relay.lock().unwrap() = Some(relay);
        *self.instructions.lock().unwrap() = Some(instructions);
        Box::new(FakeSessionModel {
            turns: self.turns_tx.clone(),
            interrupts: self.interrupts.clone(),
        })
    }

    fn health(&self, provider: &Provider) -> Result<(), String> {
        scripted_problem(&self.unhealthy, provider)
    }

    fn check(&self, provider: &Provider) -> Result<(), String> {
        scripted_problem(&self.failing_checks, provider)
    }

    fn open_delegate(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
        workspace: Workspace,
    ) -> Box<dyn SessionModel> {
        let delegate = &self.delegate;
        delegate.opened.lock().unwrap().push(provider.clone());
        *delegate.events.lock().unwrap() = Some(events);
        *delegate.relay.lock().unwrap() = Some(relay);
        *delegate.instructions.lock().unwrap() = Some(instructions);
        *delegate.workspace.lock().unwrap() = Some(workspace);
        Box::new(FakeSessionModel {
            turns: delegate.turns_tx.clone(),
            interrupts: Arc::default(),
        })
    }
}

#[derive(Clone)]
pub struct FakeDelegate {
    turns_tx: std_mpsc::Sender<String>,
    turns: Arc<Mutex<std_mpsc::Receiver<String>>>,
    events: Arc<Mutex<Option<mpsc::UnboundedSender<ThinkingEvent>>>>,
    relay: Arc<Mutex<Option<Relay>>>,
    instructions: Arc<Mutex<Option<String>>>,
    workspace: Arc<Mutex<Option<Workspace>>>,
    opened: Arc<Mutex<Vec<Provider>>>,
}

impl FakeDelegate {
    fn new() -> Self {
        let (turns_tx, turns) = std_mpsc::channel();
        FakeDelegate {
            turns_tx,
            turns: Arc::new(Mutex::new(turns)),
            events: Arc::default(),
            relay: Arc::default(),
            instructions: Arc::default(),
            workspace: Arc::default(),
            opened: Arc::default(),
        }
    }

    pub fn opened(&self) -> Vec<Provider> {
        self.opened.lock().unwrap().clone()
    }

    pub fn next_turn(&self) -> String {
        recv(&self.turns, "a Turn sent to the Delegate")
    }

    pub fn assert_no_turn(&self) {
        let turn = self.turns.lock().unwrap().try_recv();
        assert!(
            turn.is_err(),
            "unexpected Turn sent to the Delegate: {turn:?}"
        );
    }

    pub fn instructions(&self) -> String {
        let instructions = self.instructions.lock().unwrap();
        instructions.clone().expect("a Delegate was opened")
    }

    pub fn workspace(&self) -> Workspace {
        let workspace = self.workspace.lock().unwrap();
        workspace.clone().expect("a Delegate was opened")
    }

    pub fn call_tool_with(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, String> {
        call_through(&self.relay(), name, arguments)
    }

    pub fn tool_names(&self) -> Vec<String> {
        with_mcp(&self.relay(), |client| async move {
            let tools = client.list_all_tools().await.map_err(|e| e.to_string())?;
            Ok(tools
                .into_iter()
                .map(|tool| tool.name.into_owned())
                .collect())
        })
        .unwrap()
    }

    pub fn turn_end(&self) {
        let events = self.events.lock().unwrap();
        let events = events.as_ref().expect("a Delegate is open");
        events
            .send(ThinkingEvent::TurnEnd)
            .expect("lucyd is reading the Delegate");
    }

    fn relay(&self) -> Relay {
        let relay = self.relay.lock().unwrap();
        relay.clone().expect("a Delegate was opened")
    }
}

pub fn stage_plugin(dir: &Path, manifest: &str, build: &str, test: &str) {
    install(dir, manifest);
    std::fs::write(dir.join("build.sh"), format!("{build}\n")).unwrap();
    std::fs::write(dir.join("test.sh"), format!("{test}\n")).unwrap();
}

fn with_mcp<F, Fut, T>(relay: &Relay, f: F) -> Result<T, String>
where
    F: FnOnce(RunningService<RoleClient, ()>) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut command = tokio::process::Command::new(&relay.program);
        command.args(&relay.args);
        let transport = TokioChildProcess::new(command).map_err(|e| e.to_string())?;
        let client = tokio::time::timeout(WAIT, ().serve(transport))
            .await
            .map_err(|_| "timed out connecting".to_owned())?
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(WAIT, f(client))
            .await
            .map_err(|_| "timed out calling".to_owned())?
    })
}

fn call_through(
    relay: &Relay,
    name: &str,
    arguments: serde_json::Value,
) -> Result<CallToolResult, String> {
    let serde_json::Value::Object(arguments) = arguments else {
        panic!("tool arguments are an object");
    };
    with_mcp(relay, |client| async move {
        client
            .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
            .await
            .map_err(|e| e.to_string())
    })
}

fn scripted_problem(
    problems: &Mutex<HashMap<String, String>>,
    provider: &Provider,
) -> Result<(), String> {
    let model = provider.model.as_deref().unwrap_or_default();
    match problems.lock().unwrap().get(model) {
        Some(problem) => Err(problem.clone()),
        None => Ok(()),
    }
}

struct FakeSessionModel {
    turns: std_mpsc::Sender<String>,
    interrupts: Arc<Mutex<usize>>,
}

impl SessionModel for FakeSessionModel {
    fn send_turn(&mut self, text: String) {
        let _ = self.turns.send(text);
    }

    fn interrupt(&mut self) {
        *self.interrupts.lock().unwrap() += 1;
    }
}

#[derive(Clone)]
pub struct FakeTts {
    sentences_tx: std_mpsc::Sender<String>,
    sentences: Arc<Mutex<std_mpsc::Receiver<String>>>,
    spoken: Arc<Mutex<Vec<String>>>,
    sounds: Arc<Mutex<Vec<Sound>>>,
    queued: Arc<Mutex<Vec<Queued>>>,
    failing: Arc<AtomicBool>,
    stops: Arc<Mutex<usize>>,
    ducks: Arc<Mutex<Vec<bool>>>,
    events: Arc<Mutex<Option<mpsc::UnboundedSender<SpeakerEvent>>>>,
    device: Arc<Mutex<Option<String>>>,
    voice: Arc<Mutex<Option<String>>>,
}

impl FakeTts {
    fn new() -> Self {
        let (sentences_tx, sentences) = std_mpsc::channel();
        FakeTts {
            sentences_tx,
            sentences: Arc::new(Mutex::new(sentences)),
            spoken: Arc::default(),
            sounds: Arc::default(),
            queued: Arc::default(),
            failing: Arc::default(),
            stops: Arc::default(),
            ducks: Arc::default(),
            events: Arc::default(),
            device: Arc::default(),
            voice: Arc::default(),
        }
    }

    pub fn voice(&self) -> Option<String> {
        self.voice.lock().unwrap().clone()
    }

    pub fn device(&self) -> Option<String> {
        self.device.lock().unwrap().clone()
    }

    pub fn lose_device(&self) {
        self.send(SpeakerEvent::DeviceLost);
    }

    pub fn next_sentence(&self) -> String {
        recv(&self.sentences, "a sentence sent to TTS")
    }

    pub fn sentences(&self) -> Vec<String> {
        self.spoken.lock().unwrap().clone()
    }

    pub fn sounds(&self) -> Vec<Sound> {
        self.sounds.lock().unwrap().clone()
    }

    pub fn queued(&self) -> Vec<Queued> {
        self.queued.lock().unwrap().clone()
    }

    pub fn stops(&self) -> usize {
        *self.stops.lock().unwrap()
    }

    pub fn ducks(&self) -> Vec<bool> {
        self.ducks.lock().unwrap().clone()
    }

    pub fn finish_playing(&self) {
        self.send(SpeakerEvent::Played);
    }

    pub fn level(&self, rms: f32) {
        self.send(SpeakerEvent::Level(rms));
    }

    pub fn fail(&self) {
        self.failing.store(true, Ordering::SeqCst);
    }

    fn send(&self, event: SpeakerEvent) {
        let events = self.events.lock().unwrap();
        let events = events.as_ref().expect("a Speaker is open");
        events.send(event).expect("lucyd is listening for playback");
    }
}

impl Tts for FakeTts {
    fn open(
        &self,
        speaker: &str,
        voice: &str,
        events: mpsc::UnboundedSender<SpeakerEvent>,
    ) -> Box<dyn Speaker> {
        *self.events.lock().unwrap() = Some(events);
        *self.device.lock().unwrap() = Some(speaker.to_owned());
        *self.voice.lock().unwrap() = Some(voice.to_owned());
        Box::new(FakeSpeaker { tts: self.clone() })
    }
}

pub const FAKE_TTS_FAILURE: &str = "the fake TTS is failing";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queued {
    Sentence(String),
    Sound(Sound),
}

struct FakeSpeaker {
    tts: FakeTts,
}

impl Speaker for FakeSpeaker {
    fn speak(&mut self, sentence: String) {
        self.tts.spoken.lock().unwrap().push(sentence.clone());
        let queued = Queued::Sentence(sentence.clone());
        self.tts.queued.lock().unwrap().push(queued);
        if self.tts.failing.load(Ordering::SeqCst) {
            self.tts.send(SpeakerEvent::Failed {
                sentence,
                reason: FAKE_TTS_FAILURE.to_owned(),
            });
        } else {
            let _ = self.tts.sentences_tx.send(sentence);
        }
    }

    fn sound(&mut self, sound: Sound) {
        self.tts.sounds.lock().unwrap().push(sound);
        self.tts.queued.lock().unwrap().push(Queued::Sound(sound));
    }

    fn duck(&mut self, ducked: bool) {
        self.tts.ducks.lock().unwrap().push(ducked);
    }

    fn stop(&mut self) {
        *self.tts.stops.lock().unwrap() += 1;
    }
}

#[derive(Clone)]
pub struct FakeNotifier {
    shown_tx: std_mpsc::Sender<Notification>,
    shown: Arc<Mutex<std_mpsc::Receiver<Notification>>>,
    offered: Arc<Mutex<Option<OfferedAction>>>,
    every: Arc<Mutex<Vec<Notification>>>,
}

type OfferedAction = (
    NotificationAction,
    mpsc::UnboundedSender<NotificationAction>,
);

impl FakeNotifier {
    fn new() -> Self {
        let (shown_tx, shown) = std_mpsc::channel();
        FakeNotifier {
            shown_tx,
            shown: Arc::new(Mutex::new(shown)),
            offered: Arc::default(),
            every: Arc::default(),
        }
    }

    pub fn next_notification(&self) -> Notification {
        recv(&self.shown, "a notification")
    }

    pub fn next_notification_of(&self, kind: lucyd::adapters::NotificationKind) -> Notification {
        loop {
            let shown = self.next_notification();
            if shown.kind == kind {
                return shown;
            }
        }
    }

    pub fn shown(&self) -> Vec<Notification> {
        self.shown.lock().unwrap().try_iter().collect()
    }

    pub fn assert_no_notification(&self) {
        let shown = self.shown.lock().unwrap().try_recv();
        assert!(shown.is_err(), "unexpected notification: {shown:?}");
    }

    pub fn pick_action(&self) {
        let offered = self.offered.lock().unwrap();
        let (action, actions) = offered.as_ref().expect("a notification offered an action");
        actions
            .send(*action)
            .expect("lucyd is listening for actions");
    }
}

impl Notifier for FakeNotifier {
    fn notify(
        &self,
        notification: Notification,
        actions: mpsc::UnboundedSender<NotificationAction>,
    ) {
        if let Some(action) = notification.action {
            *self.offered.lock().unwrap() = Some((action, actions));
        }
        self.every.lock().unwrap().push(notification.clone());
        let _ = self.shown_tx.send(notification);
    }
}

struct NoModelText(FakeNotifier);

impl Drop for NoModelText {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        for shown in self.0.every.lock().unwrap().iter() {
            for marker in lucyd::model_text::MARKERS {
                assert!(
                    !shown.summary.contains(marker) && !shown.body.contains(marker),
                    "a notification showed text written for the model: {shown:?}"
                );
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct FakeLauncher {
    opened: Arc<Mutex<Vec<String>>>,
    launched: Arc<Mutex<Vec<PathBuf>>>,
    launched_through: Arc<Mutex<Vec<Option<Vec<String>>>>>,
    terminals: Arc<Mutex<Vec<Vec<String>>>>,
    no_terminal: Arc<AtomicBool>,
}

impl FakeLauncher {
    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }

    pub fn launched(&self) -> Vec<PathBuf> {
        self.launched.lock().unwrap().clone()
    }

    pub fn terminals(&self) -> Vec<Vec<String>> {
        self.terminals.lock().unwrap().clone()
    }

    pub fn no_terminal(&self) {
        self.no_terminal.store(true, Ordering::SeqCst);
    }

    pub fn launched_through(&self) -> Vec<Option<Vec<String>>> {
        self.launched_through.lock().unwrap().clone()
    }
}

impl Launcher for FakeLauncher {
    fn open_url(&self, url: &str) -> std::io::Result<()> {
        self.opened.lock().unwrap().push(url.to_owned());
        Ok(())
    }

    fn launch(&self, entry: &Path, launcher: Option<&[String]>) -> std::io::Result<()> {
        self.launched.lock().unwrap().push(entry.to_owned());
        let launcher = launcher.map(<[String]>::to_vec);
        self.launched_through.lock().unwrap().push(launcher);
        Ok(())
    }

    fn open_terminal(&self, command: &[String]) -> std::io::Result<()> {
        if self.no_terminal.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "xdg-terminal-exec isn't installed",
            ));
        }
        self.terminals.lock().unwrap().push(command.to_vec());
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct FakeCredentials {
    values: Arc<Mutex<HashMap<(String, String), String>>>,
    failure: Arc<Mutex<Option<String>>>,
}

impl FakeCredentials {
    pub fn enter(&self, plugin: &str, name: &str, value: &str) {
        self.values
            .lock()
            .unwrap()
            .insert((plugin.into(), name.into()), value.into());
    }

    pub fn values(&self) -> Vec<String> {
        self.values.lock().unwrap().values().cloned().collect()
    }

    pub fn fail(&self, reason: &str) {
        *self.failure.lock().unwrap() = Some(reason.into());
    }
}

impl CredentialStore for FakeCredentials {
    fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, String> {
        if let Some(reason) = self.failure.lock().unwrap().clone() {
            return Err(reason);
        }
        let key = (plugin.to_owned(), name.to_owned());
        Ok(self.values.lock().unwrap().get(&key).cloned())
    }

    fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), String> {
        self.enter(plugin, name, value);
        Ok(())
    }
}

pub const SPEECH: f32 = 0.9;
pub const SILENCE: f32 = 0.0;
pub const WAKE: f32 = 0.95;

#[derive(Clone, Default)]
pub struct FakeMic {
    ring: Arc<Mutex<Option<rtrb::Producer<f32>>>>,
    opened: Arc<Mutex<Option<String>>>,
    unplugged: Arc<Mutex<Vec<String>>>,
    pulseaudio: Arc<AtomicBool>,
    fed: Arc<Mutex<usize>>,
    judged: Arc<Mutex<usize>>,
}

impl FakeMic {
    pub fn use_pulseaudio(&self) {
        self.pulseaudio.store(true, Ordering::SeqCst);
    }

    pub fn unplug(&self, name: &str) {
        self.unplugged.lock().unwrap().push(name.to_owned());
    }

    pub fn plug_in(&self, name: &str) {
        self.unplugged.lock().unwrap().retain(|n| n != name);
    }

    pub fn lose(&self) {
        drop(self.ring.lock().unwrap().take());
    }

    pub fn wait_until_open(&self) {
        let deadline = Instant::now() + WAIT;
        while self.ring.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "timed out waiting for capture");
            // sleep-ok: poll loop with a deadline
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn feed(&self, level: f32, duration: Duration) {
        let len = (duration.as_secs_f64() * 16_000.0).round() as usize;
        self.play(&vec![level; len]);
    }

    pub fn wait_until_heard(&self) {
        let chunks = *self.fed.lock().unwrap() / FAKE_VAD_CHUNK;
        wait_until("the VAD never caught up with the microphone", || {
            (*self.judged.lock().unwrap() >= chunks).then_some(())
        });
    }

    pub fn play(&self, mut samples: &[f32]) {
        *self.fed.lock().unwrap() += samples.len();
        while !samples.is_empty() {
            let written = {
                let mut ring = self.ring.lock().unwrap();
                let ring = ring.as_mut().expect("lucyd opened the microphone");
                let n = samples.len().min(ring.slots());
                for &sample in &samples[..n] {
                    ring.push(sample).unwrap();
                }
                n
            };
            samples = &samples[written..];
            if written == 0 {
                // sleep-ok: backs off while the ring is full
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    pub fn opened(&self) -> Option<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl lucyd::audio::AudioIo for FakeMic {
    fn capture(
        &self,
        mic: &str,
        ring: rtrb::Producer<f32>,
    ) -> Result<Box<dyn Send>, lucyd::audio::Error> {
        *self.ring.lock().unwrap() = Some(ring);
        *self.opened.lock().unwrap() = Some(mic.to_owned());
        Ok(Box::new(()))
    }

    fn has_device(&self, _direction: lucyd::audio::Direction, name: &str) -> bool {
        !self.unplugged.lock().unwrap().iter().any(|n| n == name)
    }

    fn is_pipewire(&self) -> bool {
        !self.pulseaudio.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
pub struct FakeEchoCanceller {
    erle_db: Arc<Mutex<Option<f32>>>,
    echo_heard: Arc<Mutex<Option<bool>>>,
    cancel_everything: Arc<AtomicBool>,
    frames: Arc<Mutex<usize>>,
}

impl FakeEchoCanceller {
    pub fn set_erle(&self, erle_db: Option<f32>) {
        *self.erle_db.lock().unwrap() = erle_db;
    }

    pub fn set_echo_heard(&self, heard: Option<bool>) {
        *self.echo_heard.lock().unwrap() = heard;
    }

    pub fn cancel_everything(&self) {
        self.cancel_everything.store(true, Ordering::SeqCst);
    }

    pub fn frames(&self) -> usize {
        *self.frames.lock().unwrap()
    }
}

impl lucyd::audio::EchoCanceller for FakeEchoCanceller {
    fn cancel(&mut self, frame: &mut [f32]) {
        *self.frames.lock().unwrap() += 1;
        if self.cancel_everything.load(Ordering::SeqCst) {
            frame.fill(0.0);
        }
    }

    fn erle_db(&self) -> Option<f32> {
        *self.erle_db.lock().unwrap()
    }

    fn echo_heard(&self) -> Option<bool> {
        *self.echo_heard.lock().unwrap()
    }
}

pub struct FakeVad(Arc<Mutex<usize>>);

const FAKE_VAD_CHUNK: usize = 512;

impl lucyd::audio::Vad for FakeVad {
    fn chunk_len(&self) -> usize {
        FAKE_VAD_CHUNK
    }

    fn speech_probability(&mut self, chunk: &[f32]) -> f32 {
        *self.0.lock().unwrap() += 1;
        chunk[0]
    }
}

#[derive(Default)]
pub struct FakeWakeWord {
    in_wake: bool,
    score: f32,
}

impl lucyd::audio::WakeWord for FakeWakeWord {
    fn score(&self) -> f32 {
        self.score
    }

    fn push(&mut self, frame: &[f32]) -> bool {
        self.score = frame[0];
        let was_in_wake = std::mem::replace(&mut self.in_wake, frame[0] == WAKE);
        self.in_wake && !was_in_wake
    }
}

#[derive(Clone)]
pub struct FakeClock(Arc<ClockShared>);

struct ClockShared {
    start: tokio::time::Instant,
    state: Mutex<ClockState>,
    settled: Condvar,
}

#[derive(Default)]
struct ClockState {
    elapsed: Duration,
    next_id: u64,
    timers: HashMap<u64, (Option<tokio::time::Instant>, Option<Waker>)>,
}

impl ClockState {
    fn now(&self, start: tokio::time::Instant) -> tokio::time::Instant {
        start + self.elapsed
    }
}

impl FakeClock {
    fn new() -> Self {
        FakeClock(Arc::new(ClockShared {
            start: tokio::time::Instant::now(),
            state: Mutex::default(),
            settled: Condvar::new(),
        }))
    }

    pub fn advance(&self, by: Duration) {
        let shared = &self.0;
        let mut state = shared.state.lock().unwrap();
        state.elapsed += by;
        let now = state.now(shared.start);
        for (_, waker) in state.timers.values_mut() {
            if let Some(waker) = waker.take() {
                waker.wake();
            }
        }
        let due = |state: &mut ClockState| {
            state
                .timers
                .values()
                .any(|(deadline, _)| deadline.is_some_and(|d| d <= now))
        };
        let (state, waited) = shared.settled.wait_timeout_while(state, WAIT, due).unwrap();
        drop(state);
        assert!(!waited.timed_out(), "a timer went off but wasn't handled");
    }

    pub fn advance_once_set(&self, by: Duration) {
        self.wait_for("a timer set to go off", true, by);
        self.advance(by);
    }

    pub fn advance_once_stopped(&self, by: Duration) {
        self.wait_for("every timer stopped", false, by);
        self.advance(by);
    }

    fn wait_for(&self, what: &str, due: bool, by: Duration) {
        let shared = &self.0;
        let state = shared.state.lock().unwrap();
        let waiting = |state: &mut ClockState| {
            let until = state.now(shared.start) + by;
            let set = state
                .timers
                .values()
                .any(|(deadline, _)| deadline.is_some_and(|d| d <= until));
            set != due
        };
        let (state, waited) = shared
            .settled
            .wait_timeout_while(state, WAIT, waiting)
            .unwrap();
        drop(state);
        assert!(!waited.timed_out(), "timed out waiting for {what}");
    }
}

impl Clock for FakeClock {
    fn now(&self) -> tokio::time::Instant {
        self.0.state.lock().unwrap().now(self.0.start)
    }

    fn timer(&self) -> Box<dyn Timer> {
        let mut state = self.0.state.lock().unwrap();
        let id = state.next_id;
        state.next_id += 1;
        state.timers.insert(id, (None, None));
        Box::new(FakeTimer {
            clock: self.0.clone(),
            id,
        })
    }
}

struct FakeTimer {
    clock: Arc<ClockShared>,
    id: u64,
}

impl Timer for FakeTimer {
    fn set(&mut self, deadline: Option<tokio::time::Instant>) {
        let mut state = self.clock.state.lock().unwrap();
        if let Some((set, waker)) = state.timers.get_mut(&self.id) {
            *set = deadline;
            if let Some(waker) = waker.take() {
                waker.wake();
            }
        }
        self.clock.settled.notify_all();
    }

    fn fired(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let clock = self.clock.clone();
        let id = self.id;
        Box::pin(std::future::poll_fn(move |cx| {
            let mut state = clock.state.lock().unwrap();
            let now = state.now(clock.start);
            match state.timers.get_mut(&id) {
                Some((Some(deadline), _)) if *deadline <= now => Poll::Ready(()),
                Some((_, waker)) => {
                    *waker = Some(cx.waker().clone());
                    Poll::Pending
                }
                None => Poll::Pending,
            }
        }))
    }
}

impl Drop for FakeTimer {
    fn drop(&mut self) {
        self.clock.state.lock().unwrap().timers.remove(&self.id);
        self.clock.settled.notify_all();
    }
}
