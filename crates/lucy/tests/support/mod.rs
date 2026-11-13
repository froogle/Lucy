//! A home of its own for `lucy`, with an in-process lucyd: its own XDG
//! dirs, and a `PATH` holding only the fake tools a test writes. The
//! full-screen pages run in-process on ratatui's `TestBackend`, pressed by a
//! [`Script`]; plain commands run the real binary.

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub use lucy::ui::Script;
use lucy::ui::Ui;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

pub struct Home {
    pub dir: tempfile::TempDir,
    _lucyd: Option<lucyd::Daemon>,
}

impl Home {
    /// A home where lucyd runs, with no adapters that hear or speak.
    pub fn with_lucyd() -> Home {
        let home = Home::without_lucyd();
        std::fs::write(home.config_dir().join("persona.md"), "You are Lucy.").unwrap();
        let daemon = lucyd::spawn(lucyd::Options {
            socket_path: home.runtime().join("lucy").join("lucyd.sock"),
            config_path: home.config_path(),
            persona_path: home.config_dir().join("persona.md"),
            lucy_program: env!("CARGO_BIN_EXE_lucy").into(),
            adapters: lucyd::adapters::Adapters::inert(),
            plugin_paths: lucyd::plugins::Paths {
                user: home.dir.path().join("plugins"),
                shipped: home.dir.path().join("shipped"),
                state: home.state(),
            },
        })
        .unwrap();
        Home {
            _lucyd: Some(daemon),
            ..home
        }
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

    /// Where setup finds things in this home: its config, its fakes on
    /// `PATH`, its lucyd, and the `lucyd` cargo builds beside `lucy`.
    pub fn env(&self) -> lucy::Env {
        lucy::Env {
            config: self.config_path(),
            path: self.bin().into(),
            socket: Some(self.runtime().join("lucy").join("lucyd.sock")),
            units: self.dir.path().join("config/systemd/user"),
            bin: Path::new(env!("CARGO_BIN_EXE_lucy"))
                .parent()
                .unwrap()
                .to_owned(),
            os_release: "/etc/os-release".into(),
        }
    }

    /// Runs setup full-screen on a test terminal, pressing `keys`. Panics
    /// if a key is left over.
    pub fn setup(&self, env: &lucy::Env, mut keys: Script) -> Setup {
        let terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut ui = Ui::new(terminal, &mut keys, "Lucy setup");
        let result = lucy::setup::run(&mut ui, env);
        let shown = ui.screen.shown();
        let lines = ui.screen.lines.clone();
        drop(ui);
        assert!(keys.is_done(), "keys left over:\n{shown}");
        Setup {
            result,
            shown,
            lines,
        }
    }

    /// Runs the `lucy` page on a test terminal against this home's lucyd,
    /// pressing `keys`; the last screen it drew.
    pub fn page(&self, mut keys: Script) -> String {
        let mut client = self.env().connect().unwrap();
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        lucy::home::run(&mut client, &mut terminal, &mut keys).unwrap();
        assert!(keys.is_done());
        terminal.backend().to_string()
    }

    /// Runs `lucy args`, answering its questions with `answers`, one a line.
    pub fn lucy(&self, args: &[&str], answers: &[&str]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lucy"))
            .args(args)
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.dir.path())
            .env("XDG_RUNTIME_DIR", self.runtime())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_STATE_HOME", self.dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = answers.join("\n");
        input.push('\n');
        // It may finish without reading everything.
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        child.wait_with_output().unwrap()
    }
}

/// How a setup run went.
pub struct Setup {
    pub result: Result<(), String>,
    /// Everything it showed: what it said, its questions and their options.
    pub shown: String,
    /// The transcript, with each answer.
    pub lines: Vec<String>,
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
