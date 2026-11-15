//! The settings `lucy` changes in `config.toml`, written one at a time with
//! every comment kept, and when lucyd takes each one up.

use std::fmt;
use std::path::Path;

use lucy_config::Tier;

/// When lucyd takes up a change to `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
            // The model stays loaded from lucyd's start.
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

/// Sets `key` to `value` in the config file at `config`, keeping its
/// comments; when lucyd takes it up.
///
/// # Errors
///
/// If the file can't be read, parsed or written.
pub fn set(config: &Path, key: &Key, value: &str) -> Result<Applies, lucy_config::Error> {
    let path = key.path();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    lucy_config::set(config, &path, value)?;
    Ok(key.applies())
}

/// Removes `key` from the config file at `config`, so its default applies;
/// when lucyd takes that up.
///
/// # Errors
///
/// If the file can't be read, parsed or written.
pub fn unset(config: &Path, key: &Key) -> Result<Applies, lucy_config::Error> {
    let path = key.path();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    lucy_config::unset(config, &path)?;
    Ok(key.applies())
}
