//! `config.toml` is read at each Session start, and the fast Model tier
//! picks the Thinking Provider that Session uses.

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

const TWO_PROVIDERS: &str = r#"
# My two Providers.
[providers.quick]
kind = "claude"
model = "haiku"

[providers.deep]
kind = "claude"
model = "some-model-only-the-cli-knows"

[tiers]
fast = "quick" # everyday talk
advanced = "deep"
"#;

#[test]
fn the_fast_tier_picks_the_provider_a_session_uses() {
    let lucyd = Harness::with_config(TWO_PROVIDERS);
    let mut client = lucyd.client();

    client.start_session().unwrap();

    let opened = lucyd.thinking.opened();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].model.as_deref(), Some("haiku"));
}

#[test]
fn config_edits_apply_at_the_next_session_without_a_restart() {
    let lucyd = Harness::with_config(TWO_PROVIDERS);
    let mut client = lucyd.client();
    client.start_session().unwrap();
    client.end_session().unwrap();

    lucy_config::set(lucyd.config_path(), &["tiers", "fast"], "deep").unwrap();
    client.start_session().unwrap();

    let opened = lucyd.thinking.opened();
    assert_eq!(
        opened[1].model.as_deref(),
        Some("some-model-only-the-cli-knows")
    );
}

#[test]
fn with_no_config_file_a_session_uses_the_default_fast_tier() {
    let lucyd = Harness::start();
    assert!(!lucyd.config_path().exists());
    let mut client = lucyd.client();

    client.start_session().unwrap();

    let opened = lucyd.thinking.opened();
    assert_eq!(opened[0].model.as_deref(), Some("haiku"));
    assert!(!opened[0].allow_api_key);
}

#[test]
fn a_tier_naming_a_missing_provider_is_spoken_with_the_fix_then_the_session_ends() {
    let lucyd = Harness::with_config("[tiers]\nfast = \"nope\"\n");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    let said = lucyd.tts.next_sentence();
    assert!(
        said.contains("nope") && said.contains("providers.nope"),
        "{said}"
    );
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.tts.finish_playing();

    assert_eq!(next_state(&mut events).state, State::Idle);
    assert!(lucyd.thinking.opened().is_empty());
}

#[test]
fn a_config_file_that_does_not_parse_is_spoken_then_the_session_ends() {
    let lucyd = Harness::with_config("this is = = not toml");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    let said = lucyd.tts.next_sentence();
    assert!(said.contains("config.toml"), "{said}");
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.tts.finish_playing();

    assert_eq!(next_state(&mut events).state, State::Idle);
}
