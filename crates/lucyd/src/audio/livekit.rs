//! The wake-word engine: LiveKit WakeWord v2 through `livekit-wakeword`, on
//! the same tract runtime as the VAD. One fixed engine with a swappable
//! model file; it is not a Provider.
//!
//! The bundled model is LiveKit's `hey_livekit.onnx` (Apache-2.0), standing
//! in until the "Hey Lucy" and "Lucy" models are trained.

use std::io;
use std::path::{Path, PathBuf};

use livekit_wakeword::WakeWordModel;

use super::{Error, WakeWord, SAMPLE_RATE};

const BUNDLED_NAME: &str = "hey_livekit.onnx";
const BUNDLED: &[u8] = include_bytes!("../../models/wake/hey_livekit.onnx");

/// Writes the bundled model into `dir`, unless it is already there, and
/// returns its path: the engine loads models from files only.
pub fn install_bundled_wake_model(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(BUNDLED_NAME);
    let installed = std::fs::metadata(&path).is_ok_and(|m| m.len() == BUNDLED.len() as u64);
    if !installed {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, BUNDLED)?;
    }
    Ok(path)
}

pub struct LiveKit {
    model: WakeWordModel,
    threshold: f32,
    /// The frame as the engine takes it.
    pcm: Vec<i16>,
}

impl LiveKit {
    /// Loads the model at `path`, which hears its phrase once it scores at
    /// least `threshold`.
    pub fn load(path: &Path, threshold: f32) -> Result<LiveKit, Error> {
        let model = WakeWordModel::new(&[path], SAMPLE_RATE)
            .map_err(|e| Error(format!("can't load the wake word {}: {e}", path.display())))?;
        Ok(LiveKit {
            model,
            threshold,
            pcm: Vec::new(),
        })
    }
}

impl WakeWord for LiveKit {
    fn push(&mut self, frame: &[f32]) -> bool {
        self.pcm.clear();
        self.pcm
            .extend(frame.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16));
        let heard = match self.model.predict(&self.pcm) {
            Ok(scores) => scores.values().any(|&score| score >= self.threshold),
            Err(e) => {
                eprintln!("lucyd: the wake word failed: {e}");
                false
            }
        };
        // Start fresh, so one utterance of the phrase is heard once.
        if heard {
            if let Err(e) = self.model.reset() {
                eprintln!("lucyd: can't reset the wake word: {e}");
            }
        }
        heard
    }
}
