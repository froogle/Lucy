use std::io;
use std::path::{Path, PathBuf};

use livekit_wakeword::WakeWordModel;

use super::{Error, WakeWord, SAMPLE_RATE};

use lucy_config::BUNDLED_WAKE_MODEL as BUNDLED_NAME;
const BUNDLED: &[u8] = include_bytes!("../../models/wake/hey_lucy.onnx");

/// Writes the bundled model into `dir`, unless it is already there, and returns its path.
///
/// The engine loads models from files only. The model is written whole or
/// not at all.
///
/// # Errors
///
/// When the model can't be written.
pub fn install_bundled_wake_model(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(BUNDLED_NAME);
    let installed = std::fs::read(&path).is_ok_and(|model| model == BUNDLED);
    if !installed {
        std::fs::create_dir_all(dir)?;
        let temp = path.with_extension("onnx.new");
        std::fs::write(&temp, BUNDLED)?;
        std::fs::rename(&temp, &path)?;
    }
    Ok(path)
}

pub struct LiveKit {
    model: WakeWordModel,
    threshold: f32,
    score: f32,
    pcm: Vec<i16>,
}

impl LiveKit {
    /// Loads the model at `path`, which hears its phrase once it scores at
    /// least `threshold`.
    ///
    /// # Errors
    ///
    /// When the model can't be loaded.
    pub fn load(path: &Path, threshold: f32) -> Result<LiveKit, Error> {
        let model = WakeWordModel::new(&[path], SAMPLE_RATE)
            .map_err(|e| Error(format!("can't load the wake word {}: {e}", path.display())))?;
        Ok(LiveKit {
            model,
            threshold,
            score: 0.0,
            pcm: Vec::new(),
        })
    }
}

impl LiveKit {
    fn forget_this_utterance(&mut self) {
        if let Err(e) = self.model.reset() {
            eprintln!("lucyd: can't reset the wake word: {e}");
        }
    }
}

impl WakeWord for LiveKit {
    fn push(&mut self, frame: &[f32]) -> bool {
        self.pcm.clear();
        self.pcm
            .extend(frame.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16));
        self.score = match self.model.predict(&self.pcm) {
            Ok(scores) => scores.values().copied().fold(0.0, f32::max),
            Err(e) => {
                eprintln!("lucyd: the wake word failed: {e}");
                0.0
            }
        };
        let heard = self.score >= self.threshold;
        if heard {
            self.forget_this_utterance();
        }
        heard
    }

    fn score(&self) -> f32 {
        self.score
    }
}
