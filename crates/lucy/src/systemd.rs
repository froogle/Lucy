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
    let known = Command::new("systemctl")
        .args(["--user", "cat", "lucyd.service"])
        .env("PATH", &env.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !known {
        install(&env.units, &env.bin)?;
        systemctl(env, &["--user", "daemon-reload"])?;
    }
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
