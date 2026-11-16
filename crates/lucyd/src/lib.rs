//! lucyd: the one process that owns audio, the Session and every child
//! process (ADR 0006). Everything else talks to it over the public socket.

pub mod adapters;
pub mod audio;
mod authoring;
pub mod by_kind;
pub mod claude;
pub mod codex;
mod desktop;
pub mod desktop_entries;
mod input;
mod ipc;
pub mod launcher;
mod limits;
mod mcp;
pub mod model_text;
mod notifications;
pub mod notify_send;
mod persona;
pub mod piper;
mod playback;
pub mod plugins;
mod runner;
mod sentences;
mod session;
mod status;
pub mod superwhisper;
mod sync;
mod system_status;
mod wake;
pub mod whisper;

use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::UnixListener;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

pub use input::TypedAnswer;
pub use lucy_ipc::FieldKind;

pub struct Options {
    /// Where to listen; normally `lucy_ipc::default_socket_path()`.
    pub socket_path: PathBuf,
    /// `config.toml`, read at each Session start. It need not exist.
    pub config_path: PathBuf,
    /// The user's Persona; normally `persona.md` beside `config.toml`.
    pub persona_path: PathBuf,
    /// The `lucy` binary, which model processes run as `lucy mcp-relay`.
    pub lucy_program: PathBuf,
    pub adapters: adapters::Adapters,
    /// Where Plugins and Lucy's state live; normally `plugins::Paths::from_env()`.
    pub plugin_paths: plugins::Paths,
    /// What Plugins are checked against and start with; normally
    /// `plugins::Environment::from_process()`.
    pub plugin_env: plugins::Environment,
}

/// A running lucyd. Dropping it stops lucyd and removes its sockets.
pub struct Daemon {
    socket_path: PathBuf,
    mcp_socket_path: PathBuf,
    runtime: Option<Runtime>,
    plugins: std::sync::Arc<plugins::Host>,
    /// Ended first on shutdown, so no model process outlives lucyd.
    thinking: std::sync::Arc<dyn adapters::Thinking>,
    sessions: mpsc::UnboundedSender<runner::Command>,
}

/// Starts lucyd on its own runtime and returns once its sockets are
/// listening. The private MCP socket sits beside the public one.
pub fn spawn(options: Options) -> io::Result<Daemon> {
    let Options {
        socket_path,
        config_path,
        persona_path,
        lucy_program,
        adapters,
        plugin_paths,
        plugin_env,
    } = options;
    let persona = persona::Persona::new(persona_path);
    let mcp_socket_path = lucy_ipc::mcp_socket_path(&socket_path);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let listener = runtime.block_on(async { bind(&socket_path) })?;
    let mcp_listener = runtime.block_on(async { bind(&mcp_socket_path) })?;
    let page = [
        lucy_program.display().to_string(),
        "input".to_owned(),
        "--socket".to_owned(),
        socket_path.display().to_string(),
    ];
    let relays = mcp::Relays::new(lucy_program, mcp_socket_path.clone());
    let buses = status::Buses {
        status: status::StatusBus::new(),
        levels: status::LevelBus::new(),
        plugins: status::ChangeBus::default(),
        dialogs: input::Dialogs::new(page.to_vec()),
    };
    let (sessions, commands) = mpsc::unbounded_channel();
    let (indicators, fallback) =
        notifications::fallback(&buses.status, adapters.notifier.clone(), sessions.clone());
    runtime.spawn(fallback);
    let plugins_dir = plugin_paths.user.clone();
    let plugin_host = plugins::Host::new(
        plugin_paths,
        sessions.clone(),
        buses.plugins.clone(),
        adapters.credentials.clone(),
        plugin_env,
    );
    let thinking = adapters.thinking.clone();
    let authoring = authoring::Authoring::new(
        &adapters,
        relays.clone(),
        config_path.clone(),
        plugins_dir,
        plugin_host.clone(),
        sessions.clone(),
    );
    let providers = ipc::Providers {
        thinking: adapters.thinking.clone(),
        config_path: config_path.clone(),
    };
    runtime.spawn(
        runner::Runner::new(
            commands,
            buses.clone(),
            adapters,
            relays.clone(),
            plugin_host.clone(),
            runner::SessionFiles {
                persona: persona.clone(),
                config_path,
            },
            authoring.clone(),
        )
        .run(),
    );
    runtime.spawn(mcp::serve(
        mcp_listener,
        relays,
        persona,
        sessions.clone(),
        authoring,
    ));
    runtime.spawn(ipc::serve(
        listener,
        buses,
        indicators,
        sessions.clone(),
        providers,
        plugin_host.clone(),
    ));

    Ok(Daemon {
        socket_path,
        mcp_socket_path,
        runtime: Some(runtime),
        plugins: plugin_host,
        thinking,
        sessions,
    })
}

impl Daemon {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn mcp_socket_path(&self) -> &Path {
        &self.mcp_socket_path
    }

    /// Asks the user's spoken Approval of the Plugin `plugin` as it is now,
    /// in the open Session (setup approves shipped Plugins this way). The
    /// answer is `Ok(true)` once they said yes and it is recorded, or why
    /// they couldn't be asked. Approvals are granted only by voice, so no
    /// IPC request or model tool leads here.
    pub fn request_approval(&self, plugin: &str) -> oneshot::Receiver<Result<bool, String>> {
        let (reply, answer) = oneshot::channel();
        let host = self.plugins.clone();
        let plugin = plugin.to_owned();
        let runtime = self.runtime.as_ref().expect("runtime lives until drop");
        runtime.spawn(async move {
            let _ = reply.send(host.request_approval(&plugin).await);
        });
        answer
    }

    /// Asks `question` in the Input dialog, in the open Session, as the
    /// `ask_typed` tool does: one field of `kind`, with `choices` for a
    /// choice. With no Session open, it is cancelled.
    pub fn ask_typed(
        &self,
        question: &str,
        kind: FieldKind,
        choices: Vec<String>,
    ) -> oneshot::Receiver<TypedAnswer> {
        let (reply, answer) = oneshot::channel();
        let sessions = self.sessions.clone();
        let question = question.to_owned();
        let runtime = self.runtime.as_ref().expect("runtime lives until drop");
        runtime.spawn(async move {
            let _ = reply.send(input::ask(&sessions, &question, kind, choices).await);
        });
        answer
    }

    /// Blocks until SIGINT or SIGTERM.
    pub fn wait_for_shutdown_signal(&self) -> io::Result<()> {
        use tokio::signal::unix::{signal, SignalKind};
        let runtime = self.runtime.as_ref().expect("runtime lives until drop");
        runtime.block_on(async {
            let mut term = signal(SignalKind::terminate())?;
            tokio::select! {
                r = tokio::signal::ctrl_c() => r,
                _ = term.recv() => Ok(()),
            }
        })
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.thinking.shutdown();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(&self.mcp_socket_path);
    }
}

/// Binds the socket with mode 0600 inside a 0700 runtime dir.
fn bind(socket_path: &Path) -> io::Result<UnixListener> {
    if let Some(dir) = socket_path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    if std::os::unix::net::UnixStream::connect(socket_path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("lucyd is already running on {}", socket_path.display()),
        ));
    }
    // A socket file left by a lucyd that didn't shut down cleanly.
    match std::fs::remove_file(socket_path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}
