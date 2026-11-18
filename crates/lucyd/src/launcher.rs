use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::adapters::Launcher;

pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open_url(&self, url: &str) -> io::Result<()> {
        xdg_open(url)
    }

    fn show_items(&self, paths: &[PathBuf]) -> io::Result<()> {
        let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
        if let Err(e) = show_over_dbus(&uris) {
            log::debug!("no file manager answered ShowItems: {e}");
            let folder = paths
                .first()
                .and_then(|path| path.parent())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to open"))?;
            return self.open_file(folder);
        }
        Ok(())
    }

    fn open_file(&self, path: &Path) -> io::Result<()> {
        xdg_open(
            path.to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path isn't UTF-8"))?,
        )
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
        spawn_detached(&command)
    }

    fn open_terminal(&self, command: &[String]) -> io::Result<()> {
        let mut terminal = if uwsm_is_running() {
            vec!["uwsm", "app", "--", "xdg-terminal-exec"]
        } else {
            vec!["xdg-terminal-exec"]
        };
        terminal.extend(command.iter().map(String::as_str));
        spawn_detached(&terminal)
    }

    fn open_in_browser(&self, browser: &str, page: &str) -> io::Result<()> {
        let mut command = if uwsm_is_running() {
            vec!["uwsm", "app", "--"]
        } else {
            vec![]
        };
        command.extend([browser, page]);
        spawn_detached(&command)
    }
}

const DBUS_WAIT: Duration = Duration::from_secs(5);

fn show_over_dbus(uris: &[String]) -> Result<(), zbus::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let call = async {
            let bus = zbus::Connection::session().await?;
            bus.call_method(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                Some("org.freedesktop.FileManager1"),
                "ShowItems",
                &(uris, ""),
            )
            .await
        };
        match tokio::time::timeout(DBUS_WAIT, call).await {
            Ok(answer) => answer.map(drop),
            Err(_) => Err(zbus::Error::InputOutput(
                io::Error::from(io::ErrorKind::TimedOut).into(),
            )),
        }
    })
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn xdg_open(target: &str) -> io::Result<()> {
    let mut command = if uwsm_is_running() {
        vec!["uwsm", "app", "--", "xdg-open"]
    } else {
        vec!["xdg-open"]
    };
    command.push(target);
    spawn_detached(&command)
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

fn spawn_detached(command: &[&str]) -> io::Result<()> {
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
    fn a_file_uri_escapes_everything_but_unreserved_characters() {
        assert_eq!(
            file_uri(Path::new("/home/me/1921 Naval Conference/notes #1 é.txt")),
            "file:///home/me/1921%20Naval%20Conference/notes%20%231%20%C3%A9.txt"
        );
    }

    #[test]
    fn an_empty_configured_launcher_is_an_error() {
        assert!(SystemLauncher
            .launch(Path::new("/x.desktop"), Some(&[]))
            .is_err());
    }
}
