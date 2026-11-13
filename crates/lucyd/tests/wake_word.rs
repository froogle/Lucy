//! The real wake-word engine, LiveKit WakeWord v2 on tract, with the model
//! bundled until the "Hey Lucy" models land. The recordings come from
//! livekit-wakeword's own fixtures (Apache-2.0).

use std::path::{Path, PathBuf};

use lucyd::audio::{LiveKit, WakeWord};

#[test]
fn the_bundled_model_hears_its_phrase() {
    let dir = tempfile::tempdir().unwrap();
    let model = lucyd::audio::install_bundled_wake_model(dir.path()).unwrap();
    let mut wake_word = LiveKit::load(&model, 0.5).expect("the model loads on tract");

    assert_eq!(heard(&mut wake_word, &recording("positive.wav")), 1);
}

#[test]
fn the_bundled_model_ignores_other_speech() {
    let dir = tempfile::tempdir().unwrap();
    let model = lucyd::audio::install_bundled_wake_model(dir.path()).unwrap();
    let mut wake_word = LiveKit::load(&model, 0.5).unwrap();

    assert_eq!(heard(&mut wake_word, &recording("negative.wav")), 0);
}

#[test]
fn a_file_that_is_not_a_wake_word_model_is_refused_with_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("hey_lucy.onnx");
    std::fs::write(&model, b"not onnx").unwrap();

    let err = LiveKit::load(&model, 0.5).err().expect("loading fails");

    assert!(err.to_string().contains("hey_lucy.onnx"), "{err}");
}

/// How many times `wake_word` heard its phrase in `audio`, fed in 10 ms
/// frames as the DSP thread does, with a second of silence after.
fn heard(wake_word: &mut LiveKit, audio: &[f32]) -> usize {
    let mut audio = audio.to_vec();
    audio.extend(vec![0.0; 16_000]);
    let (frames, _) = audio.as_chunks::<160>();
    frames
        .iter()
        .filter(|frame| wake_word.push(&frame[..]))
        .count()
}

fn recording(name: &str) -> Vec<f32> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wake")
        .join(name);
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().sample_rate, 16_000);
    reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32_768.0)
        .collect()
}
