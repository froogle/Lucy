//! The microphone side of the audio pipeline: capture, the DSP thread and
//! the VAD's endpointing, which together turn the microphone into
//! utterances for STT.

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
pub use self::sonora::Sonora;
use crate::adapters::SpeechSegment;
use endpoint::Endpointer;
pub use gate::BargeInEvent;
use gate::Gate;
pub(crate) use livekit::BUNDLED_NAME as BUNDLED_WAKE_MODEL;
pub use livekit::{install_bundled_wake_model, LiveKit};
pub use silero::Silero;

/// Lucy's internal sample rate: mono f32 at 16 kHz.
pub const SAMPLE_RATE: u32 = 16_000;

/// The DSP thread works in 10 ms frames.
const FRAME_LEN: usize = SAMPLE_RATE as usize / 100;

/// How much capture the ring holds before the callback drops samples.
const RING_LEN: usize = SAMPLE_RATE as usize;

/// How long the DSP thread sleeps when it has caught up with capture.
const POLL: Duration = Duration::from_millis(5);

/// The sound card. Real capture goes through cpal; the harness feeds
/// scripted samples.
pub trait AudioIo {
    /// Starts capturing `mic` (`"default"` or a pinned node name) as 16 kHz
    /// mono, pushing samples into `ring` without blocking. Capture runs until
    /// the returned handle is dropped. If the device goes away, capture
    /// stops and drops `ring`.
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error>;

    /// Whether a device with the node name `name` is there now.
    fn has_device(&self, direction: Direction, name: &str) -> bool;

    /// Whether the sound server is PipeWire, which keeps capture and
    /// playback in step for the echo canceller. On PulseAudio or ALSA
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

/// The devices a Session uses, resolved by name at its start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Devices {
    /// Where Lucy speaks: `"default"` or a pinned node name.
    pub speaker: String,
    /// The pinned devices that aren't there, so the default stands in.
    pub missing: Vec<(Direction, String)>,
    /// Speech, not only the Wake word, can barge in.
    pub speech_barge_in: bool,
}

impl Devices {
    /// The devices `audio` names, taken as they are.
    pub fn as_configured(audio: &Audio, barge_in: &BargeIn) -> Devices {
        Devices {
            speaker: audio.speaker.clone(),
            missing: vec![],
            speech_barge_in: barge_in.mode == BargeInMode::Speech,
        }
    }
}

/// The name that stands for the default device.
pub(crate) const DEFAULT: &str = "default";

/// `name`, or the default if it is pinned and not there.
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

/// The loudest the microphone has been, and the best wake-word score, since
/// setup last asked. Both are 0 to 1.
#[derive(Default)]
struct Meter {
    /// `f32` bits: for non-negative floats they order as the floats do.
    level: AtomicU32,
    wake_score: AtomicU32,
}

impl Meter {
    fn raise(cell: &AtomicU32, value: f32) {
        cell.fetch_max(value.max(0.0).to_bits(), Ordering::Relaxed);
    }

    fn take(cell: &AtomicU32) -> f32 {
        f32::from_bits(cell.swap(0, Ordering::Relaxed))
    }
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
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

/// Opens the microphone and starts the DSP thread, which reframes capture to
/// 10 ms, cancels Lucy's echo with `echo` unless `audio` leaves that to
/// PipeWire, listens for the Wake word with `wake_word` if there is one,
/// cuts capture into utterances with `vad`, and runs the Barge-in gate.
/// Capture stops once the speech receiver is dropped.
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
        cancelling: audio.echo_cancellation == EchoCancellation::Lucy,
        wake_word,
        endpointer,
        gate: Gate::default(),
        held: false,
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

/// What the DSP thread hands each frame to, and where they report.
struct Listeners {
    meter: Arc<Meter>,
    echo: Option<Box<dyn EchoCanceller>>,
    /// Lucy's own echo canceller is on.
    cancelling: bool,
    wake_word: Option<Box<dyn WakeWord>>,
    endpointer: Endpointer,
    gate: Gate,
    /// The utterance under way started while Lucy's echo could be heard and
    /// hasn't passed the gate: it may be her own voice, so it is kept from
    /// the Session, and dropped if it ends that way.
    held: bool,
    wakes: mpsc::UnboundedSender<()>,
    speech: mpsc::UnboundedSender<SpeechSegment>,
    gate_events: mpsc::UnboundedSender<BargeInEvent>,
    /// Whether an utterance is under way: set once it starts, cleared once
    /// it is sent on `speech` or dropped.
    speaking: watch::Sender<bool>,
    /// Set once the open microphone has gone away.
    lost: Arc<watch::Sender<bool>>,
}

/// Changes the running DSP thread's microphone and endpointing.
pub struct Hearing {
    io: Box<dyn AudioIo + Send>,
    /// The microphone open now.
    mic: String,
    endpointing: Endpointing,
    changes: std::sync::mpsc::Sender<Change>,
    /// True once the open microphone has gone away.
    lost: Arc<watch::Sender<bool>>,
    lost_rx: watch::Receiver<bool>,
    meter: Arc<Meter>,
    has_wake_word: bool,
    gate: mpsc::UnboundedReceiver<BargeInEvent>,
    lucy_speaking: bool,
}

/// A change the DSP thread picks up before its next frame.
enum Change {
    Mic(Box<dyn Send>, rtrb::Consumer<f32>),
    Endpointing(Endpointing),
    /// A Session starts: the gate's settings, whether speech may barge in,
    /// and whether Lucy's own echo canceller is on.
    Session(BargeIn, bool, bool),
    LucySpeaking(bool),
}

/// What the microphone reports during a Session.
pub enum Heard {
    /// The open microphone has gone away.
    Lost,
    Gate(BargeInEvent),
}

impl Hearing {
    /// Applies `[audio]`, `[endpointing]` and `[barge_in]` as read at a
    /// Session start, resolving the devices by name. A pinned device that
    /// isn't there gives way to the default, and the pin stays for the next
    /// Session. A microphone that can't be opened leaves the current one in
    /// use.
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
        let _ = self.changes.send(Change::Session(
            barge_in.clone(),
            speech_barge_in,
            cancelling,
        ));
        self.lucy_speaking = false;
        // Anything from the last Session's tail is stale.
        while self.gate.try_recv().is_ok() {}
        Devices {
            speaker,
            missing,
            speech_barge_in,
        }
    }

    /// Whether speech may barge in: as `barge_in` says on PipeWire, and
    /// elsewhere only if forced.
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

    /// Tells the gate whether Lucy is speaking.
    pub fn lucy_speaking(&mut self, speaking: bool) {
        if speaking != self.lucy_speaking {
            self.lucy_speaking = speaking;
            let _ = self.changes.send(Change::LucySpeaking(speaking));
        }
    }

    /// What the microphone reports next during a Session.
    pub async fn next(&mut self) -> Heard {
        tokio::select! {
            _ = self.lost_rx.wait_for(|lost| *lost) => Heard::Lost,
            Some(event) = self.gate.recv() => Heard::Gate(event),
        }
    }

    /// Opens the microphone that went away again, or the default if it is
    /// pinned and not there. If it can't be opened, it stays lost.
    pub fn reopen(&mut self) {
        let mic = resolve(self.io.as_ref(), Direction::Input, &self.mic)
            .unwrap_or_else(|_| DEFAULT.to_owned());
        self.open(mic);
    }

    /// Hands the DSP thread capture of `mic`, or leaves the current
    /// microphone in use if it can't be opened.
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

    /// The loudest RMS the microphone heard since this was last asked.
    pub fn take_level(&self) -> f32 {
        Meter::take(&self.meter.level)
    }

    /// Whether the Wake word can be heard at all.
    pub fn has_wake_word(&self) -> bool {
        self.has_wake_word
    }

    /// The best wake-word score since this was last asked.
    pub fn take_wake_score(&self) -> f32 {
        Meter::take(&self.meter.wake_score)
    }

    /// Resolves once the open microphone has gone away.
    pub async fn lost(&mut self) {
        // The sender lives as long as `self`.
        let _ = self.lost_rx.wait_for(|lost| *lost).await;
    }
}

/// The DSP thread.
fn dsp(
    mut _capture: Box<dyn Send>,
    mut mic: rtrb::Consumer<f32>,
    mut listeners: Listeners,
    changes: std::sync::mpsc::Receiver<Change>,
) {
    let mut frame = [0.0; FRAME_LEN];
    // The open microphone has gone away and that has been reported.
    let mut reported = false;
    while !listeners.speech.is_closed() {
        for change in changes.try_iter() {
            match change {
                Change::Mic(capture, consumer) => {
                    (_capture, mic) = (capture, consumer);
                    reported = false;
                }
                Change::Endpointing(config) => listeners.endpointer.configure(&config),
                Change::Session(barge_in, on, cancelling) => {
                    listeners.gate.configure(&barge_in, on);
                    listeners.cancelling = cancelling;
                }
                Change::LucySpeaking(speaking) => listeners.gate.lucy_speaking(speaking),
            }
        }
        listeners.report_speaking();
        if mic.is_abandoned() && mic.slots() < FRAME_LEN && !reported {
            eprintln!("lucyd: the microphone has gone away");
            listeners.lost.send_replace(true);
            reported = true;
        }
        if mic.slots() < FRAME_LEN {
            std::thread::sleep(POLL);
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
    /// Takes the next 10 ms frame of the microphone.
    fn hear(&mut self, frame: &mut [f32]) {
        Meter::raise(&self.meter.level, rms(frame));
        let own_canceller = self.echo.is_some() && self.cancelling;
        let mut erle_db = None;
        if let Some(echo) = self.echo.as_mut().filter(|_| own_canceller) {
            echo.cancel(frame);
            erle_db = echo.erle_db();
        }
        // Heard before the utterance it starts ends, so the Session it
        // opens gets that utterance.
        if let Some(wake_word) = &mut self.wake_word {
            let heard = wake_word.push(frame);
            // Before the detection, so a test that hears it sees its score.
            Meter::raise(&self.meter.wake_score, wake_word.score());
            if heard {
                let _ = self.wakes.send(());
                self.gate.wake_word();
            }
        }
        let was_speaking = self.endpointer.speaking();
        let (gate, gate_events) = (&mut self.gate, &self.gate_events);
        let segment = self.endpointer.push(frame, |probability, len| {
            if let Some(event) = gate.judge(probability, len, erle_db, own_canceller) {
                let _ = gate_events.send(event);
            }
        });
        if !was_speaking && self.endpointer.speaking() {
            self.held = self.gate.window_open();
        }
        // The user, not Lucy's echo: let the utterance through.
        if self.gate.is_cut() {
            self.held = false;
        }
        if let Some(segment) = segment {
            if !std::mem::take(&mut self.held) {
                let _ = self.speech.send(segment);
            }
        }
        if !self.endpointer.speaking() {
            self.held = false;
        }
        self.report_speaking();
    }

    /// Says whether an utterance is under way, if that changed.
    fn report_speaking(&self) {
        let speaking = self.endpointer.speaking() && !self.held;
        self.speaking
            .send_if_modified(|was| std::mem::replace(was, speaking) != speaking);
    }
}
