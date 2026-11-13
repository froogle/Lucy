//! The user's news preferences, kept in `preferences.toml` in the Plugin's
//! private state dir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalogue::DEFAULT_ROTATION;

/// How long a briefing is.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Length {
    /// A greeting and the top stories in a sentence or two.
    Brief,
    /// A roundup of about two minutes.
    Detailed,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    /// Source ids read for a briefing.
    pub rotation: Vec<String>,
    /// Source ids the user likes but Lucy can't read: their front pages are
    /// opened beside the briefing.
    pub open_beside: Vec<String>,
    /// Source ids the user doesn't want.
    pub avoid: Vec<String>,
    /// Topics to give more of.
    pub more_of: Vec<String>,
    /// Topics to give less of.
    pub less_of: Vec<String>,
    /// How many top stories a brief briefing speaks.
    pub stories: u8,
    pub length: Length,
}

impl Default for Preferences {
    fn default() -> Preferences {
        Preferences {
            rotation: DEFAULT_ROTATION.iter().map(|id| (*id).to_owned()).collect(),
            open_beside: Vec::new(),
            avoid: Vec::new(),
            more_of: Vec::new(),
            less_of: Vec::new(),
            stories: 2,
            length: Length::Brief,
        }
    }
}

/// Where the preferences live.
pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("preferences.toml")
}

impl Preferences {
    /// The saved preferences, or the defaults if there are none yet.
    pub fn load(state_dir: &Path) -> Result<Preferences, String> {
        match std::fs::read_to_string(path(state_dir)) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("preferences.toml: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(e) => Err(format!("preferences.toml can't be read: {e}")),
        }
    }

    pub fn save(&self, state_dir: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| format!("preferences.toml: {e}"))?;
        let path = path(state_dir);
        let temp = path.with_extension("toml.new");
        std::fs::write(&temp, text)
            .and_then(|()| std::fs::rename(&temp, &path))
            .map_err(|e| format!("preferences.toml can't be saved: {e}"))
    }
}
