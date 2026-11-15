//! The settings `lucy` changes in `config.toml`, written one at a time with
//! every comment kept, and when lucyd takes each one up.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use lucy_config::Tier;
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
    /// Piper's voice.
    Voice,
    Mic,
    Speaker,
}

impl Key {
    /// When lucyd takes up a change to this setting.
    pub fn applies(&self) -> Applies {
        match self {
            Key::WhisperModel => Applies::Restart,
            Key::ProviderKind(_)
            | Key::ProviderModel(_)
            | Key::Tier(_)
            | Key::Stt
            | Key::Voice
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
            Key::Voice => vec!["piper".into(), "voice".into()],
            Key::Mic => vec!["audio".into(), "mic".into()],
            Key::Speaker => vec!["audio".into(), "speaker".into()],
        }
    }
}

/// Its dotted path in `config.toml`, such as `tiers.fast`.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path().join("."))
    }
}

impl FromStr for Key {
    type Err = String;

    /// The setting at a dotted path, such as `audio.mic`; any other is
    /// file-only.
    fn from_str(name: &str) -> Result<Key, String> {
        let path: Vec<&str> = name.split('.').collect();
        let key = match path[..] {
            ["providers", provider, "kind"] => Key::ProviderKind(provider.to_owned()),
            ["providers", provider, "model"] => Key::ProviderModel(provider.to_owned()),
            ["tiers", "fast"] => Key::Tier(Tier::Fast),
            ["tiers", "advanced"] => Key::Tier(Tier::Advanced),
            ["stt", "kind"] => Key::Stt,
            ["whisper", "model"] => Key::WhisperModel,
            ["piper", "voice"] => Key::Voice,
            ["audio", "mic"] => Key::Mic,
            ["audio", "speaker"] => Key::Speaker,
            _ => return Err(format!("lucy doesn't set {name}; edit config.toml for it")),
        };
        Ok(key)
    }
}

/// One change `lucy` makes to `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Set(Key, String),
    /// Removes the setting, so its default applies.
    Unset(Key),
}

impl Change {
    fn key(&self) -> &Key {
        match self {
            Change::Set(key, _) | Change::Unset(key) => key,
        }
    }
}

/// Makes `changes` in the config file at `config`, in order, keeping its
/// comments; when lucyd takes them all up.
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
