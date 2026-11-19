//! `lucy`'s full screen ([`menu`]): a list of sections, each a page.
//! A page changes a few settings through [`settings`] or tests something
//! inside lucyd. The screen is a model driven by key presses and drawn with
//! ratatui, so tests drive it with scripted keys. [`panel`] is what the
//! Omarchy bar panel reads, through `lucy status --follow --json`, and
//! [`input::hold`] draws its Input dialog through, as `lucy input --json`.

mod credentials;
mod devices;
pub mod input;
mod kokoro;
pub mod menu;
pub mod panel;
mod piper;
mod sections;
pub mod settings;
pub mod systemd;
pub mod ui;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use lucy_ipc::{Client, Role};

/// Where `lucy` finds and puts things. [`Env::of_process`] reads them from
/// the environment; tests give their own.
#[derive(Clone)]
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
    pub omarchy: PathBuf,
    /// The dir holding `lucyd` and `lucy-indicator`.
    pub bin: PathBuf,
    /// Where to read the distro from, for installing Piper.
    pub os_release: PathBuf,
    /// Where Kokoro is installed: its venv, model and voices.
    pub kokoro: PathBuf,
}

impl Env {
    pub fn of_process(os_release: &Path) -> Env {
        let here = std::env::current_exe().unwrap_or_default();
        let config = lucy_config::xdg_dir("XDG_CONFIG_HOME", ".config");
        Env {
            config: lucy_config::default_path(),
            path: std::env::var_os("PATH").unwrap_or_default(),
            socket: lucy_ipc::default_socket_path(),
            units: config.join("systemd/user"),
            omarchy: config.join("omarchy"),
            bin: here.parent().unwrap_or(Path::new("/")).to_owned(),
            os_release: os_release.to_owned(),
            kokoro: lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share").join("lucy/kokoro"),
        }
    }

    /// A client of lucyd.
    ///
    /// # Errors
    ///
    /// As [`Env::connect_as`].
    pub fn connect(&self) -> Result<Client, lucy_ipc::Error> {
        self.connect_as(None)
    }

    /// A client of lucyd that claims `role`.
    ///
    /// # Errors
    ///
    /// `NotRunning` with no runtime dir, where there can be no socket, or when
    /// lucyd doesn't answer on it.
    pub fn connect_as(&self, role: Option<Role>) -> Result<Client, lucy_ipc::Error> {
        let path = self.socket.as_ref().ok_or(lucy_ipc::Error::NotRunning)?;
        Client::connect(path, "lucy", role)
    }
}
