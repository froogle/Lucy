//! A Plugin's `manifest.toml` (ADR 0004).

use std::path::{Component, Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    /// Shown only; Approval uses the hash.
    #[allow(dead_code)] // Read once Approvals are spoken.
    pub version: String,
    #[allow(dead_code)] // Read once Lucy edits her own Plugins.
    pub author: Author,
    pub summary: String,
    /// The executable, relative to the Plugin's folder, then its arguments.
    pub command: Vec<String>,
    #[serde(default)]
    pub tools: Vec<ToolEntry>,
    #[serde(default)]
    pub access: Access,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Author {
    Lucy,
    Human,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolEntry {
    pub name: String,
    pub handles: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub confirm: bool,
    /// Said to ask for the Confirmation, with each `{arg}` filled in.
    pub confirm_prompt: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    /// Hosts: declared and shown, not enforced in v0.1.
    #[serde(default)]
    pub network: Vec<String>,
    /// Names only.
    #[serde(default)]
    pub credentials: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

impl Manifest {
    /// Parses and validates the Manifest of the Plugin in folder `folder_name`.
    pub fn parse(text: &str, folder_name: &str) -> Result<Manifest, String> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|e| format!("its Manifest doesn't parse: {e}"))?;
        manifest.validate(folder_name)?;
        Ok(manifest)
    }

    /// The executable, relative to the Plugin's folder.
    pub fn executable(&self) -> &Path {
        Path::new(&self.command[0])
    }

    fn validate(&self, folder_name: &str) -> Result<(), String> {
        if self.name != folder_name {
            return Err(format!(
                "its Manifest names it {:?} but its folder is {folder_name:?}",
                self.name
            ));
        }
        if self.name.is_empty() || self.name.contains("__") {
            return Err("its name must be non-empty and contain no \"__\"".into());
        }
        one_line("summary", &self.summary)?;
        let Some(executable) = self.command.first() else {
            return Err("its Manifest has an empty command".into());
        };
        let inside = Path::new(executable)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !inside {
            return Err(format!(
                "its command {executable:?} must be a path inside its folder"
            ));
        }
        let mut names = std::collections::HashSet::new();
        for tool in &self.tools {
            if !names.insert(&tool.name) {
                return Err(format!("it lists the tool {:?} twice", tool.name));
            }
            one_line(&format!("handles for {}", tool.name), &tool.handles)?;
            if tool.examples.len() > 3 {
                return Err(format!("its tool {} has more than 3 examples", tool.name));
            }
        }
        Ok(())
    }
}

fn one_line(what: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.contains('\n') {
        return Err(format!("its {what} must be one line"));
    }
    Ok(())
}
