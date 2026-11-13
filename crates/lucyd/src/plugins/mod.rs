//! The Plugin host (ADRs 0002, 0004, 0005): finds Plugins, keeps only the
//! approved, working ones, and runs each in its sandbox on first use in a
//! Session, forwarding the model's calls to it over MCP stdio.

mod approval;
mod authored;
mod manage;
mod manifest;
pub(crate) mod sandbox;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ElicitRequestParams, ElicitResult,
    ElicitationAction, InputRequest, InputRequests, InputResponses, JsonObject, ProtocolVersion,
    Tool,
};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, Peer, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, RoleServer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::adapters::{Notification, NotificationKind};
use crate::mcp;
use crate::runner::{self, Command};
use crate::session::{Answer, Expects, Question};
use crate::status::ChangeBus;
pub use manage::{manage_plugin_tool, Refusal};
use manifest::{Manifest, ToolEntry};

/// How many mid-call questions one call may ask before it counts as broken.
const MAX_QUESTIONS: usize = 10;

/// What the model hears when the user didn't confirm a call.
const DECLINED: &str = "declined: the user didn't say yes, so nothing was done, and you've \
                        already told them so.";

/// What the model hears of a call a Barge-in cancelled.
const CANCELLED: &str = "cancelled: the user cut in, so the call was stopped.";

/// How long a Plugin gets to start and answer `tools/list`.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Where Plugins and Lucy's state live.
#[derive(Debug, Clone)]
pub struct Paths {
    /// User-installed and Lucy-written Plugins.
    pub user: PathBuf,
    /// Plugins shipped with Lucy. A user Plugin of the same name overrides one.
    pub shipped: PathBuf,
    /// Lucy's own state: `approvals.toml`, and each Plugin's private state
    /// dir under `plugins/<name>/`.
    pub state: PathBuf,
}

impl Paths {
    /// The standard locations, from the XDG environment.
    pub fn from_env() -> Paths {
        Paths {
            user: lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share")
                .join("lucy")
                .join("plugins"),
            shipped: PathBuf::from("/usr/share/lucy/plugins"),
            state: lucy_config::xdg_dir("XDG_STATE_HOME", ".local/state").join("lucy"),
        }
    }

    fn approvals(&self) -> PathBuf {
        self.state.join("approvals.toml")
    }

    /// The user's Plugin preferences.
    fn prefs(&self) -> PathBuf {
        self.state.join("plugins.toml")
    }

    /// Where the approved versions of the Plugin `plugin` are kept, one
    /// folder per hash, for rolling back to.
    fn versions(&self, plugin: &str) -> PathBuf {
        self.state.join("versions").join(plugin)
    }

    fn state_dir(&self, plugin: &str) -> PathBuf {
        self.state.join("plugins").join(plugin)
    }
}

/// `approvals.toml`.
#[derive(Default, Deserialize, Serialize)]
struct Approvals {
    #[serde(default)]
    plugins: BTreeMap<String, Approval>,
    /// By Plugin whose Approval was revoked: the previous version it had,
    /// kept to roll back to once the Plugin is approved again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    revoked: BTreeMap<String, Approval>,
}

#[derive(Deserialize, Serialize)]
struct Approval {
    /// The `content_hash` that was approved.
    hash: String,
    /// Each tool's input schema as approved, as JSON.
    schemas: BTreeMap<String, String>,
    /// The Manifest as approved, so an update can be told apart from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest: Option<String>,
    /// The Approval this one replaced, which the Plugin can be rolled back
    /// to with no new Approval. It has no `previous` of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Box<Approval>>,
}

/// `plugins.toml`: the user's Plugin preferences besides their Approvals,
/// and which Plugins they've been told await Approval.
#[derive(Default, Deserialize, Serialize)]
struct PluginPrefs {
    /// Plugins left out of selection, keeping their files and Approval.
    #[serde(default)]
    disabled: BTreeSet<String>,
    /// By Plugin: the hash it had when Lucy last told the user it was
    /// awaiting Approval.
    #[serde(default)]
    mentioned: BTreeMap<String, String>,
}

/// Lives as long as lucyd. Remembers which approved Plugin contents work.
pub struct Host {
    paths: Paths,
    /// Where to ask the user for a Confirmation.
    commands: mpsc::UnboundedSender<Command>,
    /// By content hash: whether that content matched its Manifest and has
    /// not failed a call, or why not.
    verdicts: Mutex<HashMap<String, Result<(), String>>>,
    /// The open Session's tools, which an Approval adds to.
    session: Mutex<Weak<SessionTools>>,
    /// Told whenever a Plugin or Approval changes.
    changes: ChangeBus,
    /// Held while changing `approvals.toml`, `plugins.toml` or a Plugin's
    /// files, so one change never undoes another.
    editing: Mutex<()>,
}

/// A Plugin's Manifest and executable as they are now.
struct Contents {
    text: String,
    manifest: Manifest,
    executable: Vec<u8>,
    hash: String,
}

impl Contents {
    fn read(name: &str, folder: &Path) -> Result<Contents, String> {
        let text = std::fs::read_to_string(folder.join("manifest.toml"))
            .map_err(|e| format!("its Manifest can't be read: {e}"))?;
        let manifest = Manifest::parse(&text, name)?;
        let executable = std::fs::read(folder.join(manifest.executable()))
            .map_err(|e| format!("its executable can't be read: {e}"))?;
        let hash = content_hash(text.as_bytes(), &executable);
        Ok(Contents {
            text,
            manifest,
            executable,
            hash,
        })
    }
}

/// What a scan found of one Plugin.
enum Scanned {
    Approved(Usable),
    /// New or changed since its last Approval.
    Awaiting(Contents),
}

/// An approved Plugin, ready to start.
struct Usable {
    name: String,
    folder: PathBuf,
    manifest: Manifest,
    hash: String,
    schemas: HashMap<String, Arc<JsonObject>>,
}

impl Host {
    pub fn new(
        paths: Paths,
        commands: mpsc::UnboundedSender<Command>,
        changes: ChangeBus,
    ) -> Arc<Host> {
        Arc::new(Host {
            paths,
            commands,
            verdicts: Mutex::default(),
            session: Mutex::default(),
            changes,
            editing: Mutex::default(),
        })
    }

    /// Scans the Plugin folders and returns the tools for a new Session.
    pub async fn open_session(self: &Arc<Self>) -> Arc<SessionTools> {
        let mut usable = Vec::new();
        let mut broken = Vec::new();
        let mut awaiting = Vec::new();
        let disabled = self.prefs().disabled;
        for (name, folder) in self.folders() {
            if disabled.contains(&name) {
                continue;
            }
            match self.scan(&name, &folder) {
                Ok(Scanned::Approved(plugin)) => usable.push(plugin),
                // Lucy ships it, so she approves it herself.
                Ok(Scanned::Awaiting(_)) if self.is_shipped(&folder) => {
                    match self.approve_shipped(&name).await {
                        Ok(plugin) => usable.push(plugin),
                        Err(reason) => broken.push(broken_message(&name, &reason)),
                    }
                }
                Ok(Scanned::Awaiting(contents)) => awaiting.push((name, contents)),
                Err(reason) => broken.push(broken_message(&name, &reason)),
            }
        }
        let awaiting = self.mention_once(awaiting);
        let mut working = Vec::new();
        for plugin in usable {
            match self.check(&plugin).await {
                Ok(()) => working.push(plugin),
                Err(reason) => broken.push(broken_message(&plugin.name, &reason)),
            }
        }
        for message in &broken {
            eprintln!("lucyd: {message}");
        }
        let tools = Arc::new(SessionTools {
            host: self.clone(),
            plugins: RwLock::new(working.into_iter().map(Arc::new).collect()),
            broken,
            awaiting,
            running: tokio::sync::Mutex::default(),
            watchers: Mutex::default(),
            barge_ins: tokio::sync::watch::Sender::new(0),
            finished: Mutex::default(),
        });
        *self.session.lock().unwrap() = Arc::downgrade(&tools);
        tools
    }

    /// Asks the user's spoken Approval of the Plugin `name` as it is now,
    /// in the open Session: the full summary for a new Plugin, or only what
    /// changed for an update, with the diff on screen. On a yes it is
    /// recorded and the open Session's model is offered its tools. `Ok`
    /// says whether the user approved; `Err` says why they couldn't be
    /// asked.
    pub async fn request_approval(&self, name: &str) -> Result<bool, String> {
        self.ask_approval(name, false).await
    }

    /// Asks the user's Approval as `request_approval` does, once neither
    /// Lucy nor the user is speaking.
    pub async fn request_approval_when_quiet(&self, name: &str) -> Result<bool, String> {
        self.ask_approval(name, true).await
    }

    async fn ask_approval(&self, name: &str, quiet: bool) -> Result<bool, String> {
        const NO_SESSION: &str = "no Session is open to ask in";
        if self.session.lock().unwrap().upgrade().is_none() {
            return Err(NO_SESSION.into());
        }
        let candidate = self.candidate(name).await?;
        let old = candidate
            .previous
            .as_deref()
            .and_then(|previous| Some((previous, Manifest::parse(previous, name).ok()?)));
        let (prompt, shown) = match old {
            Some((previous, old)) => {
                let shown = Notification {
                    summary: format!("Changes to the {name} Plugin"),
                    body: approval::diff(previous, &candidate.text),
                    action: None,
                    kind: NotificationKind::Details,
                };
                let prompt = approval::changes(&old, &candidate.plugin.manifest);
                (prompt, Some(shown))
            }
            None => (approval::summary(&candidate.plugin.manifest), None),
        };
        let question = Question {
            prompt,
            expects: Expects::Yes,
        };
        let answer = if quiet {
            runner::ask_when_quiet(&self.commands, question, shown).await
        } else {
            runner::ask(&self.commands, question, shown).await
        };
        match answer {
            Some(Answer::Yes) => {}
            Some(_) => return Ok(false),
            None => return Err(NO_SESSION.into()),
        }
        self.record(&candidate)?;
        self.offer(candidate.plugin).await;
        Ok(true)
    }

    /// Approves the shipped Plugin `name` as it is now without asking: it
    /// came with Lucy.
    async fn approve_shipped(&self, name: &str) -> Result<Usable, String> {
        let candidate = self.candidate(name).await?;
        self.record(&candidate)?;
        self.verdicts
            .lock()
            .unwrap()
            .insert(candidate.plugin.hash.clone(), Ok(()));
        Ok(candidate.plugin)
    }

    /// The Plugin `name` as it is now, checked against its live tools and
    /// ready to approve.
    async fn candidate(&self, name: &str) -> Result<Candidate, String> {
        let folder = self
            .folders()
            .remove(name)
            .ok_or_else(|| format!("there's no Plugin called {name}"))?;
        let Contents {
            text,
            manifest,
            executable,
            hash,
        } = Contents::read(name, &folder)?;
        let previous = match self.approvals()?.plugins.remove(name) {
            Some(approval) if approval.hash == hash => {
                return Err(format!("the {name} Plugin is already approved"))
            }
            Some(approval) => approval.manifest,
            None => None,
        };
        let mut plugin = Usable {
            name: name.to_owned(),
            folder,
            manifest,
            hash,
            schemas: HashMap::new(),
        };
        let listed = self.live_tools(&plugin).await?;
        matches_manifest(&plugin.manifest, &listed)?;
        plugin.schemas = listed
            .into_iter()
            .map(|tool| (tool.name.into_owned(), tool.input_schema))
            .collect();
        Ok(Candidate {
            plugin,
            text,
            executable,
            previous,
        })
    }

    /// Writes the Approval of `candidate` to `approvals.toml`, keeping the
    /// one it replaces as the version to roll back to, and keeps a copy of
    /// what was approved.
    fn record(&self, candidate: &Candidate) -> Result<(), String> {
        let plugin = &candidate.plugin;
        let _editing = self.editing.lock().unwrap();
        self.keep_version(
            &plugin.name,
            &plugin.hash,
            &candidate.text,
            &candidate.executable,
        )
        .map_err(|e| format!("its approved version couldn't be kept: {e}"))?;
        let mut approvals = self.approvals()?;
        let schemas = plugin
            .schemas
            .iter()
            .map(|(tool, schema)| (tool.clone(), serde_json::to_string(&**schema).unwrap()))
            .collect();
        let kept = approvals.revoked.remove(&plugin.name);
        let previous = approvals
            .plugins
            .remove(&plugin.name)
            .or(kept)
            .filter(|previous| previous.hash != plugin.hash)
            .map(|mut previous| {
                previous.previous = None;
                Box::new(previous)
            });
        let kept: Vec<&str> = [Some(&*plugin.hash), previous.as_ref().map(|p| &*p.hash)]
            .into_iter()
            .flatten()
            .collect();
        self.prune_versions(&plugin.name, &kept);
        approvals.plugins.insert(
            plugin.name.clone(),
            Approval {
                hash: plugin.hash.clone(),
                schemas,
                manifest: Some(candidate.text.clone()),
                previous,
            },
        );
        self.save_approvals(&approvals)
            .map_err(|e| format!("its Approval couldn't be saved: {e}"))?;
        self.changes.publish();
        Ok(())
    }

    fn save_approvals(&self, approvals: &Approvals) -> Result<(), String> {
        let text = toml::to_string(approvals).map_err(|e| format!("approvals.toml: {e}"))?;
        save_state(&self.paths.state, &self.paths.approvals(), &text).map_err(|e| e.to_string())
    }

    /// `plugins.toml`, or no preferences if it can't be read.
    fn prefs(&self) -> PluginPrefs {
        std::fs::read_to_string(self.paths.prefs())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_prefs(&self, prefs: &PluginPrefs) -> Result<(), String> {
        let text = toml::to_string(prefs).map_err(|e| format!("plugins.toml: {e}"))?;
        save_state(&self.paths.state, &self.paths.prefs(), &text)
            .map_err(|e| format!("plugins.toml couldn't be saved: {e}"))
    }

    /// For the model's instructions: a line for each Plugin in `awaiting`
    /// that the user hasn't been told about as it is now, remembering that
    /// they now have.
    fn mention_once(&self, awaiting: Vec<(String, Contents)>) -> Vec<String> {
        let _editing = self.editing.lock().unwrap();
        let mut prefs = self.prefs();
        let mut lines = Vec::new();
        for (name, contents) in awaiting {
            if prefs.mentioned.get(&name) == Some(&contents.hash) {
                continue;
            }
            lines.push(format!(
                "The {name} Plugin ({}) is awaiting the user's Approval.",
                contents.manifest.summary
            ));
            prefs.mentioned.insert(name, contents.hash);
        }
        if !lines.is_empty() {
            if let Err(e) = self.save_prefs(&prefs) {
                eprintln!("lucyd: {e}");
            }
            self.changes.publish();
        }
        lines
    }

    /// Offers the newly approved `plugin` in the open Session, if any.
    async fn offer(&self, plugin: Usable) {
        let session = self.session.lock().unwrap().upgrade();
        self.verdicts
            .lock()
            .unwrap()
            .insert(plugin.hash.clone(), Ok(()));
        if let Some(session) = session {
            session.replace(plugin).await;
        }
    }

    fn approvals(&self) -> Result<Approvals, String> {
        match std::fs::read_to_string(self.paths.approvals()) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("approvals.toml: {e}")),
            Err(_) => Ok(Approvals::default()),
        }
    }

    /// Every Plugin folder by name, a user one overriding a shipped one.
    fn folders(&self) -> BTreeMap<String, PathBuf> {
        let mut folders = BTreeMap::new();
        for root in [&self.paths.shipped, &self.paths.user] {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // `.build` holds Authoring staging.
                if !name.starts_with('.') && entry.path().is_dir() {
                    folders.insert(name, entry.path());
                }
            }
        }
        folders
    }

    fn is_shipped(&self, folder: &Path) -> bool {
        folder.starts_with(&self.paths.shipped)
    }

    /// The Plugin as it is now: usable if its exact content is approved.
    fn scan(&self, name: &str, folder: &Path) -> Result<Scanned, String> {
        let contents = Contents::read(name, folder)?;
        let approvals = self.approvals()?;
        let Some(approval) = approvals
            .plugins
            .get(name)
            .filter(|a| a.hash == contents.hash)
        else {
            return Ok(Scanned::Awaiting(contents));
        };
        let Contents { manifest, hash, .. } = contents;
        let mut schemas = HashMap::new();
        for tool in &manifest.tools {
            let schema = approval
                .schemas
                .get(&tool.name)
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| format!("its Approval has no input schema for {}", tool.name))?;
            schemas.insert(tool.name.clone(), Arc::new(schema));
        }
        Ok(Scanned::Approved(Usable {
            name: name.to_owned(),
            folder: folder.to_owned(),
            manifest,
            hash,
            schemas,
        }))
    }

    /// Checks the Manifest against the live `tools/list`, once per content.
    async fn check(&self, plugin: &Usable) -> Result<(), String> {
        if let Some(verdict) = self.verdict(&plugin.hash) {
            return verdict;
        }
        let verdict = async {
            let listed = self.live_tools(plugin).await?;
            matches_manifest(&plugin.manifest, &listed)
        }
        .await;
        self.verdicts
            .lock()
            .unwrap()
            .insert(plugin.hash.clone(), verdict.clone());
        verdict
    }

    /// Starts the Plugin just to ask for its `tools/list`.
    async fn live_tools(&self, plugin: &Usable) -> Result<Vec<Tool>, String> {
        let client = self.start(plugin).await?;
        let listed = tokio::time::timeout(START_TIMEOUT, client.list_all_tools())
            .await
            .map_err(|_| "it didn't list its tools in time".to_owned())?
            .map_err(|e| format!("it failed to list its tools: {e}"))?;
        let _ = client.cancel().await;
        Ok(listed)
    }

    fn verdict(&self, hash: &str) -> Option<Result<(), String>> {
        self.verdicts.lock().unwrap().get(hash).cloned()
    }

    fn mark_broken(&self, hash: &str, reason: String) {
        self.verdicts
            .lock()
            .unwrap()
            .insert(hash.to_owned(), Err(reason));
    }

    /// Starts the Plugin in its sandbox and connects to it.
    async fn start(&self, plugin: &Usable) -> Result<RunningService<RoleClient, ()>, String> {
        let state_dir = self.paths.state_dir(&plugin.name);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&state_dir)
            .map_err(|e| format!("its state dir can't be made: {e}"))?;

        let mut command =
            tokio::process::Command::new(plugin.folder.join(plugin.manifest.executable()));
        command
            .args(&plugin.manifest.command[1..])
            .current_dir(&plugin.folder)
            .env_clear()
            .env("LUCY_PLUGIN_STATE_DIR", &state_dir);
        for var in ["PATH", "HOME", "LANG"] {
            if let Some(value) = std::env::var_os(var) {
                command.env(var, value);
            }
        }
        sandbox::confine(
            &mut command,
            &plugin.folder,
            &state_dir,
            &plugin.manifest.access,
        )
        .map_err(|e| format!("its sandbox can't be set up: {e}"))?;
        let transport =
            TokioChildProcess::new(command).map_err(|e| format!("it doesn't start: {e}"))?;
        // Mid-call questions (`input_required`) need the 2026-07-28 protocol,
        // which has no `initialize`; older Plugins get the handshake.
        let lifecycle = ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: None,
        };
        tokio::time::timeout(
            START_TIMEOUT,
            serve_client_with_lifecycle((), transport, lifecycle),
        )
        .await
        .map_err(|_| "it didn't start in time".to_owned())?
        .map_err(|e| format!("it didn't start: {e}"))
    }
}

/// A Plugin as it is now, ready to approve.
struct Candidate {
    /// With the input schemas it lists live.
    plugin: Usable,
    /// Its Manifest.
    text: String,
    executable: Vec<u8>,
    /// The Manifest it had when last approved, if it was and that was
    /// kept.
    previous: Option<String>,
}

/// Whether the Plugin offers exactly the tools its Manifest lists.
fn matches_manifest(manifest: &Manifest, listed: &[Tool]) -> Result<(), String> {
    let live: HashSet<&str> = listed.iter().map(|tool| &*tool.name).collect();
    let declared: HashSet<&str> = manifest.tools.iter().map(|t| &*t.name).collect();
    if live != declared {
        return Err(format!(
            "its Manifest lists the tools {} but it offers {}",
            names(&declared),
            names(&live)
        ));
    }
    Ok(())
}

/// Writes `text` to `path` in Lucy's state dir `state`, all at once.
fn save_state(state: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state)?;
    let temp = path.with_extension("toml.new");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

/// What Lucy is told about a broken Plugin, ready to pass on to the user.
fn broken_message(plugin: &str, reason: &str) -> String {
    format!("The {plugin} Plugin is broken: {reason}. It's left out until it's fixed.")
}

fn names(names: &HashSet<&str>) -> String {
    let mut names: Vec<&str> = names.iter().copied().collect();
    names.sort();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

/// What an Approval pins: SHA-256 over the Manifest's length (u64, little
/// endian), the Manifest, then the executable, as lowercase hex.
fn content_hash(manifest: &[u8], executable: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update((manifest.len() as u64).to_le_bytes());
    hasher.update(manifest);
    hasher.update(executable);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The Plugin tools of one Session, and the Plugins it has started.
pub struct SessionTools {
    host: Arc<Host>,
    plugins: RwLock<Vec<Arc<Usable>>>,
    /// Why each Plugin the scan left out was left out.
    broken: Vec<String>,
    /// A line for each Plugin awaiting Approval that the user hasn't been
    /// told about yet.
    awaiting: Vec<String>,
    running: tokio::sync::Mutex<HashMap<String, RunningService<RoleClient, ()>>>,
    /// The model's connections, told when the tools change.
    watchers: Mutex<Vec<Peer<RoleServer>>>,
    /// Counts Barge-ins, so calls in flight hear of each.
    barge_ins: tokio::sync::watch::Sender<u64>,
    /// What confirmed calls returned after a Barge-in cut the model off,
    /// for the next Turn.
    finished: Mutex<Vec<String>>,
}

impl SessionTools {
    /// The user cut in: calls in flight that they didn't confirm are
    /// cancelled, and confirmed ones run on.
    pub fn barge_in(&self) {
        self.barge_ins.send_modify(|count| *count += 1);
    }

    /// What confirmed calls returned since the model was cut off, one line
    /// each, for the next Turn.
    pub fn take_finished(&self) -> Vec<String> {
        std::mem::take(&mut *self.finished.lock().unwrap())
    }

    /// For the model's instructions: which Plugins the scan left out and
    /// why, and which new ones await Approval, if any.
    pub fn scan_report(&self) -> Option<String> {
        let mut sections = Vec::new();
        if !self.broken.is_empty() {
            sections.push(format!(
                "## Broken Plugins (not part of your Persona)\n\n{}",
                self.broken.join("\n")
            ));
        }
        if !self.awaiting.is_empty() {
            sections.push(format!(
                "## Plugins awaiting Approval (not part of your Persona)\n\n{}",
                self.awaiting.join("\n")
            ));
        }
        (!sections.is_empty()).then(|| {
            format!(
                "{}\n\nTell the user about this once, briefly, in your first reply.",
                sections.join("\n\n")
            )
        })
    }

    /// Tells `peer` with `tools/list_changed` whenever the tools change.
    pub fn watch(&self, peer: Peer<RoleServer>) {
        self.watchers.lock().unwrap().push(peer);
    }

    /// Offers `plugin` in place of any Plugin of the same name, stopping
    /// that one, and tells the watchers.
    async fn replace(&self, plugin: Usable) {
        let name = plugin.name.clone();
        {
            let mut plugins = self.plugins.write().unwrap();
            plugins.retain(|p| p.name != name);
            plugins.push(Arc::new(plugin));
        }
        if let Some(client) = self.running.lock().await.remove(&name) {
            let _ = client.cancel().await;
        }
        self.tools_changed().await;
    }

    /// Tells the watchers with `tools/list_changed`.
    async fn tools_changed(&self) {
        let watchers: Vec<_> = {
            let mut watchers = self.watchers.lock().unwrap();
            watchers.retain(|peer| !peer.is_transport_closed());
            watchers.clone()
        };
        for peer in watchers {
            let _ = peer.notify_tool_list_changed().await;
        }
    }

    /// Stops offering the Plugin `name`, stopping it, and tells the
    /// watchers.
    async fn remove(&self, name: &str) {
        self.plugins.write().unwrap().retain(|p| p.name != name);
        if let Some(client) = self.running.lock().await.remove(name) {
            let _ = client.cancel().await;
        }
        self.tools_changed().await;
    }

    /// Every tool of every working Plugin, as `<plugin>__<tool>`.
    pub fn list(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        for plugin in self.working() {
            for tool in &plugin.manifest.tools {
                let mut description = tool.handles.clone();
                if !tool.examples.is_empty() {
                    let examples: Vec<String> =
                        tool.examples.iter().map(|e| format!("\"{e}\"")).collect();
                    description.push_str(&format!(". For example: {}", examples.join("; ")));
                }
                tools.push(Tool::new(
                    format!("{}__{}", plugin.name, tool.name),
                    description,
                    plugin.schemas[&tool.name].clone(),
                ));
            }
        }
        tools
    }

    /// Forwards a call to `<plugin>__<tool>`, starting the Plugin on first
    /// use. `None` if no working Plugin offers that tool.
    pub async fn call(&self, name: &str, arguments: Option<JsonObject>) -> Option<CallToolResult> {
        let (plugin_name, tool) = name.split_once("__")?;
        let plugin = self.working().into_iter().find(|p| p.name == plugin_name)?;
        let entry = plugin.manifest.tools.iter().find(|t| t.name == tool)?;
        if entry.confirm && !self.confirmed(plugin_name, entry, arguments.as_ref()).await {
            return Some(mcp::text(DECLINED));
        }
        let mut barge_ins = self.barge_ins.subscribe();
        let outcome = async {
            let peer = {
                let mut running = self.running.lock().await;
                if !running.contains_key(plugin_name) {
                    let client = self.host.start(&plugin).await?;
                    running.insert(plugin_name.to_owned(), client);
                }
                running[plugin_name].peer().clone()
            };
            let mut request = CallToolRequestParams::new(tool.to_owned());
            if let Some(arguments) = arguments {
                request = request.with_arguments(arguments);
            }
            // Each `input_required` answer asks the user, then calls again.
            for _ in 0..MAX_QUESTIONS {
                match peer.call_tool_once(request.clone()).await {
                    Ok(CallToolResponse::Complete(result)) => return Ok(result),
                    Ok(CallToolResponse::InputRequired(asked)) => {
                        let requests = asked.input_requests.unwrap_or_default();
                        request.input_responses = Some(self.answers(requests).await);
                        request.request_state = asked.request_state;
                    }
                    Ok(_) => return Err("it answered with a task, which Lucy can't follow".into()),
                    // The Plugin answered, refusing the call: it still works.
                    Err(rmcp::ServiceError::McpError(e)) => {
                        return Ok(mcp::error(e.message.into_owned()))
                    }
                    Err(e) => return Err(format!("its call failed: {e}")),
                }
            }
            Err(format!(
                "it asked more than {MAX_QUESTIONS} questions in one call"
            ))
        };
        let outcome = if entry.confirm {
            let outcome = outcome.await;
            if barge_ins.has_changed().unwrap_or(false) {
                if let Ok(result) = &outcome {
                    self.finished.lock().unwrap().push(format!(
                        "[A confirmed call finished after the user cut in: {name} returned: {}]",
                        mcp::result_text(result)
                    ));
                }
            }
            outcome
        } else {
            // Dropping the call sends the Plugin an MCP cancellation.
            tokio::select! {
                outcome = outcome => outcome,
                _ = barge_ins.changed() => return Some(mcp::error(CANCELLED)),
            }
        };
        Some(match outcome {
            Ok(result) => result,
            Err(reason) => {
                if let Some(client) = self.running.lock().await.remove(plugin_name) {
                    let _ = client.cancel().await;
                }
                self.host.mark_broken(&plugin.hash, reason.clone());
                mcp::error(broken_message(plugin_name, &reason))
            }
        })
    }

    /// Asks the user each of a Plugin's mid-call questions out loud. A
    /// question is answered when it wants one field and the user says
    /// something; anything else is declined.
    async fn answers(&self, requests: InputRequests) -> InputResponses {
        let mut answers = InputResponses::new();
        for (key, request) in requests {
            let mut answer = ElicitResult::new(ElicitationAction::Decline);
            if let InputRequest::Elicitation(request) = request {
                if let ElicitRequestParams::FormElicitationParams {
                    message,
                    requested_schema,
                    ..
                } = request.params
                {
                    let mut fields = requested_schema.properties.into_keys();
                    if let (Some(field), None) = (fields.next(), fields.next()) {
                        let question = Question {
                            prompt: message,
                            expects: Expects::Words,
                        };
                        if let Some(Answer::Said(said)) =
                            runner::ask(&self.host.commands, question, None).await
                        {
                            answer = ElicitResult::new(ElicitationAction::Accept)
                                .with_content(serde_json::json!({ field: said }));
                        }
                    }
                }
            }
            answers.insert(key, serde_json::to_value(answer).unwrap());
        }
        answers
    }

    /// Asks the user's spoken yes to a call of `tool`, showing its full
    /// arguments.
    async fn confirmed(
        &self,
        plugin: &str,
        tool: &ToolEntry,
        arguments: Option<&JsonObject>,
    ) -> bool {
        let empty = JsonObject::new();
        let arguments = arguments.unwrap_or(&empty);
        let prompt = match &tool.confirm_prompt {
            Some(prompt) => fill(prompt, arguments),
            None => format!("Shall I use the {plugin} Plugin to {}?", tool.handles),
        };
        let mut body = format!("{plugin}__{}", tool.name);
        for (name, value) in arguments {
            body.push_str(&format!("\n{name}: {}", spoken(value)));
        }
        let shown = Notification {
            summary: "Lucy needs your yes".into(),
            body,
            action: None,
            kind: NotificationKind::Details,
        };
        let question = Question {
            prompt,
            expects: Expects::Yes,
        };
        runner::ask(&self.host.commands, question, Some(shown)).await == Some(Answer::Yes)
    }

    /// Stops every Plugin started in this Session.
    pub async fn stop(&self) {
        let running: Vec<_> = self.running.lock().await.drain().collect();
        for (_, client) in running {
            let _ = client.cancel().await;
        }
    }

    fn working(&self) -> Vec<Arc<Usable>> {
        self.plugins
            .read()
            .unwrap()
            .iter()
            .filter(|p| !matches!(self.host.verdict(&p.hash), Some(Err(_))))
            .cloned()
            .collect()
    }
}

/// `template` with each `{name}` of an argument replaced by its value, in one
/// pass, so a value is never filled in itself.
fn fill(template: &str, arguments: &JsonObject) -> String {
    let mut filled = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        filled.push_str(&rest[..open]);
        rest = &rest[open..];
        let value = rest
            .find('}')
            .and_then(|close| Some((close, arguments.get(&rest[1..close])?)));
        match value {
            Some((close, value)) => {
                filled.push_str(&spoken(value));
                rest = &rest[close + 1..];
            }
            None => {
                filled.push('{');
                rest = &rest[1..];
            }
        }
    }
    filled.push_str(rest);
    filled
}

/// An argument as Lucy says it: a string as it is, anything else as JSON.
fn spoken(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
