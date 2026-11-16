//! The `superwhisper` STT Provider: the user's own Superwhisper, through its
//! CLI. Each utterance is written as a 16 kHz mono WAV, handed over with
//! `superwhisper transcribe FILE`, and its result polled for with
//! `superwhisper read`. Never `record`, `start` or `stop`: they take the
//! microphone and paste into the focused window.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::adapters::{SpeechSegment, Stt, SttError};

const POLL_EVERY: Duration = Duration::from_millis(20);
const UTTERANCE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Superwhisper {
    program: PathBuf,
    dir: PathBuf,
    utterances: AtomicU64,
}

impl Superwhisper {
    /// Runs `program` as the CLI, writing utterances into `dir`.
    pub fn new(program: PathBuf, dir: PathBuf) -> Self {
        Superwhisper {
            program,
            dir,
            utterances: AtomicU64::new(0),
        }
    }

    fn run(&self, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, String> {
        let output = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("can't run {}: {e}", self.program.display()))?;
        if !output.status.success() {
            return Err(format!(
                "{} {} failed: {}",
                self.program.display(),
                args[0].to_string_lossy(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output.stdout)
    }

    fn transcribe_file(&self, file: &Path) -> Result<String, String> {
        let started: Started = parse(&self.run(&["transcribe".as_ref(), file.as_ref()])?)?;
        let session_id = match started {
            Started {
                ok: true,
                session_id: Some(id),
                ..
            } => id,
            Started { error, .. } => {
                return Err(format!(
                    "Superwhisper refused the utterance: {}",
                    error.unwrap_or_default()
                ))
            }
        };
        let deadline = Instant::now() + UTTERANCE_TIMEOUT;
        loop {
            let latest_if_any: Option<Dictation> = parse(&self.run(&["read".as_ref()])?)?;
            if let Some(dictation) = latest_if_any.filter(|d| d.session_id == Some(session_id)) {
                return dictation.raw_transcript();
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "Superwhisper gave no result for its session {session_id}"
                ));
            }
            std::thread::sleep(POLL_EVERY);
        }
    }
}

impl Stt for Superwhisper {
    fn transcribe(&self, segment: SpeechSegment) -> Result<String, SttError> {
        let n = self.utterances.fetch_add(1, Ordering::Relaxed);
        let file = self
            .dir
            .join(format!("superwhisper-{}-{n}.wav", std::process::id()));
        let result = write_16k_mono_wav(&file, &segment.samples)
            .map_err(|e| format!("can't write {}: {e}", file.display()))
            .and_then(|()| self.transcribe_file(&file));
        let _ = std::fs::remove_file(&file);
        result.map_err(SttError)
    }
}

#[derive(Deserialize)]
struct Started {
    ok: bool,
    session_id: Option<u64>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Dictation {
    session_id: Option<u64>,
    raw: Option<String>,
    text: Option<String>,
    error: Option<String>,
}

impl Dictation {
    fn raw_transcript(self) -> Result<String, String> {
        match (self.error, self.raw.or(self.text)) {
            (Some(error), _) => Err(format!("Superwhisper failed: {error}")),
            (None, Some(text)) => Ok(text.trim().to_owned()),
            (None, None) => Err("Superwhisper's result has no transcript".to_owned()),
        }
    }
}

fn parse<'a, T: Deserialize<'a>>(stdout: &'a [u8]) -> Result<T, String> {
    serde_json::from_slice(stdout).map_err(|e| {
        format!(
            "can't read Superwhisper's reply ({e}): {}",
            String::from_utf8_lossy(stdout).trim()
        )
    })
}

fn write_16k_mono_wav(path: &Path, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::audio::SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
    }
    writer.finalize()
}
