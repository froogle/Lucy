#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lucy::menu::Open;
pub use lucy::ui::Script;
use lucy::ui::Ui;
use lucy_config::credentials::CredentialStore;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

pub struct Home {
    pub dir: tempfile::TempDir,
    _lucyd: Option<lucyd::Daemon>,
}

impl Home {
    pub fn with_lucyd() -> Home {
        Home::with_adapters(lucyd::adapters::Adapters::inert())
    }

    fn with_adapters(adapters: lucyd::adapters::Adapters) -> Home {
        let home = Home::without_lucyd();
        let daemon = home.spawn_lucyd_with(adapters);
        Home {
            _lucyd: Some(daemon),
            ..home
        }
    }

    pub fn spawn_lucyd(&self) -> lucyd::Daemon {
        self.spawn_lucyd_with(lucyd::adapters::Adapters::inert())
    }

    fn spawn_lucyd_with(&self, adapters: lucyd::adapters::Adapters) -> lucyd::Daemon {
        std::fs::write(self.config_dir().join("persona.md"), "You are Lucy.").unwrap();
        lucyd::spawn(lucyd::Options {
            socket_path: self.runtime().join("lucy").join("lucyd.sock"),
            config_path: self.config_path(),
            persona_path: self.config_dir().join("persona.md"),
            lucy_program: env!("CARGO_BIN_EXE_lucy").into(),
            adapters,
            plugin_paths: lucyd::plugins::Paths {
                user: self.dir.path().join("plugins"),
                shipped: self.dir.path().join("shipped"),
                state: self.state(),
            },
            plugin_env: lucyd::plugins::Environment::from_process(),
        })
        .unwrap()
    }

    pub fn ask_typed(
        &self,
        question: &str,
        kind: lucyd::FieldKind,
    ) -> std::sync::mpsc::Receiver<lucyd::TypedAnswer> {
        self.ask_typed_about(question, kind, Vec::new())
    }

    pub fn ask_typed_about(
        &self,
        question: &str,
        kind: lucyd::FieldKind,
        items: Vec<String>,
    ) -> std::sync::mpsc::Receiver<lucyd::TypedAnswer> {
        self.env().connect().unwrap().start_session().unwrap();
        let daemon = self._lucyd.as_ref().expect("lucyd runs in this home");
        let answer = daemon.ask_typed_about(question, kind, items);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(answer.blocking_recv().expect("lucyd answers")));
        rx
    }

    pub fn with_mic(level: f32) -> Home {
        let ears = lucyd::audio::listen(
            Box::new(SteadyMic(level)),
            &lucy_config::Audio::default(),
            Box::new(DeafVad),
            None,
            None,
            &lucy_config::Endpointing::default(),
        )
        .unwrap();
        Home::with_adapters(lucyd::adapters::Adapters {
            speech: ears.speech,
            wake_word: ears.wake_word,
            speaking: ears.speaking,
            hearing: Some(ears.hearing),
            ..lucyd::adapters::Adapters::inert()
        })
    }

    pub fn without_lucyd() -> Home {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["runtime", "config/lucy", "bin", "state"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        Home { dir, _lucyd: None }
    }

    pub fn runtime(&self) -> PathBuf {
        self.dir.path().join("runtime")
    }

    pub fn config_dir(&self) -> PathBuf {
        self.dir.path().join("config/lucy")
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir().join("config.toml")
    }

    pub fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    pub fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    pub fn fake(&self, name: &str, body: &str) {
        let log = self.bin().join(format!("{name}.log"));
        let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n{body}\n", log.display());
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg("cat > \"$1\" && chmod +x \"$1\"")
            .arg("sh")
            .arg(self.bin().join(name))
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait()
            })
            .unwrap();
        assert!(status.success());
    }

    pub fn fake_pactl(
        &self,
        sources: &[(&str, &str)],
        sinks: &[(&str, &str)],
        defaults: (&str, &str),
    ) {
        let json = |devices: &[(&str, &str)]| {
            let listed: Vec<_> = devices
                .iter()
                .map(|(name, description)| {
                    serde_json::json!({"index": 1, "name": name, "description": description})
                })
                .collect();
            serde_json::to_string(&listed).unwrap()
        };
        let (sources, sinks) = (json(sources), json(sinks));
        let (source, sink) = defaults;
        self.fake(
            "pactl",
            &format!(
                "case \"$*\" in \
                 '-f json list sources') printf '%s' '{sources}';; \
                 '-f json list sinks') printf '%s' '{sinks}';; \
                 get-default-source) echo '{source}';; \
                 get-default-sink) echo '{sink}';; \
                 esac"
            ),
        );
    }

    pub fn calls(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.bin().join(format!("{name}.log")))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn env(&self) -> lucy::Env {
        lucy::Env {
            config: self.config_path(),
            path: self.bin().into(),
            socket: Some(self.runtime().join("lucy").join("lucyd.sock")),
            units: self.dir.path().join("config/systemd/user"),
            omarchy: self.dir.path().join("config/omarchy"),
            bin: Path::new(env!("CARGO_BIN_EXE_lucy"))
                .parent()
                .unwrap()
                .to_owned(),
            os_release: "/etc/os-release".into(),
            kokoro: self.dir.path().join("data/lucy/kokoro"),
        }
    }

    pub fn tui(&self, open: Open, keys: Script) -> Tui {
        self.tui_with(&self.env(), &FakeStore::default(), open, keys)
    }

    pub fn tui_with(
        &self,
        env: &lucy::Env,
        store: &dyn CredentialStore,
        open: Open,
        mut keys: Script,
    ) -> Tui {
        let terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        let mut ui = Ui::new(terminal, &mut keys, "Lucy");
        let result = lucy::menu::run(&mut ui, env, store, open);
        let tui = Tui {
            result,
            shown: ui.screen.shown(),
            screen: ui.terminal().backend().to_string(),
        };
        drop(ui);
        assert!(keys.is_done(), "keys left over:\n{}", tui.shown);
        tui
    }

    pub fn lucy(&self, args: &[&str], answers: &[&str]) -> Output {
        let mut child = self.command(args).spawn().unwrap();
        let mut input = answers.join("\n");
        input.push('\n');
        let _may_stop_reading_early = child.stdin.take().unwrap().write_all(input.as_bytes());
        child.wait_with_output().unwrap()
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lucy"));
        command
            .args(args)
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.dir.path())
            .env("XDG_RUNTIME_DIR", self.runtime())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_STATE_HOME", self.dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    pub fn parented(&self, args: &[&str], stdin: ParentStdin) -> Parented {
        let lucy = self.command(args);
        let mut bash = Command::new("/bin/bash");
        bash.arg("-c")
            .arg(stdin.script())
            .arg("bash")
            .arg(lucy.get_program())
            .args(lucy.get_args())
            .env_clear()
            .envs(
                lucy.get_envs()
                    .filter_map(|(name, value)| Some((name, value?))),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut parent = bash.spawn().unwrap();
        let stdout = BufReader::new(parent.stdout.take().unwrap());
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                let _ = send.send(line);
            }
        });
        Parented { parent, lines }
    }
}

pub enum ParentStdin {
    Null,
    Held,
}

impl ParentStdin {
    fn script(&self) -> &'static str {
        match self {
            ParentStdin::Null => r#""$@" </dev/null & wait"#,
            ParentStdin::Held => r#"exec 3>&1; coproc "$@" >&3; wait"#,
        }
    }
}

pub struct Parented {
    parent: Child,
    lines: Receiver<String>,
}

const SOON: Duration = Duration::from_secs(10);

impl Parented {
    pub fn next(&self) -> serde_json::Value {
        let line = self.lines.recv_timeout(SOON).expect("a line within 10 s");
        serde_json::from_str(&line).unwrap()
    }

    pub fn kill_parent(&mut self) {
        self.parent.kill().unwrap();
        self.parent.wait().unwrap();
    }

    pub fn exits_soon(&self) -> bool {
        let deadline = Instant::now() + SOON;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => return true,
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

impl Drop for Parented {
    fn drop(&mut self) {
        let _ = self.parent.kill();
        let _ = self.parent.wait();
    }
}

struct SteadyMic(f32);

impl lucyd::audio::AudioIo for SteadyMic {
    fn capture(
        &self,
        _mic: &str,
        mut ring: rtrb::Producer<f32>,
    ) -> Result<Box<dyn Send>, lucyd::audio::Error> {
        let peak = self.0 * std::f32::consts::SQRT_2;
        std::thread::spawn(move || {
            let mut n = 0usize;
            while !ring.is_abandoned() {
                while ring.slots() > 0 {
                    let t = n as f32 / 16_000.0;
                    let _ = ring.push(peak * (std::f32::consts::TAU * 300.0 * t).sin());
                    n += 1;
                }
                // sleep-ok: paces the microphone to roughly real time
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        Ok(Box::new(()))
    }

    fn has_device(&self, _direction: lucyd::audio::Direction, _name: &str) -> bool {
        true
    }

    fn is_pipewire(&self) -> bool {
        true
    }
}

struct DeafVad;

impl lucyd::audio::Vad for DeafVad {
    fn chunk_len(&self) -> usize {
        512
    }

    fn speech_probability(&mut self, _chunk: &[f32]) -> f32 {
        0.0
    }
}

pub struct Tui {
    pub result: Result<(), String>,
    pub shown: String,
    pub screen: String,
}

#[derive(Default)]
pub struct FakeStore(Mutex<HashMap<(String, String), String>>);

impl FakeStore {
    pub fn value(&self, plugin: &str, name: &str) -> Option<String> {
        let key = (plugin.to_owned(), name.to_owned());
        self.0.lock().unwrap().get(&key).cloned()
    }
}

impl CredentialStore for FakeStore {
    fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, String> {
        Ok(self.value(plugin, name))
    }

    fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), String> {
        let key = (plugin.to_owned(), name.to_owned());
        self.0.lock().unwrap().insert(key, value.to_owned());
        Ok(())
    }
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}
