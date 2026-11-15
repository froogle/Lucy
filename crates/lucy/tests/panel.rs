//! What the bar panel runs: `lucy status --json`, which prints lucyd's
//! state and each choice the panel offers, and `lucy set`, which writes one.

mod support;

use std::io::{BufRead, BufReader};
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use support::{read, stderr, stdout, Home};

const CONFIGURED: &str = "# My Lucy, tuned by hand.\n\n\
[providers.claude-fast]\n\
kind = \"claude\"\n\n\
[providers.codex-fast]\n\
kind = \"codex\" # trying it out\n\n\
[tiers]\n\
fast = \"claude-fast\" # everyday talk\n\
advanced = \"codex-fast\"\n\n\
[audio]\n\
mic = \"alsa_input.usb-Yeti\"\n";

const SOURCES: &str = "60\talsa_input.usb-Yeti\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n\
77\talsa_output.hdmi.monitor\tPipeWire\ts16le 2ch 48000Hz\tIDLE\n";
const SINKS: &str = "61\talsa_output.hdmi\tPipeWire\ts16le 2ch 48000Hz\tIDLE\n";

fn home(active: &str) -> Home {
    let home = Home::without_lucyd();
    systemd_says(&home, active, Duration::ZERO);
    std::fs::write(home.config_path(), CONFIGURED).unwrap();
    home.fake(
        "pactl",
        &format!(
            "case \"$1 $3\" in \
             'list sources') printf '{SOURCES}';; \
             'list sinks') printf '{SINKS}';; \
             esac"
        ),
    );
    home
}

fn systemd_says(home: &Home, active: &str, for_: Duration) {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap() - for_;
    home.fake(
        "systemctl",
        &format!(
            "case \"$2\" in \
             show) printf 'ActiveState={active}\\nActiveEnterTimestamp=@{}\\n';; \
             esac",
            since.as_secs()
        ),
    );
}

fn snapshot(home: &Home) -> Value {
    let output = home.lucy(&["status", "--json"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).unwrap()
}

/// A running `lucy status --follow --json`, read a snapshot at a time.
struct Follow {
    child: Child,
    lines: Receiver<String>,
}

impl Follow {
    fn start(home: &Home) -> Follow {
        let mut child = home
            .command(&["status", "--follow", "--json"])
            .spawn()
            .unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                let _ = send.send(line);
            }
        });
        Follow { child, lines }
    }

    fn next(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .expect("a snapshot within 10 s");
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn following_prints_a_snapshot_each_time_lucyd_or_a_setting_changes() {
    let home = home("inactive");
    let follow = Follow::start(&home);
    assert_eq!(follow.next()["state"], "stopped");

    let lucyd = home.spawn_lucyd();
    assert_eq!(follow.next()["state"], "waiting");

    assert!(home.lucy(&["listen"], &[]).status.success());
    assert_eq!(follow.next()["state"], "session");
    assert!(home.lucy(&["end"], &[]).status.success());
    assert_eq!(follow.next()["state"], "waiting");

    assert!(home
        .lucy(&["set", "tiers.fast", "codex-fast"], &[])
        .status
        .success());
    let snapshot = follow.next();
    assert_eq!(snapshot["state"], "waiting");
    assert_eq!(snapshot["fast"]["current"], "codex-fast");

    drop(lucyd);
    assert_eq!(follow.next()["state"], "stopped");
}

#[test]
fn lucyd_dying_shows_as_failed_while_systemd_restarts_it() {
    let home = home("activating");
    let lucyd = home.spawn_lucyd();
    let follow = Follow::start(&home);
    assert_eq!(follow.next()["state"], "waiting");

    drop(lucyd);

    assert_eq!(follow.next()["state"], "failed");
}

#[test]
fn started_but_not_yet_listening_is_starting() {
    let home = home("active");

    assert_eq!(snapshot(&home)["state"], "starting");
}

#[test]
fn active_for_minutes_without_listening_is_failed_not_starting() {
    let home = home("active");
    systemd_says(&home, "active", Duration::from_secs(600));

    assert_eq!(snapshot(&home)["state"], "failed");
}

#[test]
fn set_writes_with_comments_kept_and_says_it_applies_at_the_next_session() {
    let home = home("inactive");

    let output = home.lucy(&["set", "tiers.fast", "codex-fast"], &[]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&output)).unwrap(),
        json!({"applies": "next_session", "message": "Applies at the next Session."})
    );
    let text = read(&home.config_path());
    assert!(text.starts_with("# My Lucy, tuned by hand."), "{text}");
    assert!(text.contains("kind = \"codex\" # trying it out"), "{text}");
    assert!(
        text.contains("fast = \"codex-fast\" # everyday talk"),
        "{text}"
    );
}

#[test]
fn whispers_model_set_or_unset_applies_after_a_restart() {
    let home = home("inactive");

    let set = home.lucy(&["set", "whisper.model", "base.en"], &[]);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&set)).unwrap(),
        json!({"applies": "restart", "message": "Applies after restarting lucyd."})
    );
    assert_eq!(snapshot(&home)["stt"]["current"], "base.en");

    let unset = home.lucy(&["unset", "whisper.model"], &[]);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&unset)).unwrap()["applies"],
        "restart"
    );
    assert_eq!(snapshot(&home)["stt"]["current"], Value::Null);
}

#[test]
fn set_refuses_a_setting_it_doesnt_know_and_leaves_the_file_alone() {
    let home = home("inactive");

    let output = home.lucy(&["set", "session.silence_timeout_secs", "5"], &[]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("lucy doesn't set session.silence_timeout_secs"),
        "{}",
        stderr(&output)
    );
    assert_eq!(read(&home.config_path()), CONFIGURED);
}

#[test]
fn the_snapshot_names_the_wake_word_its_model_listens_for() {
    let home = home("inactive");
    assert_eq!(snapshot(&home)["wake_word"], "Hey Lucy");

    std::fs::write(
        home.config_path(),
        "[wake_word]\nmodel = \"/models/ok_computer.onnx\"\n",
    )
    .unwrap();

    assert_eq!(snapshot(&home)["wake_word"], "Ok Computer");
}

#[test]
fn with_superwhisper_installed_speech_to_text_offers_it_beside_whispers_models() {
    let home = home("inactive");
    home.fake("superwhisper", "exit 0");

    assert_eq!(
        snapshot(&home)["stt"],
        json!({
            "key": "stt",
            "current": null,
            "options": [null, "small.en", "base.en", "superwhisper"],
        })
    );
}

#[test]
fn choosing_superwhisper_from_speech_to_text_applies_at_the_next_session() {
    let home = home("inactive");
    home.fake("superwhisper", "exit 0");

    let output = home.lucy(&["set", "stt", "superwhisper"], &[]);

    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&output)).unwrap()["applies"],
        "next_session",
        "{}",
        stderr(&output)
    );
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Superwhisper);
    assert_eq!(snapshot(&home)["stt"]["current"], "superwhisper");
}

#[test]
fn choosing_one_of_whispers_models_while_superwhisper_hears_brings_whisper_back() {
    let home = home("inactive");
    home.fake("superwhisper", "exit 0");
    assert!(home
        .lucy(&["set", "stt", "superwhisper"], &[])
        .status
        .success());

    let output = home.lucy(&["set", "stt", "base.en"], &[]);

    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&output)).unwrap()["applies"],
        "restart",
        "{}",
        stderr(&output)
    );
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Whisper);
    assert_eq!(parsed.whisper.model.as_deref(), Some("base.en"));
    let text = read(&home.config_path());
    assert!(text.starts_with("# My Lucy, tuned by hand."), "{text}");

    assert!(home.lucy(&["unset", "stt"], &[]).status.success());
    let parsed = lucy_config::load(&home.config_path()).unwrap();
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Whisper);
    assert_eq!(parsed.whisper.model, None);
    assert_eq!(snapshot(&home)["stt"]["current"], Value::Null);
}

#[test]
fn with_lucyd_stopped_the_snapshot_says_so_and_still_lists_every_choice() {
    let home = home("inactive");

    assert_eq!(
        snapshot(&home),
        json!({
            "state": "stopped",
            "wake_word": "Hey Lucy",
            "fast": {
                "key": "tiers.fast",
                "current": "claude-fast",
                "options": ["claude-fast", "codex-fast"],
            },
            "advanced": {
                "key": "tiers.advanced",
                "current": "codex-fast",
                "options": ["claude-fast", "codex-fast"],
            },
            "stt": {
                "key": "stt",
                "current": null,
                "options": [null, "small.en", "base.en"],
            },
            "mic": {
                "key": "audio.mic",
                "current": "alsa_input.usb-Yeti",
                "options": ["default", "alsa_input.usb-Yeti"],
            },
            "speaker": {
                "key": "audio.speaker",
                "current": "default",
                "options": ["default", "alsa_output.hdmi"],
            },
        })
    );
}

#[test]
fn lucy_service_starts_stops_and_restarts_lucyd_through_systemd() {
    let home = home("inactive");

    for verb in ["start", "stop", "restart"] {
        let output = home.lucy(&["service", verb], &[]);
        assert!(output.status.success(), "{verb}: {}", stderr(&output));
    }

    let calls = home.calls("systemctl");
    for call in [
        "--user start lucyd.service",
        "--user stop lucyd.service",
        "--user restart lucyd.service",
    ] {
        assert!(
            calls.contains(&call.to_owned()),
            "{call} not run: {calls:?}"
        );
    }
}

#[test]
fn lucy_service_start_clears_a_crashed_lucyds_start_limit_first() {
    let home = home("failed");

    assert!(home.lucy(&["service", "start"], &[]).status.success());

    let calls = home.calls("systemctl");
    let reset = calls
        .iter()
        .position(|c| c == "--user reset-failed lucyd.service");
    let start = calls.iter().position(|c| c == "--user start lucyd.service");
    assert!(reset.is_some() && reset < start, "{calls:?}");
}

#[test]
fn lucy_service_fails_plainly_when_systemd_refuses() {
    let home = home("inactive");
    home.fake(
        "systemctl",
        "[ \"$2\" != start ] || { echo 'no such unit' >&2; exit 5; }",
    );

    let output = home.lucy(&["service", "start"], &[]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no such unit"),
        "{}",
        stderr(&output)
    );
}
