//! A recording of speech for the live tests, resampled to 16 kHz mono.

#![allow(dead_code)]

use std::path::PathBuf;

/// The voice-loop prototype's recording, used unless `LUCY_SPEECH_WAV` names
/// another. It says "Hello! I'm Lucy. How can I help you today?"
const PROTOTYPE_WAV: &str = ".scratch/lucy-v0-1/prototypes/voice-loop/hello.wav";

pub fn speech() -> Vec<f32> {
    let path = std::env::var_os("LUCY_SPEECH_WAV")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(PROTOTYPE_WAV)
        });
    let mut reader = hound::WavReader::open(&path)
        .unwrap_or_else(|e| panic!("can't open {}: {e}", path.display()));
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|s| s.unwrap() as f32 / (1 << (spec.bits_per_sample - 1)) as f32)
            .collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    resample(&mono, spec.sample_rate, 16_000)
}

/// Linear resampling, good enough for a test recording.
fn resample(audio: &[f32], from: u32, to: u32) -> Vec<f32> {
    let len = audio.len() as u64 * u64::from(to) / u64::from(from);
    (0..len as usize)
        .map(|i| {
            let position = i as f64 * f64::from(from) / f64::from(to);
            let j = position as usize;
            let next = audio[(j + 1).min(audio.len() - 1)];
            let fraction = (position - j as f64) as f32;
            audio[j] + (next - audio[j]) * fraction
        })
        .collect()
}
