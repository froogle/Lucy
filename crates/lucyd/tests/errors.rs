//! Failures are spoken plainly and never hang a Session.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::adapters::{RateLimit, RateLimitStatus};

#[test]
fn an_stt_failure_is_apologised_for_and_the_session_carries_on() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    lucyd.say_but_stt_fails();

    assert_eq!(lucyd.tts.next_sentence(), "Sorry, I didn't catch that.");
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Listening);

    let a_good_utterance_in_between = "hello";
    lucyd.say(a_good_utterance_in_between);
    assert_eq!(lucyd.thinking.next_turn(), "hello");
    lucyd.thinking.turn_end();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say_but_stt_fails();
    assert_eq!(lucyd.tts.next_sentence(), "Sorry, I didn't catch that.");
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    assert_eq!(next_state(&mut events).state, State::Listening);
}

#[test]
fn two_stt_failures_in_a_row_end_the_session() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say_but_stt_fails();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    while next_state(&mut events).state != State::Listening {}

    lucyd.say_but_stt_fails();

    assert_eq!(lucyd.tts.next_sentence(), "Sorry, I didn't catch that.");
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    assert_eq!(next_state(&mut events).state, State::Idle);
    lucyd.thinking.assert_no_turn();
}

#[test]
fn a_dead_thinking_process_is_respawned_silently_and_the_turn_resent() {
    let lucyd = Harness::start();
    let _keeps_notifications_quiet = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();

    lucyd.thinking.dies();

    assert_eq!(lucyd.thinking.next_turn(), "what's the weather");
    let opened = lucyd.thinking.opened();
    assert_eq!(opened.len(), 2);
    assert_eq!(opened[0], opened[1], "the same Provider");
    lucyd.thinking.delta("Sunny.");
    lucyd.thinking.turn_end();
    assert_eq!(lucyd.tts.next_sentence(), "Sunny.");
    lucyd.tts.finish_playing();
    let states: Vec<State> = (0..3).map(|_| next_state(&mut events).state).collect();
    assert_eq!(states, [State::Thinking, State::Speaking, State::Listening]);
    lucyd.notifier.assert_no_notification();

    lucyd.say("and tomorrow");
    lucyd.thinking.next_turn();
    lucyd.thinking.dies();
    assert_eq!(lucyd.thinking.next_turn(), "and tomorrow");
    assert_eq!(lucyd.thinking.opened().len(), 3);
}

#[test]
fn when_the_respawned_thinking_process_dies_too_she_says_so_notifies_and_ends() {
    let lucyd = Harness::start();
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();
    lucyd.thinking.dies();
    lucyd.thinking.next_turn();

    lucyd.thinking.dies();

    assert_eq!(
        lucyd.tts.next_sentence(),
        "I've lost my connection to Claude."
    );
    let shown = lucyd.notifier.next_notification();
    assert!(shown.summary.contains("Claude"), "{shown:?}");
    lucyd.tts.finish_playing();
    let states: Vec<State> = (0..4).map(|_| next_state(&mut events).state).collect();
    assert_eq!(
        states,
        [State::Thinking, State::Error, State::Speaking, State::Idle]
    );
    let opened = lucyd.thinking.opened();
    assert_eq!(opened.len(), 2);
    assert_eq!(opened[0], opened[1], "never another Provider");
}

#[test]
fn a_plan_limit_is_spoken_with_its_reset_time_and_ends_with_no_tier_switch() {
    let lucyd = Harness::with_config(TWO_TIERS);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("hello");
    lucyd.thinking.next_turn();

    lucyd.thinking.rate_limit(RateLimit {
        status: RateLimitStatus::Rejected,
        rate_limit_type: Some("five_hour".into()),
        resets_at: Some(in_an_hour()),
        utilization: Some(1.0),
    });
    lucyd.thinking.turn_end();

    let said = lucyd.tts.next_sentence();
    assert!(
        said.starts_with("You've reached your Claude five-hour usage limit. It resets "),
        "{said}"
    );
    lucyd.tts.finish_playing();
    let states: Vec<State> = (0..4).map(|_| next_state(&mut events).state).collect();
    assert_eq!(
        states,
        [State::Thinking, State::Error, State::Speaking, State::Idle]
    );
    let opened = lucyd.thinking.opened();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].model.as_deref(), Some("haiku"));
}

#[test]
fn a_limit_with_no_reset_time_is_still_spoken() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("hello");
    lucyd.thinking.next_turn();

    lucyd.thinking.rate_limit(RateLimit {
        status: RateLimitStatus::Rejected,
        rate_limit_type: None,
        resets_at: None,
        utilization: None,
    });

    assert_eq!(
        lucyd.tts.next_sentence(),
        "You've reached your Claude usage limit."
    );
}

#[test]
fn a_limit_warning_is_not_an_error() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("hello");
    lucyd.thinking.next_turn();

    lucyd.thinking.rate_limit(RateLimit {
        status: RateLimitStatus::AllowedWarning,
        rate_limit_type: Some("seven_day".into()),
        resets_at: Some(in_an_hour()),
        utilization: Some(0.9),
    });
    lucyd.thinking.delta("Hi.");
    lucyd.thinking.turn_end();

    assert_eq!(lucyd.tts.next_sentence(), "Hi.");
    let states: Vec<State> = (0..2).map(|_| next_state(&mut events).state).collect();
    assert_eq!(states, [State::Thinking, State::Speaking]);
}

const TWO_TIERS: &str = r#"
[providers.quick]
kind = "claude"
model = "haiku"

[providers.deep]
kind = "claude"
model = "opus"

[tiers]
fast = "quick"
advanced = "deep"
"#;

fn in_an_hour() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    now.as_secs() + 3600
}
