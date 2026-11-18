//! A Hand-off through the real `claude` adapter, against the fake `claude`
//! (`fixtures/claude/fake-claude`) and a fake login shell. The Session's own
//! model is the harness's scripted one.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::adapters::{Notification, NotificationKind};
use lucyd::claude::Claude;
use serde_json::json;

const CONVERSATION: &str = "09dd6ca6-f8e2-473e-812c-ab5acf9cdfd5";
const RECORDED_ANSWER: &str = "Got it, I'll remember the word kiwi.";

struct Dirs {
    work: tempfile::TempDir,
    config: tempfile::TempDir,
    folder: tempfile::TempDir,
    bin: tempfile::TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        let dir = || tempfile::tempdir().unwrap();
        Dirs {
            work: dir(),
            config: dir(),
            folder: dir(),
            bin: dir(),
        }
    }

    fn claude(&self) -> Arc<Claude> {
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/fake-claude");
        let claude = Claude::new(fake, self.work.path().to_owned())
            .with_config_dir(self.config.path().to_owned());
        Arc::new(claude)
    }

    fn shell(&self, script: &str) -> PathBuf {
        let program = self.bin.path().join("login-shell");
        let written = Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
                "sh",
            ])
            .arg(format!("#!/bin/sh\n{script}\n"))
            .arg(&program)
            .status()
            .unwrap();
        assert!(written.success());
        program
    }

    fn argv(&self) -> Vec<String> {
        let argv = std::fs::read_to_string(self.folder.path().join("argv")).unwrap();
        argv.split('\0').map(str::to_owned).collect()
    }

    fn environ(&self) -> Vec<String> {
        let environ = std::fs::read_to_string(self.folder.path().join("environ")).unwrap();
        environ.lines().map(str::to_owned).collect()
    }
}

fn with_claude(dirs: &Dirs, env: &[(&str, &str)]) -> Harness {
    let lucyd = Harness::with_env(env);
    lucyd.thinking.hand_off.open_on(dirs.claude());
    lucyd
}

fn path() -> String {
    std::env::var("PATH").unwrap()
}

fn hand_off(lucyd: &Harness, folder: &Path) -> Notification {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say("tell Claude to remember the word kiwi");
    lucyd.thinking.next_turn();
    let arguments =
        json!({"request": "remember the word kiwi", "folder": folder.to_str().unwrap()});
    let result = lucyd
        .thinking
        .call_tool_with("hand_to_claude", arguments)
        .unwrap();
    assert_eq!(result.is_error, Some(false), "{result:?}");
    lucyd.thinking.turn_end();
    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff)
}

#[test]
fn the_users_own_claude_runs_with_their_settings_lucys_relay_and_the_permission_tool() {
    let dirs = Dirs::new();
    let shell = dirs.shell(&format!(
        "printf 'PATH={}\\0HANDOFF_TOKEN=from-login-shell\\0ANTHROPIC_API_KEY=sk-ant-billed\\0'",
        path()
    ));
    let lucyd = with_claude(
        &dirs,
        &[("SHELL", shell.to_str().unwrap()), ("PATH", &path())],
    );

    let finished = hand_off(&lucyd, dirs.folder.path());

    let argv = dirs.argv();
    let after = |flag: &str| {
        let at = argv.iter().position(|arg| arg == flag);
        at.map(|at| argv[at + 1].clone())
            .unwrap_or_else(|| panic!("no {flag} in {argv:?}"))
    };
    assert_eq!(after("--setting-sources"), "user,project,local");
    assert_eq!(after("--permission-prompt-tool"), "mcp__lucy__permission");
    let mcp_config: serde_json::Value = serde_json::from_str(&after("--mcp-config")).unwrap();
    let relay = &mcp_config["mcpServers"]["lucy"];
    assert_eq!(relay["args"][0], "mcp-relay", "{mcp_config}");
    for flag in [
        "--strict-mcp-config",
        "--model",
        "--tools",
        "--permission-mode",
    ] {
        assert!(!argv.iter().any(|arg| arg == flag), "{flag} in {argv:?}");
    }

    let environ = dirs.environ();
    assert!(environ
        .iter()
        .any(|var| var == "HANDOFF_TOKEN=from-login-shell"));
    assert!(
        !environ
            .iter()
            .any(|var| var.starts_with("ANTHROPIC_API_KEY=")),
        "{environ:?}"
    );

    assert_eq!(finished.summary, "Claude has finished");
    assert!(
        finished.body.starts_with(RECORDED_ANSWER),
        "{}",
        finished.body
    );
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(finished.body.contains(&resume), "{}", finished.body);
    let folder = dirs.folder.path().to_str().unwrap();
    assert!(finished.body.contains(folder), "{}", finished.body);
}

fn continue_it(lucyd: &Harness) -> Notification {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say("tell Claude yes, go ahead");
    lucyd.thinking.next_turn();
    let arguments = json!({"request": "yes, go ahead", "continue": true});
    let result = lucyd
        .thinking
        .call_tool_with("hand_to_claude", arguments)
        .unwrap();
    assert_eq!(result.is_error, Some(false), "{result:?}");
    lucyd.thinking.turn_end();
    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff)
}

fn assert_resumed(dirs: &Dirs) {
    let argv = dirs.argv();
    let at = argv.iter().position(|arg| arg == "--resume");
    let resumed = at.map(|at| argv[at + 1].as_str());
    assert_eq!(resumed, Some(CONVERSATION), "{argv:?}");
}

#[test]
fn continue_runs_claude_with_resume_in_the_folder_at_a_later_session() {
    let dirs = Dirs::new();
    let shell = dirs.shell(&format!("printf 'PATH={}\\0'", path()));
    let lucyd = with_claude(
        &dirs,
        &[("SHELL", shell.to_str().unwrap()), ("PATH", &path())],
    );
    hand_off(&lucyd, dirs.folder.path());
    lucyd.thinking.next_turn();
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();
    std::fs::remove_file(dirs.folder.path().join("argv")).unwrap();

    let finished = continue_it(&lucyd);

    assert_eq!(finished.summary, "Claude has finished");
    assert_resumed(&dirs);
}

#[test]
fn continue_works_after_lucyd_restarts() {
    let dirs = Dirs::new();
    let shell = dirs.shell(&format!("printf 'PATH={}\\0'", path()));
    let env = [("SHELL", shell.to_str().unwrap()), ("PATH", &path())];
    let lucyd = with_claude(&dirs, &env);
    hand_off(&lucyd, dirs.folder.path());
    lucyd.thinking.next_turn();
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();
    std::fs::remove_file(dirs.folder.path().join("argv")).unwrap();

    let lucyd = lucyd.restart();
    lucyd.thinking.hand_off.open_on(dirs.claude());
    continue_it(&lucyd);

    assert_resumed(&dirs);
}

#[test]
fn the_hand_off_transcript_is_left_in_place() {
    let dirs = Dirs::new();
    let shell = dirs.shell(&format!("printf 'PATH={}\\0'", path()));
    let lucyd = with_claude(
        &dirs,
        &[("SHELL", shell.to_str().unwrap()), ("PATH", &path())],
    );

    hand_off(&lucyd, dirs.folder.path());
    lucyd.client().end_session().unwrap();
    drop(lucyd);

    let folder = std::fs::canonicalize(dirs.folder.path()).unwrap();
    let project: String = folder
        .to_str()
        .unwrap()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let transcript = dirs
        .config
        .path()
        .join("projects")
        .join(project)
        .join(format!("{CONVERSATION}.jsonl"));
    assert!(transcript.is_file(), "{} is gone", transcript.display());
}

#[test]
fn a_turn_claude_ends_with_an_error_is_notified_as_unfinished_with_its_message() {
    let dirs = Dirs::new();
    std::fs::write(
        dirs.folder.path().join("turn1.jsonl"),
        format!(
            "{}\n{}\n",
            json!({"type": "system", "subtype": "init", "session_id": CONVERSATION}),
            json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
                   "result": "Linear's MCP server isn't connected", "session_id": CONVERSATION}),
        ),
    )
    .unwrap();
    let shell = dirs.shell(&format!("printf 'PATH={}\\0'", path()));
    let lucyd = with_claude(
        &dirs,
        &[("SHELL", shell.to_str().unwrap()), ("PATH", &path())],
    );

    let unfinished = hand_off(&lucyd, dirs.folder.path());

    assert_eq!(unfinished.summary, "Claude couldn't finish");
    assert!(
        unfinished
            .body
            .starts_with("Linear's MCP server isn't connected"),
        "{}",
        unfinished.body
    );
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(unfinished.body.contains(&resume), "{}", unfinished.body);
}

#[test]
fn a_login_shell_that_hangs_falls_back_to_lucyds_own_environment_after_5_s() {
    let dirs = Dirs::new();
    let shell = dirs.shell("printf 'HANDOFF_TOKEN=from-login-shell\\0'\nexec sleep 600");
    let lucyd = with_claude(
        &dirs,
        &[
            ("SHELL", shell.to_str().unwrap()),
            ("PATH", &path()),
            ("LUCYD_OWN", "yes"),
        ],
    );
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    lucyd.say("tell Claude to remember the word kiwi");
    lucyd.thinking.next_turn();
    let folder = dirs.folder.path().to_str().unwrap();
    let arguments = json!({"request": "remember the word kiwi", "folder": folder});
    lucyd
        .thinking
        .call_tool_with("hand_to_claude", arguments)
        .unwrap();

    lucyd.clock.advance_once_set(Duration::from_secs(5));

    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    let environ = dirs.environ();
    assert!(
        environ.iter().any(|var| var == "LUCYD_OWN=yes"),
        "{environ:?}"
    );
    assert!(!environ.iter().any(|var| var.starts_with("HANDOFF_TOKEN=")));
}

#[test]
fn a_claude_that_dies_is_notified_with_its_exit_status_and_the_end_of_its_stderr() {
    let dirs = Dirs::new();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/fake-claude");
    let dying = dirs.bin.path().join("claude");
    harness::write_script(
        &dying,
        &format!(
            "#!/bin/sh\n\
             [ -e dies ] || exec '{fake}' \"$@\"\n\
             read -r line\n\
             echo 'claude starting' >&2\n\
             echo 'claude ERROR the MCP config is invalid' >&2\n\
             exit 3\n",
            fake = fake.display(),
        ),
    );
    std::fs::write(dirs.folder.path().join("dies"), "").unwrap();
    let shell = dirs.shell(&format!("printf 'PATH={}\\0'", path()));
    let lucyd = Harness::with_env(&[("SHELL", shell.to_str().unwrap()), ("PATH", &path())]);
    let claude = Claude::new(dying, dirs.work.path().to_owned())
        .with_config_dir(dirs.config.path().to_owned());
    lucyd.thinking.hand_off.open_on(Arc::new(claude));

    let unfinished = hand_off(&lucyd, dirs.folder.path());

    assert_eq!(unfinished.summary, "Claude couldn't finish");
    assert!(
        unfinished.body.starts_with(
            "Claude stopped unexpectedly (exit status: 3): claude ERROR the MCP config is invalid"
        ),
        "{}",
        unfinished.body
    );
}
