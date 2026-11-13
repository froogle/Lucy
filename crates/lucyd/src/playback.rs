//! TTS playback through cpal, at the voice's native rate.
//!
//! The PipeWire host puts every cpal stream of the process in one
//! `node.group`, so playback shares a driver with capture. The callback only
//! moves samples between lock-free rings. A player thread feeds it, reports
//! each sentence once it has played, sends the level about 30 times a second,
//! and downsamples what actually played to the 16 kHz echo-cancellation
//! reference.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use tokio::sync::mpsc;

use crate::adapters::{Sound, Speaker, SpeakerEvent};

/// Lucy's internal rate, which the echo canceller works at.
pub const REFERENCE_RATE: u32 = 16_000;

/// How often the level is reported while speaking: about 30 Hz.
const LEVEL_PERIOD: Duration = Duration::from_millis(33);

/// Seconds of audio the playback ring holds before the player keeps the rest.
const RING_SECS: usize = 10;

/// The rate cues are made at when nothing is open yet: that of the default
/// voice, so the stream rarely has to reopen for speech.
const SOUND_RATE: u32 = 22_050;

/// The longest a cue keeps playing after the Speaker is dropped.
const PLAY_OUT: Duration = Duration::from_secs(1);

/// One synthesised sentence: mono samples from -1.0 to 1.0.
pub struct Pcm {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// Turns a sentence into audio, or says why it couldn't.
pub type Synthesise = Box<dyn FnMut(&str) -> Result<Pcm, String> + Send>;

/// Where the 16 kHz copy of what Lucy played goes. Full means dropped.
pub type Reference = Arc<Mutex<rtrb::Producer<f32>>>;

/// Starts a Speaker that synthesises each sentence with `synthesise` and
/// plays it on the default output device.
pub fn speaker(
    mut synthesise: Synthesise,
    events: mpsc::UnboundedSender<SpeakerEvent>,
    reference: Reference,
) -> Box<dyn Speaker> {
    let (sentences, sentences_rx) = std_mpsc::channel::<Request>();
    let (items, items_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        for request in sentences_rx {
            let item = match request {
                Request::Sound(sound) => Item::Sound(sound),
                Request::Sentence(sentence) => match synthesise(&sentence) {
                    Ok(pcm) => Item::Audio(pcm, sentence),
                    Err(e) => {
                        eprintln!("lucyd: TTS failed: {e}");
                        Item::Failed(sentence)
                    }
                },
            };
            if items.send(item).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || Player::new(events, reference).run(items_rx));
    Box::new(PlaybackSpeaker { sentences })
}

struct PlaybackSpeaker {
    sentences: std_mpsc::Sender<Request>,
}

/// What the Speaker queues, in order.
enum Request {
    Sentence(String),
    Sound(Sound),
}

impl Speaker for PlaybackSpeaker {
    fn speak(&mut self, sentence: String) {
        let _ = self.sentences.send(Request::Sentence(sentence));
    }

    fn sound(&mut self, sound: Sound) {
        let _ = self.sentences.send(Request::Sound(sound));
    }
}

enum Item {
    /// A sentence's audio, and its text in case it can't be played.
    Audio(Pcm, String),
    Failed(String),
    Sound(Sound),
}

/// The output stream and what the player shares with its callback.
struct Output {
    _stream: cpal::Stream,
    sample_rate: u32,
    pcm: rtrb::Producer<f32>,
    /// A copy of every sample the callback played.
    played: rtrb::Consumer<f32>,
    downsampler: Downsampler,
}

struct Player {
    events: mpsc::UnboundedSender<SpeakerEvent>,
    reference: Reference,
    output: Option<Output>,
    /// Samples that didn't fit in the ring yet.
    backlog: VecDeque<f32>,
    /// Samples queued for playback so far.
    written: u64,
    /// Samples the callback has played so far.
    consumed: Arc<AtomicU64>,
    /// RMS of the callback's latest block, as f32 bits.
    rms: Arc<AtomicU32>,
    /// Each sentence's end position, with its text if it failed.
    pending: VecDeque<(u64, Option<String>)>,
    /// The last item queued was a cue.
    sound_last: bool,
}

impl Player {
    fn new(events: mpsc::UnboundedSender<SpeakerEvent>, reference: Reference) -> Self {
        Player {
            events,
            reference,
            output: None,
            backlog: VecDeque::new(),
            written: 0,
            consumed: Arc::default(),
            rms: Arc::default(),
            pending: VecDeque::new(),
            sound_last: false,
        }
    }

    /// Plays until the Speaker is dropped, which stops playback at once,
    /// unless the last thing queued was a cue: that plays out.
    fn run(mut self, items: std_mpsc::Receiver<Item>) {
        let mut next_tick = Instant::now() + LEVEL_PERIOD;
        loop {
            match items.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
                Ok(item) => self.queue(item),
                Err(std_mpsc::RecvTimeoutError::Timeout) => {
                    self.tick();
                    next_tick = Instant::now() + LEVEL_PERIOD;
                }
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    if self.sound_last {
                        self.play_out();
                    }
                    return;
                }
            }
        }
    }

    fn play_out(&mut self) {
        let deadline = Instant::now() + PLAY_OUT;
        while self.output.is_some()
            && self.consumed.load(Ordering::Acquire) < self.written
            && Instant::now() < deadline
        {
            std::thread::sleep(LEVEL_PERIOD);
            self.tick();
        }
    }

    fn queue(&mut self, item: Item) {
        self.sound_last = matches!(item, Item::Sound(_));
        match item {
            Item::Audio(pcm, sentence) => match self.output_at(pcm.sample_rate) {
                Ok(()) => {
                    self.written += pcm.samples.len() as u64;
                    self.backlog.extend(pcm.samples);
                    self.pending.push_back((self.written, None));
                }
                Err(e) => {
                    eprintln!("lucyd: can't play speech: {e}");
                    self.pending.push_back((self.written, Some(sentence)));
                }
            },
            Item::Failed(sentence) => self.pending.push_back((self.written, Some(sentence))),
            Item::Sound(sound) => {
                let rate = self.output.as_ref().map_or(SOUND_RATE, |o| o.sample_rate);
                match self.output_at(rate) {
                    Ok(()) => {
                        let samples = cue(sound, rate);
                        self.written += samples.len() as u64;
                        self.backlog.extend(samples);
                    }
                    Err(e) => eprintln!("lucyd: can't play a sound: {e}"),
                }
            }
        }
        self.tick();
    }

    /// Opens the output stream at `sample_rate`, unless it is already open.
    /// A stream at another rate is reopened once it has played everything.
    fn output_at(&mut self, sample_rate: u32) -> Result<(), String> {
        let idle = self.backlog.is_empty() && self.consumed.load(Ordering::Acquire) >= self.written;
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
            sample_rate,
            self.consumed.clone(),
            self.rms.clone(),
        )?);
        Ok(())
    }

    fn tick(&mut self) {
        let Some(output) = &mut self.output else {
            self.report_finished();
            return;
        };
        while !self.backlog.is_empty() && !output.pcm.is_full() {
            let _ = output.pcm.push(self.backlog.pop_front().unwrap());
        }
        let played: Vec<f32> = std::iter::from_fn(|| output.played.pop().ok()).collect();
        let mut reference = self.reference.lock().unwrap();
        output.downsampler.push(&played, |samples| {
            for &s in samples {
                let _ = reference.push(s);
            }
        });
        drop(reference);
        if self.consumed.load(Ordering::Acquire) < self.written {
            let rms = f32::from_bits(self.rms.load(Ordering::Relaxed));
            let _ = self.events.send(SpeakerEvent::Level(rms));
        }
        self.report_finished();
    }

    /// Reports, in order, every sentence that has finished playing.
    fn report_finished(&mut self) {
        let consumed = self.consumed.load(Ordering::Acquire);
        while let Some((end, _)) = self.pending.front() {
            if *end > consumed {
                return;
            }
            let (_, failed) = self.pending.pop_front().unwrap();
            let _ = self.events.send(match failed {
                None => SpeakerEvent::Played,
                Some(sentence) => SpeakerEvent::Failed(sentence),
            });
        }
    }
}

/// A cue as mono samples at `rate`: two short notes, rising to open a
/// Session and falling, softer, to close one.
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
                // A sine envelope, so each note starts and ends without a click.
                let envelope = (std::f32::consts::PI * i as f32 / len as f32).sin();
                let phase = 2.0 * std::f32::consts::PI * hz * i as f32 / rate;
                gain * envelope * phase.sin()
            })
        })
        .collect()
}

fn open_output(
    sample_rate: u32,
    consumed: Arc<AtomicU64>,
    rms: Arc<AtomicU32>,
) -> Result<Output, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
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
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                let (mut played_now, mut sum) = (0u64, 0.0f32);
                for frame in data.chunks_mut(channels as usize) {
                    let sample = match source.pop() {
                        Ok(sample) => {
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
            |e| eprintln!("lucyd: playback error: {e}"),
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

/// Converts mono audio at a voice's rate to 16 kHz with rubato's FFT
/// resampler, the only resampler Lucy runs herself.
pub struct Downsampler {
    /// None when the voice is already 16 kHz.
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

    /// Adds `samples` and hands every complete chunk of 16 kHz output to `out`.
    pub fn push(&mut self, samples: &[f32], mut out: impl FnMut(&[f32])) {
        let Some(resampler) = &mut self.resampler else {
            out(samples);
            return;
        };
        self.input.extend_from_slice(samples);
        let chunk = resampler.input_frames_next();
        while self.input.len() >= chunk {
            let input = InterleavedSlice::new(&self.input[..chunk], 1, chunk).unwrap();
            let capacity = self.output.len();
            let mut output = InterleavedSlice::new_mut(&mut self.output, 1, capacity).unwrap();
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

    /// Rising zero crossings per second, after skipping the resampler's
    /// start-up delay.
    fn frequency(samples: &[f32], rate: u32) -> f32 {
        let samples = &samples[rate as usize / 10..];
        let rising = samples.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0);
        rising.count() as f32 / (samples.len() as f32 / rate as f32)
    }

    #[test]
    fn the_reference_is_the_played_audio_at_16_khz() {
        let mut downsampler = Downsampler::new(22_050);
        let mut reference = Vec::new();

        // Fed in uneven pieces, as the playback callback hands it over.
        for piece in sine(440.0, 22_050, 2.0).chunks(333) {
            downsampler.push(piece, |out| reference.extend_from_slice(out));
        }

        // Two seconds in, less what is still buffered: under one chunk.
        assert!(
            (30_000..=32_000).contains(&reference.len()),
            "{}",
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
