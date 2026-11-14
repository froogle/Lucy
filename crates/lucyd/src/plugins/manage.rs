//! Managing installed Plugins: enable, disable, rollback and remove by
//! voice through `manage_plugin`, and listing, enabling and revoking over
//! the public socket. Each change applies to the open Session at once and is
//! announced as `plugins.changed`.

use std::fmt;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use lucy_ipc::{ApprovalInfo, ApprovalsList, PluginAuthor, PluginInfo, PluginStatus, PluginsList};
use rmcp::model::{CallToolResult, JsonObject, Tool};

use super::manifest::{Author, Manifest};
use super::{
    broken_message, content_hash, Contents, Host, Scanned, SessionTools, Usable, DECLINED,
};
use crate::adapters::{Notification, NotificationKind};
use crate::mcp;
use crate::runner;
use crate::session::{Answer, Expects, Question};

/// Why a Plugin couldn't be changed.
#[derive(Debug)]
pub enum Refusal {
    NoSuchPlugin(String),
    NotApproved(String),
    /// Lucy changes only her own Plugins.
    WrittenByHuman(String),
    NoPreviousVersion(String),
    /// The kept previous version no longer matches its Approval.
    PreviousVersionDamaged(String),
    NotRestored(String, std::io::Error),
    Shipped(String),
    NotDeleted(String, std::io::Error),
    Broken {
        plugin: String,
        reason: String,
    },
    /// `approvals.toml` or `plugins.toml` couldn't be read or saved.
    Records(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoSuchPlugin(name) => write!(f, "There's no Plugin called {name}."),
            Refusal::NotApproved(name) => write!(f, "The {name} Plugin has no Approval."),
            Refusal::WrittenByHuman(name) => write!(
                f,
                "The {name} Plugin was written by a human, so Lucy won't change its files. Its \
                 author can roll it back."
            ),
            Refusal::NoPreviousVersion(name) => write!(
                f,
                "No previous approved version of the {name} Plugin is kept."
            ),
            Refusal::PreviousVersionDamaged(name) => write!(
                f,
                "The previous version of the {name} Plugin wasn't kept intact."
            ),
            Refusal::NotRestored(name, e) => write!(
                f,
                "The previous version of the {name} Plugin couldn't be put back: {e}"
            ),
            Refusal::Shipped(name) => write!(
                f,
                "The {name} Plugin was shipped with Lucy, so it can't be removed, but it can be \
                 disabled."
            ),
            Refusal::NotDeleted(name, e) => {
                write!(f, "The {name} Plugin couldn't be deleted: {e}")
            }
            Refusal::Broken { plugin, reason } => write!(f, "{}", broken_message(plugin, reason)),
            Refusal::Records(reason) => {
                write!(
                    f,
                    "Lucy's Plugin records couldn't be read or saved: {reason}"
                )
            }
        }
    }
}

pub fn manage_plugin_tool() -> Tool {
    mcp::tool(
        "manage_plugin",
        "Enable, disable, roll back or remove one of your Plugins when the user asks. Disabling \
         keeps its files and Approval, and enabling brings it straight back. Rollback returns it \
         to its previous approved version. Remove deletes it after asking the user; Plugins \
         shipped with Lucy can be disabled but not removed.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "plugin": {"type": "string", "description": "The Plugin's name"},
                "action": {"type": "string", "enum": ["enable", "disable", "rollback", "remove"]},
            },
            "required": ["plugin", "action"],
        }),
    )
}

impl SessionTools {
    /// `manage_plugin`, called by the Session's model.
    pub async fn manage(&self, arguments: &JsonObject) -> CallToolResult {
        let plugin = arguments.get("plugin").and_then(|v| v.as_str());
        let action = arguments.get("action").and_then(|v| v.as_str());
        let (plugin, action) = (plugin.unwrap_or_default(), action.unwrap_or_default());
        let host = &self.host;
        let outcome = match action {
            "enable" => host
                .set_enabled(plugin, true)
                .await
                .map(|()| format!("The {plugin} Plugin is enabled.")),
            "disable" => host.set_enabled(plugin, false).await.map(|()| {
                format!("The {plugin} Plugin is disabled. Its files and Approval are kept.")
            }),
            "rollback" => host
                .rollback(plugin)
                .await
                .map(|()| format!("The {plugin} Plugin is back to its previous approved version.")),
            "remove" => {
                if let Err(refusal) = host.removable(plugin) {
                    return mcp::error(refusal.to_string());
                }
                if !host.confirm_removal(plugin).await {
                    return mcp::text(DECLINED);
                }
                host.remove(plugin)
                    .await
                    .map(|()| format!("The {plugin} Plugin is removed."))
            }
            _ => return mcp::error("action must be enable, disable, rollback or remove."),
        };
        match outcome {
            Ok(done) => mcp::text(done),
            Err(refusal) => mcp::error(refusal.to_string()),
        }
    }
}

impl Host {
    /// `plugins.list`.
    pub fn list(&self) -> PluginsList {
        let disabled = self.prefs().disabled;
        let plugins = self
            .folders()
            .into_iter()
            .map(|(name, folder)| {
                let (manifest, status, problem) = match self.scan(&name, &folder) {
                    Ok(Scanned::Approved(plugin)) => match self.verdict(&plugin.hash) {
                        Some(Err(reason)) => {
                            (Some(plugin.manifest), PluginStatus::Broken, Some(reason))
                        }
                        _ => (Some(plugin.manifest), PluginStatus::Approved, None),
                    },
                    Ok(Scanned::Awaiting(contents)) => (
                        Some(contents.manifest),
                        PluginStatus::AwaitingApproval,
                        None,
                    ),
                    Err(reason) => (None, PluginStatus::Broken, Some(reason)),
                };
                PluginInfo {
                    shipped: self.is_shipped(&folder),
                    enabled: !disabled.contains(&name),
                    name,
                    version: manifest.as_ref().map(|m| m.version.clone()),
                    author: manifest.as_ref().map(|m| match m.author {
                        Author::Lucy => PluginAuthor::Lucy,
                        Author::Human => PluginAuthor::Human,
                    }),
                    credentials: manifest
                        .as_ref()
                        .map(|m| m.access.credentials.clone())
                        .unwrap_or_default(),
                    summary: manifest.map(|m| m.summary),
                    status,
                    problem,
                }
            })
            .collect();
        PluginsList { plugins }
    }

    /// `approvals.list`.
    pub fn approval_list(&self) -> Result<ApprovalsList, Refusal> {
        let approvals = self
            .approvals()
            .map_err(Refusal::Records)?
            .plugins
            .into_iter()
            .map(|(plugin, approval)| ApprovalInfo {
                plugin,
                hash: approval.hash,
                previous_hash: approval.previous.map(|previous| previous.hash),
            })
            .collect();
        Ok(ApprovalsList { approvals })
    }

    /// Leaves the Plugin `name` out of selection, keeping its files and
    /// Approval, or brings it back.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), Refusal> {
        self.edit(name, || {
            self.folder(name)?;
            let mut prefs = self.prefs();
            if enabled {
                prefs.disabled.remove(name);
            } else {
                prefs.disabled.insert(name.to_owned());
            }
            self.save_prefs(&prefs).map_err(Refusal::Records)
        })
        .await
    }

    /// Withdraws the current Approval of the Plugin `name`, so it awaits
    /// Approval again. Its previous approved version is kept, to roll back
    /// to once it is approved again.
    pub async fn revoke(&self, name: &str) -> Result<(), Refusal> {
        self.edit(name, || {
            let mut approvals = self.approvals().map_err(Refusal::Records)?;
            let Some(approval) = approvals.plugins.remove(name) else {
                return Err(Refusal::NotApproved(name.to_owned()));
            };
            match approval.previous {
                Some(previous) => {
                    self.prune_versions(name, &[&previous.hash]);
                    approvals.revoked.insert(name.to_owned(), *previous);
                }
                None => {
                    let _ = std::fs::remove_dir_all(self.paths.versions(name));
                }
            }
            self.save_approvals(&approvals).map_err(Refusal::Records)
        })
        .await
    }

    /// Puts back the Plugin's previous approved version and its Approval,
    /// with no new Approval: that exact content was approved before. Lucy
    /// changes only her own Plugins.
    async fn rollback(&self, name: &str) -> Result<(), Refusal> {
        self.edit(name, || {
            let folder = self.folder(name)?;
            let current = Contents::read(name, &folder).map_err(|reason| Refusal::Broken {
                plugin: name.to_owned(),
                reason,
            })?;
            if !matches!(current.manifest.author, Author::Lucy) {
                return Err(Refusal::WrittenByHuman(name.to_owned()));
            }
            let mut approvals = self.approvals().map_err(Refusal::Records)?;
            let Some(mut approval) = approvals.plugins.remove(name) else {
                return Err(Refusal::NotApproved(name.to_owned()));
            };
            let Some(mut previous) = approval.previous.take() else {
                return Err(Refusal::NoPreviousVersion(name.to_owned()));
            };
            let kept = self.paths.versions(name).join(&previous.hash);
            let damaged = || Refusal::PreviousVersionDamaged(name.to_owned());
            let (text, executable) = std::fs::read_to_string(kept.join("manifest.toml"))
                .and_then(|text| Ok((text, std::fs::read(kept.join("executable"))?)))
                .ok()
                .filter(|(text, executable)| {
                    content_hash(text.as_bytes(), executable) == previous.hash
                })
                .ok_or_else(damaged)?;
            let manifest = Manifest::parse(&text, name).map_err(|_| damaged())?;
            let restore = || -> std::io::Result<()> {
                replace_file(&folder.join(manifest.executable()), &executable, 0o755)?;
                replace_file(&folder.join("manifest.toml"), text.as_bytes(), 0o644)
            };
            restore().map_err(|e| Refusal::NotRestored(name.to_owned(), e))?;
            previous.previous = Some(Box::new(approval));
            approvals.plugins.insert(name.to_owned(), *previous);
            self.save_approvals(&approvals).map_err(Refusal::Records)
        })
        .await
    }

    /// Whether the Plugin `name` can be removed: it exists and wasn't
    /// shipped with Lucy.
    fn removable(&self, name: &str) -> Result<(), Refusal> {
        if self.is_shipped(&self.folder(name)?) {
            return Err(Refusal::Shipped(name.to_owned()));
        }
        Ok(())
    }

    /// Asks the user's spoken yes to removing the Plugin `name`.
    async fn confirm_removal(&self, name: &str) -> bool {
        let question = Question {
            prompt: format!(
                "Shall I remove the {name} Plugin? Its files and its Approval will be deleted."
            ),
            expects: Expects::Yes,
        };
        let shown = Notification {
            summary: "Lucy needs your yes".into(),
            body: format!("manage_plugin\nplugin: {name}\naction: remove"),
            action: None,
            kind: NotificationKind::Details,
        };
        runner::ask(&self.commands, question, Some(shown)).await == Some(Answer::Yes)
    }

    /// Deletes the Plugin's folder, its Approval and what Lucy remembers
    /// of it.
    async fn remove(&self, name: &str) -> Result<(), Refusal> {
        self.edit(name, || {
            self.removable(name)?;
            let folder = self.paths.user.join(name);
            std::fs::remove_dir_all(&folder)
                .map_err(|e| Refusal::NotDeleted(name.to_owned(), e))?;
            let mut approvals = self.approvals().map_err(Refusal::Records)?;
            let approved = approvals.plugins.remove(name).is_some();
            if approvals.revoked.remove(name).is_some() || approved {
                self.save_approvals(&approvals).map_err(Refusal::Records)?;
            }
            let _ = std::fs::remove_dir_all(self.paths.versions(name));
            let mut prefs = self.prefs();
            prefs.disabled.remove(name);
            prefs.mentioned.remove(name);
            self.save_prefs(&prefs).map_err(Refusal::Records)
        })
        .await
    }

    /// Makes `change` to the Plugin `name` while holding the editing lock,
    /// then announces it and applies it to the open Session.
    async fn edit(
        &self,
        name: &str,
        change: impl FnOnce() -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        {
            let _editing = self.editing.lock().unwrap();
            change()?;
        }
        self.changed(name).await;
        Ok(())
    }

    /// The folder of the Plugin `name`.
    fn folder(&self, name: &str) -> Result<PathBuf, Refusal> {
        self.folders()
            .remove(name)
            .ok_or_else(|| Refusal::NoSuchPlugin(name.to_owned()))
    }

    /// Keeps a copy of an approved version of the Plugin `name`, to roll
    /// back to.
    pub(super) fn keep_version(
        &self,
        name: &str,
        hash: &str,
        manifest: &str,
        executable: &[u8],
    ) -> std::io::Result<()> {
        let dir = self.paths.versions(name).join(hash);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        std::fs::write(dir.join("manifest.toml"), manifest)?;
        std::fs::write(dir.join("executable"), executable)
    }

    /// Deletes every kept version of the Plugin `name` but those in `keep`.
    pub(super) fn prune_versions(&self, name: &str, keep: &[&str]) {
        let Ok(entries) = std::fs::read_dir(self.paths.versions(name)) else {
            return;
        };
        for entry in entries.flatten() {
            if !keep.iter().any(|hash| entry.file_name() == *hash) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Announces a change to the Plugin `name`, and brings the open
    /// Session's tools in line with it.
    async fn changed(&self, name: &str) {
        self.changes.publish();
        let Some(session) = self.session.lock().unwrap().upgrade() else {
            return;
        };
        match self.usable(name).await {
            Some(plugin) => session.replace(plugin).await,
            None => session.remove(name).await,
        }
    }

    /// The Plugin `name`, if it is enabled, approved and works.
    async fn usable(&self, name: &str) -> Option<Usable> {
        if self.prefs().disabled.contains(name) {
            return None;
        }
        let folder = self.folders().remove(name)?;
        let Ok(Scanned::Approved(plugin)) = self.scan(name, &folder) else {
            return None;
        };
        self.check(&plugin).await.ok()?;
        Some(plugin)
    }
}

/// Writes `bytes` to `path` with `mode`, replacing it all at once, so a
/// running copy of it is never written to. A child process writes it: a
/// write fd held here could leak into a Plugin lucyd forks meanwhile and
/// make running the file fail with "Text file busy".
fn replace_file(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".rollback");
    let mut writer = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\"", "sh"])
        .arg(&temp)
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    let written = writer.stdin.take().expect("piped").write_all(bytes);
    let status = writer.wait()?;
    written?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "writing it failed: {status}"
        )));
    }
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&temp, path)
}
