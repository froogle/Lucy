use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;

use super::manifest::Requires;

/// The env vars Plugins start with and are checked against: lucyd's own.
#[derive(Debug, Clone)]
pub struct Environment(HashMap<OsString, OsString>);

impl Environment {
    /// lucyd's process environment.
    pub fn from_process() -> Environment {
        Environment(std::env::vars_os().collect())
    }

    /// Just the env vars `vars`, as `(name, value)`.
    pub fn new<'a>(vars: impl IntoIterator<Item = (&'a str, &'a str)>) -> Environment {
        Environment(
            vars.into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        )
    }

    pub(super) fn var(&self, name: &str) -> Option<&OsStr> {
        self.0.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    /// Whether a Plugin with `requires` applies here.
    pub(super) fn meets(&self, requires: &Requires) -> bool {
        requires.env.iter().all(|name| self.var(name).is_some())
            && requires.commands.iter().all(|name| self.on_path(name))
    }

    fn on_path(&self, command: &str) -> bool {
        let Some(path) = self.var("PATH") else {
            return false;
        };
        std::env::split_paths(path).any(|dir| {
            std::fs::metadata(dir.join(command))
                .is_ok_and(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
        })
    }
}
