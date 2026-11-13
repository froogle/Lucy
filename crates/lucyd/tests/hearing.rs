//! Lucy hears the user: microphone samples go through the DSP thread and the
//! VAD's endpointing, and each utterance reaches STT and then Thinking.
//!
//! The fake microphone feeds samples whose value the fake VAD reads as the
//! speech probability, so a test scripts speech and silence as levels.

mod harness;

use std::time::Duration;

use harness::{Harness, SILENCE, SPEECH};

/// Between the default end and start thresholds.
const WAVERING: f32 = 0.4;
use lucy_config::Config;

#[test]
fn an_utterance_from_the_microphone_reaches_thinking() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("what time is it");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(1024));
    lucyd.mic.feed(SILENCE, ms(1024));

    assert_eq!(lucyd.thinking.next_turn(), "what time is it");
}

#[test]
fn an_utterance_keeps_half_a_second_before_the_speech_and_ends_after_700_ms_of_silence() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(2048));
    lucyd.mic.feed(SPEECH, ms(1024));
    lucyd.mic.feed(SILENCE, ms(2048));
    lucyd.thinking.next_turn();

    assert_segments(&lucyd, &[ms(500 + 1024 + 700)]);
}

#[test]
fn a_sound_shorter_than_100_ms_is_not_an_utterance() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(64));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.thinking.next_turn();

    // Only the second sound, with the first inside its pre-roll's reach
    // but not its own utterance.
    assert_segments(&lucyd, &[ms(500 + 512 + 700)]);
}

#[test]
fn speech_that_wavers_above_the_end_threshold_is_one_utterance() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(WAVERING, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.thinking.next_turn();

    assert_segments(&lucyd, &[ms(500 + 512 + 1024 + 512 + 700)]);
}

#[test]
fn pauses_shorter_than_700_ms_do_not_end_an_utterance() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(512));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(512));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.thinking.next_turn();

    assert_segments(&lucyd, &[ms(500 + 512 * 5 + 700)]);
}

#[test]
fn an_utterance_is_cut_at_30_seconds() {
    let lucyd = Harness::hearing(Config::default());
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("on and on");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(31_000));

    assert_eq!(lucyd.thinking.next_turn(), "on and on");
    assert_segments(&lucyd, &[ms(30_000)]);
}

#[test]
fn endpointing_comes_from_config() {
    let mut config = Config::default();
    config.endpointing.start_threshold = 0.8;
    config.endpointing.end_threshold = 0.6;
    config.endpointing.pre_roll_ms = 200;
    config.endpointing.end_silence_ms = 300;
    let lucyd = Harness::hearing(config);
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    // Below the configured end threshold, so silence.
    lucyd.mic.feed(WAVERING, ms(1024));
    lucyd.thinking.next_turn();

    assert_segments(&lucyd, &[ms(200 + 512 + 300)]);
}

#[test]
fn the_default_microphone_is_opened_unless_one_is_pinned() {
    let lucyd = Harness::hearing(Config::default());
    assert_eq!(lucyd.mic.opened().as_deref(), Some("default"));

    let mut config = Config::default();
    config.audio.mic = "alsa_input.usb-Blue_Yeti".to_owned();
    let lucyd = Harness::hearing(config);
    assert_eq!(
        lucyd.mic.opened().as_deref(),
        Some("alsa_input.usb-Blue_Yeti")
    );
}

/// Each utterance STT was given lasted as long as `expected`, give or take
/// one 32 ms VAD chunk.
fn assert_segments(lucyd: &Harness, expected: &[Duration]) {
    let segments = lucyd.stt.segments();
    assert_eq!(segments.len(), expected.len(), "{segments:?}");
    for (segment, expected) in segments.iter().zip(expected) {
        assert!(
            segment.abs_diff(*expected) <= ms(32),
            "{segment:?} is not {expected:?}"
        );
    }
}

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}
