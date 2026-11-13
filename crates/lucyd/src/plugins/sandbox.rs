//! Landlock confinement for a Plugin: it may read and run the system, read
//! its own folder and its declared `read` paths, and write only its declared
//! `write` paths and its private state dir.
//!
//! An Authoring Delegate, and the build and tests it submits, are confined
//! the same way to their workspace: no user data.

use std::io;
use std::path::{Path, PathBuf};

use landlock::{
    Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreated,
    RulesetCreatedAttr, RulesetStatus, ABI,
};

use super::manifest::Access as Declared;
use crate::adapters::Workspace;

const ABI_VERSION: ABI = ABI::V5;

/// Read-only system paths every Plugin needs to run at all.
const SYSTEM: &[&str] = &["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];

/// What a Delegate's CLI and toolchain read besides the system: process
/// info. `/dev/null` and `/dev/urandom` are always allowed.
const DELEGATE_SYSTEM: &[&str] = &["/proc"];

/// Makes `command` apply the Plugin's Landlock rules just before it execs.
pub fn confine(
    command: &mut tokio::process::Command,
    folder: &Path,
    state_dir: &Path,
    declared: &Declared,
) -> io::Result<()> {
    let readable = [folder.to_owned()]
        .into_iter()
        .chain(declared.read.iter().map(|p| expand(p)));
    let writable = declared
        .write
        .iter()
        .map(|p| expand(p))
        .chain([state_dir.to_owned()]);
    restrict(command.as_std_mut(), readable, writable)
}

/// Makes `command` apply a Delegate's Landlock rules just before it execs:
/// the system and `readable` read-only, `writable` read-write, and nothing
/// else. The network stays open.
pub fn confine_delegate(
    command: &mut std::process::Command,
    readable: &[PathBuf],
    writable: &[PathBuf],
) -> io::Result<()> {
    let readable = DELEGATE_SYSTEM
        .iter()
        .map(PathBuf::from)
        .chain(readable.iter().cloned());
    restrict(command, readable, writable.iter().cloned())
}

/// Makes `command`, a Delegate's CLI `program`, work in `workspace` and
/// apply a Delegate's Landlock rules: it may write the workspace and
/// `cli_home`, where the CLI keeps its login and settings, and read and run
/// `program`.
pub fn confine_cli(
    command: &mut std::process::Command,
    workspace: &Workspace,
    program: &Path,
    cli_home: &[PathBuf],
) -> io::Result<()> {
    command
        .current_dir(&workspace.dir)
        .env("TMPDIR", &workspace.tmp);
    let writable: Vec<PathBuf> = [workspace.dir.clone(), workspace.tmp.clone()]
        .into_iter()
        .chain(cli_home.iter().cloned())
        .chain(workspace.writable.iter().cloned())
        .collect();
    let mut readable = workspace.readable.clone();
    readable.extend(program_dir(program));
    confine_delegate(command, &readable, &writable)
}

/// The user's home directory.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Makes `command` apply Landlock rules allowing the system and `readable`
/// read-only and `writable` read-write, just before it execs.
fn restrict(
    command: &mut std::process::Command,
    readable: impl IntoIterator<Item = PathBuf>,
    writable: impl IntoIterator<Item = PathBuf>,
) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut ruleset = Some(ruleset(readable, writable).map_err(io::Error::other)?);
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
    readable: impl IntoIterator<Item = PathBuf>,
    writable: impl IntoIterator<Item = PathBuf>,
) -> Result<RulesetCreated, landlock::RulesetError> {
    let read = AccessFs::from_read(ABI_VERSION);
    let all = AccessFs::from_all(ABI_VERSION);
    let readable = SYSTEM.iter().map(PathBuf::from).chain(readable);

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
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

/// The directory holding the executable `program` (searched for on `PATH`
/// if it is a bare name), with symlinks resolved, so a sandbox can let it
/// run.
pub fn program_dir(program: &Path) -> Option<PathBuf> {
    let found = if program.components().count() > 1 {
        Some(program.to_owned())
    } else {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(program))
            .find(|path| path.is_file())
    }?;
    Some(std::fs::canonicalize(found).ok()?.parent()?.to_owned())
}
