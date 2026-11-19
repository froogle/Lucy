//! `lucy::settings`: what the TUI writes to `config.toml`, comments kept.
//! Also when lucyd takes each change up.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

use lucy::settings::{self, Applies, Change, Key};
use lucy_config::Tier;

const COMMENTED: &str = "# My Lucy, tuned by hand.\n\n\
[session]\n\
silence_timeout_secs = 45 # long enough for me\n\n\
[piper]\n\
voice = \"en_GB-cori-medium\" # her usual voice\n";

fn config(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

fn set(key: Key, value: &str) -> Change {
    Change::Set(key, value.to_owned())
}

#[test]
fn a_change_is_written_with_every_comment_kept() {
    let (_dir, path) = config(COMMENTED);

    settings::write(&path, &[set(Key::PiperVoice, "en_GB-cori-high")]).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# My Lucy, tuned by hand."), "{text}");
    assert!(text.contains("45 # long enough for me"), "{text}");
    assert!(
        text.contains("voice = \"en_GB-cori-high\" # her usual voice"),
        "{text}"
    );
}

#[test]
fn providers_tiers_speech_voice_and_devices_apply_at_the_next_session() {
    let (_dir, path) = config("");
    let changes = [
        set(Key::ProviderKind("codex-fast".into()), "codex"),
        set(Key::ProviderModel("codex-fast".into()), "gpt-5"),
        set(Key::Tier(Tier::Fast), "codex-fast"),
        set(Key::Stt, "superwhisper"),
        set(Key::PiperVoice, "en_GB-cori-high"),
        set(Key::Tts, "kokoro"),
        set(Key::KokoroVoice, "bm_george"),
        set(Key::Mic, "alsa_input.usb-Yeti"),
        set(Key::Speaker, "alsa_output.hdmi"),
    ];

    for change in &changes {
        assert_eq!(
            settings::write(&path, std::slice::from_ref(change)).unwrap(),
            Applies::NextSession,
            "{change:?}"
        );
    }

    let parsed = lucy_config::load(&path).unwrap();
    assert_eq!(parsed.tiers.fast, "codex-fast");
    assert_eq!(
        parsed.providers["codex-fast"].kind,
        lucy_config::Kind::Codex
    );
    assert_eq!(
        parsed.providers["codex-fast"].model.as_deref(),
        Some("gpt-5")
    );
    assert_eq!(parsed.stt.kind, lucy_config::SttKind::Superwhisper);
    assert_eq!(parsed.piper.voice, "en_GB-cori-high");
    assert_eq!(parsed.audio.mic, "alsa_input.usb-Yeti");
    assert_eq!(parsed.audio.speaker, "alsa_output.hdmi");
}

#[test]
fn whispers_model_stays_loaded_so_it_applies_after_a_restart() {
    let (_dir, path) = config("[whisper]\nmodel = \"small.en\" # on the GPU\n");

    assert_eq!(
        settings::write(&path, &[set(Key::WhisperModel, "base.en")]).unwrap(),
        Applies::Restart
    );
    assert_eq!(
        lucy_config::load(&path).unwrap().whisper.model.as_deref(),
        Some("base.en")
    );

    assert_eq!(
        settings::write(&path, &[Change::Unset(Key::WhisperModel)]).unwrap(),
        Applies::Restart
    );
    assert_eq!(lucy_config::load(&path).unwrap().whisper.model, None);
}

#[test]
fn changes_written_together_apply_when_the_latest_of_them_does() {
    let (_dir, path) = config("");

    let applies = settings::write(
        &path,
        &[set(Key::Stt, "whisper"), set(Key::WhisperModel, "base.en")],
    )
    .unwrap();

    assert_eq!(applies, Applies::Restart);
}
