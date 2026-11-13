//! Sessions on a `codex` Provider, through the harness with the real `codex`
//! adapter driving a fake `codex app-server` (`fixtures/codex/fake-codex`).
//! Lucy uses Codex only on a ChatGPT login unless the Provider opts out.

mod harness;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness::{next_state, Harness};
use lucy_ipc::{ProviderKind, State};
use lucyd::codex::Codex;

const CODEX_TIERS: &str = r#"
[providers.chatgpt]
kind = "codex"
model = "gpt-6-luna"

[tiers]
fast = "chatgpt"
advanced = "chatgpt"
"#;

const API_KEY_LOGIN: &str =
    r#"{"account":{"type":"apiKey"},"userId":null,"requiresOpenaiAuth":true}"#;

fn fake_codex() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/fake-codex")
}

/// lucyd running the real adapter against the fake app-server in `work`.
fn lucyd(work: &Path, config: &str) -> Harness {
    Harness::with_thinking(
        Some(config),
        Arc::new(Codex::new(fake_codex(), work.to_owned())),
    )
}

/// Each line Lucy sent the fake, across all its processes.
fn sent(work: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(work.join("stdin"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn turns(work: &Path) -> usize {
    sent(work)
        .iter()
        .filter(|message| message["method"] == "turn/start")
        .count()
}

/// Opens a Session and returns what Lucy says before ending it by herself.
fn refused(lucyd: &Harness) -> String {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    let said = lucyd.tts.next_sentence();
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Idle);
    said
}

#[test]
fn a_session_on_codex_speaks_each_reply_sentence_by_sentence() {
    let work = tempfile::tempdir().unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    lucyd.say("Remember the word kiwi.");
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Got it, I'll remember the word kiwi."
    );
    assert_eq!(
        lucyd.tts.next_sentence(),
        "I'm ready for whatever comes next."
    );
    lucyd.tts.finish_playing();
    lucyd.tts.finish_playing();
    while next_state(&mut events).state != State::Listening {}

    lucyd.say("What word?");
    assert_eq!(lucyd.tts.next_sentence(), "Kiwi.");
    assert_eq!(
        lucyd.tts.next_sentence(),
        "You asked me to remember that word."
    );
    events.end_session().unwrap();

    let pids = std::fs::read_to_string(work.path().join("pids")).unwrap();
    assert_eq!(pids.lines().count(), 1, "one app-server for the Session");
    assert_eq!(turns(work.path()), 2);
}

#[test]
fn being_signed_out_is_spoken_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(
        work.path().join("account.json"),
        r#"{"account":null,"userId":null,"requiresOpenaiAuth":true}"#,
    )
    .unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);

    let said = refused(&lucyd);

    assert!(said.contains("codex login"), "{said}");
    assert_eq!(turns(work.path()), 0);
}

#[test]
fn a_login_that_is_not_chatgpt_is_refused_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("account.json"), API_KEY_LOGIN).unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);

    let said = refused(&lucyd);

    assert!(said.contains("ChatGPT"), "{said}");
    assert!(said.contains("allow_api_key"), "{said}");
    assert_eq!(turns(work.path()), 0, "nothing was sent to codex");
}

#[test]
fn a_missing_codex_is_spoken_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    let codex = Codex::new(work.path().join("no-such-codex"), work.path().to_owned());
    let lucyd = Harness::with_thinking(Some(CODEX_TIERS), Arc::new(codex));

    let said = refused(&lucyd);

    assert!(said.contains("install"), "{said}");
}

#[test]
fn allow_api_key_lets_codex_bill_an_api_key() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("account.json"), API_KEY_LOGIN).unwrap();
    let config = CODEX_TIERS.replace(
        "model = \"gpt-6-luna\"",
        "model = \"gpt-6-luna\"\nallow_api_key = true",
    );
    let lucyd = lucyd(work.path(), &config);
    let mut client = lucyd.client();
    client.start_session().unwrap();

    lucyd.say("Remember the word kiwi.");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Got it, I'll remember the word kiwi."
    );
    let argv = std::fs::read_to_string(work.path().join("argv")).unwrap();
    assert!(!argv.contains("forced_login_method"), "{argv:?}");
    assert!(!sent(work.path())
        .iter()
        .any(|message| message["method"] == "account/read"));
}

#[test]
fn providers_status_shows_a_signed_out_codex_without_a_model_call() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(
        work.path().join("account.json"),
        r#"{"account":null,"userId":null,"requiresOpenaiAuth":true}"#,
    )
    .unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);
    let mut client = lucyd.client();

    let status = client.providers_status().unwrap();

    let codex = status
        .providers
        .iter()
        .find(|p| p.name == "chatgpt")
        .unwrap();
    assert_eq!(codex.kind, ProviderKind::Codex);
    let problem = codex.problem.as_deref().unwrap_or_default();
    assert!(problem.contains("codex login"), "{problem}");
    assert_eq!(turns(work.path()), 0);
}

#[test]
fn provider_check_is_a_real_reply_ok_turn() {
    let work = tempfile::tempdir().unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);
    let mut client = lucyd.client();

    assert_eq!(client.check_provider("chatgpt").unwrap().problem, None);

    let turn = sent(work.path())
        .into_iter()
        .find(|message| message["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["input"][0]["text"], "Reply with just OK.");
}

#[test]
fn provider_check_reports_a_failed_turn() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(
        work.path().join("turn1.jsonl"),
        concat!(
            r#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"failed","#,
            r#""error":{"message":"You've hit your usage limit.","codexErrorInfo":"usageLimitExceeded","additionalDetails":null}}}}"#,
            "\n"
        ),
    )
    .unwrap();
    let lucyd = lucyd(work.path(), CODEX_TIERS);
    let mut client = lucyd.client();

    let problem = client.check_provider("chatgpt").unwrap().problem.unwrap();

    assert!(problem.contains("usage limit"), "{problem}");
}
