use std::path::Path;
use std::process::Command;

use super::manifest::{Author, Manifest};
use super::Host;
use crate::sync::Locked;

impl Host {
    pub fn may_author(&self, name: &str) -> Result<(), String> {
        let Some(folder) = self.folders().remove(name) else {
            return Ok(());
        };
        if self.is_shipped(&folder) {
            return Err(format!("a Plugin called {name} ships with Lucy"));
        }
        let manifest = std::fs::read_to_string(folder.join("manifest.toml"))
            .ok()
            .and_then(|text| Manifest::parse(&text, name).ok());
        match manifest {
            Some(manifest) if matches!(manifest.author, Author::Lucy) => Ok(()),
            _ => Err(format!(
                "there's already a Plugin called {name} that Lucy didn't write"
            )),
        }
    }

    pub fn install_authored(&self, name: &str, staging: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(staging.join("manifest.toml"))
            .map_err(|e| format!("Its Manifest can't be read: {e}"))?;
        let manifest = Manifest::parse(&text, name).map_err(|e| format!("In the Plugin, {e}"))?;
        if !matches!(manifest.author, Author::Lucy) {
            return Err("Its Manifest must say author = \"lucy\".".into());
        }
        self.may_author(name)?;
        let executable = manifest.executable();
        if !staging.join(executable).is_file() {
            return Err(format!(
                "Its executable {} wasn't built.",
                executable.display()
            ));
        }
        let folder = self.paths.user.join(name);
        let _editing = self.editing.locked();
        replace(&folder, staging, executable)
            .map_err(|e| format!("It couldn't be installed: {e}"))?;
        self.changes.publish();
        Ok(())
    }

    pub async fn check_authored(&self, name: &str) -> Result<(), String> {
        self.candidate(name)
            .await
            .map(drop)
            .map_err(|e| format!("The installed Plugin failed its check: {e}."))
    }
}

fn replace(folder: &Path, staging: &Path, executable: &Path) -> std::io::Result<()> {
    if folder.exists() {
        std::fs::remove_dir_all(folder)?;
    }
    std::fs::create_dir_all(folder)?;
    for entry in std::fs::read_dir(staging)? {
        let entry = entry?;
        if entry.file_name() != "target" {
            copy_in_child(&entry.path(), &folder.join(entry.file_name()))?;
        }
    }
    let target = folder.join(executable);
    if !target.exists() {
        std::fs::create_dir_all(target.parent().unwrap_or(folder))?;
        copy_in_child(&staging.join(executable), &target)?;
    }
    Ok(())
}

fn copy_in_child(from: &Path, to: &Path) -> std::io::Result<()> {
    let status = Command::new("cp").arg("-a").arg(from).arg(to).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "cp {} failed",
            from.display()
        )));
    }
    Ok(())
}
