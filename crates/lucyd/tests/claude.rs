//! The `claude` Thinking Provider, driven through the `Thinking` seam against
//! a fake `claude` that replays a recorded real stream-json transcript
//! (`fixtures/claude/`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lucy_config::{Config, Provider, Tier};
use lucyd::adapters::{Relay, Thinking, ThinkingEvent};
use lucyd::claude::Claude;
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(5);
const LIVE_WAIT: Duration = Duration::from_secs(60);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude")
}

/// The default fast-tier Provider.
fn fast() -> Provider {
    Config::default().provider(Tier::Fast).unwrap().clone()
}

/// The fixture `program` in `work`, with the fake CLI's version already
/// through the subscription guard (`tests/subscription_guard.rs`) and its
/// records of that cleared, so a Session spawns just its own process.
fn verified(work: &Path, program: &str) -> Claude {
    Claude::new(fixtures().join("fake-claude"), work.to_owned())
        .check(&fast())
        .unwrap();
    for record in ["argv", "pids", "stdin"] {
        std::fs::remove_file(work.join(record)).unwrap();
    }
    Claude::new(fixtures().join(program), work.to_owned())
}

fn relay() -> Relay {
    Relay {
        program: PathBuf::from("/usr/bin/lucy"),
        args: vec!["mcp-relay".into(), "--token".into(), "abc".into()],
    }
}

/// Waits until `path` exists and returns its contents.
fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(contents) = std::fs::read_to_string(path) {
            return contents;
        }
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// One Turn's events up to `TurnEnd`: text with `|` at each `SentenceEnd`,
/// and the rate-limit events.
fn read_turn(
    events: &mut mpsc::UnboundedReceiver<ThinkingEvent>,
    wait: Duration,
) -> (String, Vec<ThinkingEvent>) {
    let mut text = String::new();
    let mut rate_limits = vec![];
    let deadline = Instant::now() + wait;
    loop {
        match events.try_recv() {
            Ok(ThinkingEvent::Delta(delta)) => text.push_str(&delta),
            Ok(ThinkingEvent::SentenceEnd) => text.push('|'),
            Ok(ThinkingEvent::TurnEnd) => return (text, rate_limits),
            Ok(event @ ThinkingEvent::RateLimit(_)) => rate_limits.push(event),
            Ok(ThinkingEvent::Unavailable(reason)) => panic!("Provider unavailable: {reason}"),
            Ok(ThinkingEvent::Died) => panic!("claude died: {text:?}"),
            Err(_) => {
                assert!(Instant::now() < deadline, "the Turn never ended: {text:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn args(work: &Path) -> Vec<String> {
    let argv = wait_for_file(&work.join("argv"));
    argv.split_terminator('\0').map(str::to_owned).collect()
}

fn value_of<'a>(args: &'a [String], flag: &str) -> &'a str {
    let i = args
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("{flag} missing from {args:?}"));
    &args[i + 1]
}

#[test]
fn one_process_is_prespawned_and_serves_every_turn() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude");
    let (tx, mut events) = mpsc::unbounded_channel();

    let mut model = claude.open(&fast(), tx, relay(), String::new());
    // Spawned before any Turn is sent.
    wait_for_file(&work.path().join("argv"));

    model.send_turn("Remember the word kiwi.".into());
    let (text, rate_limits) = read_turn(&mut events, WAIT);
    assert_eq!(
        text,
        "Got it, I'll remember the word kiwi.| I'm ready for whatever comes next.|"
    );
    assert_eq!(rate_limits.len(), 1);

    model.send_turn("What word?".into());
    let (text, rate_limits) = read_turn(&mut events, WAIT);
    assert_eq!(text, "Kiwi.| You asked me to remember that word.|");
    assert!(rate_limits.is_empty());

    let stdin = std::fs::read_to_string(work.path().join("stdin")).unwrap();
    let lines: Vec<serde_json::Value> = stdin
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            serde_json::json!({"type":"user","message":{"role":"user","content":"Remember the word kiwi."}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":"What word?"}}),
        ]
    );
    let pids = std::fs::read_to_string(work.path().join("pids")).unwrap();
    assert_eq!(pids.lines().count(), 1, "one process for the Session");

    // Ending the Session ends the process.
    let pid = pids.trim().to_owned();
    drop(model);
    let deadline = Instant::now() + WAIT;
    while Path::new("/proc").join(&pid).exists() {
        assert!(Instant::now() < deadline, "claude outlived its Session");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn claude_dying_mid_turn_reports_that_it_died() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "dying-claude");
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());

    model.send_turn("Remember the word kiwi.".into());

    let deadline = Instant::now() + WAIT;
    let mut text = String::new();
    loop {
        match events.try_recv() {
            Ok(ThinkingEvent::Delta(delta)) => text.push_str(&delta),
            Ok(ThinkingEvent::Died) => break,
            Ok(ThinkingEvent::TurnEnd) => panic!("the Turn ended as if claude had finished"),
            Ok(_) => {}
            Err(_) => {
                assert!(Instant::now() < deadline, "no Died: {text:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    assert_eq!(text, "Got it, I'll remember the word kiw");
}

#[test]
fn ending_the_session_is_not_a_death() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude");
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());
    model.send_turn("Remember the word kiwi.".into());
    read_turn(&mut events, WAIT);

    drop(model);

    // Every sender goes once claude has been stopped.
    let rest = std::thread::spawn(move || {
        std::iter::from_fn(|| events.blocking_recv()).collect::<Vec<_>>()
    });
    let deadline = Instant::now() + WAIT;
    while !rest.is_finished() {
        assert!(Instant::now() < deadline, "claude's events never closed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let rest = rest.join().unwrap();
    assert!(!rest.contains(&ThinkingEvent::Died), "{rest:?}");
}

#[test]
fn runs_with_an_isolated_config_naming_only_the_relay() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude");
    let (tx, _events) = mpsc::unbounded_channel();
    let _model = claude.open(&fast(), tx, relay(), "Speak like a pirate.".to_owned());
    let args = args(work.path());

    for flag in [
        "-p",
        "--verbose",
        "--include-partial-messages",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--disable-slash-commands",
    ] {
        assert!(
            args.iter().any(|a| a == flag),
            "{flag} missing from {args:?}"
        );
    }
    assert!(!args.iter().any(|a| a == "--bare"));
    assert_eq!(value_of(&args, "--input-format"), "stream-json");
    assert_eq!(value_of(&args, "--output-format"), "stream-json");
    assert_eq!(value_of(&args, "--setting-sources"), "");
    assert_eq!(value_of(&args, "--tools"), "");
    assert_eq!(value_of(&args, "--permission-mode"), "dontAsk");
    assert_eq!(value_of(&args, "--allowedTools"), "mcp__lucy__*");
    let mcp: serde_json::Value = serde_json::from_str(value_of(&args, "--mcp-config")).unwrap();
    assert_eq!(
        mcp,
        serde_json::json!({"mcpServers": {"lucy": {
            "command": "/usr/bin/lucy",
            "args": ["mcp-relay", "--token", "abc"],
        }}})
    );
    assert_eq!(value_of(&args, "--model"), "haiku");
    assert_eq!(
        value_of(&args, "--append-system-prompt"),
        "Speak like a pirate."
    );
}

#[test]
fn any_model_the_cli_accepts_is_passed_through() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude");
    let (tx, _events) = mpsc::unbounded_channel();
    let provider = Provider {
        model: Some("claude-not-yet-released".into()),
        ..fast()
    };

    let _model = claude.open(&provider, tx, relay(), String::new());

    assert_eq!(
        value_of(&args(work.path()), "--model"),
        "claude-not-yet-released"
    );
}

/// Opt-in: `cargo test -p lucyd --test claude -- --ignored`. Uses the real
/// `claude` on PATH and the user's subscription.
#[test]
#[ignore]
fn live_claude_keeps_context_across_turns() {
    let work = tempfile::tempdir().unwrap();
    let claude = Claude::new(PathBuf::from("claude"), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    // A relay that exits at once: no tools, which this test doesn't need.
    let relay = Relay {
        program: PathBuf::from("true"),
        args: vec![],
    };
    let mut model = claude.open(&fast(), tx, relay, String::new());

    model.send_turn("Remember the word kiwi. Reply with just OK.".into());
    read_turn(&mut events, LIVE_WAIT);
    model.send_turn("What word did I ask you to remember? One word.".into());
    let (reply, _) = read_turn(&mut events, LIVE_WAIT);
    assert!(reply.to_lowercase().contains("kiwi"), "{reply:?}");
}
