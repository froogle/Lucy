//! The Wake word opens a Session hands-free: a start sound, Listening, and
//! the speech straight after it kept. A false wake ends after 8 s.

mod harness;

use std::time::{Duration, Instant};

use harness::{next_state, Harness, SILENCE, SPEECH, WAKE};
use lucy_ipc::State;
use lucyd::adapters::Sound;

/// Config whose wake-word model listens for "hey lucy".
const HEY_LUCY: &str = "[wake_word]\nmodel = \"/models/hey_lucy.onnx\"\n";

#[test]
fn the_wake_word_opens_a_session_in_listening_with_a_start_sound() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();

    lucyd.wake();

    let opened = next_state(&mut events);
    assert_eq!(opened.state, State::Listening);
    assert!(opened.session_id.is_some());
    assert_eq!(lucyd.tts.sounds(), [Sound::Start]);
}

#[test]
fn the_wake_word_and_a_request_in_one_breath_send_only_the_request() {
    let lucyd = Harness::hearing(HEY_LUCY);
    let mut events = lucyd.state_client();
    lucyd.stt.script("Hey Lucy, what's the time?");

    // "Hey Lucy" and "what's the time" with no pause between them.
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(WAKE, ms(512));
    lucyd.mic.feed(SPEECH, ms(1024));
    lucyd.mic.feed(SILENCE, ms(1024));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.thinking.next_turn(), "what's the time?");
    // The whole utterance, Wake word and pre-roll included, reached STT.
    let segments = lucyd.stt.segments();
    assert_eq!(segments.len(), 1);
    let expected = ms(500 + 512 + 1024 + 700);
    assert!(segments[0].abs_diff(expected) <= ms(32), "{segments:?}");
}

#[test]
fn with_no_speech_after_the_wake_word_the_session_ends_after_8_seconds_with_a_closing_sound() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();

    lucyd.wake();
    assert_eq!(next_state(&mut events).state, State::Listening);
    let woken = Instant::now();

    assert_eq!(next_state_within(&mut events, 10).state, State::Idle);
    let open_for = woken.elapsed();
    assert!(
        open_for >= ms(7_900) && open_for < ms(9_000),
        "{open_for:?}"
    );
    assert_eq!(lucyd.tts.sounds(), [Sound::Start, Sound::Closing]);
    assert_eq!(lucyd.tts.sentences(), Vec::<String>::new());
}

#[test]
fn a_silence_timeout_shorter_than_8_seconds_ends_a_false_wake_sooner() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 0.3\n");
    let mut events = lucyd.state_client();

    lucyd.wake();
    assert_eq!(next_state(&mut events).state, State::Listening);

    assert_eq!(next_state_within(&mut events, 3).state, State::Idle);
}

#[test]
fn a_silence_timeout_ends_with_a_closing_sound() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 0.3\n");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    assert_eq!(next_state(&mut events).state, State::Idle);

    assert_eq!(lucyd.tts.sounds(), [Sound::Start, Sound::Closing]);
}

#[test]
fn ending_a_session_on_request_plays_no_closing_sound() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();

    client.end_session().unwrap();

    assert_eq!(lucyd.tts.sounds(), [Sound::Start]);
}

#[test]
fn the_wake_word_while_listening_is_ordinary_speech_with_the_wake_word_stripped() {
    let lucyd = Harness::with_config(HEY_LUCY);
    let mut events = lucyd.state_client();
    lucyd.wake();
    let opened = next_state(&mut events);

    lucyd.wake();
    lucyd.say("hey lucy and tomorrow");

    assert_eq!(lucyd.thinking.next_turn(), "and tomorrow");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    assert_eq!(
        lucyd.client().status().unwrap().session_id,
        opened.session_id
    );
    assert_eq!(lucyd.tts.sounds(), [Sound::Start]);
}

#[test]
fn only_a_leading_wake_word_is_stripped() {
    let lucyd = Harness::with_config(HEY_LUCY);
    let mut client = lucyd.client();
    client.start_session().unwrap();

    lucyd.say("Hey, Lucy's sister says hey Lucy");

    assert_eq!(
        lucyd.thinking.next_turn(),
        "Hey, Lucy's sister says hey Lucy"
    );
}

#[test]
fn the_wake_word_alone_is_no_speech() {
    // A Plugin's question shows what was heard: the words, or "no answer"
    // once the wait for one is over. That wait doesn't run while a reply
    // is being transcribed, so the reply is always heard first.
    let lucyd = Harness::with_config(&format!("{HEY_LUCY}[confirmation]\ntimeout_secs = 0.3\n"));
    lucyd.install_plugin(
        "asker",
        "name = \"asker\"\nversion = \"0.1.0\"\nauthor = \"human\"\nsummary = \"Asks\"\n\
         command = [\"./fixture-plugin\", \"ask\"]\n\n\
         [[tools]]\nname = \"ask\"\nhandles = \"ask something\"\n",
    );
    lucyd.approve("asker");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.say("find my album");
    lucyd.thinking.next_turn();
    let thinking = lucyd.thinking.clone();
    let call = std::thread::spawn(move || {
        thinking.call_tool_with(
            "asker__ask",
            serde_json::json!({"question": "Which album?"}),
        )
    });
    lucyd.tts.next_sentence();

    lucyd.say("Hey Lucy.");
    lucyd.tts.finish_playing();

    let result = call.join().unwrap().unwrap();
    let said: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect();
    assert_eq!(said, "no answer");
}

/// Waits up to `secs` for the next state event.
fn next_state_within(client: &mut lucy_ipc::Client, secs: u64) -> lucy_ipc::StateEvent {
    client
        .set_read_timeout(Some(Duration::from_secs(secs)))
        .unwrap();
    next_state(client)
}

// Tickets 07 and 11 together: a Session the Wake word opens re-resolves a
// pinned microphone too.
#[test]
fn a_wake_with_the_pinned_mic_missing_falls_back_with_one_notification() {
    let lucyd = Harness::hearing(&format!(
        "{HEY_LUCY}[audio]\nmic = \"alsa_input.usb-Blue_Yeti\"\n"
    ));
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = lucyd.state_client();
    lucyd.mic.unplug("alsa_input.usb-Blue_Yeti");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(WAKE, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.mic.opened().as_deref(), Some("default"));
    let shown = lucyd.notifier.next_notification();
    assert!(shown.body.contains("alsa_input.usb-Blue_Yeti"), "{shown:?}");
    lucyd.notifier.assert_no_notification();
    assert_eq!(lucyd.tts.sounds(), [Sound::Start]);
}

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}
