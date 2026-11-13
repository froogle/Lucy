//! lucyd: the one process that owns audio, the Session and every child
//! process (ADR 0006). Everything else talks to it over the public socket.

pub mod adapters;
mod ipc;
mod runner;
mod session;

use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lucy_config::Config;
use lucy_ipc::Status;
use tokio::net::UnixListener;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, watch};

pub struct Options {
    /// Where to listen; normally `lucy_ipc::default_socket_path()`.
    pub socket_path: PathBuf,
    pub config: Config,
    pub adapters: adapters::Adapters,
}

/// A running lucyd. Dropping it stops lucyd and removes the socket.
pub struct Daemon {
    socket_path: PathBuf,
    runtime: Option<Runtime>,
}

/// Starts lucyd on its own runtime and returns once the socket is listening.
pub fn spawn(options: Options) -> io::Result<Daemon> {
    let Options {
        socket_path,
        config,
        adapters,
    } = options;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let listener = runtime.block_on(async { bind(&socket_path) })?;
    let (status, status_rx) = watch::channel(Status {
        state: lucy_ipc::State::Idle,
        session_id: None,
    });
    let (sessions, commands) = mpsc::unbounded_channel();
    runtime.spawn(
        runner::Runner::new(
            commands,
            status,
            adapters,
            Duration::from_secs_f64(config.session.silence_timeout_secs),
        )
        .run(),
    );
    runtime.spawn(ipc::serve(listener, status_rx, sessions));

    Ok(Daemon {
        socket_path,
        runtime: Some(runtime),
    })
}

impl Daemon {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
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
