use std::ffi::OsStr;

use tokio::process::Command;

const CURRENT: &str = "omarchy-hyprland-workspace-layout-current";
const TOGGLE: &str = "omarchy-hyprland-workspace-layout-toggle";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tiling,
    Scrolling,
    Floating,
}

impl Mode {
    pub fn parse(name: &str) -> Option<Mode> {
        Some(match name {
            "tiling" => Mode::Tiling,
            "scrolling" => Mode::Scrolling,
            "floating" => Mode::Floating,
            _ => return None,
        })
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
    for _ in 0..3 {
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
