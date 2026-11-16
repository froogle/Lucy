//! The real Launcher: starts apps and opens URLs as the user's own launcher
//! would, through uwsm when it is running so that they get their own
//! systemd scope rather than living in lucyd's.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::adapters::Launcher;

pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open_url(&self, url: &str) -> io::Result<()> {
        let mut command = if uwsm_is_running() {
            vec!["uwsm", "app", "--", "xdg-open"]
        } else {
            vec!["xdg-open"]
        };
        command.push(url);
        spawn(&command)
    }

    fn launch(&self, entry: &Path, launcher: Option<&[String]>) -> io::Result<()> {
        let mut command: Vec<&str> = match launcher {
            Some(configured) => configured.iter().map(String::as_str).collect(),
            None if uwsm_is_running() => vec!["uwsm", "app", "--"],
            None => vec!["gio", "launch"],
        };
        let entry = entry
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path isn't UTF-8"))?;
        command.push(entry);
        spawn(&command)
    }

    fn open_terminal(&self, command: &[String]) -> io::Result<()> {
        let mut terminal = if uwsm_is_running() {
            vec!["uwsm", "app", "--", "xdg-terminal-exec"]
        } else {
            vec!["xdg-terminal-exec"]
        };
        terminal.extend(command.iter().map(String::as_str));
        spawn(&terminal)
    }
}

fn uwsm_is_running() -> bool {
    Command::new("uwsm")
        .args(["check", "is-active"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Starts `command` without waiting for it: an app runs for as long as the
/// user keeps it open. A thread reaps it when it exits.
fn spawn(command: &[&str]) -> io::Result<()> {
    let [program, args @ ..] = command else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the launcher command is empty",
        ));
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_configured_launcher_is_run_with_the_entry_path_appended() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let record = format!(
            "printf '%s\\n' \"$@\" > {}.tmp && mv {0}.tmp {0}",
            out.display()
        );
        let configured = [
            "sh".to_owned(),
            "-c".to_owned(),
            record,
            "sh".to_owned(),
            "--flag".to_owned(),
        ];

        SystemLauncher
            .launch(
                Path::new("/usr/share/applications/firefox.desktop"),
                Some(&configured),
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !out.exists() {
            assert!(Instant::now() < deadline, "the launcher never ran");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "--flag\n/usr/share/applications/firefox.desktop\n"
        );
    }

    #[test]
    fn an_empty_configured_launcher_is_an_error() {
        assert!(SystemLauncher
            .launch(Path::new("/x.desktop"), Some(&[]))
            .is_err());
    }
}
