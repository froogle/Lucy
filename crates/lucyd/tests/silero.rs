//! The real Silero VAD, run on tract through `ort`.

mod recording;

use lucyd::audio::{Silero, Vad};

#[test]
fn silence_is_not_speech() {
    let mut vad = Silero::load().expect("the bundled model loads on tract");

    let silence = vec![0.0; 16_000];
    let probabilities = judge(&mut vad, &silence);

    assert!(probabilities.iter().all(|&p| p < 0.1), "{probabilities:?}");
}

/// Set `LUCY_SPEECH_WAV` to a recording of speech, or run from a checkout
/// with the voice-loop prototype's `hello.wav`.
#[test]
#[ignore = "needs a speech recording"]
fn a_recording_of_speech_is_speech_and_the_silence_after_it_is_not() {
    let mut vad = Silero::load().unwrap();
    let mut audio = recording::speech();
    audio.extend(vec![0.0; 16_000]);

    let probabilities = judge(&mut vad, &audio);

    let peak = probabilities.iter().cloned().fold(0.0, f32::max);
    assert!(peak > 0.9, "{probabilities:?}");
    let tail = &probabilities[probabilities.len() - 10..];
    assert!(tail.iter().all(|&p| p < 0.35), "{probabilities:?}");
}

fn judge(vad: &mut Silero, audio: &[f32]) -> Vec<f32> {
    audio
        .chunks_exact(vad.chunk_len())
        .map(|chunk| vad.speech_probability(chunk))
        .collect()
}
