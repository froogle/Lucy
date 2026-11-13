//! Drives the real `lucy` binary against an in-process lucyd.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn lucy(runtime_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lucy"));
    command.env("XDG_RUNTIME_DIR", runtime_dir);
    command
}

fn start_lucyd(runtime_dir: &Path) -> lucyd::Daemon {
    lucyd::spawn(lucyd::Options {
        socket_path: runtime_dir.join("lucy").join("lucyd.sock"),
        config: lucy_config::Config::default(),
    })
    .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn commands_say_plainly_when_lucyd_is_not_running() {
    let dir = tempfile::tempdir().unwrap();

    for command in ["status", "events"] {
        let output = lucy(dir.path()).arg(command).output().unwrap();

        assert!(!output.status.success(), "lucy {command} should fail");
        let stderr = stderr(&output);
        assert!(
            stderr.contains("lucyd isn't running"),
            "{command}: {stderr}"
        );
        assert!(
            stderr.contains("systemctl --user start lucyd"),
            "{command}: {stderr}"
        );
    }
}

#[test]
fn status_prints_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let _lucyd = start_lucyd(dir.path());

    let output = lucy(dir.path()).arg("status").output().unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "State: Idle\n");
}

#[test]
fn events_prints_the_raw_ndjson_stream() {
    let dir = tempfile::tempdir().unwrap();
    let _lucyd = start_lucyd(dir.path());

    let mut child = lucy(dir.path())
        .arg("events")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    let event: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(
        event,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "state",
            "params": {"state": "idle", "session_id": null},
        })
    );
}
