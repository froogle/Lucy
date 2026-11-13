//! The Plugin host (ADRs 0002, 0004, 0005): finds Plugins, keeps only the
//! approved, working ones, and runs each in its sandbox on first use in a
//! Session, forwarding the model's calls to it over MCP stdio.

mod manifest;
mod sandbox;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::mcp;
use manifest::Manifest;

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

    fn state_dir(&self, plugin: &str) -> PathBuf {
        self.state.join("plugins").join(plugin)
    }
}

/// `approvals.toml`.
#[derive(Default, Deserialize)]
struct Approvals {
    #[serde(default)]
    plugins: HashMap<String, Approval>,
}

#[derive(Deserialize)]
struct Approval {
    /// The `content_hash` that was approved.
    hash: String,
    /// Each tool's input schema as approved, as JSON.
    schemas: HashMap<String, String>,
}

/// Lives as long as lucyd. Remembers which approved Plugin contents work.
pub struct Host {
    paths: Paths,
    /// By content hash: whether that content matched its Manifest and has
    /// not failed a call, or why not.
    verdicts: Mutex<HashMap<String, Result<(), String>>>,
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
    pub fn new(paths: Paths) -> Arc<Host> {
        Arc::new(Host {
            paths,
            verdicts: Mutex::default(),
        })
    }

    /// Scans the Plugin folders and returns the tools for a new Session.
    pub async fn open_session(self: &Arc<Self>) -> Arc<SessionTools> {
        let mut usable = Vec::new();
        let mut broken = Vec::new();
        for (name, folder) in self.folders() {
            match self.approved(&name, &folder) {
                Ok(Some(plugin)) => usable.push(plugin),
                Ok(None) => {}
                Err(reason) => broken.push(broken_message(&name, &reason)),
            }
        }
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
        Arc::new(SessionTools {
            host: self.clone(),
            plugins: working,
            broken,
            running: tokio::sync::Mutex::default(),
        })
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

    /// The Plugin, if its exact content is approved.
    fn approved(&self, name: &str, folder: &Path) -> Result<Option<Usable>, String> {
        let text = std::fs::read_to_string(folder.join("manifest.toml"))
            .map_err(|e| format!("its Manifest can't be read: {e}"))?;
        let manifest = Manifest::parse(&text, name)?;
        let executable = std::fs::read(folder.join(manifest.executable()))
            .map_err(|e| format!("its executable can't be read: {e}"))?;
        let hash = content_hash(text.as_bytes(), &executable);

        let approvals: Approvals = match std::fs::read_to_string(self.paths.approvals()) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("approvals.toml: {e}"))?,
            Err(_) => Approvals::default(),
        };
        let Some(approval) = approvals.plugins.get(name).filter(|a| a.hash == hash) else {
            return Ok(None);
        };
        let mut schemas = HashMap::new();
        for tool in &manifest.tools {
            let schema = approval
                .schemas
                .get(&tool.name)
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| format!("its Approval has no input schema for {}", tool.name))?;
            schemas.insert(tool.name.clone(), Arc::new(schema));
        }
        Ok(Some(Usable {
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
            let client = self.start(plugin).await?;
            let listed = tokio::time::timeout(START_TIMEOUT, client.list_all_tools())
                .await
                .map_err(|_| "it didn't list its tools in time".to_owned())?
                .map_err(|e| format!("it failed to list its tools: {e}"))?;
            let _ = client.cancel().await;
            let live: HashSet<&str> = listed.iter().map(|tool| &*tool.name).collect();
            let declared: HashSet<&str> = plugin.manifest.tools.iter().map(|t| &*t.name).collect();
            if live != declared {
                return Err(format!(
                    "its Manifest lists the tools {} but it offers {}",
                    names(&declared),
                    names(&live)
                ));
            }
            Ok(())
        }
        .await;
        self.verdicts
            .lock()
            .unwrap()
            .insert(plugin.hash.clone(), verdict.clone());
        verdict
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
        tokio::time::timeout(START_TIMEOUT, ().serve(transport))
            .await
            .map_err(|_| "it didn't start in time".to_owned())?
            .map_err(|e| format!("it didn't start: {e}"))
    }
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
    plugins: Vec<Usable>,
    /// Why each Plugin the scan left out was left out.
    broken: Vec<String>,
    running: tokio::sync::Mutex<HashMap<String, RunningService<RoleClient, ()>>>,
}

impl SessionTools {
    /// For the model's instructions: which Plugins the scan left out and
    /// why, if any.
    pub fn scan_report(&self) -> Option<String> {
        (!self.broken.is_empty()).then(|| {
            format!(
                "## Broken Plugins (not part of your Persona)\n\n{}\n\nTell the user about this once, \
                 briefly, in your first reply.",
                self.broken.join("\n")
            )
        })
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
        let plugin = self
            .working()
            .find(|p| p.name == plugin_name && p.manifest.tools.iter().any(|t| t.name == tool))?;
        let outcome = async {
            let peer = {
                let mut running = self.running.lock().await;
                if !running.contains_key(plugin_name) {
                    let client = self.host.start(plugin).await?;
                    running.insert(plugin_name.to_owned(), client);
                }
                running[plugin_name].peer().clone()
            };
            let mut request = CallToolRequestParams::new(tool.to_owned());
            if let Some(arguments) = arguments {
                request = request.with_arguments(arguments);
            }
            match peer.call_tool(request).await {
                Ok(result) => Ok(result),
                // The Plugin answered, refusing the call: it still works.
                Err(rmcp::ServiceError::McpError(e)) => Ok(mcp::error(e.message.into_owned())),
                Err(e) => Err(format!("its call failed: {e}")),
            }
        }
        .await;
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

    /// Stops every Plugin started in this Session.
    pub async fn stop(&self) {
        let running: Vec<_> = self.running.lock().await.drain().collect();
        for (_, client) in running {
            let _ = client.cancel().await;
        }
    }

    fn working(&self) -> impl Iterator<Item = &Usable> {
        self.plugins
            .iter()
            .filter(|p| !matches!(self.host.verdict(&p.hash), Some(Err(_))))
    }
}
