//! lucyd: the one process that owns audio, the Session and every child
//! process (ADR 0006). Everything else talks to it over the public socket.

pub mod adapters;
pub mod claude;
mod desktop;
pub mod desktop_entries;
mod ipc;
pub mod launcher;
mod mcp;
mod persona;
pub mod plugins;
mod runner;
mod session;
mod status;
mod system_status;

use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::UnixListener;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

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
    pub plugins: plugins::Paths,
}

/// A running lucyd. Dropping it stops lucyd and removes its sockets.
pub struct Daemon {
    socket_path: PathBuf,
    mcp_socket_path: PathBuf,
    runtime: Option<Runtime>,
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
        plugins,
    } = options;
    let persona = persona::Persona::new(persona_path);
    let mcp_socket_path = lucy_ipc::mcp_socket_path(&socket_path);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let listener = runtime.block_on(async { bind(&socket_path) })?;
    let mcp_listener = runtime.block_on(async { bind(&mcp_socket_path) })?;
    let relays = mcp::Relays::new(lucy_program, mcp_socket_path.clone());
    let status = status::StatusBus::new();
    let desktop = desktop::Desktop {
        launcher: adapters.launcher.clone(),
        applications: adapters.applications.clone(),
    };
    let (sessions, commands) = mpsc::unbounded_channel();
    let providers = ipc::Providers {
        thinking: adapters.thinking.clone(),
        config_path: config_path.clone(),
    };
    runtime.spawn(
        runner::Runner::new(
            commands,
            status.clone(),
            adapters,
            relays.clone(),
            plugins::Host::new(plugins),
            persona.clone(),
            config_path,
        )
        .run(),
    );
    runtime.spawn(mcp::serve(
        mcp_listener,
        relays,
        persona,
        sessions.clone(),
        desktop,
    ));
    runtime.spawn(ipc::serve(listener, status, sessions, providers));

    Ok(Daemon {
        socket_path,
        mcp_socket_path,
        runtime: Some(runtime),
    })
}

impl Daemon {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn mcp_socket_path(&self) -> &Path {
        &self.mcp_socket_path
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
