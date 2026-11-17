//! The Plugin host (ADRs 0002, 0004, 0005): finds and runs approved Plugins.
//! It keeps only the approved, working ones, and runs each in its sandbox on
//! first use in a Session, forwarding the model's calls to it over MCP stdio.

mod approval;
mod authored;
mod environment;
mod manage;
mod manifest;
mod questions;
pub(crate) mod sandbox;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use lucy_ipc::FieldKind;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ElicitRequestParams, ElicitResult,
    ElicitationAction, InputRequest, InputRequests, InputResponses, JsonObject, ProtocolVersion,
    RequestMetaObject, Tool,
};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, Peer, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, RoleServer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};

use crate::adapters::{CredentialStore, Notification, NotificationKind};
use crate::input::{self, Filled, TypedAnswer};
use crate::mcp;
use crate::model_text::CALL_FINISHED;
use crate::runner::{self, Command, When};
use crate::session::{Answer, Expects, Question};
use crate::status::ChangeBus;
use crate::sync::{Locked, POISONED};
pub use environment::Environment;
pub use manage::{approve_plugin_tool, manage_plugin_tool, Refusal};
use manifest::{Manifest, ScreenUse, ToolEntry};

const MAX_QUESTIONS: usize = 10;

const SYSTEM_ENV: [&str; 3] = ["PATH", "HOME", "LANG"];

const DECLINED: &str = "declined: the user didn't say yes, so nothing was done, and you've \
                        already told them so.";

const LOOK_PROMPT: &str = "I need to look at your screen. Okay?";

const LOOK_DECLINED: &str = "declined: the user hasn't let you look at their screen in this \
                             Session, so nothing was captured. Don't ask to look again until \
                             their next Session.";

const CANCELLED: &str = "cancelled: the user cut in, so the call was stopped.";

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

    fn prefs(&self) -> PathBuf {
        self.state.join("plugins.toml")
    }

    fn versions(&self, plugin: &str) -> PathBuf {
        self.state.join("versions").join(plugin)
    }

    fn state_dir(&self, plugin: &str) -> PathBuf {
        self.state.join("plugins").join(plugin)
    }
}

#[derive(Default, Deserialize, Serialize)]
struct Approvals {
    #[serde(default)]
    plugins: BTreeMap<String, Approval>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    revoked: BTreeMap<String, Approval>,
}

#[derive(Deserialize, Serialize)]
struct Approval {
    hash: String,
    schemas: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Box<Approval>>,
}

#[derive(Default, Deserialize, Serialize)]
struct PluginPrefs {
    #[serde(default)]
    disabled: BTreeSet<String>,
    #[serde(default)]
    mentioned: BTreeMap<String, String>,
}

pub struct Host {
    paths: Paths,
    commands: mpsc::UnboundedSender<Command>,
    verdicts: Mutex<HashMap<String, Result<(), String>>>,
    session: Mutex<Weak<SessionTools>>,
    changes: ChangeBus,
    editing: Mutex<()>,
    credentials: Arc<dyn CredentialStore>,
    environment: Environment,
}

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

enum Scanned {
    Approved(Usable),
    Awaiting(Contents),
    NotForThisDesktop(Manifest),
}

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
        credentials: Arc<dyn CredentialStore>,
        environment: Environment,
    ) -> Arc<Host> {
        Arc::new(Host {
            paths,
            commands,
            verdicts: Mutex::default(),
            session: Mutex::default(),
            changes,
            editing: Mutex::default(),
            credentials,
            environment,
        })
    }

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
                Ok(Scanned::Awaiting(_)) if self.is_shipped(&folder) => {
                    match self.approve_shipped(&name).await {
                        Ok(plugin) => usable.push(plugin),
                        Err(reason) => broken.push(broken_message(&name, &reason)),
                    }
                }
                Ok(Scanned::Awaiting(contents)) => awaiting.push((name, contents)),
                Ok(Scanned::NotForThisDesktop(_)) => {}
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
            barge_ins: watch::Sender::new(0),
            finished: Mutex::default(),
            screen_off: AtomicBool::new(false),
            first_look: tokio::sync::Mutex::new(FirstLook::Unasked),
            looks: watch::Sender::new(0),
        });
        *self.session.locked() = Arc::downgrade(&tools);
        tools
    }

    pub async fn request_approval(&self, name: &str) -> Result<bool, String> {
        let verdict = self.ask_approval(name, When::Now).await?;
        Ok(verdict == Verdict::Approved)
    }

    pub async fn request_approval_when_quiet(
        &self,
        name: &str,
        kept: Notification,
    ) -> Result<bool, String> {
        let verdict = self
            .ask_approval(name, When::QuietOrNextSession(kept))
            .await?;
        Ok(verdict == Verdict::Approved)
    }

    async fn ask_approval(&self, name: &str, when: When) -> Result<Verdict, String> {
        const NO_SESSION: &str = "no Session is open to ask in";
        if !when.keeps() && self.session.locked().upgrade().is_none() {
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
        let answer = runner::ask_at(&self.commands, question, shown, when).await;
        match answer {
            Some(Answer::Yes) => {}
            Some(Answer::TimedOut) => return Ok(Verdict::TimedOut),
            Some(_) => return Ok(Verdict::Declined),
            None => return Err(NO_SESSION.into()),
        }
        self.record(&candidate)?;
        self.offer(candidate.plugin).await;
        Ok(Verdict::Approved)
    }

    async fn approve_shipped(&self, name: &str) -> Result<Usable, String> {
        let candidate = self.candidate(name).await?;
        self.record(&candidate)?;
        self.verdicts
            .locked()
            .insert(candidate.plugin.hash.clone(), Ok(()));
        Ok(candidate.plugin)
    }

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
        if !self.environment.meets(&manifest.requires) {
            return Err(format!("the {name} Plugin isn't for this desktop"));
        }
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

    fn record(&self, candidate: &Candidate) -> Result<(), String> {
        let plugin = &candidate.plugin;
        let _editing = self.editing.locked();
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
            .map(|(tool, schema)| {
                (
                    tool.clone(),
                    serde_json::to_string(&**schema).expect("a JSON object always serialises"),
                )
            })
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

    fn mention_once(&self, awaiting: Vec<(String, Contents)>) -> Vec<String> {
        let _editing = self.editing.locked();
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

    async fn offer(&self, plugin: Usable) {
        let session = self.session.locked().upgrade();
        self.verdicts.locked().insert(plugin.hash.clone(), Ok(()));
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

    fn folders(&self) -> BTreeMap<String, PathBuf> {
        let mut folders = BTreeMap::new();
        for root in [&self.paths.shipped, &self.paths.user] {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let staging_or_hidden = name.starts_with('.');
                if !staging_or_hidden && entry.path().is_dir() {
                    folders.insert(name, entry.path());
                }
            }
        }
        folders
    }

    fn is_shipped(&self, folder: &Path) -> bool {
        folder.starts_with(&self.paths.shipped)
    }

    fn scan(&self, name: &str, folder: &Path) -> Result<Scanned, String> {
        let contents = Contents::read(name, folder)?;
        if !self.environment.meets(&contents.manifest.requires) {
            return Ok(Scanned::NotForThisDesktop(contents.manifest));
        }
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
            .locked()
            .insert(plugin.hash.clone(), verdict.clone());
        verdict
    }

    async fn live_tools(&self, plugin: &Usable) -> Result<Vec<Tool>, String> {
        let client = self.start(plugin, &[]).await?;
        let listed = tokio::time::timeout(START_TIMEOUT, client.list_all_tools())
            .await
            .map_err(|_| "it didn't list its tools in time".to_owned())?
            .map_err(|e| format!("it failed to list its tools: {e}"))?;
        let _ = client.cancel().await;
        Ok(listed)
    }

    fn verdict(&self, hash: &str) -> Option<Result<(), String>> {
        self.verdicts.locked().get(hash).cloned()
    }

    fn mark_broken(&self, hash: &str, reason: String) {
        self.verdicts.locked().insert(hash.to_owned(), Err(reason));
    }

    async fn keep(&self, plugin: &str, name: &str, value: &str) -> Result<(), String> {
        let store = self.credentials.clone();
        let (plugin, name, value) = (plugin.to_owned(), name.to_owned(), value.to_owned());
        let kept = tokio::task::spawn_blocking(move || store.set(&plugin, &name, &value)).await;
        kept.map_err(|e| e.to_string())
            .flatten()
            .map_err(|e| format!("What the user typed can't be kept: {e}."))
    }

    async fn credentials(&self, plugin: &Usable) -> Result<Credentials, String> {
        let store = self.credentials.clone();
        let name = plugin.name.clone();
        let declared = plugin.manifest.access.credentials.clone();
        let found = tokio::task::spawn_blocking(move || {
            declared
                .into_iter()
                .map(|credential| Ok((credential.clone(), store.get(&name, &credential)?)))
                .collect::<Result<Vec<_>, String>>()
        })
        .await
        .map_err(|e| e.to_string())
        .flatten()
        .map_err(|e| {
            format!(
                "The {} Plugin's credentials can't be read: {e}.",
                plugin.name
            )
        })?;
        let mut credentials = Credentials::default();
        for (name, value) in found {
            match value {
                Some(value) => credentials.entered.push(Credential { name, value }),
                None => credentials.missing.push(name),
            }
        }
        Ok(credentials)
    }

    async fn ask_missing(
        &self,
        plugin: &Usable,
        credentials: Credentials,
    ) -> Result<Vec<Credential>, String> {
        let mut entered = credentials.entered;
        for name in credentials.missing {
            let what = format!("your {}", words(&name));
            let message = format!("The {} Plugin needs {name}.", plugin.name);
            let value =
                match input::ask_for(&self.commands, &what, &message, &name, FieldKind::Secret)
                    .await
                {
                    TypedAnswer::Answer(value) => value,
                    TypedAnswer::Cancelled => {
                        return Err(unentered(
                            &plugin.name,
                            &name,
                            "the box for it was cancelled",
                        ))
                    }
                    TypedAnswer::TimedOut => {
                        return Err(unentered(&plugin.name, &name, "the box for it timed out"))
                    }
                };
            self.keep(&plugin.name, &name, &value).await?;
            entered.push(Credential { name, value });
        }
        Ok(entered)
    }

    async fn entered(&self, plugin: &Usable) -> Result<Vec<Credential>, String> {
        let read = self.credentials(plugin).await?;
        self.ask_missing(plugin, read).await
    }

    async fn start(
        &self,
        plugin: &Usable,
        env: &[Credential],
    ) -> Result<RunningService<RoleClient, ()>, String> {
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
        let required = plugin.manifest.requires.env.iter().map(String::as_str);
        for var in SYSTEM_ENV.into_iter().chain(required) {
            if let Some(value) = self.environment.var(var) {
                command.env(var, value);
            }
        }
        command.envs(env.iter().map(|c| (&c.name, &c.value)));
        sandbox::confine(
            &mut command,
            &plugin.folder,
            &state_dir,
            &plugin.manifest.access,
            &self.environment,
        )
        .map_err(|e| format!("its sandbox can't be set up: {e}"))?;
        let transport =
            TokioChildProcess::new(command).map_err(|e| format!("it doesn't start: {e}"))?;
        let input_required_protocol = ProtocolVersion::V_2026_07_28;
        let lifecycle = ClientLifecycleMode::Auto {
            preferred_versions: vec![input_required_protocol],
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

#[derive(Default)]
struct Credentials {
    entered: Vec<Credential>,
    missing: Vec<String>,
}

struct Credential {
    name: String,
    value: String,
}

const ACRONYMS: [&str; 10] = [
    "API", "URL", "URI", "ID", "SSH", "AWS", "JWT", "HTTP", "SMTP", "IMAP",
];

fn words(name: &str) -> String {
    let parts: Vec<String> = name
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| match &*word.to_ascii_uppercase() {
            "OAUTH" => "OAuth".to_owned(),
            upper if ACRONYMS.contains(&upper) => upper.to_owned(),
            _ => word.to_ascii_lowercase(),
        })
        .collect();
    let spoken = parts.join(" ");
    let mut chars = spoken.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => spoken,
    }
}

fn unentered(plugin: &str, name: &str, why: &str) -> String {
    format!(
        "The {plugin} Plugin needs {name}, but {why}, so nothing was done. The \
         user can enter it by running `lucy credentials {plugin}` in a terminal, never by voice \
         or through you."
    )
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Approved,
    Declined,
    TimedOut,
}

struct Candidate {
    plugin: Usable,
    text: String,
    executable: Vec<u8>,
    previous: Option<String>,
}

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

fn save_state(state: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state)?;
    let temp = path.with_extension("toml.new");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

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

pub struct SessionTools {
    host: Arc<Host>,
    plugins: RwLock<Vec<Arc<Usable>>>,
    broken: Vec<String>,
    awaiting: Vec<String>,
    running: tokio::sync::Mutex<HashMap<String, RunningService<RoleClient, ()>>>,
    watchers: Mutex<Vec<Peer<RoleServer>>>,
    barge_ins: watch::Sender<u64>,
    finished: Mutex<Vec<String>>,
    screen_off: AtomicBool,
    first_look: tokio::sync::Mutex<FirstLook>,
    looks: watch::Sender<u64>,
}

#[derive(Debug, PartialEq, Eq)]
struct Declined;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstLook {
    Unasked,
    Allowed,
    Declined,
}

impl SessionTools {
    pub fn barge_in(&self) {
        self.barge_ins.send_modify(|count| *count += 1);
    }

    pub fn turn_screen_off(&self) {
        self.screen_off.store(true, Ordering::SeqCst);
    }

    pub fn looks(&self) -> watch::Receiver<u64> {
        self.looks.subscribe()
    }

    fn offers(&self, tool: &ToolEntry) -> bool {
        tool.screen.is_none() || !self.screen_off.load(Ordering::SeqCst)
    }

    async fn look(&self) -> Result<(), Declined> {
        let mut first = self.first_look.lock().await;
        if *first == FirstLook::Unasked {
            let question = Question {
                prompt: LOOK_PROMPT.into(),
                expects: Expects::Yes,
            };
            *first = match runner::ask(&self.host.commands, question, None).await {
                Some(Answer::Yes) => FirstLook::Allowed,
                Some(_) => FirstLook::Declined,
                None => return Err(Declined),
            };
        }
        if *first == FirstLook::Declined {
            return Err(Declined);
        }
        self.looks.send_modify(|looks| *looks += 1);
        Ok(())
    }

    pub fn take_finished(&self) -> Vec<String> {
        std::mem::take(&mut *self.finished.locked())
    }

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
                "## Plugins awaiting Approval (not part of your Persona)\n\n{}\n\nOffer to go \
                 through one with the user. If they want to, call approve_plugin with its name: \
                 Lucy reads it out and hears their yes or no herself. There is no other way to \
                 approve one.",
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

    pub fn watch(&self, peer: Peer<RoleServer>) {
        self.watchers.locked().push(peer);
    }

    async fn replace(&self, plugin: Usable) {
        let name = plugin.name.clone();
        {
            let mut plugins = self.plugins.write().expect(POISONED);
            plugins.retain(|p| p.name != name);
            plugins.push(Arc::new(plugin));
        }
        if let Some(client) = self.running.lock().await.remove(&name) {
            let _ = client.cancel().await;
        }
        self.tools_changed().await;
    }

    async fn tools_changed(&self) {
        let watchers: Vec<_> = {
            let mut watchers = self.watchers.locked();
            watchers.retain(|peer| !peer.is_transport_closed());
            watchers.clone()
        };
        for peer in watchers {
            let _ = peer.notify_tool_list_changed().await;
        }
    }

    async fn remove(&self, name: &str) {
        self.plugins
            .write()
            .expect(POISONED)
            .retain(|p| p.name != name);
        if let Some(client) = self.running.lock().await.remove(name) {
            let _ = client.cancel().await;
        }
        self.tools_changed().await;
    }

    pub fn list(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        for plugin in self.working() {
            for tool in plugin.manifest.tools.iter().filter(|t| self.offers(t)) {
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

    pub async fn call(&self, name: &str, arguments: Option<JsonObject>) -> Option<CallToolResult> {
        let (plugin_name, tool) = name.split_once("__")?;
        let plugin = self.working().into_iter().find(|p| p.name == plugin_name)?;
        let entry = plugin
            .manifest
            .tools
            .iter()
            .find(|t| t.name == tool && self.offers(t))?;
        let mut read_before_confirming = None;
        if !self.running.lock().await.contains_key(plugin_name) {
            match self.host.credentials(&plugin).await {
                Ok(read) => read_before_confirming = Some(read),
                Err(unreadable) => return Some(mcp::error(unreadable)),
            }
        }
        if entry.screen == Some(ScreenUse::Look) && self.look().await == Err(Declined) {
            return Some(mcp::text(LOOK_DECLINED));
        }
        if entry.confirm && !self.confirmed(plugin_name, entry, arguments.as_ref()).await {
            return Some(mcp::text(DECLINED));
        }
        let mut entered = match read_before_confirming {
            Some(read) => match self.host.ask_missing(&plugin, read).await {
                Ok(entered) => Some(entered),
                Err(reason) => return Some(mcp::error(reason)),
            },
            None => None,
        };
        let mut barge_ins = self.barge_ins.subscribe();
        let outcome = async {
            let peer = {
                let mut running = self.running.lock().await;
                if !running.contains_key(plugin_name) {
                    let entered = match entered.take() {
                        Some(entered) => entered,
                        None => {
                            drop(running);
                            let entered = match self.host.entered(&plugin).await {
                                Ok(entered) => entered,
                                Err(reason) => return Ok(mcp::error(reason)),
                            };
                            running = self.running.lock().await;
                            entered
                        }
                    };
                    if !running.contains_key(plugin_name) {
                        let client = self.host.start(&plugin, &entered).await?;
                        running.insert(plugin_name.to_owned(), client);
                    }
                }
                running[plugin_name].peer().clone()
            };
            let mut request = CallToolRequestParams::new(tool.to_owned());
            if let Some(arguments) = arguments {
                request = request.with_arguments(arguments);
            }
            for _ in 0..MAX_QUESTIONS {
                match peer.call_tool_once(request.clone()).await {
                    Ok(CallToolResponse::Complete(result)) => return Ok(result),
                    Ok(CallToolResponse::InputRequired(asked)) => {
                        let requests = asked.input_requests.unwrap_or_default();
                        match self.answers(&plugin, requests).await {
                            Ok(answers) => request.input_responses = Some(answers),
                            Err(unkept) => return Ok(mcp::error(unkept)),
                        }
                        request.request_state = asked.request_state;
                    }
                    Ok(_) => return Err("it answered with a task, which Lucy can't follow".into()),
                    Err(rmcp::ServiceError::McpError(refusal)) => {
                        return Ok(mcp::error(refusal.message.into_owned()))
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
                    self.finished.locked().push(format!(
                        "{CALL_FINISHED} {name} returned: {}]",
                        mcp::result_text(result)
                    ));
                }
            }
            outcome
        } else {
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

    async fn answers(
        &self,
        plugin: &Usable,
        requests: InputRequests,
    ) -> Result<InputResponses, String> {
        let mut answers = InputResponses::new();
        for (key, request) in requests {
            let answer = match request {
                InputRequest::Elicitation(request) => {
                    let meta = request.extensions.get::<RequestMetaObject>().cloned();
                    self.answer(plugin, request.params, meta).await?
                }
                _ => ElicitResult::new(ElicitationAction::Decline),
            };
            answers.insert(
                key,
                serde_json::to_value(answer).expect("an ElicitResult always serialises"),
            );
        }
        Ok(answers)
    }

    async fn answer(
        &self,
        plugin: &Usable,
        params: ElicitRequestParams,
        meta: Option<RequestMetaObject>,
    ) -> Result<ElicitResult, String> {
        let declined = ElicitResult::new(ElicitationAction::Decline);
        let ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            meta: in_params,
        } = params
        else {
            return Ok(declined);
        };
        let meta = meta.or(in_params);
        let credentials = &plugin.manifest.access.credentials;
        match questions::read(requested_schema, meta.as_ref(), credentials) {
            None => Ok(declined),
            Some(questions::Question::Spoken { field }) => {
                let question = Question {
                    prompt: message,
                    expects: Expects::Words,
                };
                Ok(
                    match runner::ask(&self.host.commands, question, None).await {
                        Some(Answer::Said(said)) => ElicitResult::new(ElicitationAction::Accept)
                            .with_content(serde_json::json!({ field: said })),
                        _ => declined,
                    },
                )
            }
            Some(questions::Question::Typed(asked)) => {
                let (fields, replies) = asked.split();
                let filled = input::fill(&self.host.commands, &message, fields).await;
                if let Filled::Values(values) = &filled {
                    for (name, value) in replies.secrets(values) {
                        self.host.keep(&plugin.name, name, value).await?;
                    }
                }
                Ok(replies.answer(filled))
            }
        }
    }

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

    pub async fn stop(&self) {
        let running: Vec<_> = self.running.lock().await.drain().collect();
        for (_, client) in running {
            let _ = client.cancel().await;
        }
    }

    fn working(&self) -> Vec<Arc<Usable>> {
        self.plugins
            .read()
            .expect(POISONED)
            .iter()
            .filter(|p| !matches!(self.host.verdict(&p.hash), Some(Err(_))))
            .cloned()
            .collect()
    }
}

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

fn spoken(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
