//! The microphone side of the audio pipeline: capture, the DSP thread and
//! the VAD's endpointing, which together turn the microphone into
//! utterances for STT.

pub(crate) mod cpal;
mod endpoint;
mod livekit;
mod silero;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lucy_config::{Audio, Endpointing};
use tokio::sync::{mpsc, watch};

pub use self::cpal::Cpal;
use crate::adapters::SpeechSegment;
use endpoint::Endpointer;
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
}

impl Devices {
    /// The devices `audio` names, taken as they are.
    pub fn as_configured(audio: &Audio) -> Devices {
        Devices {
            speaker: audio.speaker.clone(),
            missing: vec![],
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

/// A wake-word detector. It hears every frame, in and out of Sessions.
pub trait WakeWord: Send {
    /// Takes the next 10 ms frame. True if it completes the Wake word.
    fn push(&mut self, frame: &[f32]) -> bool;
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
}

/// Opens the microphone and starts the DSP thread, which reframes capture to
/// 10 ms, listens for the Wake word with `wake_word` if there is one, and
/// cuts capture into utterances with `vad`. Capture stops once the speech
/// receiver is dropped.
pub fn listen(
    io: Box<dyn AudioIo + Send>,
    audio: &Audio,
    vad: Box<dyn Vad>,
    wake_word: Option<Box<dyn WakeWord>>,
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
    let lost = Arc::new(lost);
    let endpointer = Endpointer::new(vad, endpointing);
    let listeners = Listeners {
        wake_word,
        endpointer,
        wakes,
        speech,
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
    };
    Ok(Ears {
        hearing,
        speech: speech_rx,
        wake_word: wakes_rx,
    })
}

/// What the DSP thread hands each frame to, and where they report.
struct Listeners {
    wake_word: Option<Box<dyn WakeWord>>,
    endpointer: Endpointer,
    wakes: mpsc::UnboundedSender<()>,
    speech: mpsc::UnboundedSender<SpeechSegment>,
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
}

/// A change the DSP thread picks up before its next frame.
enum Change {
    Mic(Box<dyn Send>, rtrb::Consumer<f32>),
    Endpointing(Endpointing),
}

impl Hearing {
    /// Applies `[audio]` and `[endpointing]` as read at a Session start,
    /// resolving the devices by name. A pinned device that isn't there gives
    /// way to the default, and the pin stays for the next Session. A
    /// microphone that can't be opened leaves the current one in use.
    pub fn configure(&mut self, audio: &Audio, endpointing: &Endpointing) -> Devices {
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
        Devices { speaker, missing }
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

    /// Resolves once the open microphone has gone away.
    pub async fn lost(&mut self) {
        // The sender lives as long as `self`.
        let _ = self.lost_rx.wait_for(|lost| *lost).await;
    }
}

/// The DSP thread. Echo cancellation (ticket 09) will take each frame here
/// before the listeners.
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
            }
        }
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
        // Heard before the utterance it starts ends, so the Session it
        // opens gets that utterance.
        if let Some(wake_word) = &mut listeners.wake_word {
            if wake_word.push(&frame) {
                let _ = listeners.wakes.send(());
            }
        }
        if let Some(segment) = listeners.endpointer.push(&frame) {
            let _ = listeners.speech.send(segment);
        }
    }
}
