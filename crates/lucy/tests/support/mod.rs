//! A home of its own for `lucy`, with an in-process lucyd: its own XDG
//! dirs, and a `PATH` holding only the fake tools a test writes. The full
//! screen runs in-process on ratatui's `TestBackend`, pressed by a
//! [`Script`], with a fake keyring; plain commands run the real binary.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

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
    /// A home where lucyd runs, with no adapters that hear or speak.
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

    /// Starts a lucyd with inert adapters in this home, as `systemctl
    /// start` would; it runs until dropped.
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
        })
        .unwrap()
    }

    /// A home where lucyd hears a microphone that stays at `level` (RMS),
    /// with nothing else that hears or speaks.
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

    /// Puts a fake `name` on `PATH` that runs `body` with `sh`, after
    /// logging its arguments to `<name>.log` beside it.
    pub fn fake(&self, name: &str, body: &str) {
        let log = self.bin().join(format!("{name}.log"));
        let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n{body}\n", log.display());
        // Written by a child, so no fd of this process leaks into a fork.
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

    /// Each run of the fake `name`, as its arguments.
    pub fn calls(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.bin().join(format!("{name}.log")))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Where `lucy` finds things in this home: its config, its fakes on
    /// `PATH`, its lucyd, and the `lucyd` cargo builds beside `lucy`.
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
        }
    }

    /// Runs `lucy` full-screen on a test terminal, opened at `open`,
    /// pressing `keys`, with an empty keyring. Panics if a key is left over.
    pub fn tui(&self, open: Open, keys: Script) -> Tui {
        self.tui_with(&self.env(), &FakeStore::default(), open, keys)
    }

    /// As [`Home::tui`], finding things through `env` and keeping
    /// credentials in `store`.
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

    /// Runs `lucy args`, answering its questions with `answers`, one a line.
    pub fn lucy(&self, args: &[&str], answers: &[&str]) -> Output {
        let mut child = self.command(args).spawn().unwrap();
        let mut input = answers.join("\n");
        input.push('\n');
        // It may finish without reading everything.
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        child.wait_with_output().unwrap()
    }

    /// `lucy args` in this home, its stdio piped, not yet started.
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
}

/// A microphone that hears a steady tone at an RMS level, from when lucyd
/// opens it until it lets go.
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

/// A VAD that never hears speech.
struct DeafVad;

impl lucyd::audio::Vad for DeafVad {
    fn chunk_len(&self) -> usize {
        512
    }

    fn speech_probability(&mut self, _chunk: &[f32]) -> f32 {
        0.0
    }
}

/// How a run of `lucy` went.
pub struct Tui {
    pub result: Result<(), String>,
    /// Everything it showed: what it said, its questions, lists and their
    /// options.
    pub shown: String,
    /// The last screen it drew.
    pub screen: String,
}

/// A keyring in memory, in place of the Secret Service.
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
