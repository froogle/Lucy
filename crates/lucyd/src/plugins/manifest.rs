use std::path::{Component, Path};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub author: Author,
    pub summary: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub tools: Vec<ToolEntry>,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub requires: Requires,
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
    pub confirm_prompt: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub credentials: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

impl Manifest {
    pub fn parse(text: &str, folder_name: &str) -> Result<Manifest, String> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|e| format!("its Manifest doesn't parse: {e}"))?;
        manifest.validate(folder_name)?;
        Ok(manifest)
    }

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
        for path in self.access.read.iter().chain(&self.access.write) {
            if Path::new(path)
                .components()
                .any(|c| c == Component::ParentDir)
            {
                return Err(format!(
                    "its declared path {path:?} must not contain \"..\""
                ));
            }
        }
        for credential in &self.access.credentials {
            credential_name(credential)?;
        }
        for var in &self.requires.env {
            required_var(var)?;
        }
        for command in &self.requires.commands {
            if command.is_empty() || command.contains('/') {
                return Err(format!(
                    "its required command {command:?} must be a bare name, found on PATH"
                ));
            }
        }
        Ok(())
    }
}

fn credential_name(name: &str) -> Result<(), String> {
    if !is_env_name(name) {
        return Err(format!(
            "its credential {name:?} must be an env var name: uppercase letters, digits and \
             underscores, not starting with a digit"
        ));
    }
    if super::SYSTEM_ENV.contains(&name) || is_lucys(name) {
        return Err(format!(
            "its credential {name:?} names an env var Lucy or the system sets"
        ));
    }
    Ok(())
}

fn required_var(name: &str) -> Result<(), String> {
    if !is_env_name(name) || is_lucys(name) {
        return Err(format!(
            "its required env var {name:?} must be an env var name that Lucy doesn't set and \
             the loader doesn't read"
        ));
    }
    Ok(())
}

fn is_env_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_lucys(name: &str) -> bool {
    name.starts_with("LUCY_") || name.starts_with("LD_")
}

fn one_line(what: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.contains('\n') {
        return Err(format!("its {what} must be one line"));
    }
    Ok(())
}
