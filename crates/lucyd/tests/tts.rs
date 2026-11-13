mod harness;

use harness::{next_state, Harness};
use lucy_ipc::{Event, LoudnessEvent, State, Topic};

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
        Event::Level(LoudnessEvent { rms: 0.25 })
    );
    assert_eq!(
        meter.next_event().unwrap(),
        Event::Level(LoudnessEvent { rms: 0.5 })
    );

    lucyd.tts.finish_playing();
    events.end_session().unwrap();
    // `next_state` fails on any event other than `state`.
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
    // An Indicator keeps the notification fallback quiet.
    let _indicator = lucyd.indicator("lucy-indicator");
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
        "It's three o'clock."
    );
    assert_eq!(lucyd.notifier.next_notification().body, "Time for tea");
    while next_state(&mut events).state != State::Listening {}

    // The Session carries on with the next Turn.
    lucyd.say("thanks");
    assert_eq!(lucyd.thinking.next_turn(), "thanks");
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}
