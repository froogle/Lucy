//! The `codex` Thinking Provider, driven through the `Thinking` seam.
//! A fake `codex app-server` streams turns shaped like the app-server
//! protocol (`fixtures/codex/`).
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lucy_config::{Kind, Provider};
use lucyd::adapters::{Relay, Thinking, ThinkingEvent};
use lucyd::codex::Codex;
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod harness;
use harness::wait_until;

const WAIT: Duration = Duration::from_secs(5);
const LIVE_WAIT: Duration = Duration::from_secs(60);

fn fake_codex() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/fake-codex")
}

fn codex_provider(model: Option<&str>) -> Provider {
    Provider {
        kind: Kind::Codex,
        model: model.map(str::to_owned),
        allow_api_key: false,
    }
}

fn relay() -> Relay {
    Relay {
        program: PathBuf::from("/usr/bin/lucy"),
        args: vec!["mcp-relay".into(), "--token".into(), "abc".into()],
    }
}

fn sent(work: &Path) -> Vec<Value> {
    std::fs::read_to_string(work.join("stdin"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn requests(work: &Path, method: &str) -> Vec<Value> {
    sent(work)
        .into_iter()
        .filter(|message| message["method"] == method)
        .map(|message| message["params"].clone())
        .collect()
}

fn wait_for_requests(work: &Path, method: &str, count: usize) -> Vec<Value> {
    wait_until(&format!("{count} {method} never sent"), || {
        let found = requests(work, method);
        (found.len() >= count).then_some(found)
    })
}

fn read_turn(events: &mut mpsc::UnboundedReceiver<ThinkingEvent>, wait: Duration) -> String {
    let mut text = String::new();
    let deadline = Instant::now() + wait;
    loop {
        match events.try_recv() {
            Ok(ThinkingEvent::Delta(delta)) => text.push_str(&delta),
            Ok(ThinkingEvent::SentenceEnd) => text.push('|'),
            Ok(ThinkingEvent::TurnEnd) => return text,
            Ok(ThinkingEvent::RateLimit(_)) => {}
            Ok(ThinkingEvent::Unavailable(reason)) => panic!("Provider unavailable: {reason}"),
            Ok(ThinkingEvent::Died) => panic!("codex died: {text:?}"),
            Err(_) => {
                assert!(Instant::now() < deadline, "the Turn never ended: {text:?}");
                // sleep-ok: poll loop with a deadline
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn args(work: &Path) -> Vec<String> {
    let argv = wait_until("codex never started", || {
        std::fs::read_to_string(work.join("argv")).ok()
    });
    argv.split_terminator('\0').map(str::to_owned).collect()
}

fn settings(work: &Path) -> Vec<String> {
    let args = args(work);
    args.windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1].clone())
        .collect()
}

#[test]
fn one_app_server_serves_every_turn_of_a_session() {
    let work = tempfile::tempdir().unwrap();
    let codex = Codex::new(fake_codex(), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();

    let mut model = codex.open(&codex_provider(None), tx, relay(), String::new());
    model.send_turn("Remember the word kiwi.".into());
    assert_eq!(
        read_turn(&mut events, WAIT),
        "Got it, I'll remember the word kiwi.| I'm ready for whatever comes next.|"
    );
    model.send_turn("What word?".into());
    assert_eq!(
        read_turn(&mut events, WAIT),
        "Kiwi.| You asked me to remember that word.|"
    );

    let turns = requests(work.path(), "turn/start");
    assert_eq!(
        turns,
        [
            json!({"threadId": "thread-1", "input": [
                {"type": "text", "text": "Remember the word kiwi.", "text_elements": []},
            ]}),
            json!({"threadId": "thread-1", "input": [
                {"type": "text", "text": "What word?", "text_elements": []},
            ]}),
        ]
    );
    let pids = std::fs::read_to_string(work.path().join("pids")).unwrap();
    assert_eq!(pids.lines().count(), 1, "one process for the Session");

    let pid = pids.trim().to_owned();
    drop(model);
    wait_until("codex outlived its Session", || {
        (!Path::new("/proc").join(&pid).exists()).then_some(())
    });
    let rest = std::thread::spawn(move || {
        std::iter::from_fn(|| events.blocking_recv()).collect::<Vec<_>>()
    });
    wait_until("codex's events never closed", || {
        rest.is_finished().then_some(())
    });
    assert!(!rest.join().unwrap().contains(&ThinkingEvent::Died));
}

#[test]
fn runs_with_the_relay_as_its_mcp_server_and_a_chatgpt_login() {
    let work = tempfile::tempdir().unwrap();
    let codex = Codex::new(fake_codex(), work.path().to_owned());
    let (tx, _events) = mpsc::unbounded_channel();

    let _model = codex.open(
        &codex_provider(Some("gpt-6-luna")),
        tx,
        relay(),
        "Speak like a pirate.".to_owned(),
    );

    assert_eq!(args(work.path())[0], "app-server");
    let settings = settings(work.path());
    for setting in [
        r#"forced_login_method="chatgpt""#,
        r#"mcp_servers.lucy.command="/usr/bin/lucy""#,
        r#"mcp_servers.lucy.args=["mcp-relay","--token","abc"]"#,
        r#"mcp_servers.lucy.default_tools_approval_mode="approve""#,
        "features.shell_tool=false",
        r#"web_search="disabled""#,
    ] {
        assert!(
            settings.iter().any(|s| s == setting),
            "{setting} missing from {settings:?}"
        );
    }
    let thread = wait_for_requests(work.path(), "thread/start", 1).remove(0);
    assert_eq!(thread["model"], "gpt-6-luna");
    assert_eq!(thread["developerInstructions"], "Speak like a pirate.");
    assert_eq!(thread["cwd"], work.path().to_str().unwrap());
    assert_eq!(thread["sandbox"], "read-only");
    assert_eq!(thread["ephemeral"], true);
}

#[test]
fn approvals_are_declined_in_process() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(
        work.path().join("turn1.jsonl"),
        concat!(
            r#"{"id":0,"method":"item/commandExecution/requestApproval","params":{"kind":"command","threadId":"thread-1","turnId":"turn-1","itemId":"call_1","startedAtMs":1794579110000,"command":"cat ~/.ssh/id_ed25519","cwd":"/work","reason":null}}"#,
            "\n",
            r#"{"id":1,"method":"item/fileChange/requestApproval","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"call_2","startedAtMs":1794579110000,"reason":null,"grantRoot":null}}"#,
            "\n",
            r#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed","error":null}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let codex = Codex::new(fake_codex(), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = codex.open(&codex_provider(None), tx, relay(), String::new());

    model.send_turn("Read my SSH key.".into());
    read_turn(&mut events, WAIT);

    let answers = wait_until("the approvals were never answered", || {
        let answers: Vec<Value> = sent(work.path())
            .into_iter()
            .filter(|m| m.get("method").is_none())
            .collect();
        (answers.len() == 2).then_some(answers)
    });
    assert_eq!(
        answers,
        [
            json!({"id": 0, "result": {"decision": "decline"}}),
            json!({"id": 1, "result": {"decision": "decline"}}),
        ]
    );
}

fn generating(work: &Path) {
    let recorded = std::fs::read_to_string(fake_codex().with_file_name("turn1.jsonl")).unwrap();
    let partial: String = recorded
        .lines()
        .take_while(|line| !line.contains(r#""delta":" I'm""#))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(work.join("turn1.jsonl"), partial).unwrap();
}

#[test]
fn interrupting_stops_the_turn_and_silences_it() {
    let work = tempfile::tempdir().unwrap();
    generating(work.path());
    let codex = Codex::new(fake_codex(), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = codex.open(&codex_provider(None), tx, relay(), String::new());
    model.send_turn("Remember the word kiwi.".into());
    let first = wait_until("the reply never started", || match events.try_recv() {
        Ok(ThinkingEvent::Delta(delta)) => Some(delta),
        _ => None,
    });

    model.interrupt();

    let interrupts = wait_for_requests(work.path(), "turn/interrupt", 1);
    assert_eq!(
        interrupts,
        [json!({"threadId": "thread-1", "turnId": "turn-1"})]
    );
    model.send_turn("What word?".into());
    let heard = first + &read_turn(&mut events, WAIT);
    let interrupted = heard
        .strip_suffix("Kiwi.| You asked me to remember that word.|")
        .unwrap_or_else(|| panic!("{heard:?}"));
    assert!(
        "Got it, I'll remember the word kiwi.".starts_with(interrupted),
        "{heard:?}"
    );
}

#[test]
fn codex_dying_mid_turn_reports_that_it_died() {
    let work = tempfile::tempdir().unwrap();
    generating(work.path());
    std::fs::write(work.path().join("dies"), "").unwrap();
    let codex = Codex::new(fake_codex(), work.path().to_owned());
    let (tx, mut events) = mpsc::unbounded_channel();
    let mut model = codex.open(&codex_provider(None), tx, relay(), String::new());

    model.send_turn("Remember the word kiwi.".into());

    let mut text = String::new();
    wait_until("no Died", || match events.try_recv() {
        Ok(ThinkingEvent::Delta(delta)) => {
            text.push_str(&delta);
            None
        }
        Ok(ThinkingEvent::Died) => Some(()),
        Ok(ThinkingEvent::TurnEnd) => panic!("the Turn ended as if codex had finished"),
        _ => None,
    });
    assert_eq!(text, "Got it, I'll remember the word kiwi.");
}

#[test]
#[ignore = "uses the real codex on PATH and spends the user's ChatGPT subscription"]
fn live_codex_keeps_context_across_turns() {
    let work = tempfile::tempdir().unwrap();
    let codex = Codex::new(PathBuf::from("codex"), work.path().to_owned());
    let provider = codex_provider(Some(lucy_config::recommended_model(
        Kind::Codex,
        lucy_config::Tier::Fast,
    )));
    codex.check(&provider).unwrap();
    let (tx, mut events) = mpsc::unbounded_channel();
    let relay = Relay {
        program: PathBuf::from("true"),
        args: vec![],
    };
    let mut model = codex.open(&provider, tx, relay, String::new());

    model.send_turn("Remember the word kiwi. Reply with just OK.".into());
    read_turn(&mut events, LIVE_WAIT);
    model.send_turn("What word did I ask you to remember? One word.".into());
    let reply = read_turn(&mut events, LIVE_WAIT);
    assert!(reply.to_lowercase().contains("kiwi"), "{reply:?}");
}
