//! Landlock confinement for a Plugin: it may read and run the system, read
//! its own folder and its declared `read` paths, and write only its declared
//! `write` paths and its private state dir.

use std::io;
use std::path::{Path, PathBuf};

use landlock::{
    Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreated,
    RulesetCreatedAttr, RulesetStatus, ABI,
};

use super::manifest::Access as Declared;

const ABI_VERSION: ABI = ABI::V5;

/// Read-only system paths every Plugin needs to run at all.
const SYSTEM: &[&str] = &["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];

/// Makes `command` apply the Plugin's Landlock rules just before it execs.
pub fn confine(
    command: &mut tokio::process::Command,
    folder: &Path,
    state_dir: &Path,
    declared: &Declared,
) -> io::Result<()> {
    let mut ruleset = Some(ruleset(folder, state_dir, declared).map_err(io::Error::other)?);
    // SAFETY: the closure only makes the landlock_restrict_self and prctl
    // syscalls on a ruleset built before the fork.
    unsafe {
        command.pre_exec(move || {
            let ruleset = ruleset
                .take()
                .ok_or_else(|| io::Error::other("sandbox applied twice"))?;
            let status = ruleset.restrict_self().map_err(io::Error::other)?;
            if status.ruleset == RulesetStatus::NotEnforced {
                return Err(io::Error::other("Landlock isn't available"));
            }
            Ok(())
        });
    }
    Ok(())
}

fn ruleset(
    folder: &Path,
    state_dir: &Path,
    declared: &Declared,
) -> Result<RulesetCreated, landlock::RulesetError> {
    let read = AccessFs::from_read(ABI_VERSION);
    let all = AccessFs::from_all(ABI_VERSION);
    let readable = SYSTEM
        .iter()
        .map(PathBuf::from)
        .chain([folder.to_owned()])
        .chain(declared.read.iter().map(|p| expand(p)));
    let writable = declared
        .write
        .iter()
        .map(|p| expand(p))
        .chain([state_dir.to_owned()]);

    let mut ruleset = Ruleset::default().handle_access(all)?.create()?;
    ruleset = add(ruleset, readable, read)?;
    ruleset = add(ruleset, writable, all)?;
    ruleset = add(
        ruleset,
        ["/dev/null".into()],
        AccessFs::from_file(ABI_VERSION),
    )?;
    add(
        ruleset,
        ["/dev/urandom".into()],
        AccessFs::from_read(ABI_VERSION) & AccessFs::from_file(ABI_VERSION),
    )
}

/// Allows `access` beneath each path that exists.
fn add(
    mut ruleset: RulesetCreated,
    paths: impl IntoIterator<Item = PathBuf>,
    access: BitFlags<AccessFs>,
) -> Result<RulesetCreated, landlock::RulesetError> {
    for path in paths {
        let Ok(fd) = PathFd::new(&path) else {
            continue;
        };
        let access = if path.is_dir() {
            access
        } else {
            access & AccessFs::from_file(ABI_VERSION)
        };
        ruleset = ruleset.add_rule(PathBeneath::new(fd, access))?;
    }
    Ok(ruleset)
}

/// Expands a leading `~/` to the home directory.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}
