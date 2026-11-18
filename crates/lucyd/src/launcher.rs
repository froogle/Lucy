use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::adapters::Launcher;
use crate::desktop_entries;

pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open_url(&self, url: &str) -> io::Result<()> {
        let default = stdout_of("xdg-settings", &["get", "default-web-browser"]);
        let command = browser_command(
            url,
            default.as_deref(),
            &desktop_entries::xdg_dirs(),
            || stdout_of("omarchy-cmd-default-browser", &[]),
        );
        spawn_app(command.iter().map(String::as_str))
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
        spawn_app(page_command(browser, page))
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
    spawn_app(["xdg-open", target])
}

fn spawn_app<'a>(command: impl IntoIterator<Item = &'a str>) -> io::Result<()> {
    let mut app = if uwsm_is_running() {
        vec!["uwsm", "app", "--"]
    } else {
        vec![]
    };
    app.extend(command);
    spawn_detached(&app)
}

const OMARCHY_WRAPPER: &str = "omarchy-launch-browser";

fn browser_command(
    url: &str,
    default: Option<&str>,
    dirs: &[PathBuf],
    omarchy_default: impl FnOnce() -> Option<String>,
) -> Vec<String> {
    match default.and_then(|entry| browser_program(entry, dirs, omarchy_default)) {
        Some(browser) => page_command(&browser, url).map(str::to_owned).to_vec(),
        None => {
            log::debug!("no program found for the default browser {default:?}");
            vec!["xdg-open".to_owned(), url.to_owned()]
        }
    }
}

fn page_command<'a>(browser: &'a str, page: &'a str) -> [&'a str; 3] {
    [browser, "--new-window", page]
}

fn browser_program(
    entry: &str,
    dirs: &[PathBuf],
    omarchy_default: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let mut program = desktop_entries::exec_program(entry, dirs)?;
    if file_name(&program) == OMARCHY_WRAPPER {
        program = desktop_entries::exec_program(&omarchy_default()?, dirs)?;
    }
    let runs_another_program = [OMARCHY_WRAPPER, "flatpak", "env"].contains(&file_name(&program));
    (!runs_another_program).then_some(program)
}

fn file_name(program: &str) -> &str {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
}

fn stdout_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!stdout.is_empty()).then_some(stdout)
        }
        Ok(output) => {
            log::debug!("{program} found no default browser: {}", output.status);
            None
        }
        Err(e) => {
            log::debug!("{program} didn't run: {e}");
            None
        }
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
    use std::os::unix::fs::PermissionsExt;
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

        assert_eq!(
            read_once_written(&out),
            "--flag\n/usr/share/applications/firefox.desktop\n"
        );
    }

    fn read_once_written(out: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !out.exists() {
            assert!(Instant::now() < deadline, "the command never ran");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::read_to_string(out).unwrap()
    }

    #[test]
    fn a_file_uri_escapes_everything_but_unreserved_characters() {
        assert_eq!(
            file_uri(Path::new("/home/me/1921 Naval Conference/notes #1 é.txt")),
            "file:///home/me/1921%20Naval%20Conference/notes%20%231%20%C3%A9.txt"
        );
    }

    fn applications(entries: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in entries {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        dir
    }

    const PAGE: &str = "https://www.youtube.com/results?search_query=otters";

    #[test]
    fn the_default_browser_opens_the_page_in_a_new_window() {
        let apps = applications(&[(
            "brave-browser.desktop",
            "[Desktop Entry]\nType=Application\nName=Brave\nExec=brave %U\n\n\
             [Desktop Action new-private-window]\nExec=brave --incognito\n",
        )]);

        assert_eq!(
            browser_command(
                PAGE,
                Some("brave-browser.desktop"),
                &[apps.path().to_owned()],
                || None
            ),
            ["brave", "--new-window", PAGE]
        );
    }

    #[test]
    fn omarchys_wrapper_is_never_run() {
        let apps = applications(&[
            (
                "omarchy-browser.desktop",
                "[Desktop Entry]\nExec=omarchy-launch-browser %U\n",
            ),
            ("brave-browser.desktop", "[Desktop Entry]\nExec=brave %U\n"),
        ]);
        let dirs = [apps.path().to_owned()];

        assert_eq!(
            browser_command(PAGE, Some("brave-browser.desktop"), &dirs, || None),
            ["brave", "--new-window", PAGE]
        );
        assert_eq!(
            browser_command(PAGE, Some("omarchy-browser.desktop"), &dirs, || None),
            ["xdg-open", PAGE]
        );
        assert_eq!(
            browser_command(PAGE, Some("omarchy-browser.desktop"), &dirs, || Some(
                "brave-browser.desktop".to_owned()
            )),
            ["brave", "--new-window", PAGE]
        );
        assert_eq!(
            browser_command(PAGE, Some("omarchy-browser.desktop"), &dirs, || Some(
                "omarchy-browser.desktop".to_owned()
            )),
            ["xdg-open", PAGE]
        );
    }

    #[test]
    fn omarchys_wrapper_is_recognised_by_its_file_name() {
        let apps = applications(&[(
            "omarchy-browser.desktop",
            "[Desktop Entry]\nExec=/some/path/omarchy-launch-browser %U\n",
        )]);

        assert_eq!(
            browser_command(
                PAGE,
                Some("omarchy-browser.desktop"),
                &[apps.path().to_owned()],
                || None
            ),
            ["xdg-open", PAGE]
        );
    }

    #[test]
    fn an_exec_that_isnt_plainly_the_browser_falls_back_to_xdg_open() {
        let apps = applications(&[
            (
                "flatpak.desktop",
                "[Desktop Entry]\nExec=flatpak run com.brave.Browser %U\n",
            ),
            (
                "env.desktop",
                "[Desktop Entry]\nExec=env GDK_BACKEND=x11 brave %U\n",
            ),
            (
                "quoted.desktop",
                "[Desktop Entry]\nExec=\"/opt/My Browser/brave\" %U\n",
            ),
        ]);
        let dirs = [apps.path().to_owned()];

        for default in ["flatpak.desktop", "env.desktop", "quoted.desktop"] {
            assert_eq!(
                browser_command(PAGE, Some(default), &dirs, || None),
                ["xdg-open", PAGE],
                "{default}"
            );
        }
    }

    #[test]
    fn the_earliest_data_dir_holding_the_entry_wins() {
        let home = applications(&[(
            "brave-browser.desktop",
            "[Desktop Entry]\nExec=/home/me/bin/brave %U\n",
        )]);
        let system = applications(&[
            ("brave-browser.desktop", "[Desktop Entry]\nExec=brave %U\n"),
            ("firefox.desktop", "[Desktop Entry]\nExec=firefox %u\n"),
        ]);
        let dirs = [home.path().to_owned(), system.path().to_owned()];

        assert_eq!(
            browser_command(PAGE, Some("brave-browser.desktop"), &dirs, || None),
            ["/home/me/bin/brave", "--new-window", PAGE]
        );
        assert_eq!(
            browser_command(PAGE, Some("firefox.desktop"), &dirs, || None),
            ["firefox", "--new-window", PAGE]
        );
    }

    #[test]
    fn a_browser_that_cant_be_worked_out_falls_back_to_xdg_open() {
        let apps = applications(&[(
            "broken.desktop",
            "[Desktop Entry]\nType=Application\nName=Broken\n",
        )]);
        let dirs = [apps.path().to_owned()];

        for default in [None, Some("missing.desktop"), Some("broken.desktop")] {
            assert_eq!(
                browser_command(PAGE, default, &dirs, || None),
                ["xdg-open", PAGE],
                "{default:?}"
            );
        }
    }

    #[test]
    fn a_named_browser_opens_its_page_in_a_new_window() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let browser = dir.path().join("browser");
        std::fs::write(
            &browser,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}.tmp && mv {0}.tmp {0}\n",
                out.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755)).unwrap();

        SystemLauncher
            .open_in_browser(browser.to_str().unwrap(), "brave://settings/accessibility")
            .unwrap();

        assert_eq!(
            read_once_written(&out),
            "--new-window\nbrave://settings/accessibility\n"
        );
    }

    #[test]
    fn an_empty_configured_launcher_is_an_error() {
        assert!(SystemLauncher
            .launch(Path::new("/x.desktop"), Some(&[]))
            .is_err());
    }
}
