use std::io;
use std::path::{Path, PathBuf};

use landlock::{
    Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreated,
    RulesetCreatedAttr, RulesetStatus, ABI,
};

use super::manifest::Access as Declared;
use super::Environment;
use crate::adapters::Workspace;

const ABI_VERSION: ABI = ABI::V5;

const SYSTEM: &[&str] = &["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];

const DELEGATE_SYSTEM: &[&str] = &["/proc"];

pub fn confine(
    command: &mut tokio::process::Command,
    folder: &Path,
    state_dir: &Path,
    declared: &Declared,
    environment: &Environment,
) -> io::Result<()> {
    let readable = [folder.to_owned()]
        .into_iter()
        .chain(declared.read.iter().filter_map(|p| expand(p, environment)));
    let writable = declared
        .write
        .iter()
        .filter_map(|p| expand(p, environment))
        .chain([state_dir.to_owned()]);
    restrict(command.as_std_mut(), readable, writable)
}

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

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

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

fn expand(path: &str, environment: &Environment) -> Option<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(home().join(rest));
    }
    let Some(var) = path.strip_prefix('$') else {
        return Some(PathBuf::from(path));
    };
    let (name, rest) = var.split_once('/').unwrap_or((var, ""));
    let dir = PathBuf::from(environment.var(name)?);
    Some(if rest.is_empty() { dir } else { dir.join(rest) })
}

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
