//! The tests setup runs inside lucyd: `test.mic_level` and `test.wake_word`.

mod harness;

use std::time::Duration;

use harness::{wait_until, Harness, SILENCE, WAKE};
use lucy_ipc::{error_code, State};

/// Quiet enough that the fake VAD doesn't hear speech.
const MURMUR: f32 = 0.3;

fn refusal<T: std::fmt::Debug>(outcome: Result<T, lucy_ipc::Error>) -> (i64, String) {
    match outcome {
        Err(lucy_ipc::Error::Rpc { code, message }) => (code, message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_mic_level_is_how_loud_the_microphone_has_been_since_last_asked() {
    let lucyd = Harness::hearing("");
    lucyd.mic.wait_until_open();
    let mut client = lucyd.client();

    lucyd.mic.feed(MURMUR, Duration::from_millis(100));
    let rms = wait_until("the murmur's level", || {
        let rms = client.mic_level().unwrap().rms;
        (rms > 0.0).then_some(rms)
    });

    assert!((rms - MURMUR).abs() < 0.01, "{rms}");
    assert_eq!(client.mic_level().unwrap().rms, 0.0);
}

#[test]
fn the_mic_level_follows_the_microphone_config_names_now() {
    let lucyd = Harness::hearing("");
    lucyd.mic.wait_until_open();
    std::fs::write(lucyd.config_path(), "[audio]\nmic = \"usb-mic\"\n").unwrap();

    lucyd.client().mic_level().unwrap();

    lucyd.mic.wait_until_open();
    assert_eq!(lucyd.mic.opened().as_deref(), Some("usb-mic"));
    assert_eq!(lucyd.client().status().unwrap().state, State::Idle);
}

#[test]
fn the_mic_level_is_refused_while_a_session_is_open() {
    let lucyd = Harness::hearing("");
    let mut client = lucyd.client();
    client.start_session().unwrap();

    let (code, _) = refusal(client.mic_level());

    assert_eq!(code, error_code::SESSION_OPEN);
    client.end_session().unwrap();
}

#[test]
fn the_setup_tests_say_when_lucyd_has_no_microphone() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    let (code, message) = refusal(client.mic_level());
    assert_eq!(code, error_code::NO_MICROPHONE);
    assert!(message.contains("microphone"), "{message}");

    let (code, _) = refusal(client.test_wake_word(1.0));
    assert_eq!(code, error_code::NO_MICROPHONE);
}

#[test]
fn hearing_the_wake_word_in_its_test_reports_the_score_and_opens_no_session() {
    let lucyd = Harness::hearing("");
    lucyd.mic.wait_until_open();
    let mut client = lucyd.client();
    let test = std::thread::spawn(move || client.test_wake_word(5.0));

    // The test is listening once a Session can't start alongside it.
    wait_until("the wake word test to start", || {
        lucyd.client().mic_level().is_err().then_some(())
    });
    lucyd.mic.feed(SILENCE, Duration::from_millis(100));
    lucyd.mic.feed(WAKE, Duration::from_millis(100));
    let result = test.join().unwrap().unwrap();

    assert!(result.heard);
    assert_eq!(result.score, WAKE);
    lucyd.mic.feed(SILENCE, Duration::from_millis(100));
    assert_eq!(lucyd.client().status().unwrap().state, State::Idle);
}

#[test]
fn a_wake_word_test_that_hears_nothing_reports_the_best_score_when_it_times_out() {
    let lucyd = Harness::hearing("");
    lucyd.mic.wait_until_open();
    let mut client = lucyd.client();
    let test = std::thread::spawn(move || client.test_wake_word(0.5));

    wait_until("the wake word test to start", || {
        lucyd.client().mic_level().is_err().then_some(())
    });
    lucyd.mic.feed(MURMUR, Duration::from_millis(100));
    let result = test.join().unwrap().unwrap();

    assert!(!result.heard);
    assert_eq!(result.score, MURMUR);
}

#[test]
fn the_wake_word_test_is_refused_while_a_session_is_open() {
    let lucyd = Harness::hearing("");
    let mut client = lucyd.client();
    client.start_session().unwrap();

    let (code, _) = refusal(client.test_wake_word(1.0));

    assert_eq!(code, error_code::SESSION_OPEN);
    client.end_session().unwrap();
}
