use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalogue::DEFAULT_ROTATION;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Length {
    Brief,
    Detailed,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub rotation: Vec<String>,
    pub open_beside: Vec<String>,
    pub avoid: Vec<String>,
    pub more_of: Vec<String>,
    pub less_of: Vec<String>,
    pub stories: u8,
    pub length: Length,
    pub found: Vec<Found>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub feed: String,
    pub front_page: String,
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
            found: Vec::new(),
        }
    }
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("preferences.toml")
}

impl Preferences {
    pub fn name_of(&self, id: &str) -> Option<String> {
        match crate::catalogue::by_id(id) {
            Some(source) => Some(source.name.to_owned()),
            None => self.found_by_id(id).map(|found| found.name.clone()),
        }
    }

    pub fn found_by_id(&self, id: &str) -> Option<&Found> {
        self.found.iter().find(|found| found.id == id)
    }

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
