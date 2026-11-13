//! Runs the real `lucy` binary against an in-process lucyd, in a home of its
//! own: its own XDG dirs, and a `PATH` holding only the fake tools a test
//! writes.

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}
