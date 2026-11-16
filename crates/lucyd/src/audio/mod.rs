//! The microphone side of the audio pipeline, which turns the microphone into utterances.
//! It holds capture, the DSP thread and the VAD's endpointing, feeding STT.

pub(crate) mod cpal;
mod endpoint;
mod gate;
mod livekit;
mod silero;
mod sonora;

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lucy_config::{Audio, BargeIn, BargeInMode, EchoCancellation, Endpointing};
use tokio::sync::{mpsc, watch};

pub use self::cpal::Cpal;
pub use self::sonora::{too_quiet, Sonora};
use crate::adapters::SpeechSegment;
use endpoint::Endpointer;
pub use gate::BargeInEvent;
use gate::{Echo, Gate, Judged};
pub use livekit::{install_bundled_wake_model, LiveKit};
pub use silero::Silero;

pub const SAMPLE_RATE: u32 = 16_000;

const FRAME_MS: usize = 10;

const FRAME_LEN: usize = SAMPLE_RATE as usize * FRAME_MS / 1000;

const RING_LEN: usize = SAMPLE_RATE as usize;

const CAUGHT_UP_SLEEP: Duration = Duration::from_millis(5);

/// The sound card. Real capture goes through cpal; the harness feeds
/// scripted samples.
pub trait AudioIo {
    /// Starts capturing `mic` (`"default"` or a pinned node name) as 16 kHz mono.
    /// It pushes samples into `ring` without blocking. Capture runs until
    /// the returned handle is dropped. If the device goes away, capture
    /// stops and drops `ring`.
    ///
    /// # Errors
    ///
    /// When the microphone can't be opened.
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error>;

    /// Whether a device with the node name `name` is there now.
    fn has_device(&self, direction: Direction, name: &str) -> bool;

    /// Whether the sound server is PipeWire.
    /// PipeWire keeps capture and playback in step for the echo canceller. On
    /// PulseAudio or ALSA
    /// Barge-in by speech is off unless forced.
    fn is_pipewire(&self) -> bool;
}

/// Which way a device's audio goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// A microphone.
    Input,
    /// A speaker.
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Devices {
    pub speaker: String,
    pub missing: Vec<(Direction, String)>,
    pub speech_barge_in: bool,
}

impl Devices {
    pub fn as_configured(audio: &Audio, barge_in: &BargeIn) -> Devices {
        Devices {
            speaker: audio.speaker.clone(),
            missing: vec![],
            speech_barge_in: barge_in.mode == BargeInMode::Speech,
        }
    }
}

pub(crate) const DEFAULT: &str = "default";

fn resolve(io: &dyn AudioIo, direction: Direction, name: &str) -> Result<String, String> {
    if name == DEFAULT || io.has_device(direction, name) {
        Ok(name.to_owned())
    } else {
        Err(name.to_owned())
    }
}

/// A voice-activity detector.
pub trait Vad: Send {
    /// The number of 16 kHz samples it judges at a time.
    fn chunk_len(&self) -> usize;

    /// How likely `chunk` (of `chunk_len` samples) is to be speech, 0 to 1.
    fn speech_probability(&mut self, chunk: &[f32]) -> f32;
}

/// Cancels Lucy's own voice from the microphone, with what she played as
/// its reference.
pub trait EchoCanceller: Send {
    /// Cancels Lucy's voice from the next 10 ms frame of the microphone, in
    /// place.
    fn cancel(&mut self, frame: &mut [f32]);

    /// How much echo it removes (ERLE, in dB), once it can tell.
    fn erle_db(&self) -> Option<f32>;

    /// Whether Lucy's voice reaches the microphone at all: `None` until it
    /// can tell.
    fn echo_heard(&self) -> Option<bool>;

    /// Whether Lucy's voice has been in its reference just now.
    fn lucy_playing(&self) -> bool {
        false
    }
}

/// A wake-word detector. It hears every frame, in and out of Sessions.
pub trait WakeWord: Send {
    /// Takes the next 10 ms frame. True if it completes the Wake word.
    fn push(&mut self, frame: &[f32]) -> bool;

    /// The score the last frame got, from 0 to 1, for setup's wake-word
    /// test.
    fn score(&self) -> f32 {
        0.0
    }
}

#[derive(Default)]
struct Meter {
    level: AtomicU32,
    wake_score: AtomicU32,
}

impl Meter {
    fn raise(cell: &AtomicU32, value: f32) {
        cell.fetch_max(as_ordered_bits(value), Ordering::Relaxed);
    }

    fn take(cell: &AtomicU32) -> f32 {
        f32::from_bits(cell.swap(0, Ordering::Relaxed))
    }
}

fn as_ordered_bits(value: f32) -> u32 {
    value.max(0.0).to_bits()
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

fn dbfs(rms: f32) -> f32 {
    20.0 * rms.max(1e-6).log10()
}

/// Why the microphone can't be heard.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// What the microphone gives lucyd.
pub struct Ears {
    pub hearing: Hearing,
    /// Utterances, cut by the VAD.
    pub speech: mpsc::UnboundedReceiver<SpeechSegment>,
    /// One message each time the Wake word is heard.
    pub wake_word: mpsc::UnboundedReceiver<()>,
    /// Whether the user is in the middle of an utterance.
    pub speaking: watch::Receiver<bool>,
}

/// Opens the microphone and starts the DSP thread.
///
/// The thread reframes capture to 10 ms, cancels Lucy's echo with `echo`
/// unless `audio` leaves that to PipeWire, listens for the Wake word with
/// `wake_word` if there is one, cuts capture into utterances with `vad`, and
/// runs the Barge-in gate. Capture stops once the speech receiver is dropped.
///
/// # Errors
///
/// When the microphone can't be opened or the DSP thread can't start.
pub fn listen(
    io: Box<dyn AudioIo + Send>,
    audio: &Audio,
    vad: Box<dyn Vad>,
    wake_word: Option<Box<dyn WakeWord>>,
    echo: Option<Box<dyn EchoCanceller>>,
    endpointing: &Endpointing,
) -> Result<Ears, Error> {
    let mic = resolve(io.as_ref(), Direction::Input, &audio.mic).unwrap_or_else(|missing| {
        eprintln!("lucyd: microphone {missing} not found; using the default");
        DEFAULT.to_owned()
    });
    let (producer, consumer) = rtrb::RingBuffer::new(RING_LEN);
    let capture = io.capture(&mic, producer)?;
    let (speech, speech_rx) = mpsc::unbounded_channel();
    let (wakes, wakes_rx) = mpsc::unbounded_channel();
    let (changes, changes_rx) = std::sync::mpsc::channel();
    let (lost, lost_rx) = watch::channel(false);
    let (speaking, speaking_rx) = watch::channel(false);
    let (gate_events, gate_rx) = mpsc::unbounded_channel();
    let lost = Arc::new(lost);
    let meter = Arc::new(Meter::default());
    let has_wake_word = wake_word.is_some();
    let endpointer = Endpointer::new(vad, endpointing);
    let listeners = Listeners {
        meter: meter.clone(),
        echo,
        own_canceller_on: audio.echo_cancellation == EchoCancellation::Lucy,
        wake_word,
        endpointer,
        gate: Gate::default(),
        maybe_own_voice: false,
        wakes,
        speech,
        speaking,
        gate_events,
        lost: lost.clone(),
    };
    std::thread::Builder::new()
        .name("lucyd-dsp".to_owned())
        .spawn(move || dsp(capture, consumer, listeners, changes_rx))
        .map_err(|e| Error(format!("can't start the DSP thread: {e}")))?;
    let hearing = Hearing {
        io,
        mic,
        endpointing: endpointing.clone(),
        changes,
        lost,
        lost_rx,
        meter,
        has_wake_word,
        gate: gate_rx,
        lucy_speaking: false,
    };
    Ok(Ears {
        hearing,
        speech: speech_rx,
        wake_word: wakes_rx,
        speaking: speaking_rx,
    })
}

struct Listeners {
    meter: Arc<Meter>,
    echo: Option<Box<dyn EchoCanceller>>,
    own_canceller_on: bool,
    wake_word: Option<Box<dyn WakeWord>>,
    endpointer: Endpointer,
    gate: Gate,
    maybe_own_voice: bool,
    wakes: mpsc::UnboundedSender<()>,
    speech: mpsc::UnboundedSender<SpeechSegment>,
    gate_events: mpsc::UnboundedSender<BargeInEvent>,
    speaking: watch::Sender<bool>,
    lost: Arc<watch::Sender<bool>>,
}

/// Changes the running DSP thread's microphone and endpointing.
pub struct Hearing {
    io: Box<dyn AudioIo + Send>,
    mic: String,
    endpointing: Endpointing,
    changes: std::sync::mpsc::Sender<Change>,
    lost: Arc<watch::Sender<bool>>,
    lost_rx: watch::Receiver<bool>,
    meter: Arc<Meter>,
    has_wake_word: bool,
    gate: mpsc::UnboundedReceiver<BargeInEvent>,
    lucy_speaking: bool,
}

enum Change {
    Mic(Box<dyn Send>, rtrb::Consumer<f32>),
    Endpointing(Endpointing),
    Session {
        barge_in: BargeIn,
        speech_barges_in: bool,
        own_canceller_on: bool,
    },
    LucySpeaking(bool),
}

pub enum Heard {
    Lost,
    Gate(BargeInEvent),
}

impl Hearing {
    pub fn configure(
        &mut self,
        audio: &Audio,
        endpointing: &Endpointing,
        barge_in: &BargeIn,
    ) -> Devices {
        let mut missing = vec![];
        let mut pick = |direction, name| {
            resolve(self.io.as_ref(), direction, name).unwrap_or_else(|name| {
                missing.push((direction, name));
                DEFAULT.to_owned()
            })
        };
        let mic = pick(Direction::Input, &audio.mic);
        let speaker = pick(Direction::Output, &audio.speaker);
        if mic != self.mic || *self.lost.borrow() {
            self.open(mic);
        }
        if *endpointing != self.endpointing {
            let _ = self.changes.send(Change::Endpointing(endpointing.clone()));
            self.endpointing = endpointing.clone();
        }
        let speech_barge_in = self.speech_barge_in(barge_in);
        let cancelling = audio.echo_cancellation == EchoCancellation::Lucy;
        let _ = self.changes.send(Change::Session {
            barge_in: barge_in.clone(),
            speech_barges_in: speech_barge_in,
            own_canceller_on: cancelling,
        });
        self.lucy_speaking = false;
        self.drop_stale_gate_events();
        Devices {
            speaker,
            missing,
            speech_barge_in,
        }
    }

    fn drop_stale_gate_events(&mut self) {
        while self.gate.try_recv().is_ok() {}
    }

    fn speech_barge_in(&self, barge_in: &BargeIn) -> bool {
        if barge_in.mode != BargeInMode::Speech {
            return false;
        }
        if self.io.is_pipewire() {
            return true;
        }
        if barge_in.force {
            eprintln!(
                "lucyd: warning: Barge-in by speech is forced on without PipeWire; \
                 Lucy may hear herself and stop"
            );
        } else {
            eprintln!(
                "lucyd: Barge-in by speech is off without PipeWire; the Wake word still \
                 cuts in. Set [barge_in] force = true to turn it on"
            );
        }
        barge_in.force
    }

    pub fn lucy_speaking(&mut self, speaking: bool) {
        if speaking != self.lucy_speaking {
            self.lucy_speaking = speaking;
            let _ = self.changes.send(Change::LucySpeaking(speaking));
        }
    }

    pub async fn next(&mut self) -> Heard {
        tokio::select! {
            _ = self.lost_rx.wait_for(|lost| *lost) => Heard::Lost,
            Some(event) = self.gate.recv() => Heard::Gate(event),
        }
    }

    pub fn reopen(&mut self) {
        let mic = resolve(self.io.as_ref(), Direction::Input, &self.mic)
            .unwrap_or_else(|_| DEFAULT.to_owned());
        self.open(mic);
    }

    fn open(&mut self, mic: String) {
        let (producer, consumer) = rtrb::RingBuffer::new(RING_LEN);
        match self.io.capture(&mic, producer) {
            Ok(capture) => {
                let _ = self.changes.send(Change::Mic(capture, consumer));
                self.lost.send_replace(false);
                self.mic = mic;
            }
            Err(e) => eprintln!("lucyd: can't open the microphone {mic}: {e}"),
        }
    }

    pub fn take_level(&self) -> f32 {
        Meter::take(&self.meter.level)
    }

    pub fn has_wake_word(&self) -> bool {
        self.has_wake_word
    }

    pub fn take_wake_score(&self) -> f32 {
        Meter::take(&self.meter.wake_score)
    }

    pub async fn lost(&mut self) {
        let _ = self.lost_rx.wait_for(|lost| *lost).await;
    }
}

fn dsp(
    mut _capture: Box<dyn Send>,
    mut mic: rtrb::Consumer<f32>,
    mut listeners: Listeners,
    changes: std::sync::mpsc::Receiver<Change>,
) {
    let mut frame = [0.0; FRAME_LEN];
    let mut loss_reported = false;
    while !listeners.speech.is_closed() {
        for change in changes.try_iter() {
            match change {
                Change::Mic(capture, consumer) => {
                    (_capture, mic) = (capture, consumer);
                    loss_reported = false;
                }
                Change::Endpointing(config) => listeners.endpointer.configure(&config),
                Change::Session {
                    barge_in,
                    speech_barges_in: on,
                    own_canceller_on: cancelling,
                } => {
                    log::debug!(
                        "gate: Session starts, speech barges in={on}, own canceller={cancelling}"
                    );
                    listeners.gate.configure(&barge_in, on);
                    listeners.own_canceller_on = cancelling;
                }
                Change::LucySpeaking(speaking) => {
                    log::debug!("gate: Lucy speaking={speaking}");
                    listeners.gate.lucy_speaking(speaking);
                }
            }
        }
        listeners.report_speaking();
        if mic.is_abandoned() && mic.slots() < FRAME_LEN && !loss_reported {
            eprintln!("lucyd: the microphone has gone away");
            listeners.lost.send_replace(true);
            loss_reported = true;
        }
        if mic.slots() < FRAME_LEN {
            std::thread::sleep(CAUGHT_UP_SLEEP);
            continue;
        }
        let chunk = mic.read_chunk(FRAME_LEN).expect("a frame is ready");
        let (first, second) = chunk.as_slices();
        frame[..first.len()].copy_from_slice(first);
        frame[first.len()..].copy_from_slice(second);
        chunk.commit_all();
        listeners.hear(&mut frame);
    }
}

impl Listeners {
    fn hear(&mut self, frame: &mut [f32]) {
        let level = rms(frame);
        Meter::raise(&self.meter.level, level);
        let (echo, lucy_playing) = match self.echo.as_mut().filter(|_| self.own_canceller_on) {
            Some(canceller) => {
                canceller.cancel(frame);
                let echo = Echo::Cancelled {
                    erle_db: canceller.erle_db(),
                    reaches: canceller.echo_heard().into(),
                };
                (echo, canceller.lucy_playing())
            }
            None => (Echo::Elsewhere, false),
        };
        self.hear_wake_word_before_its_utterance_ends(frame);
        let was_speaking = self.endpointer.speaking();
        let wake_score = self.wake_word.as_ref().map(|wake_word| wake_word.score());
        let cancelled = rms(frame);
        let (gate, gate_events) = (&mut self.gate, &self.gate_events);
        let segment = self.endpointer.push(frame, |probability, len| {
            let Judged { event, converged } = gate.judge(probability, len, echo);
            if gate.window_open() || lucy_playing {
                log::debug!(
                    "gate: p={probability:.2} echo={echo:?} settled={converged} window={} \
                     on={} cut={} mic_dbfs={:.1} cancelled_dbfs={:.1} wake={wake_score:?} \
                     event={event:?}",
                    gate.window_open(),
                    gate.on(),
                    gate.is_cut(),
                    dbfs(level),
                    dbfs(cancelled),
                );
            }
            if let Some(event) = event {
                let _ = gate_events.send(event);
            }
        });
        if !was_speaking && self.endpointer.speaking() {
            self.maybe_own_voice = self.gate.window_open();
        }
        let user_cut_in = self.gate.is_cut();
        if user_cut_in {
            self.maybe_own_voice = false;
        }
        if let Some(segment) = segment {
            if !std::mem::take(&mut self.maybe_own_voice) {
                let _ = self.speech.send(segment);
            }
        }
        if !self.endpointer.speaking() {
            self.maybe_own_voice = false;
        }
        self.report_speaking();
    }

    fn hear_wake_word_before_its_utterance_ends(&mut self, frame: &[f32]) {
        if let Some(wake_word) = &mut self.wake_word {
            let heard = wake_word.push(frame);
            Meter::raise(&self.meter.wake_score, wake_word.score());
            if heard {
                log::debug!(
                    "wake word heard, gate window open={}",
                    self.gate.window_open()
                );
                let _ = self.wakes.send(());
                self.gate.cut_by_wake_word();
            }
        }
    }

    fn report_speaking(&self) {
        let speaking = self.endpointer.speaking() && !self.maybe_own_voice;
        self.speaking
            .send_if_modified(|was| std::mem::replace(was, speaking) != speaking);
    }
}
