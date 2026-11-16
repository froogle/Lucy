//! The subscription guard, through the harness and the real `claude` adapter.
//! The adapter drives a fake `claude` CLI (`fixtures/claude/fake-claude`).
//! When the CLI's version changes, Lucy re-verifies before the Session goes
//! on: the login must be claude.ai, and one turn must show
//! `apiKeySource: "none"` and a `rate_limit_event`.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::claude::Claude;

const VERIFY_PROMPT: &str = "Reply with just OK.";

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/fake-claude")
}

fn lucyd(work: &Path, config: Option<&str>) -> Harness {
    Harness::with_thinking(
        config,
        Arc::new(Claude::new(fake_claude(), work.to_owned())),
    )
}

fn sent(work: &Path) -> Vec<String> {
    std::fs::read_to_string(work.join("stdin"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn verifications(work: &Path) -> usize {
    sent(work)
        .iter()
        .filter(|line| line.contains(VERIFY_PROMPT))
        .count()
}

fn calls(work: &Path) -> Vec<String> {
    std::fs::read_to_string(work.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn talk(lucyd: &Harness) -> String {
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.say("Remember the word kiwi.");
    let heard = lucyd.tts.next_sentence();
    assert_eq!(
        lucyd.tts.next_sentence(),
        "I'm ready for whatever comes next."
    );
    client.end_session().unwrap();
    heard
}

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

fn api_key_turn() -> String {
    let recorded = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/turn1.jsonl"),
    )
    .unwrap();
    recorded
        .lines()
        .filter(|line| !line.contains(r#""type":"rate_limit_event""#))
        .map(|line| {
            line.replace(
                r#""apiKeySource":"none""#,
                r#""apiKeySource":"ANTHROPIC_API_KEY""#,
            ) + "\n"
        })
        .collect()
}

#[test]
fn a_new_version_is_verified_once_before_the_session_goes_on() {
    let work = tempfile::tempdir().unwrap();
    let lucyd = lucyd(work.path(), None);

    assert_eq!(talk(&lucyd), "Got it, I'll remember the word kiwi.");

    assert_eq!(verifications(work.path()), 1);
    assert!(sent(work.path())[0].contains(VERIFY_PROMPT));
    assert!(calls(work.path()).iter().any(|c| c == "auth"));

    talk(&lucyd);
    assert_eq!(verifications(work.path()), 1, "same version, no new check");

    std::fs::write(work.path().join("version"), "2.1.324 (Claude Code)\n").unwrap();
    assert_eq!(talk(&lucyd), "Got it, I'll remember the word kiwi.");
    assert_eq!(verifications(work.path()), 2, "new version, checked again");
}

#[test]
fn a_version_that_bills_an_api_key_is_refused_with_the_reason() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("turn1.jsonl"), api_key_turn()).unwrap();
    let lucyd = lucyd(work.path(), None);

    let said = refused(&lucyd);

    assert!(said.contains("API key"), "{said}");
    assert!(said.contains("allow_api_key"), "{said}");
    assert_eq!(sent(work.path()).len(), 1, "only the check reached claude");

    refused(&lucyd);
    assert_eq!(
        verifications(work.path()),
        2,
        "still unverified, so the next Session checks and refuses again"
    );
}

#[test]
fn a_login_that_is_not_a_claude_subscription_is_refused_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(
        work.path().join("auth.json"),
        r#"{"loggedIn":true,"authMethod":"api_key","apiProvider":"firstParty"}"#,
    )
    .unwrap();
    let lucyd = lucyd(work.path(), None);

    let said = refused(&lucyd);

    assert!(said.contains("/login"), "{said}");
    assert!(said.contains("allow_api_key"), "{said}");
    assert!(sent(work.path()).is_empty(), "nothing was sent to claude");
}

#[test]
fn being_signed_out_is_spoken_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("auth.json"), r#"{"loggedIn":false}"#).unwrap();
    let lucyd = lucyd(work.path(), None);

    let said = refused(&lucyd);

    assert!(said.contains("/login"), "{said}");
}

#[test]
fn a_missing_claude_is_spoken_with_the_fix() {
    let work = tempfile::tempdir().unwrap();
    let claude = Claude::new(work.path().join("no-such-claude"), work.path().to_owned());
    let lucyd = Harness::with_thinking(None, Arc::new(claude));

    let said = refused(&lucyd);

    assert!(said.contains("install"), "{said}");
}

#[test]
fn allow_api_key_turns_the_guard_off() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("turn1.jsonl"), api_key_turn()).unwrap();
    std::fs::write(work.path().join("auth.json"), r#"{"loggedIn":false}"#).unwrap();
    let config = r#"
[providers.claude-fast]
kind = "claude"
model = "haiku"
allow_api_key = true
"#;
    let lucyd = lucyd(work.path(), Some(config));

    assert_eq!(talk(&lucyd), "Got it, I'll remember the word kiwi.");

    assert_eq!(verifications(work.path()), 0);
    assert!(!calls(work.path()).iter().any(|c| c == "auth"));
}

#[test]
fn provider_check_is_a_real_reply_ok_turn_that_verifies_the_version() {
    let work = tempfile::tempdir().unwrap();
    let lucyd = lucyd(work.path(), None);
    let mut client = lucyd.client();

    assert_eq!(client.check_provider("claude-fast").unwrap().problem, None);
    assert_eq!(sent(work.path()).len(), 1);
    assert!(sent(work.path())[0].contains(VERIFY_PROMPT));

    talk(&lucyd);
    assert_eq!(verifications(work.path()), 1, "already verified");
}

#[test]
fn provider_check_reports_a_version_that_bills_an_api_key() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("turn1.jsonl"), api_key_turn()).unwrap();
    let lucyd = lucyd(work.path(), None);
    let mut client = lucyd.client();

    let problem = client.check_provider("claude-fast").unwrap().problem;

    assert!(problem.unwrap().contains("allow_api_key"));
}

#[test]
fn providers_status_shows_a_signed_out_claude_without_a_model_call() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("auth.json"), r#"{"loggedIn":false}"#).unwrap();
    let lucyd = lucyd(work.path(), None);
    let mut client = lucyd.client();

    let status = client.providers_status().unwrap();

    assert_eq!(status.providers.len(), 2);
    for provider in &status.providers {
        let problem = provider.problem.as_deref().unwrap_or_default();
        assert!(problem.contains("/login"), "{problem}");
    }
    assert!(sent(work.path()).is_empty());
}

#[test]
fn stopping_lucyd_mid_session_deletes_the_conversation_claude_kept() {
    let work = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let claude = Claude::new(fake_claude(), work.path().to_owned())
        .with_config_dir(config.path().to_owned());
    let lucyd = Harness::with_thinking(None, Arc::new(claude));
    lucyd.client().start_session().unwrap();
    lucyd.say("Remember the word kiwi.");
    lucyd.tts.next_sentence();
    let conversations_kept = || {
        let projects = std::fs::read_dir(config.path().join("projects"));
        projects
            .into_iter()
            .flatten()
            .flat_map(|project| std::fs::read_dir(project.unwrap().path()).unwrap())
            .count()
    };
    assert_ne!(conversations_kept(), 0, "claude kept no conversation");

    drop(lucyd);

    assert_eq!(conversations_kept(), 0);
}
