//! XDG desktop entries, and finding the app a spoken name means.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// An installed app, from its `.desktop` file.
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

/// The apps shown to the user from the entries in `dirs`. An entry in an
/// earlier dir replaces one with the same file name in a later dir, even
/// when the earlier one is hidden.
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

/// The entry at `path`, if it is an app the user can see.
fn parse(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
    let mut generic_name = None;
    let mut keywords = Vec::new();
    let mut app = false;
    let mut hidden = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim_start();
        match key.trim_end() {
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

/// What a spoken name matched.
pub enum Found<'a> {
    One(&'a DesktopEntry),
    Several(Vec<&'a DesktopEntry>),
    None,
}

/// The entries `spoken` names. An app's own name beats a generic name or
/// keyword, so "Firefox" isn't ambiguous just because another app lists
/// "firefox" as a keyword.
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
