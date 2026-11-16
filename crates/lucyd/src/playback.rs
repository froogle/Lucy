use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use tokio::sync::mpsc;

use crate::adapters::{Sound, Speaker, SpeakerEvent};
use crate::audio;
use crate::sync::Locked;

pub const REFERENCE_RATE: u32 = 16_000;

const LEVEL_PERIOD: Duration = Duration::from_millis(33);

const RING_SECS: usize = 10;

const DEFAULT_VOICE_RATE: u32 = 22_050;

const CUE_PLAY_OUT: Duration = Duration::from_secs(1);

const DUCKED_GAIN_MINUS_12_DB: f32 = 0.25;

const FULL_VOLUME_BITS: u32 = 0;

pub struct Pcm {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub type Synthesise = Box<dyn FnMut(&str) -> Result<Pcm, String> + Send>;

pub type Reference = Arc<Mutex<rtrb::Producer<f32>>>;

pub fn speaker(
    mut synthesise: Synthesise,
    device: &str,
    events: mpsc::UnboundedSender<SpeakerEvent>,
    reference: Reference,
) -> Box<dyn Speaker> {
    let (sentences, sentences_rx) = std_mpsc::channel::<(u64, Request)>();
    let (items, items_rx) = std_mpsc::channel();
    let control = Arc::new(Control::default());
    let synth_control = control.clone();
    std::thread::spawn(move || {
        for (epoch, request) in sentences_rx {
            let stopped_before_synthesis = epoch != synth_control.epoch.load(Ordering::Acquire);
            if stopped_before_synthesis {
                continue;
            }
            let item = match request {
                Request::Sound(sound) => Item::Sound(sound),
                Request::Sentence(sentence) => match synthesise(&sentence) {
                    Ok(pcm) => Item::Audio(pcm, sentence),
                    Err(reason) => {
                        eprintln!("lucyd: TTS failed: {reason}");
                        Item::Failed(Failure { sentence, reason })
                    }
                },
            };
            if items.send((epoch, item)).is_err() {
                return;
            }
        }
    });
    let device = device.to_owned();
    let player_control = control.clone();
    std::thread::spawn(move || {
        Player::new(device, events, reference, player_control).run(items_rx);
    });
    Box::new(PlaybackSpeaker { sentences, control })
}

struct PlaybackSpeaker {
    sentences: std_mpsc::Sender<(u64, Request)>,
    control: Arc<Control>,
}

#[derive(Default)]
struct Control {
    epoch: AtomicU64,
    flushed: AtomicU64,
    gain: AtomicU32,
}

impl Control {
    fn gain(&self) -> f32 {
        match self.gain.load(Ordering::Relaxed) {
            FULL_VOLUME_BITS => 1.0,
            bits => f32::from_bits(bits),
        }
    }
}

enum Request {
    Sentence(String),
    Sound(Sound),
}

impl PlaybackSpeaker {
    fn send(&self, request: Request) {
        let epoch = self.control.epoch.load(Ordering::Acquire);
        let _ = self.sentences.send((epoch, request));
    }
}

impl Speaker for PlaybackSpeaker {
    fn speak(&mut self, sentence: String) {
        self.send(Request::Sentence(sentence));
    }

    fn sound(&mut self, sound: Sound) {
        self.send(Request::Sound(sound));
    }

    fn duck(&mut self, ducked: bool) {
        let gain = if ducked {
            DUCKED_GAIN_MINUS_12_DB.to_bits()
        } else {
            FULL_VOLUME_BITS
        };
        self.control.gain.store(gain, Ordering::Relaxed);
    }

    fn stop(&mut self) {
        self.duck(false);
        self.control.epoch.fetch_add(1, Ordering::AcqRel);
    }
}

enum Item {
    Audio(Pcm, String),
    Failed(Failure),
    Sound(Sound),
}

struct Failure {
    sentence: String,
    reason: String,
}

struct Output {
    _stream: cpal::Stream,
    sample_rate: u32,
    pcm: rtrb::Producer<f32>,
    played: rtrb::Consumer<f32>,
    downsampler: Downsampler,
}

struct Player {
    device: String,
    events: mpsc::UnboundedSender<SpeakerEvent>,
    reference: Reference,
    output: Option<Output>,
    backlog: VecDeque<f32>,
    queued: u64,
    consumed: Arc<AtomicU64>,
    rms: Arc<AtomicU32>,
    sentence_ends: VecDeque<(u64, Option<Failure>)>,
    cue_last: bool,
    control: Arc<Control>,
}

impl Player {
    fn new(
        device: String,
        events: mpsc::UnboundedSender<SpeakerEvent>,
        reference: Reference,
        control: Arc<Control>,
    ) -> Self {
        Player {
            device,
            events,
            reference,
            output: None,
            backlog: VecDeque::new(),
            queued: 0,
            consumed: Arc::default(),
            rms: Arc::default(),
            sentence_ends: VecDeque::new(),
            cue_last: false,
            control,
        }
    }

    fn run(mut self, items: std_mpsc::Receiver<(u64, Item)>) {
        let mut next_tick = Instant::now() + LEVEL_PERIOD;
        loop {
            match items.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
                Ok((epoch, item)) => {
                    self.flush_if_stopped();
                    if epoch == self.control.epoch.load(Ordering::Acquire) {
                        self.queue(item);
                    }
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => {
                    self.tick();
                    next_tick = Instant::now() + LEVEL_PERIOD;
                }
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    if self.cue_last {
                        self.play_out();
                    }
                    return;
                }
            }
        }
    }

    fn play_out(&mut self) {
        let deadline = Instant::now() + CUE_PLAY_OUT;
        while self.output.is_some()
            && self.consumed.load(Ordering::Acquire) < self.queued
            && Instant::now() < deadline
        {
            std::thread::sleep(LEVEL_PERIOD);
            self.tick();
        }
    }

    fn queue(&mut self, item: Item) {
        self.cue_last = matches!(item, Item::Sound(_));
        match item {
            Item::Audio(pcm, sentence) => match self.output_at(pcm.sample_rate) {
                Ok(()) => {
                    self.queued += pcm.samples.len() as u64;
                    self.backlog.extend(pcm.samples);
                    self.sentence_ends.push_back((self.queued, None));
                }
                Err(e) => {
                    let reason = format!("can't play speech: {e}");
                    eprintln!("lucyd: {reason}");
                    let failure = Failure { sentence, reason };
                    self.sentence_ends.push_back((self.queued, Some(failure)));
                }
            },
            Item::Failed(failure) => self.sentence_ends.push_back((self.queued, Some(failure))),
            Item::Sound(sound) => {
                let rate = self
                    .output
                    .as_ref()
                    .map_or(DEFAULT_VOICE_RATE, |o| o.sample_rate);
                match self.output_at(rate) {
                    Ok(()) => {
                        let samples = cue(sound, rate);
                        self.queued += samples.len() as u64;
                        self.backlog.extend(samples);
                    }
                    Err(e) => eprintln!("lucyd: can't play a sound: {e}"),
                }
            }
        }
        self.tick();
    }

    fn output_at(&mut self, sample_rate: u32) -> Result<(), String> {
        let idle = self.backlog.is_empty() && self.consumed.load(Ordering::Acquire) >= self.queued;
        if let Some(output) = &self.output {
            if output.sample_rate == sample_rate {
                return Ok(());
            }
            if idle {
                self.output = None;
                return self.output_at(sample_rate);
            }
            return Err(format!(
                "the voice changed rate from {} to {sample_rate} Hz",
                output.sample_rate
            ));
        }
        self.output = Some(open_output(
            &self.device,
            sample_rate,
            self.consumed.clone(),
            self.rms.clone(),
            self.control.clone(),
            self.events.clone(),
        )?);
        Ok(())
    }

    fn flush_if_stopped(&mut self) {
        let epoch = self.control.epoch.load(Ordering::Acquire);
        if self.control.flushed.load(Ordering::Acquire) == epoch {
            return;
        }
        self.queued -= self.backlog.len() as u64;
        self.backlog.clear();
        self.sentence_ends.clear();
        self.cue_last = false;
        self.control.flushed.store(epoch, Ordering::Release);
    }

    fn tick(&mut self) {
        self.flush_if_stopped();
        let Some(output) = &mut self.output else {
            self.report_finished();
            return;
        };
        while !output.pcm.is_full() {
            let Some(sample) = self.backlog.pop_front() else {
                break;
            };
            let _ = output.pcm.push(sample);
        }
        let played: Vec<f32> = std::iter::from_fn(|| output.played.pop().ok()).collect();
        let mut reference = self.reference.locked();
        output.downsampler.push(&played, |samples| {
            for &s in samples {
                let _ = reference.push(s);
            }
        });
        drop(reference);
        if self.consumed.load(Ordering::Acquire) < self.queued {
            let rms = f32::from_bits(self.rms.load(Ordering::Relaxed));
            let _ = self.events.send(SpeakerEvent::Level(rms));
        }
        self.report_finished();
    }

    fn report_finished(&mut self) {
        let consumed = self.consumed.load(Ordering::Acquire);
        while let Some((_, failed)) = self.sentence_ends.pop_front_if(|(end, _)| *end <= consumed) {
            let _ = self.events.send(match failed {
                None => SpeakerEvent::Played,
                Some(Failure { sentence, reason }) => SpeakerEvent::Failed { sentence, reason },
            });
        }
    }
}

fn cue(sound: Sound, rate: u32) -> Vec<f32> {
    let (notes, gain): (&[(f32, f32)], f32) = match sound {
        Sound::Start => (&[(660.0, 0.09), (880.0, 0.12)], 0.25),
        Sound::Closing => (&[(880.0, 0.09), (587.0, 0.16)], 0.12),
    };
    let rate = rate as f32;
    notes
        .iter()
        .flat_map(|&(hz, secs)| {
            let len = (rate * secs) as usize;
            (0..len).map(move |i| {
                let click_free_envelope = (std::f32::consts::PI * i as f32 / len as f32).sin();
                let phase = 2.0 * std::f32::consts::PI * hz * i as f32 / rate;
                gain * click_free_envelope * phase.sin()
            })
        })
        .collect()
}

fn open_output(
    device: &str,
    sample_rate: u32,
    consumed: Arc<AtomicU64>,
    rms: Arc<AtomicU32>,
    control: Arc<Control>,
    events: mpsc::UnboundedSender<SpeakerEvent>,
) -> Result<Output, String> {
    let device = audio::cpal::find(audio::Direction::Output, device)?;
    let channels = device
        .default_output_config()
        .map_err(|e| e.to_string())?
        .channels();
    let config = cpal::StreamConfig {
        channels,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let ring = sample_rate as usize * RING_SECS;
    let (pcm, mut source) = rtrb::RingBuffer::new(ring);
    let (mut played_tx, played) = rtrb::RingBuffer::new(ring);
    let mut drained_epoch = 0;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                let (mut played_now, mut sum) = (0u64, 0.0f32);
                let epoch = control.epoch.load(Ordering::Acquire);
                if epoch != drained_epoch {
                    let player_flushed = control.flushed.load(Ordering::Acquire) == epoch;
                    let mut dropped = 0;
                    while source.pop().is_ok() {
                        dropped += 1;
                    }
                    consumed.fetch_add(dropped, Ordering::Release);
                    if !player_flushed {
                        data.fill(0.0);
                        return;
                    }
                    drained_epoch = epoch;
                }
                let gain = control.gain();
                for frame in data.chunks_mut(channels as usize) {
                    let sample = match source.pop() {
                        Ok(sample) => {
                            let sample = sample * gain;
                            played_now += 1;
                            sum += sample * sample;
                            let _ = played_tx.push(sample);
                            sample
                        }
                        Err(_) => 0.0,
                    };
                    frame.fill(sample);
                }
                if played_now > 0 {
                    let level = (sum / played_now as f32).sqrt();
                    rms.store(level.to_bits(), Ordering::Relaxed);
                    consumed.fetch_add(played_now, Ordering::Release);
                }
            },
            move |e| {
                eprintln!("lucyd: playback error: {e}");
                if audio::cpal::is_device_lost(&e) {
                    let _ = events.send(SpeakerEvent::DeviceLost);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Output {
        _stream: stream,
        sample_rate,
        pcm,
        played,
        downsampler: Downsampler::new(sample_rate),
    })
}

pub struct Downsampler {
    resampler: Option<Fft<f32>>,
    input: Vec<f32>,
    output: Vec<f32>,
}

impl Downsampler {
    pub fn new(from_rate: u32) -> Self {
        let resampler = (from_rate != REFERENCE_RATE).then(|| {
            Fft::new(
                from_rate as usize,
                REFERENCE_RATE as usize,
                1024,
                1,
                FixedSync::Input,
            )
            .expect("valid resampler rates")
        });
        let output = vec![0.0; resampler.as_ref().map_or(0, |r| r.output_frames_max())];
        Downsampler {
            resampler,
            input: Vec::new(),
            output,
        }
    }

    pub fn push(&mut self, samples: &[f32], mut out: impl FnMut(&[f32])) {
        let Some(resampler) = &mut self.resampler else {
            out(samples);
            return;
        };
        self.input.extend_from_slice(samples);
        let chunk = resampler.input_frames_next();
        while self.input.len() >= chunk {
            let input = InterleavedSlice::new(&self.input[..chunk], 1, chunk)
                .expect("a mono slice of `chunk` samples holds `chunk` frames");
            let capacity = self.output.len();
            let mut output = InterleavedSlice::new_mut(&mut self.output, 1, capacity)
                .expect("a mono buffer of `capacity` samples holds `capacity` frames");
            let (_, written) = resampler
                .process_into_buffer(&input, &mut output, None)
                .expect("buffers sized by the resampler");
            out(&self.output[..written]);
            self.input.drain(..chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn frequency(samples: &[f32], rate: u32) -> f32 {
        let past_startup_delay = rate as usize / 10;
        let samples = &samples[past_startup_delay..];
        let rising = samples.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0);
        rising.count() as f32 / (samples.len() as f32 / rate as f32)
    }

    #[test]
    fn the_reference_is_the_played_audio_at_16_khz() {
        let mut downsampler = Downsampler::new(22_050);
        let mut reference = Vec::new();

        let uneven_pieces = 333;
        for piece in sine(440.0, 22_050, 2.0).chunks(uneven_pieces) {
            downsampler.push(piece, |out| reference.extend_from_slice(out));
        }

        assert!(
            (30_000..=32_000).contains(&reference.len()),
            "two seconds in, less what is still buffered (under one chunk): {}",
            reference.len()
        );
        let hz = frequency(&reference, 16_000);
        assert!((435.0..=445.0).contains(&hz), "{hz} Hz");
    }

    #[test]
    fn a_16_khz_voice_is_its_own_reference() {
        let mut downsampler = Downsampler::new(16_000);
        let mut reference = Vec::new();

        downsampler.push(&[0.1, 0.2, 0.3], |out| reference.extend_from_slice(out));

        assert_eq!(reference, [0.1, 0.2, 0.3]);
    }
}
