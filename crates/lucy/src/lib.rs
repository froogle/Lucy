//! `lucy`'s full-screen pages: `lucy` with no arguments ([`home`]) and
//! `lucy setup` ([`setup`]). Each is a model driven by key presses and
//! drawn with ratatui, so tests drive them with scripted keys.

pub mod home;
mod piper;
pub mod setup;
mod systemd;
pub mod ui;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use lucy_ipc::Client;

/// Where setup finds and puts things. [`Env::of_process`] reads them from
/// the environment; tests give their own.
pub struct Env {
    /// `config.toml`.
    pub config: PathBuf,
    /// The `PATH` tools are found and run on.
    pub path: OsString,
    /// lucyd's public socket; None if there is no runtime dir.
    pub socket: Option<PathBuf>,
    /// The systemd user unit dir, where units go if systemd doesn't know
    /// them.
    pub units: PathBuf,
    /// The dir holding `lucyd` and `lucy-indicator`.
    pub bin: PathBuf,
    /// Where to read the distro from, for installing Piper.
    pub os_release: PathBuf,
}

impl Env {
    pub fn of_process(os_release: &Path) -> Env {
        let here = std::env::current_exe().unwrap_or_default();
        Env {
            config: lucy_config::default_path(),
            path: std::env::var_os("PATH").unwrap_or_default(),
            socket: lucy_ipc::default_socket_path(),
            units: lucy_config::xdg_dir("XDG_CONFIG_HOME", ".config").join("systemd/user"),
            bin: here.parent().unwrap_or(Path::new("/")).to_owned(),
            os_release: os_release.to_owned(),
        }
    }

    /// A client of lucyd.
    pub fn connect(&self) -> Result<Client, lucy_ipc::Error> {
        // With no runtime dir there can be no socket, so lucyd isn't running.
        let path = self.socket.as_ref().ok_or(lucy_ipc::Error::NotRunning)?;
        Client::connect(path, "lucy", None)
    }
}
