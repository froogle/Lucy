use std::collections::HashMap;
use std::path::PathBuf;

use std::fmt;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::screen::Point;

const TOP: &str = "2";

const OVERLAY: &str = "3";

pub struct Hyprland {
    socket: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Address(String);

impl Address {
    pub fn new(address: &str) -> Address {
        Address(address.to_owned())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Deserialize)]
pub struct Window {
    pub address: Address,
    pub class: String,
    pub title: String,
    pub workspace: Workspace,
    pub floating: bool,
    pub fullscreen: u8,
    pub pinned: bool,
    pub at: [i32; 2],
    pub size: [i32; 2],
    pub monitor: i64,
    #[serde(default)]
    pub pid: i64,
    #[serde(default)]
    pub xwayland: bool,
    #[serde(default)]
    pub visible: bool,
    #[serde(default, rename = "focusHistoryID")]
    pub focus_history: i64,
}

#[derive(Deserialize)]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub transform: u8,
    pub focused: bool,
}

#[derive(Deserialize)]
pub struct Workspace {
    pub name: String,
}

#[derive(Deserialize)]
pub struct Layers {
    levels: HashMap<String, Vec<Layer>>,
}

#[derive(Deserialize)]
struct Layer {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[derive(Deserialize)]
struct Active {
    address: Option<Address>,
}

impl Hyprland {
    pub fn new(socket: PathBuf) -> Hyprland {
        Hyprland { socket }
    }

    pub async fn windows(&self) -> Result<Vec<Window>, String> {
        self.query("clients").await
    }

    pub async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        self.query("monitors").await
    }

    pub async fn layers(&self) -> Result<Vec<Layers>, String> {
        let monitors: HashMap<String, Layers> = self.query("layers").await?;
        Ok(monitors.into_values().collect())
    }

    pub async fn active_window(&self) -> Result<Option<Address>, String> {
        let active: Active = self.query("activewindow").await?;
        Ok(active.address)
    }

    pub async fn dispatch(&self, dispatcher: &str) -> Result<(), String> {
        let answer = self.request(&format!("dispatch {dispatcher}")).await?;
        match answer.trim() {
            "ok" => Ok(()),
            refused => Err(format!("Hyprland refused it: {refused}")),
        }
    }

    pub async fn exec(&self, command: &str) -> Result<(), String> {
        self.dispatch(&format!("hl.dsp.exec_cmd({})", lua_string(command)))
            .await
    }

    async fn query<T: for<'de> Deserialize<'de>>(&self, what: &str) -> Result<T, String> {
        let answer = self.request(&format!("j/{what}")).await?;
        serde_json::from_str(&answer)
            .map_err(|e| format!("Hyprland's answer to {what} doesn't read: {e}"))
    }

    async fn request(&self, request: &str) -> Result<String, String> {
        let unreachable = |e: std::io::Error| format!("Hyprland can't be reached: {e}");
        let mut stream = UnixStream::connect(&self.socket)
            .await
            .map_err(unreachable)?;
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(unreachable)?;
        let mut answer = String::new();
        stream
            .read_to_string(&mut answer)
            .await
            .map_err(unreachable)?;
        Ok(answer)
    }
}

pub fn window_at(windows: &[Window], at: Point) -> Option<&Window> {
    windows
        .iter()
        .filter(|w| w.visible)
        .filter(|w| contains(w.at, w.size, at))
        .min_by_key(|w| (w.fullscreen == 0, !w.floating, w.focus_history))
}

pub fn over_windows(layers: &[Layers], at: Point) -> bool {
    layers
        .iter()
        .flat_map(|monitor| [TOP, OVERLAY].map(|level| monitor.levels.get(level)))
        .flatten()
        .flatten()
        .any(|layer| contains([layer.x, layer.y], [layer.w, layer.h], at))
}

fn contains(origin: [i32; 2], size: [i32; 2], at: Point) -> bool {
    let (left, top) = (f64::from(origin[0]), f64::from(origin[1]));
    let (right, bottom) = (left + f64::from(size[0]), top + f64::from(size[1]));
    (left..right).contains(&at.x) && (top..bottom).contains(&at.y)
}

pub(crate) fn lua_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
