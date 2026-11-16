use std::ffi::OsStr;

use tokio::process::Command;

const CURRENT: &str = "omarchy-hyprland-workspace-layout-current";
const TOGGLE: &str = "omarchy-hyprland-workspace-layout-toggle";

const MOST_TOGGLES: usize = Mode::ALL.len();

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tiling,
    Scrolling,
    Floating,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Tiling, Mode::Scrolling, Mode::Floating];

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Tiling => "tiling",
            Mode::Scrolling => "scrolling",
            Mode::Floating => "floating",
        }
    }

    pub fn parse(name: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|mode| mode.as_str() == name)
    }

    fn from_omarchy(name: &str) -> Option<Mode> {
        match name {
            "dwindle" => Some(Mode::Tiling),
            "scrolling" => Some(Mode::Scrolling),
            "floating" => Some(Mode::Floating),
            _ => None,
        }
    }

    fn back(self) -> Mode {
        match self {
            Mode::Tiling => Mode::Floating,
            Mode::Scrolling => Mode::Tiling,
            Mode::Floating => Mode::Scrolling,
        }
    }
}

pub async fn set(path: &OsStr, mode: Mode) -> Result<bool, String> {
    let mut changed = false;
    for _ in 0..MOST_TOGGLES {
        let current = run(path, CURRENT, &[]).await?;
        let current = Mode::from_omarchy(current.trim());
        if current == Some(mode) {
            return Ok(changed);
        }
        let back = current.is_some_and(|current| current.back() == mode);
        run(path, TOGGLE, if back { &["back"] } else { &[] }).await?;
        changed = true;
    }
    Err("Omarchy's layout didn't change to it".into())
}

async fn run(path: &OsStr, command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .env("PATH", path)
        .output()
        .await
        .map_err(|e| format!("{command} can't run: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
