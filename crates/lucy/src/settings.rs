//! The settings `lucy` changes in `config.toml`, and when lucyd takes each up.
//! They are written one at a time with every comment kept.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use lucy_config::{SttKind, Tier};
use serde::Serialize;

/// When lucyd takes up a change to `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Applies {
    /// lucyd reads it again at each Session start.
    NextSession,
    /// lucyd reads it only when it starts.
    Restart,
}

impl fmt::Display for Applies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Applies::NextSession => write!(f, "Applies at the next Session."),
            Applies::Restart => write!(f, "Applies after restarting lucyd."),
        }
    }
}

/// A setting `lucy` writes. Every other setting is file-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// `[providers.<name>] kind`.
    ProviderKind(String),
    /// `[providers.<name>] model`.
    ProviderModel(String),
    /// The Provider a Model tier thinks with.
    Tier(Tier),
    /// The STT Provider.
    Stt,
    /// Whisper's model; unset, lucyd sizes it to the computer.
    WhisperModel,
    /// The TTS Provider.
    Tts,
    /// Piper's voice.
    PiperVoice,
    /// Kokoro's voice.
    KokoroVoice,
    Mic,
    Speaker,
}

impl Key {
    pub fn applies(&self) -> Applies {
        match self {
            Key::WhisperModel => Applies::Restart,
            Key::ProviderKind(_)
            | Key::ProviderModel(_)
            | Key::Tier(_)
            | Key::Stt
            | Key::Tts
            | Key::PiperVoice
            | Key::KokoroVoice
            | Key::Mic
            | Key::Speaker => Applies::NextSession,
        }
    }

    fn path(&self) -> Vec<String> {
        match self {
            Key::ProviderKind(name) => vec!["providers".into(), name.clone(), "kind".into()],
            Key::ProviderModel(name) => vec!["providers".into(), name.clone(), "model".into()],
            Key::Tier(tier) => vec!["tiers".into(), tier.to_string()],
            Key::Stt => vec!["stt".into(), "kind".into()],
            Key::WhisperModel => vec!["whisper".into(), "model".into()],
            Key::Tts => vec!["tts".into(), "kind".into()],
            Key::PiperVoice => vec!["piper".into(), "voice".into()],
            Key::KokoroVoice => vec!["kokoro".into(), "voice".into()],
            Key::Mic => vec!["audio".into(), "mic".into()],
            Key::Speaker => vec!["audio".into(), "speaker".into()],
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path().join("."))
    }
}

impl FromStr for Key {
    type Err = String;

    fn from_str(name: &str) -> Result<Key, String> {
        let path: Vec<&str> = name.split('.').collect();
        let key = match path[..] {
            ["providers", provider, "kind"] => Key::ProviderKind(provider.to_owned()),
            ["providers", provider, "model"] => Key::ProviderModel(provider.to_owned()),
            ["tiers", "fast"] => Key::Tier(Tier::Fast),
            ["tiers", "advanced"] => Key::Tier(Tier::Advanced),
            ["stt", "kind"] => Key::Stt,
            ["whisper", "model"] => Key::WhisperModel,
            ["tts", "kind"] => Key::Tts,
            ["piper", "voice"] => Key::PiperVoice,
            ["kokoro", "voice"] => Key::KokoroVoice,
            ["audio", "mic"] => Key::Mic,
            ["audio", "speaker"] => Key::Speaker,
            _ => return Err(format!("lucy doesn't set {name}; edit config.toml for it")),
        };
        Ok(key)
    }
}

pub(crate) const WHISPER_MODELS: [(Option<&str>, &str); 3] = [
    (
        None,
        "Sized to this computer: small.en on a GPU, base.en on the CPU",
    ),
    (Some("small.en"), "small.en: more accurate, best on a GPU"),
    (Some("base.en"), "base.en: faster on the CPU"),
];

/// One change `lucy` makes to `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Set(Key, String),
    Unset(Key),
}

impl Change {
    fn key(&self) -> &Key {
        match self {
            Change::Set(key, _) | Change::Unset(key) => key,
        }
    }
}

pub const SPEECH: &str = "stt";

enum Speech {
    Superwhisper,
    Whisper(Option<String>),
}

impl Speech {
    fn changes(self) -> Vec<Change> {
        let kind = |kind: SttKind| Change::Set(Key::Stt, kind.config_name());
        match self {
            Speech::Superwhisper => vec![kind(SttKind::Superwhisper)],
            Speech::Whisper(Some(model)) => vec![
                kind(SttKind::Whisper),
                Change::Set(Key::WhisperModel, model),
            ],
            Speech::Whisper(None) => vec![kind(SttKind::Whisper), Change::Unset(Key::WhisperModel)],
        }
    }
}

/// What `lucy set <name> <value>` changes.
///
/// `name` is a [`Key`]'s dotted path or [`SPEECH`], whose value is
/// `superwhisper` or one of Whisper's models.
///
/// # Errors
///
/// If `lucy` doesn't set `name`.
pub fn parse_set(name: &str, value: String) -> Result<Vec<Change>, String> {
    let superwhisper = SttKind::Superwhisper.config_name();
    match name {
        SPEECH if value == superwhisper => Ok(Speech::Superwhisper.changes()),
        SPEECH => Ok(Speech::Whisper(Some(value)).changes()),
        name => Ok(vec![Change::Set(name.parse()?, value)]),
    }
}

/// What `lucy unset <name>` changes.
///
/// Unsetting [`SPEECH`] leaves Whisper hearing, with its model sized to the
/// computer.
///
/// # Errors
///
/// If `lucy` doesn't set `name`.
pub fn parse_unset(name: &str) -> Result<Vec<Change>, String> {
    match name {
        SPEECH => Ok(Speech::Whisper(None).changes()),
        name => Ok(vec![Change::Unset(name.parse()?)]),
    }
}

/// Makes `changes` in the config file at `config`, in order, keeping its
/// comments. Returns when lucyd takes them all up.
///
/// # Errors
///
/// If the file can't be read, parsed or written. The changes before the
/// one that failed stay written.
pub fn write(config: &Path, changes: &[Change]) -> Result<Applies, lucy_config::Error> {
    let mut applies = Applies::NextSession;
    for change in changes {
        let path = change.key().path();
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        match change {
            Change::Set(_, value) => lucy_config::set(config, &path, value.as_str())?,
            Change::Unset(_) => lucy_config::unset(config, &path)?,
        }
        applies = applies.max(change.key().applies());
    }
    Ok(applies)
}
