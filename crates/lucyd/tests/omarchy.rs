//! The shipped Omarchy Plugin, run by lucyd.
//! It is approved when lucyd scans it on Omarchy, asks the user's yes before
//! a force-quit, and is left out quietly anywhere else. Hyprland is a fake socket served here, and Omarchy's
//! commands are fakes on `PATH`; the Plugin's own tests cover its tools.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harness::{next_state, Harness};
use lucy_ipc::{Client, PluginStatus, State};
use serde_json::json;

fn omarchy_binary() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let binary = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("lucy-omarchy"))
        .find(|path| path.is_file());
    binary.expect(
        "lucy-omarchy not found; run `cargo build -p lucy-omarchy` or `cargo test --workspace`",
    )
}

struct Omarchy {
    dir: tempfile::TempDir,
    dispatched: Arc<Mutex<Vec<String>>>,
}

impl Omarchy {
    fn new() -> Omarchy {
        let dir = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let hypr = dir.path().join("hypr").join("sig");
        std::fs::create_dir_all(&hypr).unwrap();
        let listener = UnixListener::bind(hypr.join(".socket.sock")).unwrap();
        let dispatched = Arc::new(Mutex::new(Vec::new()));
        let recorded = dispatched.clone();
        std::thread::spawn(move || {
            let slack = json!({
                "address": "0xb3", "class": "Slack", "title": "general - Slack",
                "workspace": {"id": 3, "name": "3"},
                "floating": false, "fullscreen": 0, "pinned": false,
                "at": [0, 0], "size": [800, 600], "monitor": 0,
            });
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = vec![0; 1 << 16];
                let read = stream.read(&mut request).unwrap();
                let request = String::from_utf8(request[..read].to_vec()).unwrap();
                let answer = match request.as_str() {
                    "j/clients" => json!([slack]).to_string(),
                    "j/activewindow" => slack.to_string(),
                    dispatch => {
                        let dispatch = dispatch.strip_prefix("dispatch ").unwrap_or(dispatch);
                        recorded.lock().unwrap().push(dispatch.to_owned());
                        "ok".to_owned()
                    }
                };
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        Omarchy { dir, dispatched }
    }

    fn lucyd(&self) -> Harness {
        let runtime = self.dir.path().display().to_string();
        let bin = self.dir.path().join("bin");
        let lucyd = Harness::with_env(&[
            ("HYPRLAND_INSTANCE_SIGNATURE", "sig"),
            ("XDG_RUNTIME_DIR", &runtime),
            ("OMARCHY_PATH", "/usr/share/omarchy"),
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("PATH", &bin.display().to_string()),
        ]);
        let runnable_in_the_sandbox = install_omarchy(&lucyd).join("bin");
        fake_commands(&runnable_in_the_sandbox);
        std::os::unix::fs::symlink(&runnable_in_the_sandbox, &bin).unwrap();
        lucyd
    }

    fn dispatched(&self) -> Vec<String> {
        self.dispatched.lock().unwrap().clone()
    }
}

fn fake_commands(bin: &Path) {
    std::fs::create_dir(bin).unwrap();
    for command in [
        "omarchy-hyprland-workspace-layout-current",
        "omarchy-hyprland-workspace-layout-toggle",
    ] {
        write_executable(&bin.join(command), "#!/bin/sh\necho dwindle\n");
    }
    write_executable(
        &bin.join("omarchy-toggle-bar"),
        "#!/bin/sh\n\
         echo \"$*\" >> \"$LUCY_PLUGIN_STATE_DIR/bar\"\n\
         for file in omarchy-theme-set.lock lucy/probe hypr/probe probe; do\n\
         \x20 echo x 2>/dev/null > \"$XDG_RUNTIME_DIR/$file\" && echo $file >> \"$LUCY_PLUGIN_STATE_DIR/wrote\"\n\
         done\n\
         true\n",
    );
}

fn recorded(lucyd: &Harness, name: &str) -> String {
    std::fs::read_to_string(lucyd.state_dir().join("plugins/omarchy").join(name))
        .unwrap_or_default()
}

fn write_executable(path: &Path, script: &str) {
    let mut writer = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod +x \"$1\"", "sh"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    assert!(writer.wait().unwrap().success());
}

fn install_omarchy(lucyd: &Harness) -> PathBuf {
    let manifest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lucy-omarchy/manifest.toml"),
    )
    .unwrap();
    let folder = lucyd.install_shipped_plugin("omarchy", &manifest);
    let copied = std::process::Command::new("cp")
        .arg(omarchy_binary())
        .arg(folder.join("lucy-omarchy"))
        .status()
        .unwrap();
    assert!(copied.success());
    folder
}

fn in_a_session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    events
}

fn force_quit_slack(
    lucyd: &Harness,
    events: &mut Client,
) -> std::thread::JoinHandle<Result<rmcp::model::CallToolResult, String>> {
    lucyd.say("force quit slack");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let thinking = lucyd.thinking.clone();
    let call = std::thread::spawn(move || {
        thinking.call_tool_with("omarchy__kill_window", json!({"target": "slack"}))
    });
    assert_eq!(next_state(events).state, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Shall I force-quit that window (slack)? Anything unsaved in it will be lost."
    );
    lucyd.tts.finish_playing();
    call
}

#[test]
fn on_omarchy_the_shipped_plugin_is_approved_at_scan_and_offered_without_asking() {
    let desktop = Omarchy::new();
    let lucyd = desktop.lucyd();
    let _events = in_a_session(&lucyd);

    let mut tools: Vec<String> = lucyd
        .thinking
        .tool_names()
        .into_iter()
        .filter(|tool| tool.starts_with("omarchy__"))
        .collect();
    tools.sort();

    assert_eq!(
        tools,
        [
            "omarchy__bar",
            "omarchy__click",
            "omarchy__idle_lock",
            "omarchy__key",
            "omarchy__kill_window",
            "omarchy__layout",
            "omarchy__list_windows",
            "omarchy__night_light",
            "omarchy__screenshot",
            "omarchy__screenshot_for_model",
            "omarchy__scroll",
            "omarchy__theme",
            "omarchy__type",
            "omarchy__window",
            "omarchy__workspace",
        ]
    );
    assert_eq!(lucyd.tts.sentences(), Vec::<String>::new());
    let approvals = lucyd.client().approvals().unwrap().approvals;
    assert!(approvals.iter().any(|a| a.plugin == "omarchy"));
}

#[test]
fn a_window_call_reaches_hyprland_without_a_confirmation() {
    let desktop = Omarchy::new();
    let lucyd = desktop.lucyd();
    let _events = in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with(
            "omarchy__window",
            json!({"action": "move_to_workspace", "target": "slack", "workspace": 1}),
        )
        .unwrap();

    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(
        desktop.dispatched(),
        ["hl.dsp.window.move({ workspace = \"1\", follow = false, window = \"address:0xb3\" })"]
    );
}

#[test]
fn a_settings_call_runs_omarchys_command_in_the_sandbox_without_a_confirmation() {
    let desktop = Omarchy::new();
    let lucyd = desktop.lucyd();
    let _events = in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with("omarchy__bar", json!({"action": "hide"}))
        .unwrap();

    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(recorded(&lucyd, "bar"), "on\n");
}

#[test]
fn omarchys_commands_take_their_locks_but_write_nowhere_else_in_the_runtime_dir() {
    let desktop = Omarchy::new();
    let runtime = desktop.dir.path();
    std::fs::write(runtime.join("omarchy-theme-set.lock"), "").unwrap();
    std::fs::create_dir(runtime.join("lucy")).unwrap();
    let lucyd = desktop.lucyd();
    let _events = in_a_session(&lucyd);

    lucyd
        .thinking
        .call_tool_with("omarchy__bar", json!({"action": "hide"}))
        .unwrap();

    assert_eq!(recorded(&lucyd, "wrote"), "omarchy-theme-set.lock\n");
}

#[test]
fn a_force_quit_runs_only_after_a_spoken_yes() {
    let desktop = Omarchy::new();
    let lucyd = desktop.lucyd();
    let mut events = in_a_session(&lucyd);
    let call = force_quit_slack(&lucyd, &mut events);

    lucyd.say("Yes.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(
        desktop.dispatched(),
        ["hl.dsp.window.kill({ window = \"address:0xb3\" })"]
    );
}

#[test]
fn a_force_quit_declined_leaves_the_window_alone() {
    let desktop = Omarchy::new();
    let lucyd = desktop.lucyd();
    let mut events = in_a_session(&lucyd);
    let call = force_quit_slack(&lucyd, &mut events);

    lucyd.say("No.");

    call.join().unwrap().unwrap();
    assert_eq!(desktop.dispatched(), Vec::<String>::new());
}

#[test]
fn on_another_desktop_it_is_left_out_quietly() {
    let desktop = Omarchy::new();
    let bin = desktop.dir.path().join("bin");
    fake_commands(&bin);
    let lucyd = Harness::with_env(&[("PATH", &bin.display().to_string())]);
    install_omarchy(&lucyd);
    let _events = in_a_session(&lucyd);

    let tools = lucyd.thinking.tool_names();
    assert!(
        !tools.iter().any(|t| t.starts_with("omarchy__")),
        "{tools:?}"
    );
    let instructions = lucyd.thinking.instructions();
    assert!(!instructions.contains("omarchy"), "{instructions}");
    let plugins = lucyd.client().plugins().unwrap().plugins;
    let omarchy = plugins.iter().find(|p| p.name == "omarchy").unwrap();
    assert_eq!(omarchy.status, PluginStatus::NotForThisDesktop);
}
