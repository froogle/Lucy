//! The Voice preview: `test.voice_preview` speaks a sentence outside any
//! Session.

mod harness;

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, Harness, Queued, FAKE_TTS_FAILURE};
use lucy_ipc::{error_code, Event, State, Topic};
use lucyd::adapters::Sound;

/// Starts a Voice preview of `text` on a fresh client, without waiting for it.
fn preview(lucyd: &Harness, text: &str) -> JoinHandle<Result<(), lucy_ipc::Error>> {
    let mut client = lucyd.client();
    let text = text.to_owned();
    std::thread::spawn(move || client.voice_preview(&text))
}

#[test]
fn a_voice_preview_plays_start_the_text_and_closing_and_replies_once_played() {
    let lucyd = Harness::start();
    let mut observer = lucyd.client();
    observer.subscribe(&[Topic::State, Topic::Level]).unwrap();
    next_state(&mut observer);
    let (replied_tx, replied) = mpsc::channel();
    let mut client = lucyd.client();
    std::thread::spawn(move || replied_tx.send(client.voice_preview("hello")));

    assert_eq!(lucyd.tts.next_sentence(), "hello");
    lucyd.tts.level(0.5);
    // A request the runner handles after the preview's: once it returns,
    // a preview that replied without waiting for playback has replied.
    assert!(lucyd.client().end_session().is_err());
    assert!(
        replied.recv_timeout(Duration::from_millis(100)).is_err(),
        "replied before playback finished"
    );
    lucyd.tts.finish_playing();
    replied.recv().unwrap().unwrap();

    assert_eq!(
        lucyd.tts.queued(),
        [
            Queued::Sound(Sound::Start),
            Queued::Sentence("hello".into()),
            Queued::Sound(Sound::Closing),
        ]
    );
    assert_eq!(lucyd.client().status().unwrap().state, State::Idle);
    // Nothing was sent during the preview: the first event is the Session's.
    lucyd.client().start_session().unwrap();
    match observer.next_event().unwrap() {
        Event::State(status) => assert_eq!(status.state, State::Listening),
        other => panic!("expected the Session's state, got {other:?}"),
    }
    lucyd.client().end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

/// The JSON-RPC error a refused request got.
fn refusal(outcome: Result<(), lucy_ipc::Error>) -> (i64, String) {
    match outcome {
        Err(lucy_ipc::Error::Rpc { code, message }) => (code, message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_voice_preview_is_refused_while_a_session_is_open() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();

    let (code, _) = refusal(client.voice_preview("hello"));

    assert_eq!(code, error_code::SESSION_OPEN);
    assert!(lucyd.tts.sentences().is_empty());
    client.end_session().unwrap();
}

#[test]
fn a_second_voice_preview_is_refused_as_busy_while_one_plays() {
    let lucyd = Harness::start();
    let first = preview(&lucyd, "hello");
    assert_eq!(lucyd.tts.next_sentence(), "hello");

    let (code, message) = refusal(lucyd.client().voice_preview("again"));

    assert_eq!(code, error_code::BUSY);
    assert!(message.contains("another Voice preview"), "{message}");
    lucyd.tts.finish_playing();
    first.join().unwrap().unwrap();
    assert_eq!(lucyd.tts.sentences(), ["hello"]);
}

#[test]
fn a_voice_preview_can_speak_in_a_voice_that_is_not_running() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    let previewed =
        std::thread::spawn(move || client.voice_preview_in("hello", Some("en_GB-alba-medium")));

    assert_eq!(lucyd.tts.next_sentence(), "hello");
    assert_eq!(lucyd.tts.voice().as_deref(), Some("en_GB-alba-medium"));
    lucyd.tts.finish_playing();
    previewed.join().unwrap().unwrap();

    lucyd.client().start_session().unwrap();
    assert_eq!(
        lucyd.tts.voice(),
        None,
        "a Session speaks in the running voice"
    );
    lucyd.client().end_session().unwrap();
}

#[test]
fn a_voice_preview_of_empty_text_is_refused() {
    let lucyd = Harness::start();

    for text in ["", "  "] {
        let (code, _) = refusal(lucyd.client().voice_preview(text));
        assert_eq!(code, error_code::INVALID_PARAMS);
    }
    assert!(lucyd.tts.sentences().is_empty());
}

#[test]
fn a_voice_preview_tts_cannot_say_returns_the_reason_and_shows_no_notification() {
    let lucyd = Harness::start();
    lucyd.tts.fail();

    let (code, message) = refusal(lucyd.client().voice_preview("hello"));

    assert_eq!(code, error_code::TTS_FAILED);
    assert!(message.contains(FAKE_TTS_FAILURE), "{message}");
    lucyd.notifier.assert_no_notification();
}

#[test]
fn a_session_started_mid_preview_cuts_it_short_and_runs_normally() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    let preview = preview(&lucyd, "hello");
    assert_eq!(lucyd.tts.next_sentence(), "hello");

    lucyd.client().start_session().unwrap();

    let (code, message) = refusal(preview.join().unwrap());
    assert_eq!(code, error_code::INTERRUPTED);
    assert!(message.contains("interrupted"), "{message}");
    assert_eq!(next_state(&mut events).state, State::Listening);
    // The preview's Closing sound never plays; the Session's Start does.
    assert_eq!(lucyd.tts.sounds(), [Sound::Start, Sound::Start]);
    lucyd.say("what time is it");
    assert_eq!(lucyd.thinking.next_turn(), "what time is it");
    lucyd.thinking.delta("Three o'clock.");
    lucyd.thinking.turn_end();
    assert_eq!(lucyd.tts.next_sentence(), "Three o'clock.");
    lucyd.tts.finish_playing();
    while next_state(&mut events).state != State::Listening {}
    lucyd.client().end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn the_wake_word_mid_preview_cuts_it_short_and_opens_a_session() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    let preview = preview(&lucyd, "hello");
    assert_eq!(lucyd.tts.next_sentence(), "hello");

    lucyd.wake();

    let (code, _) = refusal(preview.join().unwrap());
    assert_eq!(code, error_code::INTERRUPTED);
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.client().end_session().unwrap();
}
