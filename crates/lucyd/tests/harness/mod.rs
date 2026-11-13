//! The lucyd harness: starts lucyd in-process with fake adapters and observes
//! it only through the real public IPC, using the `lucy-ipc` client.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use lucy_config::Config;
use lucy_ipc::{Client, Event, StateEvent, Topic};
use lucyd::adapters::{
    Adapters, Conversation, Relay, Speaker, SpeechSegment, Stt, Thinking, ThinkingEvent, Tts,
};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use tokio::sync::mpsc;

/// How long the harness waits for something lucyd should do.
const WAIT: Duration = Duration::from_secs(5);

pub struct Harness {
    // Dropped first, so lucyd stops before its runtime dir goes away.
    daemon: lucyd::Daemon,
    speech: mpsc::UnboundedSender<SpeechSegment>,
    pub stt: FakeStt,
    pub thinking: FakeThinking,
    pub tts: FakeTts,
    /// Every event line seen on the bus by a client subscribed from the start.
    bus: Arc<Mutex<Vec<String>>>,
    _dir: tempfile::TempDir,
}

impl Harness {
    /// Starts lucyd with default config and a socket in a fresh runtime dir.
    pub fn start() -> Harness {
        Harness::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("lucy").join("lucyd.sock");
        let (speech, speech_rx) = mpsc::unbounded_channel();
        let stt = FakeStt::default();
        let thinking = FakeThinking::new();
        let tts = FakeTts::new();
        let daemon = lucyd::spawn(lucyd::Options {
            socket_path,
            config,
            lucy_program: lucy_binary(),
            adapters: Adapters {
                speech: speech_rx,
                stt: Arc::new(stt.clone()),
                thinking: Box::new(thinking.clone()),
                tts: Box::new(tts.clone()),
            },
        })
        .expect("lucyd starts");
        let harness = Harness {
            daemon,
            speech,
            stt,
            thinking,
            tts,
            bus: Arc::default(),
            _dir: dir,
        };
        harness.record_bus();
        harness
    }

    pub fn socket_path(&self) -> &Path {
        self.daemon.socket_path()
    }

    pub fn mcp_socket_path(&self) -> &Path {
        self.daemon.mcp_socket_path()
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

    /// A client that has completed the `hello` handshake.
    pub fn client(&self) -> Client {
        let client = Client::connect(self.socket_path(), "harness", None).expect("hello succeeds");
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

    /// The user says something that the fake STT will hear as `transcript`.
    pub fn say(&self, transcript: &str) {
        self.stt.script(transcript);
        self.speech.send(SpeechSegment { samples: vec![] }).unwrap();
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

    /// The recorder has seen a Session and then Idle again.
    fn bus_is_idle(&self) -> bool {
        let bus = self.bus.lock().unwrap();
        let idle = |line: &String| line.contains(r#""state":"idle""#);
        bus.len() > 1 && bus.last().is_some_and(idle)
    }
}

/// The next event, which must be a `state` event.
pub fn next_state(client: &mut Client) -> StateEvent {
    match client.next_event().expect("a state event") {
        Event::State(state) => state,
    }
}

/// The real `lucy` binary, which cargo builds next to this test's binary
/// when it builds the `lucy` package.
fn lucy_binary() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let lucy = exe.parent().unwrap().parent().unwrap().join("lucy");
    assert!(
        lucy.exists(),
        "{} not found; run `cargo build -p lucy` or `cargo test --workspace`",
        lucy.display()
    );
    lucy
}

/// Waits for `receiver`, failing the test rather than hanging.
fn recv<T>(receiver: &Mutex<std_mpsc::Receiver<T>>, what: &str) -> T {
    receiver
        .lock()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// STT that hears whatever the test scripted, in order.
#[derive(Clone, Default)]
pub struct FakeStt {
    scripted: Arc<Mutex<VecDeque<String>>>,
    heard: Arc<Mutex<Vec<String>>>,
}

impl FakeStt {
    fn script(&self, transcript: &str) {
        self.scripted
            .lock()
            .unwrap()
            .push_back(transcript.to_owned());
    }

    fn heard(&self) -> Vec<String> {
        self.heard.lock().unwrap().clone()
    }
}

impl Stt for FakeStt {
    fn transcribe(&self, _segment: SpeechSegment) -> String {
        let transcript = self
            .scripted
            .lock()
            .unwrap()
            .pop_front()
            .expect("a scripted transcript");
        self.heard.lock().unwrap().push(transcript.clone());
        transcript
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
    /// The open Conversation's event stream.
    events: Arc<Mutex<Option<mpsc::UnboundedSender<ThinkingEvent>>>>,
    /// How the open Conversation's model reaches Lucy's tools.
    relay: Arc<Mutex<Option<Relay>>>,
    streamed: Arc<Mutex<Vec<String>>>,
}

impl FakeThinking {
    fn new() -> Self {
        let (turns_tx, turns) = std_mpsc::channel();
        FakeThinking {
            turns_tx,
            turns: Arc::new(Mutex::new(turns)),
            events: Arc::default(),
            relay: Arc::default(),
            streamed: Arc::default(),
        }
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

    /// The model calls one of Lucy's tools through the real relay.
    pub fn call_tool(&self, name: &str) -> Result<CallToolResult, String> {
        self.with_mcp(|client| async move {
            client
                .call_tool(CallToolRequestParams::new(name.to_owned()))
                .await
                .map_err(|e| e.to_string())
        })
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

    /// Spawns the relay the last Conversation was given, as the model
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
            .expect("a Conversation was opened");
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

    fn send(&self, event: ThinkingEvent) {
        let events = self.events.lock().unwrap();
        let events = events.as_ref().expect("a Conversation is open");
        events
            .send(event)
            .expect("lucyd is reading the Conversation");
    }

    fn streamed(&self) -> Vec<String> {
        self.streamed.lock().unwrap().clone()
    }
}

impl Thinking for FakeThinking {
    fn open(
        &self,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
    ) -> Box<dyn Conversation> {
        *self.events.lock().unwrap() = Some(events);
        *self.relay.lock().unwrap() = Some(relay);
        Box::new(FakeConversation {
            turns: self.turns_tx.clone(),
        })
    }
}

struct FakeConversation {
    turns: std_mpsc::Sender<String>,
}

impl Conversation for FakeConversation {
    fn send_turn(&mut self, text: String) {
        let _ = self.turns.send(text);
    }
}

/// TTS that records each sentence and "plays" it until the test finishes it.
#[derive(Clone)]
pub struct FakeTts {
    sentences_tx: std_mpsc::Sender<String>,
    sentences: Arc<Mutex<std_mpsc::Receiver<String>>>,
    spoken: Arc<Mutex<Vec<String>>>,
    /// The open Speaker's "finished playing" channel.
    played: Arc<Mutex<Option<mpsc::UnboundedSender<()>>>>,
}

impl FakeTts {
    fn new() -> Self {
        let (sentences_tx, sentences) = std_mpsc::channel();
        FakeTts {
            sentences_tx,
            sentences: Arc::new(Mutex::new(sentences)),
            spoken: Arc::default(),
            played: Arc::default(),
        }
    }

    /// Waits for the next sentence handed to TTS.
    pub fn next_sentence(&self) -> String {
        recv(&self.sentences, "a sentence sent to TTS")
    }

    /// Every sentence handed to TTS so far.
    pub fn sentences(&self) -> Vec<String> {
        self.spoken.lock().unwrap().clone()
    }

    /// The oldest queued sentence finishes playing.
    pub fn finish_playing(&self) {
        let played = self.played.lock().unwrap();
        let played = played.as_ref().expect("a Speaker is open");
        played.send(()).expect("lucyd is listening for playback");
    }
}

impl Tts for FakeTts {
    fn open(&self, played: mpsc::UnboundedSender<()>) -> Box<dyn Speaker> {
        *self.played.lock().unwrap() = Some(played);
        Box::new(FakeSpeaker {
            sentences: self.sentences_tx.clone(),
            spoken: self.spoken.clone(),
        })
    }
}

struct FakeSpeaker {
    sentences: std_mpsc::Sender<String>,
    spoken: Arc<Mutex<Vec<String>>>,
}

impl Speaker for FakeSpeaker {
    fn speak(&mut self, sentence: String) {
        self.spoken.lock().unwrap().push(sentence.clone());
        let _ = self.sentences.send(sentence);
    }
}
