//! The lucyd harness: starts lucyd in-process with fake adapters and observes
//! it only through the real public IPC, using the `lucy-ipc` client.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use lucy_config::{Config, Provider};
use lucy_ipc::{Client, Event, StateEvent, Topic};
use lucyd::adapters::{
    Adapters, Launcher, Notification, NotificationAction, Notifier, RateLimit, Relay, SessionModel,
    Sound, Speaker, SpeakerEvent, SpeechSegment, Stt, SttError, Thinking, ThinkingEvent, Tts,
    Workspace,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use tokio::sync::{mpsc, watch};

/// How long the harness waits for something lucyd should do.
const WAIT: Duration = Duration::from_secs(5);

pub struct Harness {
    // Dropped first, so lucyd stops before its runtime dir goes away.
    daemon: lucyd::Daemon,
    /// Where `say` sends utterances, unless lucyd hears through `mic`.
    speech: Option<mpsc::UnboundedSender<SpeechSegment>>,
    /// Where `wake` sends scripted detections, unless lucyd hears through
    /// `mic`.
    wake_word: Option<mpsc::UnboundedSender<()>>,
    /// Where `start_speaking` and `say` say whether the user is mid
    /// utterance, unless lucyd hears through `mic`.
    speaking: Option<watch::Sender<bool>>,
    pub mic: FakeMic,
    /// Lucy's echo canceller, under `hearing`.
    pub echo: FakeEchoCanceller,
    pub stt: FakeStt,
    pub thinking: FakeThinking,
    pub tts: FakeTts,
    pub notifier: FakeNotifier,
    pub launcher: FakeLauncher,
    /// Where lucyd looks for desktop entries: the user's dir, then the
    /// system's.
    applications: [PathBuf; 2],
    /// Every event line seen on the bus by a client subscribed from the start.
    bus: Arc<Mutex<Vec<String>>>,
    config_path: PathBuf,
    plugin_paths: lucyd::plugins::Paths,
    persona_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// The `persona.md` of a user who has had a Session before.
const PERSONA: &str = "You are Lucy.";

impl Harness {
    /// Starts lucyd with no config file and a socket in a fresh runtime dir,
    /// for a user who already has a Persona.
    pub fn start() -> Harness {
        Harness::launch(None, Some(PERSONA), None, None, None)
    }

    /// Starts lucyd with `config` as its `config.toml`.
    pub fn with_config(config: &str) -> Harness {
        Harness::launch(Some(config), Some(PERSONA), None, None, None)
    }

    /// Starts lucyd for a user who has never had a Session: no `persona.md`.
    pub fn first_run() -> Harness {
        Harness::launch(None, None, None, None, None)
    }

    /// Starts lucyd with a real Thinking adapter in place of the fake.
    pub fn with_thinking(config: Option<&str>, thinking: Arc<dyn Thinking>) -> Harness {
        Harness::launch(config, Some(PERSONA), Some(thinking), None, None)
    }

    /// Starts lucyd with `superwhisper` as its `superwhisper` STT Provider
    /// and `config` as its `config.toml`.
    pub fn with_superwhisper(config: &str, superwhisper: Arc<dyn Stt>) -> Harness {
        Harness::launch(Some(config), Some(PERSONA), None, None, Some(superwhisper))
    }

    /// Starts lucyd with `superwhisper` as its `superwhisper` STT Provider
    /// and a real Thinking adapter in place of the fake.
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

    /// Starts lucyd hearing through the fake microphone, VAD and wake word,
    /// with the real DSP thread and endpointing in between, and `config` as
    /// its `config.toml`. lucyd starts listening with its microphone and
    /// endpointing, as `main` does, and reads them again at each Session
    /// start.
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

    /// `ears` is what lucyd hears through instead of `say`.
    fn launch(
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
                applications: applications.to_vec(),
            },
        })
        .expect("lucyd starts");
        let harness = Harness {
            daemon,
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
            applications,
            bus: Arc::default(),
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

    /// The `config.toml` lucyd reads at each Session start.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn mcp_socket_path(&self) -> &Path {
        self.daemon.mcp_socket_path()
    }

    /// A scratch directory for the test's own files, outside every Plugin
    /// folder and Lucy's state.
    pub fn scratch(&self) -> PathBuf {
        let scratch = self._dir.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        scratch
    }

    /// Where user and Lucy-written Plugins live.
    pub fn plugins_dir(&self) -> &Path {
        &self.plugin_paths.user
    }

    /// Lucy's state dir.
    pub fn state_dir(&self) -> &Path {
        &self.plugin_paths.state
    }

    /// Installs a user Plugin: `manifest` plus the fixture Plugin executable
    /// as `./fixture-plugin`. Returns its folder.
    pub fn install_plugin(&self, name: &str, manifest: &str) -> PathBuf {
        install(&self.plugin_paths.user.join(name), manifest)
    }

    /// Installs a shipped Plugin, as `install_plugin` does.
    pub fn install_shipped_plugin(&self, name: &str, manifest: &str) -> PathBuf {
        install(&self.plugin_paths.shipped.join(name), manifest)
    }

    /// Records an Approval for the Plugin `name` as it is now (the user copy
    /// if there is one), saving an input schema for each Manifest tool that
    /// marks it as approved: `{"type": "object", "description": "approved
    /// <tool>", "properties": {..}}`.
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

    /// Starts the spoken Approval of the Plugin `name` in the open Session,
    /// as setup or a finished build would.
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

    /// The user's `persona.md`.
    pub fn persona_path(&self) -> &Path {
        &self.persona_path
    }

    /// Runs `lucy mcp-relay` against lucyd's MCP socket with `args` after the
    /// socket option, and no input.
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

    /// Installs a user's desktop entry named `file_name`, of type
    /// Application, with the given `[Desktop Entry]` keys. Returns its path.
    pub fn desktop_entry(&self, file_name: &str, keys: &str) -> PathBuf {
        write_desktop_entry(&self.applications[0], file_name, keys)
    }

    /// Installs a system-wide desktop entry, as `desktop_entry` does.
    pub fn system_desktop_entry(&self, file_name: &str, keys: &str) -> PathBuf {
        write_desktop_entry(&self.applications[1], file_name, keys)
    }

    /// A client that has completed the `hello` handshake.
    pub fn client(&self) -> Client {
        let client = Client::connect(self.socket_path(), "harness", None).expect("hello succeeds");
        client.set_read_timeout(Some(WAIT)).unwrap();
        client
    }

    /// A client that claims the `indicator` role, as `lucy-indicator` or a
    /// third-party Indicator would.
    pub fn indicator(&self, name: &str) -> Client {
        let client = Client::connect(self.socket_path(), name, Some(lucy_ipc::Role::Indicator))
            .expect("hello succeeds");
        client.set_read_timeout(Some(WAIT)).unwrap();
        client
    }

    /// A client subscribed to `state`, with the initial state event consumed.
    pub fn state_client(&self) -> Client {
        let mut client = self.client();
        client.subscribe(&[Topic::State]).unwrap();
        next_state(&mut client);
        client
    }

    /// The user starts an utterance, which `say` ends.
    pub fn start_speaking(&self) {
        let speaking = self.speaking.as_ref().expect("lucyd hears through `mic`");
        speaking.send_replace(true);
    }

    /// The user says something that the fake STT will hear as `transcript`.
    pub fn say(&self, transcript: &str) {
        self.stt.script(transcript);
        self.end_utterance();
    }

    /// The user says something and the fake STT fails on it.
    pub fn say_but_stt_fails(&self) {
        self.stt.script_failure();
        self.end_utterance();
    }

    /// The VAD cuts the utterance, as the DSP thread does: the utterance,
    /// then the user no longer speaking.
    fn end_utterance(&self) {
        let speech = self.speech.as_ref().expect("lucyd hears through `mic`");
        speech.send(SpeechSegment { samples: vec![] }).unwrap();
        self.speaking.as_ref().unwrap().send_replace(false);
    }

    /// The wake-word engine hears the Wake word.
    pub fn wake(&self) {
        let wake_word = self.wake_word.as_ref().expect("lucyd hears through `mic`");
        wake_word.send(()).unwrap();
    }

    /// The user makes a sound that the fake STT recognises as no speech.
    pub fn mumble(&self) {
        self.say("");
    }

    /// Sends one raw line on a fresh connection and returns the reply line,
    /// for checking the wire format without the client in the way.
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

    /// The "never on the bus" rule: no transcript the STT produced and no text
    /// the Thinking Provider streamed or TTS spoke appears in any IPC message.
    /// Call it once a Session has ended.
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

    /// The recorder, subscribed to `state` only, never received a `level`
    /// event. Call it after `assert_nothing_private_on_bus`.
    pub fn assert_no_level_on_bus(&self) {
        let bus = self.bus.lock().unwrap();
        let level = bus.iter().find(|line| line.contains(r#""method":"level""#));
        assert!(level.is_none(), "a state-only subscriber got {level:?}");
    }

    /// The recorder has seen a Session and then Idle again.
    fn bus_is_idle(&self) -> bool {
        let bus = self.bus.lock().unwrap();
        let idle = |line: &String| line.contains(r#""state":"idle""#);
        bus.len() > 1 && bus.last().is_some_and(idle)
    }
}

/// A spoken Approval in progress.
pub struct ApprovalRequest(Mutex<std_mpsc::Receiver<Result<bool, String>>>);

impl ApprovalRequest {
    /// Waits for it to end: whether the user approved, or why they couldn't
    /// be asked.
    pub fn outcome(&self) -> Result<bool, String> {
        recv(&self.0, "the Approval to end")
    }
}

/// A connection to Lucy's tools that the model process keeps open, counting
/// the `tools/list_changed` notifications it gets.
pub struct ToolWatch {
    changed: Mutex<std_mpsc::Receiver<()>>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl ToolWatch {
    /// Waits for a `tools/list_changed`.
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

/// Writes a desktop entry of type Application with `keys`; a `Type` key in
/// `keys` overrides it.
fn write_desktop_entry(dir: &Path, file_name: &str, keys: &str) -> PathBuf {
    let path = dir.join(file_name);
    std::fs::write(
        &path,
        format!("[Desktop Entry]\nType=Application\n{keys}\n"),
    )
    .unwrap();
    path
}

/// Polls until `found` returns something, or fails saying `what`.
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

/// The next event, which must be a `state` event.
pub fn next_state(client: &mut Client) -> StateEvent {
    match client.next_event().expect("a state event") {
        Event::State(state) => state,
        other => panic!("expected a state event, got {other:?}"),
    }
}

/// The profile dir cargo puts final binaries in (`target/debug`). Test
/// binaries sit somewhere below it, and how deep depends on the cargo version,
/// so walk up to the first ancestor holding `name` as a file.
fn in_profile_dir(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .skip(1)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| exe.parent().unwrap().join(name))
}

/// The real `lucy` binary, which cargo builds when it builds the `lucy`
/// package.
fn lucy_binary() -> PathBuf {
    let lucy = in_profile_dir("lucy");
    assert!(
        lucy.exists(),
        "{} not found; run `cargo build -p lucy` or `cargo test --workspace`",
        lucy.display()
    );
    lucy
}

/// The fixture Plugin executable (`examples/fixture-plugin.rs`).
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
    // `cp` copies it so that no write fd here leaks into a child another
    // test forks and makes running it fail with "Text file busy".
    let copied = Command::new("cp")
        .arg(fixture_plugin())
        .arg(dir.join("fixture-plugin"))
        .status()
        .unwrap();
    assert!(copied.success());
    dir.to_owned()
}

/// What an Approval pins: SHA-256 over the Manifest's length (u64, little
/// endian), the Manifest, then the executable, as lowercase hex.
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

/// Waits for `receiver`, failing the test rather than hanging.
fn recv<T>(receiver: &Mutex<std_mpsc::Receiver<T>>, what: &str) -> T {
    receiver
        .lock()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// STT that hears whatever the test scripted, in order, or fails where it
/// was scripted to.
#[derive(Clone, Default)]
pub struct FakeStt {
    scripted: Arc<Mutex<VecDeque<Result<String, SttError>>>>,
    heard: Arc<Mutex<Vec<String>>>,
    /// How long each utterance it was given lasted.
    segments: Arc<Mutex<Vec<Duration>>>,
    /// How long each transcription takes.
    delay: Arc<Mutex<Duration>>,
}

impl FakeStt {
    /// Makes each transcription take `delay`.
    pub fn take(&self, delay: Duration) {
        *self.delay.lock().unwrap() = delay;
    }

    /// The next utterance will be heard as `transcript`.
    pub fn script(&self, transcript: &str) {
        self.scripted
            .lock()
            .unwrap()
            .push_back(Ok(transcript.to_owned()));
    }

    /// The next utterance fails to transcribe.
    pub fn script_failure(&self) {
        let failure = SttError("the fake STT failed".to_owned());
        self.scripted.lock().unwrap().push_back(Err(failure));
    }

    fn heard(&self) -> Vec<String> {
        self.heard.lock().unwrap().clone()
    }

    /// How long each utterance transcribed so far lasted, at 16 kHz.
    pub fn segments(&self) -> Vec<Duration> {
        self.segments.lock().unwrap().clone()
    }
}

impl Stt for FakeStt {
    fn transcribe(&self, segment: SpeechSegment) -> Result<String, SttError> {
        let length = Duration::from_secs_f64(segment.samples.len() as f64 / 16_000.0);
        self.segments.lock().unwrap().push(length);
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

/// A scripted Thinking Provider: the test reads each Turn's text and streams
/// the reply event by event, in the shape the real `claude` adapter streams a
/// recorded transcript (`tests/claude.rs`): deltas, a `SentenceEnd` after each
/// sentence, then `TurnEnd`.
#[derive(Clone)]
pub struct FakeThinking {
    turns_tx: std_mpsc::Sender<String>,
    turns: Arc<Mutex<std_mpsc::Receiver<String>>>,
    /// The open SessionModel's event stream.
    events: Arc<Mutex<Option<mpsc::UnboundedSender<ThinkingEvent>>>>,
    /// How the open SessionModel reaches Lucy's tools.
    relay: Arc<Mutex<Option<Relay>>>,
    /// The instructions the open SessionModel was given.
    instructions: Arc<Mutex<Option<String>>>,
    streamed: Arc<Mutex<Vec<String>>>,
    opened: Arc<Mutex<Vec<Provider>>>,
    /// How many times a Turn was interrupted.
    interrupts: Arc<Mutex<usize>>,
    /// Problems scripted for `health` and `check`, by model.
    unhealthy: Arc<Mutex<HashMap<String, String>>>,
    failing_checks: Arc<Mutex<HashMap<String, String>>>,
    /// The scripted Delegate, which `open_delegate` opens.
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

    /// Health checks of a Provider using `model` report `problem`.
    pub fn unhealthy(&self, model: &str, problem: &str) {
        let mut unhealthy = self.unhealthy.lock().unwrap();
        unhealthy.insert(model.to_owned(), problem.to_owned());
    }

    /// Round-trip checks of a Provider using `model` report `problem`.
    pub fn fails_check(&self, model: &str, problem: &str) {
        let mut failing = self.failing_checks.lock().unwrap();
        failing.insert(model.to_owned(), problem.to_owned());
    }

    /// The Provider settings of each SessionModel opened, in order.
    pub fn opened(&self) -> Vec<Provider> {
        self.opened.lock().unwrap().clone()
    }

    /// How many times lucyd has interrupted the model's Turn.
    pub fn interrupts(&self) -> usize {
        *self.interrupts.lock().unwrap()
    }

    /// Waits for the text of the next Turn sent to the model.
    pub fn next_turn(&self) -> String {
        recv(&self.turns, "a Turn sent to Thinking")
    }

    /// Fails if a Turn was sent that the test hasn't read.
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

    /// The model process ends on its own.
    pub fn dies(&self) {
        self.send(ThinkingEvent::Died);
    }

    /// The model process reports where the subscription stands.
    pub fn rate_limit(&self, limit: RateLimit) {
        self.send(ThinkingEvent::RateLimit(limit));
    }

    /// The Provider turns out to be unusable, for `reason`.
    pub fn unavailable(&self, reason: &str) {
        self.send(ThinkingEvent::Unavailable(reason.to_owned()));
    }

    /// The instructions the last SessionModel was opened with.
    pub fn instructions(&self) -> String {
        self.instructions
            .lock()
            .unwrap()
            .clone()
            .expect("a SessionModel was opened")
    }

    /// The model calls one of Lucy's tools through the real relay.
    pub fn call_tool(&self, name: &str) -> Result<CallToolResult, String> {
        self.call_tool_with(name, serde_json::json!({}))
    }

    /// The model calls one of Lucy's tools with `arguments`, a JSON object.
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

    /// The tools the model is offered.
    pub fn tools(&self) -> Vec<Tool> {
        self.with_mcp(
            |client| async move { client.list_all_tools().await.map_err(|e| e.to_string()) },
        )
        .unwrap()
    }

    /// The names of the tools the model is offered.
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

    /// Connects to Lucy's tools through the relay and stays connected, as
    /// the model process does, watching for `tools/list_changed`.
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

    /// Spawns the relay the last SessionModel was given, as the model
    /// process would, and runs `f` against it as an MCP client.
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

/// A scripted Delegate: the test reads each Turn it is sent, does its work
/// in the workspace, and calls the Delegate tools through its real relay.
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

    /// The Provider settings of each Delegate opened, in order.
    pub fn opened(&self) -> Vec<Provider> {
        self.opened.lock().unwrap().clone()
    }

    /// Waits for the text of the next Turn sent to the Delegate.
    pub fn next_turn(&self) -> String {
        recv(&self.turns, "a Turn sent to the Delegate")
    }

    /// Fails if a Turn was sent to the Delegate that the test hasn't read.
    pub fn assert_no_turn(&self) {
        let turn = self.turns.lock().unwrap().try_recv();
        assert!(
            turn.is_err(),
            "unexpected Turn sent to the Delegate: {turn:?}"
        );
    }

    /// The system prompt the last Delegate was opened with.
    pub fn instructions(&self) -> String {
        let instructions = self.instructions.lock().unwrap();
        instructions.clone().expect("a Delegate was opened")
    }

    /// Where the last Delegate was opened to work.
    pub fn workspace(&self) -> Workspace {
        let workspace = self.workspace.lock().unwrap();
        workspace.clone().expect("a Delegate was opened")
    }

    /// The Delegate calls one of Lucy's tools with `arguments`, a JSON
    /// object, through its relay.
    pub fn call_tool_with(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, String> {
        call_through(&self.relay(), name, arguments)
    }

    /// The names of the tools the Delegate is offered.
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

    /// The Delegate ends its Turn.
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

/// Writes a Plugin into `dir` as a Delegate would: `manifest`, the fixture
/// Plugin executable as `./fixture-plugin`, and `build.sh` and `test.sh`
/// running `build` and `test` (shell commands).
pub fn stage_plugin(dir: &Path, manifest: &str, build: &str, test: &str) {
    install(dir, manifest);
    std::fs::write(dir.join("build.sh"), format!("{build}\n")).unwrap();
    std::fs::write(dir.join("test.sh"), format!("{test}\n")).unwrap();
}

/// Spawns `relay` as a model process would, and runs `f` against it as an
/// MCP client.
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

/// Calls the tool `name` with `arguments`, a JSON object, through `relay`.
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

/// TTS that records each sentence and "plays" it until the test finishes it,
/// or fails every sentence once told to.
#[derive(Clone)]
pub struct FakeTts {
    sentences_tx: std_mpsc::Sender<String>,
    sentences: Arc<Mutex<std_mpsc::Receiver<String>>>,
    spoken: Arc<Mutex<Vec<String>>>,
    sounds: Arc<Mutex<Vec<Sound>>>,
    /// Every sentence and cue queued, in order.
    queued: Arc<Mutex<Vec<Queued>>>,
    failing: Arc<AtomicBool>,
    /// How many times playback was stopped and flushed.
    stops: Arc<Mutex<usize>>,
    /// Each change to whether playback is ducked, in order.
    ducks: Arc<Mutex<Vec<bool>>>,
    /// The open Speaker's event channel.
    events: Arc<Mutex<Option<mpsc::UnboundedSender<SpeakerEvent>>>>,
    /// The device the open Speaker plays on.
    device: Arc<Mutex<Option<String>>>,
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
        }
    }

    /// The device the last Speaker was opened on.
    pub fn device(&self) -> Option<String> {
        self.device.lock().unwrap().clone()
    }

    /// The output device goes away.
    pub fn lose_device(&self) {
        self.send(SpeakerEvent::DeviceLost);
    }

    /// Waits for the next sentence handed to TTS.
    pub fn next_sentence(&self) -> String {
        recv(&self.sentences, "a sentence sent to TTS")
    }

    /// Every sentence handed to TTS so far.
    pub fn sentences(&self) -> Vec<String> {
        self.spoken.lock().unwrap().clone()
    }

    /// Every cue played so far.
    pub fn sounds(&self) -> Vec<Sound> {
        self.sounds.lock().unwrap().clone()
    }

    /// Every sentence and cue queued so far, in order.
    pub fn queued(&self) -> Vec<Queued> {
        self.queued.lock().unwrap().clone()
    }

    /// How many times lucyd stopped and flushed playback.
    pub fn stops(&self) -> usize {
        *self.stops.lock().unwrap()
    }

    /// Each time lucyd ducked (true) or unducked (false) playback, in order.
    pub fn ducks(&self) -> Vec<bool> {
        self.ducks.lock().unwrap().clone()
    }

    /// The oldest queued sentence finishes playing.
    pub fn finish_playing(&self) {
        self.send(SpeakerEvent::Played);
    }

    /// Playback reports its level.
    pub fn level(&self, rms: f32) {
        self.send(SpeakerEvent::Level(rms));
    }

    /// From now on, every sentence fails to synthesise.
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
    fn open(&self, speaker: &str, events: mpsc::UnboundedSender<SpeakerEvent>) -> Box<dyn Speaker> {
        *self.events.lock().unwrap() = Some(events);
        *self.device.lock().unwrap() = Some(speaker.to_owned());
        Box::new(FakeSpeaker { tts: self.clone() })
    }
}

/// Why the fake TTS fails a sentence, once told to.
pub const FAKE_TTS_FAILURE: &str = "the fake TTS is failing";

/// Something queued on a Speaker.
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

/// Notifications that lucyd showed, in order.
#[derive(Clone)]
pub struct FakeNotifier {
    shown_tx: std_mpsc::Sender<Notification>,
    shown: Arc<Mutex<std_mpsc::Receiver<Notification>>>,
    /// The last notification that offered an action.
    offered: Arc<Mutex<Option<OfferedAction>>>,
}

/// An action, and where lucyd listens for it.
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
        }
    }

    /// Waits for the next notification lucyd shows.
    pub fn next_notification(&self) -> Notification {
        recv(&self.shown, "a notification")
    }

    /// Waits for the next notification of `kind`, skipping any others.
    pub fn next_notification_of(&self, kind: lucyd::adapters::NotificationKind) -> Notification {
        loop {
            let shown = self.next_notification();
            if shown.kind == kind {
                return shown;
            }
        }
    }

    /// Fails if lucyd showed a notification the test hasn't read.
    pub fn assert_no_notification(&self) {
        let shown = self.shown.lock().unwrap().try_recv();
        assert!(shown.is_err(), "unexpected notification: {shown:?}");
    }

    /// The user picks the action of the last notification that offered one.
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
        let _ = self.shown_tx.send(notification);
    }
}

/// A launcher that records what it was asked to open instead of opening it.
#[derive(Clone, Default)]
pub struct FakeLauncher {
    opened: Arc<Mutex<Vec<String>>>,
    launched: Arc<Mutex<Vec<PathBuf>>>,
    /// The configured launcher command of each launch.
    launched_through: Arc<Mutex<Vec<Option<Vec<String>>>>>,
}

impl FakeLauncher {
    /// Every URL opened so far.
    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }

    /// The desktop entry of every app launched so far.
    pub fn launched(&self) -> Vec<PathBuf> {
        self.launched.lock().unwrap().clone()
    }

    /// The `[desktop] launcher` command each app was launched through, in
    /// order; `None` for the default.
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
}

/// A level the fake VAD hears as speech.
pub const SPEECH: f32 = 0.9;
/// A level the fake VAD hears as silence.
pub const SILENCE: f32 = 0.0;
/// A level the fake VAD hears as speech and the fake wake word as the Wake
/// word.
pub const WAKE: f32 = 0.95;

/// A sound card with a microphone the test feeds with samples at 16 kHz.
/// Every device is there unless the test unplugs it.
#[derive(Clone, Default)]
pub struct FakeMic {
    ring: Arc<Mutex<Option<rtrb::Producer<f32>>>>,
    /// The device lucyd asked for.
    opened: Arc<Mutex<Option<String>>>,
    unplugged: Arc<Mutex<Vec<String>>>,
    pulseaudio: Arc<AtomicBool>,
    /// Samples fed so far.
    fed: Arc<Mutex<usize>>,
    /// VAD chunks the fake VAD has judged so far, under `Harness::hearing`.
    judged: Arc<Mutex<usize>>,
}

impl FakeMic {
    /// The sound server is PulseAudio, not PipeWire.
    pub fn use_pulseaudio(&self) {
        self.pulseaudio.store(true, Ordering::SeqCst);
    }

    /// The device with node name `name` is no longer there.
    pub fn unplug(&self, name: &str) {
        self.unplugged.lock().unwrap().push(name.to_owned());
    }

    /// The device with node name `name` is there again.
    pub fn plug_in(&self, name: &str) {
        self.unplugged.lock().unwrap().retain(|n| n != name);
    }

    /// The open microphone goes away: capture stops.
    pub fn lose(&self) {
        drop(self.ring.lock().unwrap().take());
    }

    /// Waits until lucyd has a microphone open.
    pub fn wait_until_open(&self) {
        let deadline = Instant::now() + WAIT;
        while self.ring.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "timed out waiting for capture");
            // sleep-ok: poll loop with a deadline
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Feeds `duration` of samples at `level`, waiting while the ring is full.
    pub fn feed(&self, level: f32, duration: Duration) {
        let len = (duration.as_secs_f64() * 16_000.0).round() as usize;
        self.play(&vec![level; len]);
    }

    /// Waits until the fake VAD has judged every whole chunk fed so far, so
    /// whatever lucyd is told next applies only to what is fed after.
    pub fn wait_until_heard(&self) {
        let chunks = *self.fed.lock().unwrap() / FAKE_VAD_CHUNK;
        wait_until("the VAD never caught up with the microphone", || {
            (*self.judged.lock().unwrap() >= chunks).then_some(())
        });
    }

    /// Feeds `samples`, waiting while the ring is full.
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

    /// The device lucyd opened.
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

/// An echo canceller that passes the microphone through, or silences it all,
/// and reports the ERLE the test sets.
#[derive(Clone, Default)]
pub struct FakeEchoCanceller {
    erle_db: Arc<Mutex<Option<f32>>>,
    cancel_everything: Arc<AtomicBool>,
    frames: Arc<Mutex<usize>>,
}

impl FakeEchoCanceller {
    pub fn set_erle(&self, erle_db: Option<f32>) {
        *self.erle_db.lock().unwrap() = erle_db;
    }

    /// From now on it hears everything as Lucy's echo.
    pub fn cancel_everything(&self) {
        self.cancel_everything.store(true, Ordering::SeqCst);
    }

    /// How many 10 ms frames it has cleaned.
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
}

/// A VAD that hears each chunk's first sample as its speech probability.
/// Counts the chunks it judges.
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

/// A wake word that hears the Wake word where a run of `WAKE` frames starts.
#[derive(Default)]
pub struct FakeWakeWord {
    in_wake: bool,
}

impl lucyd::audio::WakeWord for FakeWakeWord {
    fn push(&mut self, frame: &[f32]) -> bool {
        let was_in_wake = std::mem::replace(&mut self.in_wake, frame[0] == WAKE);
        self.in_wake && !was_in_wake
    }
}
