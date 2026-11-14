//! Lucy's own echo canceller: WebRTC's AEC3, through `sonora`, its pure-Rust
//! port. The render reference is the 16 kHz copy of what playback actually
//! played; AEC3 finds the delay between it and the microphone itself.

use std::collections::VecDeque;

use sonora::config::EchoCanceller as Aec3;
use sonora::{AudioProcessing, Config, StreamConfig};

use super::{EchoCanceller, FRAME_LEN, SAMPLE_RATE};

/// How often the ERLE is read, in frames.
const STATS_EVERY: usize = 50;

/// The stretch of Lucy's speech the echo is measured over: the last 10 s, in
/// frames.
const WINDOW: usize = 1000;
/// The longest delay between what Lucy played and its echo at the
/// microphone, in frames.
const MAX_LAG: usize = 50;
/// Frames compared at a time: longer than playback's ~33 ms hand-over of the
/// reference, so its bursts even out.
const BLOCK: usize = 5;
/// Frames after the reference last arrived that still count as Lucy
/// speaking.
const HOLD: usize = 10;
/// How often the echo is measured, in frames of Lucy's speech.
const MEASURE_EVERY: usize = 100;
/// How far over its quiet level the cancelled microphone has to be for a
/// block to count as the user talking: 10 dB, as a power ratio.
const USER_OVER_QUIET: f64 = 10.0;
/// How far over the microphone's typical level while Lucy is loud a block
/// can count, as a power ratio. AEC3's suppressor takes the user's voice out
/// where Lucy is loudest, so their blocks it hides from [`USER_OVER_QUIET`]
/// follow her voice; this keeps a few of them from passing for echo.
const CAP_OVER_LOUD: f64 = 4.0;

pub struct Sonora {
    apm: AudioProcessing,
    /// What Lucy played, at 16 kHz.
    reference: rtrb::Consumer<f32>,
    render: [f32; FRAME_LEN],
    scratch: [f32; FRAME_LEN],
    frames: usize,
    erle_db: Option<f32>,
    echo: EchoMeter,
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
            echo: EchoMeter::default(),
        }
    }
}

impl EchoCanceller for Sonora {
    fn cancel(&mut self, frame: &mut [f32]) {
        let mut played = None;
        while let Ok(chunk) = self.reference.read_chunk(FRAME_LEN) {
            let (first, second) = chunk.as_slices();
            self.render[..first.len()].copy_from_slice(first);
            self.render[first.len()..].copy_from_slice(second);
            chunk.commit_all();
            *played.get_or_insert(0.0) += power(&self.render);
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
        self.echo.push(power(&self.scratch), power(frame), played);
        self.frames += 1;
        if self.frames.is_multiple_of(STATS_EVERY) {
            let erle = self.apm.statistics().echo_return_loss_enhancement;
            self.erle_db = erle.map(|erle| erle as f32);
        }
    }

    fn erle_db(&self) -> Option<f32> {
        self.erle_db
    }

    fn echo_heard(&self) -> Option<bool> {
        self.echo.heard
    }
}

/// The mean square of `samples`.
fn power(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32
}

/// One frame of Lucy's speech: its power at the microphone, after the
/// canceller, and of what she played.
#[derive(Clone, Copy)]
struct Heard {
    mic: f32,
    cancelled: f32,
    played: f32,
}

/// Whether Lucy's voice reaches the microphone, over the last [`WINDOW`]
/// frames of her speaking: whether the part of the microphone's power that
/// follows what she played, at the delay where most does, is at least as loud
/// as the noise floor. Blocks where the cancelled microphone is loud are the
/// user, or echo the canceller hasn't learned yet, and are left out.
struct EchoMeter {
    frames: VecDeque<Heard>,
    /// Frames since the reference last arrived.
    since_played: usize,
    since_measured: usize,
    /// `None` until the window has filled, and while the user talks over
    /// most of it.
    heard: Option<bool>,
}

impl Default for EchoMeter {
    fn default() -> EchoMeter {
        EchoMeter {
            frames: VecDeque::with_capacity(WINDOW + MAX_LAG),
            since_played: HOLD + 1,
            since_measured: 0,
            heard: None,
        }
    }
}

impl EchoMeter {
    /// Takes a frame's power at the microphone and after the canceller, and
    /// the power of the reference that arrived with it, if any did.
    fn push(&mut self, mic: f32, cancelled: f32, played: Option<f32>) {
        self.since_played = match played {
            Some(_) => 0,
            None => self.since_played.saturating_add(1),
        };
        if self.since_played > HOLD {
            return;
        }
        if self.frames.len() == WINDOW + MAX_LAG {
            self.frames.pop_front();
        }
        self.frames.push_back(Heard {
            mic,
            cancelled,
            played: played.unwrap_or(0.0),
        });
        self.since_measured += 1;
        if self.frames.len() == WINDOW + MAX_LAG && self.since_measured >= MEASURE_EVERY {
            self.since_measured = 0;
            self.heard = self.measure();
        }
    }

    fn measure(&mut self) -> Option<bool> {
        let frames = self.frames.make_contiguous();
        // The mean of `what` over block `k`'s frames, or those `lag` before.
        let block = |k: usize, lag: usize, what: fn(&Heard) -> f32| {
            let start = MAX_LAG + k * BLOCK - lag;
            frames[start..start + BLOCK]
                .iter()
                .map(|frame| f64::from(what(frame)))
                .sum::<f64>()
                / BLOCK as f64
        };
        let blocks = WINDOW / BLOCK;
        let cancelled: Vec<f64> = (0..blocks).map(|k| block(k, 0, |f| f.cancelled)).collect();
        let quiet_level = percentile(&cancelled, 0.1);
        let kept: Vec<usize> = (0..blocks)
            .filter(|&k| cancelled[k] <= quiet_level * USER_OVER_QUIET)
            .collect();
        if kept.len() < blocks / 2 {
            return None;
        }
        let mic: Vec<f64> = kept.iter().map(|&k| block(k, 0, |f| f.mic)).collect();
        let heard = (0..=MAX_LAG).any(|lag| {
            let played: Vec<f64> = kept.iter().map(|&k| block(k, lag, |f| f.played)).collect();
            echo_reaches_floor(&played, &mic)
        });
        Some(heard)
    }
}

/// Whether the part of `mic` that follows `played` (a least-squares line
/// through them, `mic` capped at [`CAP_OVER_LOUD`] times its median while
/// `played` is in its loudest quarter) is at least as loud as the noise
/// floor: the mean of `mic` while `played` is in its quietest quarter.
fn echo_reaches_floor(played: &[f64], mic: &[f64]) -> bool {
    let loudest = percentile(played, 0.75);
    let cap = CAP_OVER_LOUD * percentile(&mic_while(played, mic, |p| p >= loudest), 0.5);
    let mic: Vec<f64> = mic.iter().map(|m| m.min(cap)).collect();
    let n = played.len() as f64;
    let played_mean = played.iter().sum::<f64>() / n;
    let mic_mean = mic.iter().sum::<f64>() / n;
    let (mut covariance, mut variance) = (0.0, 0.0);
    for (p, m) in played.iter().zip(&mic) {
        covariance += (p - played_mean) * (m - mic_mean);
        variance += (p - played_mean).powi(2);
    }
    if variance <= 0.0 {
        return false;
    }
    let echo = covariance / variance * played_mean;
    let quietest = percentile(played, 0.25);
    let quiet = mic_while(played, &mic, |p| p <= quietest);
    let floor = quiet.iter().sum::<f64>() / quiet.len() as f64;
    echo > 0.0 && echo >= floor
}

/// `mic` where `played` is as `when` says.
fn mic_while(played: &[f64], mic: &[f64], when: impl Fn(f64) -> bool) -> Vec<f64> {
    (played.iter().zip(mic))
        .filter(|(p, _)| when(**p))
        .map(|(_, m)| *m)
        .collect()
}

/// The value `p` of the way up `values` sorted.
fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() - 1) as f64 * p) as usize]
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

#[cfg(test)]
mod echo_heard {
    use std::f32::consts::TAU;

    use super::*;

    /// Lucy's speaker and the microphone in one room, through `Sonora`.
    struct Room {
        sonora: Sonora,
        reference: rtrb::Producer<f32>,
        /// What the speaker played at each sample of the microphone's
        /// timeline.
        played: Vec<f32>,
        /// Samples Lucy has spoken.
        spoken: usize,
        noise: u32,
    }

    /// When the microphone hears what the speaker plays.
    const DELAY: usize = 70 * SAMPLE_RATE as usize / 1000;

    /// Frames of reference playback hands over at a time: its ~33 ms tick,
    /// and the resampler's chunks.
    const BURST: usize = 5;

    /// Lucy's voice: tones, in syllables of changing loudness.
    fn lucy(i: usize) -> f32 {
        let t = i as f32 / SAMPLE_RATE as f32;
        let loudness = [1.0, 0.3, 0.7, 0.0, 0.5, 1.0, 0.2, 0.8][(t * 4.0) as usize % 8];
        loudness
            * (0.2 * (TAU * 220.0 * t).sin()
                + 0.1 * (TAU * 570.0 * t).sin()
                + 0.05 * (TAU * 1330.0 * t).sin())
    }

    /// The user's voice: other tones, as loud as Lucy's.
    fn user(i: usize) -> f32 {
        let t = i as f32 / SAMPLE_RATE as f32;
        0.2 * (TAU * 310.0 * t).sin() + 0.1 * (TAU * 890.0 * t).sin()
    }

    impl Room {
        fn new() -> Room {
            let (reference, consumer) = rtrb::RingBuffer::new(SAMPLE_RATE as usize);
            Room {
                sonora: Sonora::new(consumer),
                reference,
                played: vec![],
                spoken: 0,
                noise: 1,
            }
        }

        /// The room's noise, about −55 dBFS.
        fn noise(&mut self) -> f32 {
            // xorshift32
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            (self.noise as f32 / u32::MAX as f32 - 0.5) * 0.006
        }

        /// Runs `seconds` with Lucy speaking or not, her voice reaching the
        /// microphone `echo` times as loud, and the user talking where
        /// `talking` says, from the seconds since this started.
        fn run(&mut self, seconds: f32, speaking: bool, echo: f32, talking: impl Fn(f32) -> bool) {
            let frames = (seconds * 100.0) as usize;
            for n in 0..frames {
                let start = self.played.len();
                if speaking && n.is_multiple_of(BURST) {
                    // Handed over a little ahead of the speaker.
                    for i in 0..BURST * FRAME_LEN {
                        self.reference.push(lucy(self.spoken + i)).unwrap();
                    }
                }
                for _ in 0..FRAME_LEN {
                    let voice = if speaking { lucy(self.spoken) } else { 0.0 };
                    self.spoken += usize::from(speaking);
                    self.played.push(voice);
                }
                let talks = talking(n as f32 / 100.0);
                let mut frame: Vec<f32> = (start..start + FRAME_LEN)
                    .map(|i| {
                        let heard = i.checked_sub(DELAY).map_or(0.0, |i| self.played[i]);
                        echo * heard + if talks { user(i) } else { 0.0 } + self.noise()
                    })
                    .collect();
                self.sonora.cancel(&mut frame);
            }
        }

        fn speaks(&mut self, seconds: f32, echo: f32) {
            self.run(seconds, true, echo, |_| false);
        }
    }

    /// Echo from desk speakers: about 20 dB over the noise.
    const SPEAKERS: f32 = 0.3;

    #[test]
    fn lucys_voice_reaching_the_microphone_is_heard() {
        let mut room = Room::new();
        room.speaks(12.0, SPEAKERS);
        assert_eq!(room.sonora.echo_heard(), Some(true));
    }

    #[test]
    fn with_headphones_lucys_voice_is_not_heard() {
        let mut room = Room::new();
        room.speaks(12.0, 0.0);
        assert_eq!(room.sonora.echo_heard(), Some(false));
    }

    #[test]
    fn until_lucy_has_spoken_for_10_s_it_cannot_tell() {
        let mut room = Room::new();
        // Only her speech counts.
        room.run(20.0, false, SPEAKERS, |_| false);
        room.speaks(9.0, SPEAKERS);
        assert_eq!(room.sonora.echo_heard(), None);
    }

    /// Echo about 4 dB over the noise.
    const FAINT: f32 = 0.025;

    #[test]
    fn faint_echo_is_heard() {
        let mut room = Room::new();
        room.speaks(12.0, FAINT);
        assert_eq!(room.sonora.echo_heard(), Some(true));
    }

    #[test]
    fn echo_below_the_noise_floor_is_not_heard() {
        let mut room = Room::new();
        // About 6 dB under the noise.
        room.speaks(12.0, 0.008);
        assert_eq!(room.sonora.echo_heard(), Some(false));
    }

    #[test]
    fn while_the_user_talks_over_most_of_it_it_cannot_tell() {
        let mut room = Room::new();
        room.run(12.0, true, 0.0, |t| t % 1.0 < 0.7);
        assert_eq!(room.sonora.echo_heard(), None);
    }

    #[test]
    fn the_user_talking_over_some_of_it_still_lets_it_tell() {
        let mut room = Room::new();
        room.run(12.0, true, 0.0, |t| t % 10.0 < 2.0);
        assert_eq!(room.sonora.echo_heard(), Some(false));
    }

    #[test]
    fn a_switch_from_headphones_to_speakers_shows_within_10_s_of_lucys_speech() {
        let mut room = Room::new();
        room.speaks(12.0, 0.0);
        assert_eq!(room.sonora.echo_heard(), Some(false));
        room.speaks(10.0, SPEAKERS);
        assert_eq!(room.sonora.echo_heard(), Some(true));
    }

    #[test]
    fn a_switch_from_speakers_to_headphones_shows_within_10_s_of_lucys_speech() {
        let mut room = Room::new();
        room.speaks(12.0, SPEAKERS);
        assert_eq!(room.sonora.echo_heard(), Some(true));
        room.speaks(10.0, 0.0);
        assert_eq!(room.sonora.echo_heard(), Some(false));
    }
}
