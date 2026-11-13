//! Lucy's configuration: types, defaults and loading.
//!
//! lucyd runs with no config file at all; every setting has a default here.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The whole of `config.toml`. Settings arrive with the tickets that use them.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub session: Session,
}

/// `[session]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Seconds of silence in Listening before the Session ends.
    pub silence_timeout_secs: f64,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            silence_timeout_secs: 20.0,
        }
    }
}

/// `$XDG_CONFIG_HOME/lucy/config.toml`, falling back to `~/.config`.
pub fn default_path() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
        .join("lucy")
        .join("config.toml")
}

/// The XDG base directory in `var`, or `~/<fallback>` if it is unset or empty.
pub fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(fallback)
        })
}

/// Loads the config at `path`. A missing file means all defaults.
pub fn load(path: &Path) -> Result<Config, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(Error::Read(path.to_owned(), e)),
    };
    toml::from_str(&text).map_err(|e| Error::Parse(path.to_owned(), e))
}

#[derive(Debug)]
pub enum Error {
    Read(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(path, e) => write!(f, "can't read {}: {e}", path.display()),
            Error::Parse(path, e) => write!(f, "can't parse {}: {e}", path.display()),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn silence_timeout_defaults_to_twenty_seconds_and_can_be_set() {
        assert_eq!(Config::default().session.silence_timeout_secs, 20.0);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[session]\nsilence_timeout_secs = 45\n").unwrap();
        assert_eq!(load(&path).unwrap().session.silence_timeout_secs, 45.0);
    }

    #[test]
    fn malformed_file_is_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is = = not toml").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("config.toml"), "{err}");
    }
}
