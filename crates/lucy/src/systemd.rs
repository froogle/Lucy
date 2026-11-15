//! Running Lucy as systemd user services tied to the graphical session.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::Env;

const UNITS: [(&str, &str, &str); 2] = [
    (
        "lucyd.service",
        "lucyd",
        include_str!("../../../dist/systemd/lucyd.service"),
    ),
    (
        "lucy-indicator.service",
        "lucy-indicator",
        include_str!("../../../dist/systemd/lucy-indicator.service"),
    ),
];

/// Enables and starts `lucyd.service` and `lucy-indicator.service`. If
/// systemd doesn't know them (Lucy wasn't installed as a package), they are
/// written to the user's unit dir first, running the programs in
/// `env.bin`. systemctl's output is kept off the full screen.
pub fn enable(env: &Env) -> Result<(), String> {
    make_known(env)?;
    systemctl(
        env,
        &[
            "--user",
            "enable",
            "--now",
            "lucyd.service",
            "lucy-indicator.service",
        ],
    )
}

/// Stops both services starting at login; they keep running until stopped.
pub fn disable(env: &Env) -> Result<(), String> {
    systemctl(
        env,
        &[
            "--user",
            "disable",
            "lucyd.service",
            "lucy-indicator.service",
        ],
    )
}

/// Whether `lucyd.service` starts at login.
pub fn is_enabled(env: &Env) -> bool {
    quietly(env, &["--user", "is-enabled", "lucyd.service"])
}

/// Starts, stops or restarts `lucyd.service`, writing the units first if
/// systemd doesn't know them.
pub fn run(env: &Env, verb: Verb) -> Result<(), String> {
    make_known(env)?;
    let verb = match verb {
        Verb::Start => "start",
        Verb::Stop => "stop",
        Verb::Restart => "restart",
    };
    systemctl(env, &["--user", verb, "lucyd.service"])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    Start,
    Stop,
    Restart,
}

/// Writes the units if systemd doesn't know them.
fn make_known(env: &Env) -> Result<(), String> {
    if !quietly(env, &["--user", "cat", "lucyd.service"]) {
        install(&env.units, &env.bin)?;
        systemctl(env, &["--user", "daemon-reload"])?;
    }
    Ok(())
}

/// Whether `systemctl args` succeeds, its output kept off the screen.
fn quietly(env: &Env, args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .env("PATH", &env.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Writes the units into `dir`, running the programs in `bin`.
fn install(dir: &Path, bin: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    for (name, program, text) in UNITS {
        let exec = format!("ExecStart={}", bin.join(program).display());
        let text: String = text
            .lines()
            .map(|line| match line.starts_with("ExecStart=") {
                true => format!("{exec}\n"),
                false => format!("{line}\n"),
            })
            .collect();
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn systemctl(env: &Env, args: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(args)
        .env("PATH", &env.path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run systemctl: {e}"))?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!(
            "systemctl {} failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}
