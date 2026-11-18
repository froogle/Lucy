//! XDG desktop entries, and finding the app a spoken name means.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub path: PathBuf,
    pub name: String,
    pub generic_name: Option<String>,
    pub keywords: Vec<String>,
}

/// Where desktop entries live, most important first:
/// `$XDG_DATA_HOME/applications`, then each of `$XDG_DATA_DIRS`.
pub fn xdg_dirs() -> Vec<PathBuf> {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    std::iter::once(lucy_config::xdg_dir("XDG_DATA_HOME", ".local/share"))
        .chain(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        )
        .map(|dir| dir.join("applications"))
        .collect()
}

pub fn read_all(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in dirs {
        let Ok(files) = std::fs::read_dir(dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "desktop") && seen.insert(file.file_name()) {
                entries.extend(parse(&path));
            }
        }
    }
    entries
}

/// The program the entry `name` runs, from the first of `dirs` holding it.
/// `None` when it has no `Exec` or its program is quoted.
pub fn exec_program(name: &str, dirs: &[PathBuf]) -> Option<String> {
    let text = dirs
        .iter()
        .find_map(|dir| std::fs::read_to_string(dir.join(name)).ok())?;
    let exec = entry_keys(&text).find_map(|(key, value)| (key == "Exec").then_some(value))?;
    exec.split_whitespace()
        .next()
        .filter(|program| !program.starts_with('"'))
        .map(str::to_owned)
}

fn entry_keys(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut in_entry = false;
    text.lines().map(str::trim).filter_map(move |line| {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            return None;
        }
        let (key, value) = line.split_once('=').filter(|_| in_entry)?;
        Some((key.trim_end(), value.trim_start()))
    })
}

fn parse(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut generic_name = None;
    let mut keywords = Vec::new();
    let mut app = false;
    let mut hidden = false;
    for (key, value) in entry_keys(&text) {
        match key {
            "Type" => app = value == "Application",
            "Hidden" | "NoDisplay" => hidden |= value == "true",
            "Name" => name = Some(value.to_owned()),
            "GenericName" => generic_name = Some(value.to_owned()),
            "Keywords" => {
                keywords = value
                    .split(';')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            _ => {}
        }
    }
    if !app || hidden {
        return None;
    }
    Some(DesktopEntry {
        path: path.to_owned(),
        name: name?,
        generic_name,
        keywords,
    })
}

pub enum Found<'a> {
    One(&'a DesktopEntry),
    Several(Vec<&'a DesktopEntry>),
    None,
}

pub fn find<'a>(entries: &'a [DesktopEntry], spoken: &str) -> Found<'a> {
    let spoken = spoken.trim().to_lowercase();
    let is = |text: &str| text.to_lowercase() == spoken;
    let by_name = |e: &DesktopEntry| is(&e.name);
    let by_kind = |e: &DesktopEntry| {
        e.generic_name.as_deref().is_some_and(is) || e.keywords.iter().any(|k| is(k))
    };
    for matches_spoken in [&by_name as &dyn Fn(&DesktopEntry) -> bool, &by_kind] {
        let matches: Vec<&DesktopEntry> = entries.iter().filter(|e| matches_spoken(e)).collect();
        match matches.as_slice() {
            [] => continue,
            [one] => return Found::One(one),
            _ => return Found::Several(matches),
        }
    }
    Found::None
}
