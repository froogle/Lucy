use std::ffi::OsStr;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::run;

const LIST_THEMES: &str = "omarchy-theme-list";
const CURRENT_THEME: &str = "omarchy-theme-current";
const SET_THEME: &str = "omarchy-theme-set";
const TOGGLE_BAR: &str = "omarchy-toggle-bar";
const TOGGLE_NIGHT_LIGHT: &str = "omarchy-toggle-nightlight";
const CAPTURE_SCREENSHOT: &str = "omarchy-capture-screenshot";
const LOCK: &str = "omarchy-system-lock";
const TOGGLE_IDLE: &str = "omarchy-toggle-idle";

const BAR_SHOWN: &str = "off";
const BAR_HIDDEN: &str = "on";

#[derive(Clone, Copy)]
pub enum Theme {
    List,
    Set,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::List, Theme::Set];

    pub fn as_str(self) -> &'static str {
        match self {
            Theme::List => "list",
            Theme::Set => "set",
        }
    }
}

pub async fn themes(path: &OsStr) -> Result<Value, String> {
    let installed = installed_themes(path).await?;
    let current = run(path, CURRENT_THEME, &[]).await?;
    Ok(json!({"themes": installed, "current": current.trim()}))
}

pub async fn set_theme(path: &OsStr, name: &str) -> Result<String, String> {
    let installed = installed_themes(path).await?;
    let theme = installed
        .iter()
        .find(|theme| theme.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            format!(
                "no theme {name:?} is installed; the installed ones are {}",
                installed.join(", ")
            )
        })?;
    run(path, SET_THEME, &[theme]).await?;
    Ok(theme.clone())
}

async fn installed_themes(path: &OsStr) -> Result<Vec<String>, String> {
    let listed = run(path, LIST_THEMES, &[]).await?;
    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|theme| !theme.is_empty())
        .map(str::to_owned)
        .collect())
}

#[derive(Clone, Copy)]
pub enum Bar {
    Show,
    Hide,
    Toggle,
}

impl Bar {
    pub const ALL: [Bar; 3] = [Bar::Show, Bar::Hide, Bar::Toggle];

    pub fn as_str(self) -> &'static str {
        match self {
            Bar::Show => "show",
            Bar::Hide => "hide",
            Bar::Toggle => "toggle",
        }
    }
}

pub async fn bar(path: &OsStr, action: Bar) -> Result<&'static str, String> {
    let (arg, done) = match action {
        Bar::Show => (BAR_SHOWN, "shown"),
        Bar::Hide => (BAR_HIDDEN, "hidden"),
        Bar::Toggle => ("toggle", "toggled"),
    };
    run(path, TOGGLE_BAR, &[arg]).await?;
    Ok(done)
}

#[derive(Clone, Copy)]
pub enum NightLight {
    On,
    Off,
    Toggle,
}

impl NightLight {
    pub const ALL: [NightLight; 3] = [NightLight::On, NightLight::Off, NightLight::Toggle];

    pub fn as_str(self) -> &'static str {
        match self {
            NightLight::On => "on",
            NightLight::Off => "off",
            NightLight::Toggle => "toggle",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Screenshot {
    Screen,
    Region,
    Window,
}

impl Screenshot {
    pub const ALL: [Screenshot; 3] = [Screenshot::Screen, Screenshot::Region, Screenshot::Window];

    pub fn as_str(self) -> &'static str {
        match self {
            Screenshot::Screen => "screen",
            Screenshot::Region => "region",
            Screenshot::Window => "window",
        }
    }

    fn mode(self) -> &'static str {
        match self {
            Screenshot::Screen => "fullscreen",
            Screenshot::Region => "region",
            Screenshot::Window => "windows",
        }
    }
}

pub async fn screenshot(path: &OsStr, action: Screenshot) -> Result<&'static str, String> {
    run(path, CAPTURE_SCREENSHOT, &[action.mode(), "save"]).await?;
    Ok("saved; Omarchy shows a notification to open or edit it")
}

#[derive(Clone, Copy)]
pub enum IdleLock {
    Lock,
    StayAwake,
    AllowIdle,
}

impl IdleLock {
    pub const ALL: [IdleLock; 3] = [IdleLock::Lock, IdleLock::StayAwake, IdleLock::AllowIdle];

    pub fn as_str(self) -> &'static str {
        match self {
            IdleLock::Lock => "lock",
            IdleLock::StayAwake => "stay_awake",
            IdleLock::AllowIdle => "allow_idle",
        }
    }
}

pub async fn idle_lock(path: &OsStr, action: IdleLock) -> Result<&'static str, String> {
    match action {
        IdleLock::Lock => {
            run(path, LOCK, &[]).await?;
            Ok("locked")
        }
        IdleLock::StayAwake => {
            run(path, TOGGLE_IDLE, &["stay-awake"]).await?;
            Ok("staying awake: no idle lock or screensaver until allowed again")
        }
        IdleLock::AllowIdle => {
            run(path, TOGGLE_IDLE, &["allow-idle"]).await?;
            Ok("idle lock and screensaver allowed again")
        }
    }
}

#[derive(Deserialize)]
struct NightLightStatus {
    enabled: bool,
}

pub async fn night_light(path: &OsStr, action: NightLight) -> Result<&'static str, String> {
    let status = run(path, TOGGLE_NIGHT_LIGHT, &["--status"]).await?;
    let status: NightLightStatus = serde_json::from_str(&status)
        .map_err(|e| format!("{TOGGLE_NIGHT_LIGHT} --status said {status:?}: {e}"))?;
    let wanted = match action {
        NightLight::On => true,
        NightLight::Off => false,
        NightLight::Toggle => !status.enabled,
    };
    if wanted == status.enabled {
        return Ok("already so");
    }
    run(path, TOGGLE_NIGHT_LIGHT, &[]).await?;
    Ok(if wanted { "now on" } else { "now off" })
}
