//! The microphone side of the audio pipeline: capture, the DSP thread and
//! the VAD's endpointing, which together turn the microphone into
//! utterances for STT.

mod cpal;
mod endpoint;
mod livekit;
mod silero;

use std::fmt;
use std::time::Duration;

use lucy_config::{Audio, Endpointing};
use tokio::sync::mpsc;

pub use self::cpal::Cpal;
use crate::adapters::SpeechSegment;
use endpoint::Endpointer;
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
    /// the returned handle is dropped.
    fn capture(&self, mic: &str, ring: rtrb::Producer<f32>) -> Result<Box<dyn Send>, Error>;
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

/// Opens the microphone and starts the DSP thread, which reframes capture to
/// 10 ms and cuts it into utterances with `vad`. Capture stops once the
/// returned receiver is dropped.
pub fn listen(
    io: Box<dyn AudioIo + Send>,
    audio: &Audio,
    vad: Box<dyn Vad>,
    endpointing: &Endpointing,
) -> Result<(Hearing, mpsc::UnboundedReceiver<SpeechSegment>), Error> {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_LEN);
    let capture = io.capture(&audio.mic, producer)?;
    let (speech, speech_rx) = mpsc::unbounded_channel();
    let (changes, changes_rx) = std::sync::mpsc::channel();
    let endpointer = Endpointer::new(vad, endpointing);
    std::thread::Builder::new()
        .name("lucyd-dsp".to_owned())
        .spawn(move || dsp(capture, consumer, endpointer, changes_rx, speech))
        .map_err(|e| Error(format!("can't start the DSP thread: {e}")))?;
    let hearing = Hearing {
        io,
        audio: audio.clone(),
        endpointing: endpointing.clone(),
        changes,
    };
    Ok((hearing, speech_rx))
}

/// Changes the running DSP thread's microphone and endpointing.
pub struct Hearing {
    io: Box<dyn AudioIo + Send>,
    audio: Audio,
    endpointing: Endpointing,
    changes: std::sync::mpsc::Sender<Change>,
}

/// A change the DSP thread picks up before its next frame.
enum Change {
    Mic(Box<dyn Send>, rtrb::Consumer<f32>),
    Endpointing(Endpointing),
}

impl Hearing {
    /// Applies `[audio]` and `[endpointing]` as read at a Session start.
    /// A microphone that can't be opened leaves the current one in use.
    pub fn configure(&mut self, audio: &Audio, endpointing: &Endpointing) {
        // Ticket 11 re-resolves the devices and notifies about a missing
        // pin here.
        if *audio != self.audio {
            let (producer, consumer) = rtrb::RingBuffer::new(RING_LEN);
            match self.io.capture(&audio.mic, producer) {
                Ok(capture) => {
                    let _ = self.changes.send(Change::Mic(capture, consumer));
                    self.audio = audio.clone();
                }
                Err(e) => eprintln!("lucyd: can't open the microphone {}: {e}", audio.mic),
            }
        }
        if *endpointing != self.endpointing {
            let _ = self.changes.send(Change::Endpointing(endpointing.clone()));
            self.endpointing = endpointing.clone();
        }
    }
}

/// The DSP thread. Echo cancellation (ticket 09) and the wake word will
/// take each frame here before the endpointer.
fn dsp(
    mut _capture: Box<dyn Send>,
    mut mic: rtrb::Consumer<f32>,
    mut endpointer: Endpointer,
    changes: std::sync::mpsc::Receiver<Change>,
    speech: mpsc::UnboundedSender<SpeechSegment>,
) {
    let mut frame = [0.0; FRAME_LEN];
    while !speech.is_closed() {
        for change in changes.try_iter() {
            match change {
                Change::Mic(capture, consumer) => (_capture, mic) = (capture, consumer),
                Change::Endpointing(config) => endpointer.configure(&config),
            }
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
        if let Some(segment) = endpointer.push(&frame) {
            let _ = speech.send(segment);
        }
    }
}
