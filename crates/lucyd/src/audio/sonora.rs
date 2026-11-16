//! Lucy's own echo canceller: WebRTC's AEC3, through `sonora`, its pure-Rust
//! port. The render reference is the 16 kHz copy of what playback actually
//! played; AEC3 finds the delay between it and the microphone itself.
//! A quiet microphone is boosted ahead of AEC3, so a weak voice can still
//! Barge in: otherwise AEC3 takes it out with Lucy's echo.

use std::collections::VecDeque;

use sonora::config::{CaptureLevelAdjustment, EchoCanceller as Aec3};
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

/// The stretch the pre-gain follows: the last 30 s, in frames.
const LEVEL_WINDOW: usize = 3000;
/// The room's noise floor: this far up the window's frame levels.
const FLOOR_PERCENTILE: f64 = 0.1;
/// How far over the floor a frame has to be to count as sound, in dB. Only
/// sound enters the pre-gain's window, so a quiet room doesn't raise it.
const SOUND_OVER_FLOOR_DB: f64 = 20.0;
/// The loud end of the sound the pre-gain follows: this far up its frame
/// levels.
const LOUD_PERCENTILE: f64 = 0.99;
/// Where the pre-gain brings the loud end, in dBFS (a frame's RMS): far
/// enough under full scale that Lucy's echo doesn't clip.
const TARGET_DB: f64 = -22.0;
/// The most the pre-gain boosts, in dB.
const MAX_GAIN_DB: f64 = 24.0;
/// How far the wanted gain has to be from the one in use for it to change,
/// in dB. Each change leaks echo until AEC3 re-adapts, and resets its ERLE.
const STEP_DB: f64 = 6.0;
/// The sound the pre-gain needs before it changes, in frames.
const MIN_SOUND: usize = 300;
/// How often the floor and the pre-gain are worked out, in frames.
const GAIN_EVERY: usize = 100;

pub struct Sonora {
    apm: AudioProcessing,
    /// What Lucy played, at 16 kHz.
    reference: rtrb::Consumer<f32>,
    render: [f32; FRAME_LEN],
    scratch: [f32; FRAME_LEN],
    frames: usize,
    /// Reference frames that arrived since the stats were last read.
    rendered: usize,
    erle_db: Option<f32>,
    echo: EchoMeter,
    pre_gain: PreGain,
}

impl Sonora {
    /// Cancels whatever arrives on `reference` from the microphone.
    pub fn new(reference: rtrb::Consumer<f32>) -> Sonora {
        let stream = StreamConfig::new(SAMPLE_RATE, 1);
        let apm = AudioProcessing::builder()
            .config(Config {
                echo_canceller: Some(Aec3::default()),
                capture_level_adjustment: Some(CaptureLevelAdjustment::default()),
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
            rendered: 0,
            erle_db: None,
            echo: EchoMeter::default(),
            pre_gain: PreGain::default(),
        }
    }

    /// How much the microphone is boosted ahead of AEC3, in dB.
    pub fn gain_db(&self) -> f32 {
        self.pre_gain.gain_db as f32
    }
}

/// Whether a microphone whose loudest frame was `rms` stays under the
/// pre-gain's target even at its most, +24 dB.
pub fn too_quiet(rms: f32) -> bool {
    db(f64::from(rms).powi(2)) + MAX_GAIN_DB < TARGET_DB
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
            self.rendered += 1;
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
        let mic = power(&self.scratch);
        // The echo is measured as the microphone heard it, before the gain.
        let gain = 10f32.powf(self.gain_db() / 10.0);
        self.echo.push(mic, power(frame) / gain, played);
        if let Some(gain_db) = self.pre_gain.push(mic, !self.echo.lucy_speaking()) {
            log::debug!("canceller: pre-gain now {gain_db:.1} dB");
            self.apm
                .set_capture_pre_gain(10f32.powf(gain_db as f32 / 20.0));
        }
        self.frames += 1;
        if self.frames.is_multiple_of(STATS_EVERY) {
            let erle = self.apm.statistics().echo_return_loss_enhancement;
            self.erle_db = erle.map(|erle| erle as f32);
            if self.echo.lucy_speaking() {
                log::debug!(
                    "canceller: pre_gain_db={:.1} erle_db={:?} echo_heard={:?} \
                     reference_frames={}/{STATS_EVERY}",
                    self.gain_db(),
                    self.erle_db,
                    self.echo.heard,
                    self.rendered,
                );
            }
            self.rendered = 0;
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

    /// Lucy's reference arrived in the last [`HOLD`] frames.
    fn lucy_speaking(&self) -> bool {
        self.since_played <= HOLD
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

/// The boost ahead of AEC3: it brings the loud end of the sound the
/// microphone heard over the last [`LEVEL_WINDOW`] frames, the user's voice
/// or Lucy's echo, up towards [`TARGET_DB`]. Following the loudest sound
/// keeps her echo from clipping. It changes only while Lucy is silent, and
/// only by more than [`STEP_DB`].
struct PreGain {
    /// Every frame's level, in dB.
    levels: VecDeque<f64>,
    /// The levels of the frames that were sound.
    sound: VecDeque<f64>,
    floor_db: f64,
    frames: usize,
    gain_db: f64,
}

impl Default for PreGain {
    fn default() -> PreGain {
        PreGain {
            levels: VecDeque::with_capacity(LEVEL_WINDOW),
            sound: VecDeque::with_capacity(LEVEL_WINDOW),
            // No frame is sound until the floor is known.
            floor_db: f64::INFINITY,
            frames: 0,
            gain_db: 0.0,
        }
    }
}

impl PreGain {
    /// Takes a frame's power before the gain, and whether Lucy is silent.
    /// The new gain in dB, if it changes.
    fn push(&mut self, power: f32, lucy_silent: bool) -> Option<f64> {
        let level = db(f64::from(power));
        push_within(&mut self.levels, level);
        self.frames += 1;
        let due = self.frames.is_multiple_of(GAIN_EVERY);
        if due {
            self.floor_db = percentile(self.levels.make_contiguous(), FLOOR_PERCENTILE);
        }
        if level >= self.floor_db + SOUND_OVER_FLOOR_DB {
            push_within(&mut self.sound, level);
        }
        if !due || !lucy_silent || self.sound.len() < MIN_SOUND {
            return None;
        }
        let loud = percentile(self.sound.make_contiguous(), LOUD_PERCENTILE);
        let wanted = (TARGET_DB - loud).clamp(0.0, MAX_GAIN_DB);
        if (wanted - self.gain_db).abs() <= STEP_DB {
            return None;
        }
        self.gain_db = wanted;
        Some(wanted)
    }
}

/// Pushes `level`, dropping the oldest once there are [`LEVEL_WINDOW`].
fn push_within(levels: &mut VecDeque<f64>, level: f64) {
    if levels.len() == LEVEL_WINDOW {
        levels.pop_front();
    }
    levels.push_back(level);
}

/// `power` in dB.
fn db(power: f64) -> f64 {
    10.0 * power.max(1e-12).log10()
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

#[cfg(test)]
mod pre_gain {
    use std::f32::consts::TAU;

    use super::*;

    /// A microphone in a quiet room, through `Sonora`, with Lucy silent
    /// unless a test hands it her reference.
    struct Mic {
        sonora: Sonora,
        reference: rtrb::Producer<f32>,
        samples: usize,
        /// What came out of the last frame, and what went in.
        out_rms: f32,
        in_rms: f32,
    }

    /// `db` dBFS as an RMS level.
    fn rms_of(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    impl Mic {
        fn new() -> Mic {
            let (reference, consumer) = rtrb::RingBuffer::new(SAMPLE_RATE as usize);
            Mic {
                sonora: Sonora::new(consumer),
                reference,
                samples: 0,
                out_rms: 0.0,
                in_rms: 0.0,
            }
        }

        /// Runs `seconds` of the user talking at `voice_db` (RMS, in dBFS)
        /// half the time, in a room at about −90 dBFS, with Lucy speaking
        /// if `lucy` says so.
        fn run(&mut self, seconds: f32, voice_db: Option<f32>, lucy: bool) {
            for n in 0..(seconds * 100.0) as usize {
                if lucy {
                    for _ in 0..FRAME_LEN {
                        self.reference.push(0.01).unwrap();
                    }
                }
                // Half a second on, half off.
                let talking = voice_db.filter(|_| n % 100 < 50);
                let mut frame: Vec<f32> = (0..FRAME_LEN)
                    .map(|_| {
                        let t = self.samples as f32 / SAMPLE_RATE as f32;
                        self.samples += 1;
                        let noise = rms_of(-90.0) * (TAU * 3100.0 * t).sin();
                        let voice = talking.map_or(0.0, |db| {
                            rms_of(db) * std::f32::consts::SQRT_2 * (TAU * 300.0 * t).sin()
                        });
                        noise + voice
                    })
                    .collect();
                self.in_rms = power(&frame).sqrt();
                self.sonora.cancel(&mut frame);
                self.out_rms = power(&frame).sqrt();
            }
        }
    }

    #[test]
    fn a_quiet_voice_is_boosted_by_up_to_24_db() {
        let mut mic = Mic::new();
        mic.run(10.0, Some(-60.0), false);
        assert_eq!(mic.sonora.gain_db(), 24.0);

        // A frame of the voice comes out 24 dB louder.
        mic.run(0.2, Some(-60.0), false);
        let boost = 20.0 * (mic.out_rms / mic.in_rms).log10();
        assert!((boost - 24.0).abs() < 1.0, "{boost} dB");
    }

    #[test]
    fn a_voice_at_a_normal_level_is_not_boosted() {
        let mut mic = Mic::new();
        mic.run(10.0, Some(-15.0), false);
        assert_eq!(mic.sonora.gain_db(), 0.0);
    }

    #[test]
    fn a_voice_just_under_the_target_is_boosted_to_it() {
        let mut mic = Mic::new();
        // The loud end of a steady tone is its level.
        mic.run(10.0, Some(-35.0), false);
        assert!(
            (mic.sonora.gain_db() - 13.0).abs() < 0.5,
            "{}",
            mic.sonora.gain_db()
        );
    }

    #[test]
    fn nothing_changes_while_lucy_speaks() {
        let mut mic = Mic::new();
        mic.run(10.0, Some(-60.0), true);
        assert_eq!(mic.sonora.gain_db(), 0.0);

        // Once she stops it can.
        mic.run(1.5, Some(-60.0), false);
        assert_eq!(mic.sonora.gain_db(), 24.0);
    }

    #[test]
    fn a_gap_under_6_db_is_left_alone() {
        let mut mic = Mic::new();
        // 4 dB under the target wants 4 dB.
        mic.run(10.0, Some(-26.0), false);
        assert_eq!(mic.sonora.gain_db(), 0.0);
    }

    #[test]
    fn a_quiet_room_with_nobody_talking_raises_nothing() {
        let mut mic = Mic::new();
        mic.run(40.0, None, false);
        assert_eq!(mic.sonora.gain_db(), 0.0);
    }

    #[test]
    fn a_quiet_spell_keeps_the_gain_the_last_sound_set() {
        let mut mic = Mic::new();
        mic.run(10.0, Some(-15.0), false);
        mic.run(40.0, None, false);
        assert_eq!(mic.sonora.gain_db(), 0.0);
    }

    #[test]
    fn the_gain_follows_a_louder_voice_back_down() {
        let mut mic = Mic::new();
        mic.run(10.0, Some(-60.0), false);
        assert_eq!(mic.sonora.gain_db(), 24.0);
        // The window's loud end moves as soon as the louder voice is in it.
        mic.run(2.0, Some(-15.0), false);
        assert_eq!(mic.sonora.gain_db(), 0.0);
    }

    #[test]
    fn a_microphone_too_quiet_for_24_db_to_reach_the_target_is_too_quiet() {
        assert!(too_quiet(0.0));
        assert!(too_quiet(rms_of(-50.0)));
        assert!(!too_quiet(rms_of(-40.0)));
        assert!(!too_quiet(rms_of(-15.0)));
    }
}
