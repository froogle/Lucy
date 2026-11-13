//! Finding Piper, and installing it after the user's yes (ADR 0003): the
//! distro's package if there is one and it installs cleanly, otherwise
//! `uv tool install`, then `pipx`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Piper with its `http` extra, which lucyd runs as a server.
const PACKAGE: &str = "piper-tts[http]";

/// Whether a Python that lucyd would find can import Piper's HTTP server.
/// `PYTHONPATH` is cleared, as lucyd clears it.
pub fn installed() -> bool {
    python_candidates().iter().any(|python| {
        Command::new(python)
            .args(["-c", "import piper.http_server"])
            .env_remove("PYTHONPATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// The interpreter behind a `piper` script on `PATH`, then `python3`, as
/// lucyd looks for them.
fn python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(script) = on_path("piper") {
        if let Some(interpreter) = shebang(&script) {
            candidates.push(interpreter);
        }
    }
    candidates.push("python3".into());
    candidates
}

fn shebang(script: &Path) -> Option<PathBuf> {
    let mut head = [0; 256];
    let n = std::fs::File::open(script).ok()?.read(&mut head).ok()?;
    let line = std::str::from_utf8(&head[..n]).ok()?.lines().next()?;
    Some(line.strip_prefix("#!")?.split_whitespace().next()?.into())
}

/// `program` on `PATH`, if it is there.
pub fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// The commands to try, in order, until one installs Piper. `os_release`
/// is the text of `/etc/os-release`; `has` says whether a program is on
/// `PATH`.
pub fn install_plan(os_release: &str, has: impl Fn(&str) -> bool) -> Vec<Vec<String>> {
    let mut plan = Vec::new();
    if let Some(command) = distro_package(os_release, &has) {
        plan.push(command);
    }
    if has("uv") {
        plan.push(words(&["uv", "tool", "install", PACKAGE]));
    }
    if has("pipx") {
        plan.push(words(&["pipx", "install", PACKAGE]));
    }
    plan
}

/// The distro's own way to install Piper, from os-release's `ID`, then
/// `ID_LIKE`. Only Arch's AUR carries a `piper-tts` package, through the
/// user's AUR helper.
fn distro_package(os_release: &str, has: &impl Fn(&str) -> bool) -> Option<Vec<String>> {
    let field = |key: &str| {
        os_release.lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix('=')?;
            Some(value.trim_matches('"').to_owned())
        })
    };
    let ids = field("ID").into_iter().chain(
        field("ID_LIKE")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
    );
    for id in ids {
        if id == "arch" {
            let helper = ["paru", "yay"].into_iter().find(|helper| has(helper))?;
            return Some(words(&[helper, "-S", "--needed", "piper-tts"]));
        }
    }
    None
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has<'a>(programs: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        |program| programs.contains(&program)
    }

    #[test]
    fn on_arch_the_aur_package_comes_first_through_the_users_helper() {
        let plan = install_plan(
            "NAME=\"Arch Linux\"\nID=arch\n",
            has(&["yay", "uv", "pipx"]),
        );

        assert_eq!(
            plan,
            [
                words(&["yay", "-S", "--needed", "piper-tts"]),
                words(&["uv", "tool", "install", "piper-tts[http]"]),
                words(&["pipx", "install", "piper-tts[http]"]),
            ]
        );
    }

    #[test]
    fn a_distro_like_arch_is_read_from_id_like() {
        let plan = install_plan("ID=omarchy\nID_LIKE=\"arch\"\n", has(&["paru"]));

        assert_eq!(plan, [words(&["paru", "-S", "--needed", "piper-tts"])]);
    }

    #[test]
    fn elsewhere_uv_then_pipx_and_only_those_installed() {
        let os_release = "ID=debian\n";

        assert_eq!(
            install_plan(os_release, has(&["pipx"])),
            [words(&["pipx", "install", "piper-tts[http]"])]
        );
        assert!(install_plan(os_release, has(&[])).is_empty());
        // Arch with no AUR helper.
        assert!(install_plan("ID=arch\n", has(&[])).is_empty());
    }
}
