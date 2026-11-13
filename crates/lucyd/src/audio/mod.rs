//! The microphone side of the audio pipeline: capture, the DSP thread and
//! the VAD's endpointing, which together turn the microphone into
//! utterances for STT.

mod endpoint;
mod silero;

use std::fmt;
use std::time::Duration;

use lucy_config::{Audio, Endpointing};
use tokio::sync::mpsc;

use crate::adapters::SpeechSegment;
use endpoint::Endpointer;
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
    io: &dyn AudioIo,
    audio: &Audio,
    vad: Box<dyn Vad>,
    endpointing: &Endpointing,
) -> Result<mpsc::UnboundedReceiver<SpeechSegment>, Error> {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_LEN);
    let capture = io.capture(&audio.mic, producer)?;
    let (speech, speech_rx) = mpsc::unbounded_channel();
    let endpointer = Endpointer::new(vad, endpointing);
    std::thread::Builder::new()
        .name("lucyd-dsp".to_owned())
        .spawn(move || dsp(capture, consumer, endpointer, speech))
        .map_err(|e| Error(format!("can't start the DSP thread: {e}")))?;
    Ok(speech_rx)
}

/// The DSP thread. Echo cancellation (ticket 09) and the wake word will
/// take each frame here before the endpointer.
fn dsp(
    _capture: Box<dyn Send>,
    mut mic: rtrb::Consumer<f32>,
    mut endpointer: Endpointer,
    speech: mpsc::UnboundedSender<SpeechSegment>,
) {
    let mut frame = [0.0; FRAME_LEN];
    while !speech.is_closed() {
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
