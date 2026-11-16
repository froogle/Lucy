//! Lucy hears the user: each utterance reaches STT and then Thinking.
//! Microphone samples go through the DSP thread and the VAD's endpointing.
//!
//! The fake microphone feeds samples whose value the fake VAD reads as the
//! speech probability, so a test scripts speech and silence as levels.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::time::Duration;

use harness::{Harness, SILENCE, SPEECH};

const WAVERING: f32 = 0.4;

const SHORT_ENDPOINTING: &str = "
[endpointing]
start_threshold = 0.8
end_threshold = 0.6
pre_roll_ms = 200
end_silence_ms = 300
";

const PINNED_MIC: &str = "[audio]\nmic = \"alsa_input.usb-Blue_Yeti\"\n";

#[test]
fn an_utterance_from_the_microphone_reaches_thinking() {
    let lucyd = Harness::hearing("");
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
    let lucyd = Harness::hearing("");
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
    let lucyd = Harness::hearing("");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(64));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.thinking.next_turn();

    let only_the_second_sound = [ms(500 + 512 + 700)];
    assert_segments(&lucyd, &only_the_second_sound);
}

#[test]
fn speech_that_wavers_above_the_end_threshold_is_one_utterance() {
    let lucyd = Harness::hearing("");
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
    let lucyd = Harness::hearing("");
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
    let lucyd = Harness::hearing("");
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
    let lucyd = Harness::hearing(SHORT_ENDPOINTING);
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.stt.script("hello");

    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    let below_the_configured_end = WAVERING;
    lucyd.mic.feed(below_the_configured_end, ms(1024));
    lucyd.thinking.next_turn();

    assert_segments(&lucyd, &[ms(200 + 512 + 300)]);
}

#[test]
fn the_default_microphone_is_opened_unless_one_is_pinned() {
    let lucyd = Harness::hearing("");
    assert_eq!(lucyd.mic.opened().as_deref(), Some("default"));

    let lucyd = Harness::hearing(PINNED_MIC);
    assert_eq!(
        lucyd.mic.opened().as_deref(),
        Some("alsa_input.usb-Blue_Yeti")
    );
}

#[test]
fn the_microphone_and_endpointing_are_reread_at_each_session_start() {
    let lucyd = Harness::hearing("");
    std::fs::write(
        lucyd.config_path(),
        format!("{PINNED_MIC}{SHORT_ENDPOINTING}"),
    )
    .unwrap();
    let mut client = lucyd.client();

    client.start_session().unwrap();

    assert_eq!(
        lucyd.mic.opened().as_deref(),
        Some("alsa_input.usb-Blue_Yeti")
    );
    lucyd.stt.script("hello");
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(WAVERING, ms(1024));
    lucyd.thinking.next_turn();
    assert_segments(&lucyd, &[ms(200 + 512 + 300)]);
}

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
