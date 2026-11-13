//! Lucy's own echo canceller: WebRTC's AEC3, through `sonora`, its pure-Rust
//! port. The render reference is the 16 kHz copy of what playback actually
//! played; AEC3 finds the delay between it and the microphone itself.

use sonora::config::EchoCanceller as Aec3;
use sonora::{AudioProcessing, Config, StreamConfig};

use super::{EchoCanceller, FRAME_LEN, SAMPLE_RATE};

/// How often the ERLE is read, in frames.
const STATS_EVERY: usize = 50;

pub struct Sonora {
    apm: AudioProcessing,
    /// What Lucy played, at 16 kHz.
    reference: rtrb::Consumer<f32>,
    render: [f32; FRAME_LEN],
    scratch: [f32; FRAME_LEN],
    frames: usize,
    erle_db: Option<f32>,
}

impl Sonora {
    /// Cancels whatever arrives on `reference` from the microphone.
    pub fn new(reference: rtrb::Consumer<f32>) -> Sonora {
        let stream = StreamConfig::new(SAMPLE_RATE, 1);
        let apm = AudioProcessing::builder()
            .config(Config {
                echo_canceller: Some(Aec3::default()),
                ..Config::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Sonora {
            apm,
            reference,
            render: [0.0; FRAME_LEN],
            scratch: [0.0; FRAME_LEN],
            frames: 0,
            erle_db: None,
        }
    }
}

impl EchoCanceller for Sonora {
    fn cancel(&mut self, frame: &mut [f32]) {
        while let Ok(chunk) = self.reference.read_chunk(FRAME_LEN) {
            let (first, second) = chunk.as_slices();
            self.render[..first.len()].copy_from_slice(first);
            self.render[first.len()..].copy_from_slice(second);
            chunk.commit_all();
            let _ = self
                .apm
                .process_render_f32(&[&self.render], &mut [&mut self.scratch]);
        }
        // AEC3 estimates the real delay itself.
        let _ = self.apm.set_stream_delay_ms(0);
        self.scratch.copy_from_slice(frame);
        if self
            .apm
            .process_capture_f32(&[&self.scratch], &mut [frame])
            .is_err()
        {
            frame.copy_from_slice(&self.scratch);
        }
        self.frames += 1;
        if self.frames.is_multiple_of(STATS_EVERY) {
            let erle = self.apm.statistics().echo_return_loss_enhancement;
            self.erle_db = erle.map(|erle| erle as f32);
        }
    }

    fn erle_db(&self) -> Option<f32> {
        self.erle_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A noisy "voice": a few tones, so AEC3 has something broadband-ish.
    fn voice(i: usize) -> f32 {
        let t = i as f32 / SAMPLE_RATE as f32;
        let tau = 2.0 * std::f32::consts::PI;
        0.2 * (tau * 220.0 * t).sin()
            + 0.1 * (tau * 570.0 * t).sin()
            + 0.05 * (tau * 1330.0 * t).sin()
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s * s).sum()
    }

    #[test]
    fn lucys_voice_played_back_into_the_microphone_is_cancelled() {
        let (mut reference, consumer) = rtrb::RingBuffer::new(SAMPLE_RATE as usize);
        let mut sonora = Sonora::new(consumer);
        // The microphone hears what was played 30 ms ago, quieter.
        let delay = 480;
        let mut last_second_in = 0.0;
        let mut last_second_out = 0.0;
        let seconds = 6;
        for n in 0..seconds * 100 {
            let start = n * FRAME_LEN;
            for i in start..start + FRAME_LEN {
                reference.push(voice(i)).unwrap();
            }
            let mut frame: Vec<f32> = (start..start + FRAME_LEN)
                .map(|i| {
                    if i >= delay {
                        0.5 * voice(i - delay)
                    } else {
                        0.0
                    }
                })
                .collect();
            let heard = energy(&frame);
            sonora.cancel(&mut frame);
            if n >= (seconds - 1) * 100 {
                last_second_in += heard;
                last_second_out += energy(&frame);
            }
        }

        assert!(
            last_second_out < last_second_in / 10.0,
            "{last_second_out} left of {last_second_in}"
        );
        assert!(sonora.erle_db().is_some());
    }
}
