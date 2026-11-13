//! The `whisper` STT Provider.

mod harness;
mod recording;

use std::path::PathBuf;
use std::time::Duration;

use lucy_config::Whisper as Config;
use lucyd::adapters::{SpeechSegment, Stt};
use lucyd::whisper::Whisper;

#[test]
fn a_missing_model_is_reported_with_its_path() {
    let dir = tempfile::tempdir().unwrap();

    let gpu = Whisper::load(
        &Config {
            gpu: Some(true),
            model: None,
        },
        dir.path(),
    )
    .err()
    .unwrap();
    let cpu = Whisper::load(
        &Config {
            gpu: Some(false),
            model: None,
        },
        dir.path(),
    )
    .err()
    .unwrap();
    let named = Whisper::load(
        &Config {
            gpu: Some(true),
            model: Some("large-v3-turbo-q5_0".to_owned()),
        },
        dir.path(),
    )
    .err()
    .unwrap();

    let expect = |error: lucyd::whisper::Error, file: &str| {
        let path = dir.path().join(file);
        assert!(
            error.to_string().contains(&path.display().to_string()),
            "{error}"
        );
    };
    // small.en on the GPU, base.en on the CPU, unless one is named.
    expect(gpu, "ggml-small.en.bin");
    expect(cpu, "ggml-base.en.bin");
    expect(named, "ggml-large-v3-turbo-q5_0.bin");
}

/// Where the voice-loop prototype keeps its models, unless
/// `LUCY_WHISPER_MODELS` names another directory.
fn models_dir() -> PathBuf {
    std::env::var_os("LUCY_WHISPER_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.scratch/lucy-v0-1/prototypes/voice-loop/models")
        })
}

fn transcribe(config: Config) -> String {
    let whisper = Whisper::load(&config, &models_dir()).expect("the model loads");
    whisper.transcribe(SpeechSegment {
        samples: recording::speech(),
    })
}

#[test]
#[ignore = "needs the small.en model and a Vulkan GPU"]
fn small_en_on_the_gpu_transcribes_speech() {
    let heard = transcribe(Config {
        gpu: Some(true),
        model: None,
    });
    assert!(heard.to_lowercase().contains("lucy"), "{heard:?}");
}

#[test]
#[ignore = "needs the base.en model"]
fn base_en_on_the_cpu_transcribes_speech() {
    let heard = transcribe(Config {
        gpu: Some(false),
        model: None,
    });
    assert!(heard.to_lowercase().contains("lucy"), "{heard:?}");
}

#[test]
#[ignore = "needs the base.en model"]
fn a_long_utterance_on_the_cpu_is_transcribed_to_the_end() {
    let whisper = Whisper::load(
        &Config {
            gpu: Some(false),
            model: None,
        },
        &models_dir(),
    )
    .unwrap();
    let speech = recording::speech();
    let samples = [speech.clone(), speech.clone(), speech].concat();
    assert!(
        samples.len() > 16_000 * 8,
        "longer than the CPU's 7.68 s context"
    );

    let heard = whisper.transcribe(SpeechSegment { samples });

    assert_eq!(heard.to_lowercase().matches("lucy").count(), 3, "{heard:?}");
}

/// The whole hearing path with the real VAD and Whisper: the recording,
/// played into the microphone between silences, comes out as one utterance
/// that Whisper transcribes.
#[test]
#[ignore = "needs the small.en model and a Vulkan GPU"]
fn a_recording_played_into_the_microphone_is_heard() {
    let whisper = Whisper::load(&Config::default(), &models_dir()).unwrap();
    let config = lucy_config::Config::default();
    let mic = harness::FakeMic::default();
    let mut ears = lucyd::audio::listen(
        Box::new(mic.clone()),
        &config.audio,
        Box::new(lucyd::audio::Silero::load().unwrap()),
        None,
        &config.endpointing,
    )
    .unwrap();

    mic.feed(harness::SILENCE, Duration::from_secs(1));
    mic.play(&recording::speech());
    mic.feed(harness::SILENCE, Duration::from_secs(1));

    let segment = ears.speech.blocking_recv().expect("an utterance");
    let heard = whisper.transcribe(segment);
    assert!(heard.to_lowercase().contains("lucy"), "{heard:?}");
}
