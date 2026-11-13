//! Pinned devices are re-resolved by name at each Session start, and a
//! device lost mid-Session ends the Session.

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

const MIC: &str = "alsa_input.usb-Blue_Yeti";
const SPEAKER: &str = "alsa_output.hdmi";
const PINNED: &str =
    "[audio]\nmic = \"alsa_input.usb-Blue_Yeti\"\nspeaker = \"alsa_output.hdmi\"\n";

#[test]
fn pinned_devices_are_used_when_they_are_there() {
    let lucyd = Harness::hearing(PINNED);
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut client = lucyd.client();

    client.start_session().unwrap();

    assert_eq!(lucyd.mic.opened().as_deref(), Some(MIC));
    assert_eq!(lucyd.tts.device().as_deref(), Some(SPEAKER));
    lucyd.notifier.assert_no_notification();
}

#[test]
fn a_missing_pinned_mic_falls_back_to_the_default_with_one_notification_and_keeps_the_pin() {
    let lucyd = Harness::hearing(PINNED);
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut client = lucyd.client();
    lucyd.mic.unplug(MIC);

    client.start_session().unwrap();

    assert_eq!(lucyd.mic.opened().as_deref(), Some("default"));
    let shown = lucyd.notifier.next_notification();
    assert!(shown.body.contains(MIC), "{shown:?}");
    lucyd.notifier.assert_no_notification();
    assert_eq!(
        std::fs::read_to_string(lucyd.config_path()).unwrap(),
        PINNED
    );

    // Back for the next Session.
    client.end_session().unwrap();
    lucyd.mic.plug_in(MIC);
    client.start_session().unwrap();
    assert_eq!(lucyd.mic.opened().as_deref(), Some(MIC));
    lucyd.notifier.assert_no_notification();
}

#[test]
fn a_missing_pinned_speaker_falls_back_to_the_default_and_keeps_the_pin() {
    let lucyd = Harness::hearing(PINNED);
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut client = lucyd.client();
    lucyd.mic.unplug(SPEAKER);

    client.start_session().unwrap();

    assert_eq!(lucyd.tts.device().as_deref(), Some("default"));
    let shown = lucyd.notifier.next_notification();
    assert!(shown.body.contains(SPEAKER), "{shown:?}");

    client.end_session().unwrap();
    lucyd.mic.plug_in(SPEAKER);
    client.start_session().unwrap();
    assert_eq!(lucyd.tts.device().as_deref(), Some(SPEAKER));
}

#[test]
fn both_pins_missing_is_still_one_notification() {
    let lucyd = Harness::hearing(PINNED);
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut client = lucyd.client();
    lucyd.mic.unplug(MIC);
    lucyd.mic.unplug(SPEAKER);

    client.start_session().unwrap();

    let shown = lucyd.notifier.next_notification();
    assert!(
        shown.body.contains(MIC) && shown.body.contains(SPEAKER),
        "{shown:?}"
    );
    lucyd.notifier.assert_no_notification();
}

#[test]
fn losing_the_mic_mid_session_ends_it_with_a_notification() {
    let lucyd = Harness::hearing("");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    lucyd.mic.lose();

    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Idle);
    let shown = lucyd.notifier.next_notification();
    assert!(shown.summary.contains("microphone"), "{shown:?}");

    // The next Session opens the microphone again.
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.stt.script("hello");
    lucyd
        .mic
        .feed(harness::SILENCE, std::time::Duration::from_millis(1024));
    lucyd
        .mic
        .feed(harness::SPEECH, std::time::Duration::from_millis(512));
    lucyd
        .mic
        .feed(harness::SILENCE, std::time::Duration::from_millis(1024));
    assert_eq!(lucyd.thinking.next_turn(), "hello");
}

#[test]
fn losing_the_speaker_mid_session_ends_it_with_a_notification() {
    let lucyd = Harness::start();
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("tell me a story");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("Once upon a time.");
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();

    lucyd.tts.lose_device();

    let states: Vec<State> = (0..4).map(|_| next_state(&mut events).state).collect();
    assert_eq!(
        states,
        [State::Thinking, State::Speaking, State::Error, State::Idle]
    );
    let shown = lucyd.notifier.next_notification();
    assert!(shown.summary.contains("speaker"), "{shown:?}");
}
