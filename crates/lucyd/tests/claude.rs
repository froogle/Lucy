//! The `claude` Thinking Provider, driven through the `Thinking` seam.
//! A fake `claude` replays a recorded real stream-json transcript
//! (`fixtures/claude/`).
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lucy_config::{Config, Provider, Tier};
use lucyd::adapters::{Relay, Thinking, ThinkingEvent};
use lucyd::claude::Claude;
use tokio::sync::mpsc;

mod harness;
use harness::wait_until;

const WAIT: Duration = Duration::from_secs(5);
const LIVE_WAIT: Duration = Duration::from_secs(60);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude")
}

fn fast() -> Provider {
    Config::default().provider(Tier::Fast).unwrap().clone()
}

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

fn wait_for_file(path: &Path) -> String {
    wait_until(&format!("{} never appeared", path.display()), || {
        std::fs::read_to_string(path).ok()
    })
}

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
            Ok(ThinkingEvent::Died(_)) => panic!("claude died: {text:?}"),
            Ok(ThinkingEvent::Failed(message)) => panic!("the Turn failed: {message}"),
            Err(_) => {
                assert!(Instant::now() < deadline, "the Turn never ended: {text:?}");
                // sleep-ok: poll loop with a deadline
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

    let pid = pids.trim().to_owned();
    drop(model);
    wait_until("claude outlived its Session", || {
        (!Path::new("/proc").join(&pid).exists()).then_some(())
    });
}

#[test]
fn claude_dying_mid_turn_reports_that_it_died() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "dying-claude");
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());

    model.send_turn("Remember the word kiwi.".into());

    let mut text = String::new();
    wait_until("no Died", || match events.try_recv() {
        Ok(ThinkingEvent::Delta(delta)) => {
            text.push_str(&delta);
            None
        }
        Ok(ThinkingEvent::Died(_)) => Some(()),
        Ok(ThinkingEvent::TurnEnd) => panic!("the Turn ended as if claude had finished"),
        _ => None,
    });
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

    let rest = std::thread::spawn(move || {
        std::iter::from_fn(|| events.blocking_recv()).collect::<Vec<_>>()
    });
    wait_until("claude's events never closed", || {
        rest.is_finished().then_some(())
    });
    let rest = rest.join().unwrap();
    assert!(
        !rest
            .iter()
            .any(|event| matches!(event, ThinkingEvent::Died(_))),
        "{rest:?}"
    );
}

const SESSION_ID: &str = "09dd6ca6-f8e2-473e-812c-ab5acf9cdfd5";

fn kept(config: &Path, id: &str) -> Vec<PathBuf> {
    let mut kept = vec![];
    for project in std::fs::read_dir(config.join("projects"))
        .into_iter()
        .flatten()
    {
        let project = project.unwrap().path();
        for name in [format!("{id}.jsonl"), id.to_owned()] {
            if project.join(&name).exists() {
                kept.push(project.join(name));
            }
        }
    }
    kept
}

fn the_users_own(config: &Path, id: &str) -> PathBuf {
    let theirs = kept(config, id)[0].with_file_name("their-own.jsonl");
    std::fs::write(&theirs, "{}\n").unwrap();
    theirs
}

#[test]
fn a_sessions_conversation_is_deleted_from_disk_when_it_ends() {
    let work = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude").with_config_dir(config.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());
    model.send_turn("Remember the word kiwi.".into());
    read_turn(&mut events, WAIT);
    assert_eq!(
        kept(config.path(), SESSION_ID).len(),
        2,
        "no transcript to delete"
    );
    let theirs = the_users_own(config.path(), SESSION_ID);

    drop(model);

    assert_eq!(kept(config.path(), SESSION_ID), Vec::<PathBuf>::new());
    assert!(theirs.is_file(), "another conversation was deleted");
}

#[test]
fn stopping_lucyd_deletes_the_conversations_still_open() {
    let work = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude").with_config_dir(config.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());
    model.send_turn("Remember the word kiwi.".into());
    read_turn(&mut events, WAIT);
    let theirs = the_users_own(config.path(), SESSION_ID);

    claude.shutdown();

    assert_eq!(kept(config.path(), SESSION_ID), Vec::<PathBuf>::new());
    assert!(theirs.is_file(), "another conversation was deleted");
    drop(model);
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

fn unfinished_turn() -> String {
    let turn = std::fs::read_to_string(fixtures().join("turn1.jsonl")).unwrap();
    turn.lines()
        .take(8)
        .map(|line| format!("{line}\n"))
        .collect()
}

fn start_unfinished_turn(
    model: &mut Box<dyn lucyd::adapters::SessionModel>,
    events: &mut mpsc::UnboundedReceiver<ThinkingEvent>,
) {
    model.send_turn("Remember the word kiwi.".into());
    let mut text = String::new();
    wait_until("the Turn never got going", || {
        while let Ok(event) = events.try_recv() {
            match event {
                ThinkingEvent::Delta(delta) => text.push_str(&delta),
                ThinkingEvent::RateLimit(_) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        (text == "Got it, I'll remember the word kiw").then_some(())
    });
}

#[test]
fn an_interrupt_stops_the_turn_with_the_control_request_and_keeps_the_process() {
    let work = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude");
    std::fs::write(work.path().join("turn1.jsonl"), unfinished_turn()).unwrap();
    std::fs::write(
        work.path().join("turn2.jsonl"),
        "{\"type\":\"control_response\",\"response\":{\"subtype\":\"success\",\"request_id\":\"interrupt-1\"}}\n\
         {\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true}\n",
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("turn2.jsonl"),
        work.path().join("turn3.jsonl"),
    )
    .unwrap();
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());
    start_unfinished_turn(&mut model, &mut events);

    model.interrupt();
    model.send_turn("What word?".into());

    let (text, _) = read_turn(&mut events, WAIT);
    assert_eq!(
        text, "Kiwi.| You asked me to remember that word.|",
        "nothing more of the interrupted Turn, not even its end"
    );
    let stdin = std::fs::read_to_string(work.path().join("stdin")).unwrap();
    let interrupt: serde_json::Value = serde_json::from_str(stdin.lines().nth(1).unwrap()).unwrap();
    assert_eq!(
        interrupt,
        serde_json::json!({"type": "control_request", "request_id": "interrupt-1",
                           "request": {"subtype": "interrupt"}})
    );
    let pids = std::fs::read_to_string(work.path().join("pids")).unwrap();
    assert_eq!(pids.lines().count(), 1, "one process for the Session");
}

#[test]
fn without_the_control_interrupt_claude_gets_sigint_and_a_new_process() {
    let work = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let claude = verified(work.path(), "fake-claude").with_config_dir(config.path().to_owned());
    std::fs::write(work.path().join("turn1.jsonl"), unfinished_turn()).unwrap();
    std::fs::write(
        work.path().join("turn2.jsonl"),
        "{\"type\":\"control_response\",\"response\":{\"subtype\":\"error\",\"request_id\":\"interrupt-1\",\"error\":\"Unknown subtype\"}}\n",
    )
    .unwrap();
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = claude.open(&fast(), tx, relay(), String::new());
    start_unfinished_turn(&mut model, &mut events);
    let first = std::fs::read_to_string(work.path().join("pids")).unwrap();

    model.interrupt();

    let pids = wait_until("no new process", || {
        let pids = std::fs::read_to_string(work.path().join("pids")).unwrap();
        (pids.lines().count() == 2).then_some(pids)
    });
    wait_until("the interrupted claude outlived its SIGINT", || {
        (!Path::new("/proc").join(first.trim()).exists()).then_some(())
    });
    assert_ne!(pids.lines().nth(1), Some(first.trim()));
    let resumed = wait_until("the new process didn't resume the conversation", || {
        let args = args(work.path());
        args.iter()
            .any(|a| a == "--resume")
            .then(|| value_of(&args, "--resume").to_owned())
    });
    assert_eq!(resumed, "09dd6ca6-f8e2-473e-812c-ab5acf9cdfd5");
    std::fs::copy(
        fixtures().join("turn2.jsonl"),
        work.path().join("turn1.jsonl"),
    )
    .unwrap();
    model.send_turn("What word?".into());
    let (text, _) = read_turn(&mut events, WAIT);
    assert_eq!(text, "Kiwi.| You asked me to remember that word.|");

    drop(model);
    assert_eq!(
        kept(config.path(), SESSION_ID),
        Vec::<PathBuf>::new(),
        "both processes' conversation goes with the Session"
    );
}

#[test]
#[ignore = "uses the real claude on PATH and spends the user's subscription"]
fn live_claude_keeps_context_across_turns() {
    let work = tempfile::tempdir().unwrap();
    let claude = Claude::new(PathBuf::from("claude"), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
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
