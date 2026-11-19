#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]
mod harness;

use harness::{next_state, Harness};
use lucy_ipc::{Event, LevelEvent, State, Topic};

#[test]
fn playback_level_goes_only_to_level_subscribers() {
    let lucyd = Harness::start();
    let mut meter = lucyd.client();
    meter.subscribe(&[Topic::Level]).unwrap();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("sing to me");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("La la la.");
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();

    lucyd.tts.level(0.25);
    lucyd.tts.level(0.5);
    assert_eq!(
        meter.next_event().unwrap(),
        Event::Level(LevelEvent { rms: 0.25 })
    );
    assert_eq!(
        meter.next_event().unwrap(),
        Event::Level(LevelEvent { rms: 0.5 })
    );

    lucyd.tts.finish_playing();
    events.end_session().unwrap();
    let states: Vec<State> = (0..4).map(|_| next_state(&mut events).state).collect();
    assert_eq!(
        states,
        [
            State::Thinking,
            State::Speaking,
            State::Listening,
            State::Idle
        ]
    );
    lucyd.assert_nothing_private_on_bus();
    lucyd.assert_no_level_on_bus();
}

#[test]
fn when_tts_fails_the_reply_becomes_notification_text_and_the_session_continues() {
    let lucyd = Harness::start();
    let _keeps_notifications_quiet = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.tts.fail();

    lucyd.say("what time is it");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("It's three o'clock.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.delta(" Time for tea");
    lucyd.thinking.turn_end();

    assert_eq!(
        lucyd.notifier.next_notification().body,
        "It's three o'clock. Time for tea",
        "the whole reply, in one notification"
    );
    while next_state(&mut events).state != State::Listening {}
    lucyd.notifier.assert_no_notification();

    lucyd.say("thanks");
    assert_eq!(lucyd.thinking.next_turn(), "thanks");
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

const KOKORO: &str = "[tts]\nkind = \"kokoro\"\n[kokoro]\nvoice = \"bm_george\"\n";

#[test]
fn once_config_chooses_kokoro_a_session_speaks_through_it_in_its_voice() {
    let lucyd = Harness::with_config(KOKORO);

    lucyd.client().start_session().unwrap();

    assert_eq!(
        lucyd.tts.voice(),
        Some(lucy_config::Voice {
            kind: lucy_config::TtsKind::Kokoro,
            name: "bm_george".to_owned(),
        })
    );
    lucyd.client().end_session().unwrap();
}

#[test]
fn when_kokoro_fails_the_reply_becomes_notification_text_and_the_session_continues() {
    let lucyd = Harness::with_config(KOKORO);
    let _keeps_notifications_quiet = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    assert_eq!(
        lucyd.tts.voice().map(|voice| voice.kind),
        Some(lucy_config::TtsKind::Kokoro)
    );
    lucyd.tts.fail();

    lucyd.say("what time is it");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("It's three o'clock.");
    lucyd.thinking.turn_end();

    assert_eq!(
        lucyd.notifier.next_notification().body,
        "It's three o'clock."
    );
    while next_state(&mut events).state != State::Listening {}

    lucyd.say("thanks");
    assert_eq!(lucyd.thinking.next_turn(), "thanks");
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}
