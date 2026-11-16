//! The `whisper` STT Provider: whisper.cpp through whisper-rs, with the
//! model loaded once and kept resident. Each utterance is transcribed whole
//! once the VAD has found its end.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::adapters::{SpeechSegment, Stt, SttError};

const CPU_AUDIO_CTX_FRAMES: usize = 384;
const ENCODER_FRAMES_PER_SEC: usize = 50;

pub struct Whisper {
    state: Mutex<whisper_rs::WhisperState>,
    gpu: bool,
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl Whisper {
    /// Loads the configured model from `models_dir`.
    ///
    /// # Errors
    ///
    /// When the model can't be found or loaded.
    pub fn load(config: &lucy_config::Whisper, models_dir: &Path) -> Result<Whisper, Error> {
        let gpu = uses_gpu(config, vulkan_device_found);
        let path = model_path(config, gpu, models_dir);
        if !path.is_file() {
            return Err(Error(format!(
                "the Whisper model {} is missing",
                path.display()
            )));
        }
        whisper_rs::install_logging_hooks();
        let mut params = WhisperContextParameters::default();
        params.use_gpu(gpu);
        let context = WhisperContext::new_with_params(&path, params)
            .map_err(|e| Error(format!("can't load {}: {e}", path.display())))?;
        let state = context
            .create_state()
            .map_err(|e| Error(format!("can't start Whisper: {e}")))?;
        Ok(Whisper {
            state: Mutex::new(state),
            gpu,
        })
    }

    fn run(&self, samples: &[f32]) -> Result<String, whisper_rs::WhisperError> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_single_segment(true);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_suppress_nst(true);
        if !self.gpu && fits_cpu_audio_ctx(samples.len()) {
            params.set_audio_ctx(CPU_AUDIO_CTX_FRAMES as i32);
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.full(params, samples)?;
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(&segment.to_str_lossy()?);
        }
        Ok(text.trim().to_owned())
    }
}

impl Stt for Whisper {
    fn transcribe(&self, segment: SpeechSegment) -> Result<String, SttError> {
        self.run(&segment.samples)
            .map_err(|e| SttError(format!("Whisper failed: {e}")))
    }
}

fn uses_gpu(config: &lucy_config::Whisper, detect: impl FnOnce() -> bool) -> bool {
    config.gpu.unwrap_or_else(detect)
}

fn vulkan_device_found() -> bool {
    !whisper_rs::vulkan::list_devices().is_empty()
}

fn model_path(config: &lucy_config::Whisper, gpu: bool, models_dir: &Path) -> PathBuf {
    let default = if gpu { "small.en" } else { "base.en" };
    let model = config.model.as_deref().unwrap_or(default);
    models_dir.join(format!("ggml-{model}.bin"))
}

fn fits_cpu_audio_ctx(samples: usize) -> bool {
    samples * ENCODER_FRAMES_PER_SEC <= CPU_AUDIO_CTX_FRAMES * crate::audio::SAMPLE_RATE as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_gpu_unset_whisper_runs_on_the_cpu_when_no_vulkan_device_is_found() {
        let unset = lucy_config::Whisper::default();

        assert!(!uses_gpu(&unset, || false));
        assert!(uses_gpu(&unset, || true));
        assert_eq!(
            model_path(&unset, uses_gpu(&unset, || false), Path::new("models")),
            Path::new("models/ggml-base.en.bin")
        );
    }

    #[test]
    fn a_configured_gpu_setting_wins_over_detection() {
        let gpu = |gpu| lucy_config::Whisper {
            gpu: Some(gpu),
            model: None,
        };

        assert!(uses_gpu(&gpu(true), || false));
        assert!(!uses_gpu(&gpu(false), || true));
    }
}
