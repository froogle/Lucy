//! XDG desktop entries, and finding the app a spoken name means.

use std::path::{Path, PathBuf};

/// An installed app, from its `.desktop` file.
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub path: PathBuf,
    pub name: String,
}

/// The app entries in `dirs`.
pub fn read_all(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    for dir in dirs {
        let Ok(files) = std::fs::read_dir(dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "desktop") {
                entries.extend(parse(&path));
            }
        }
    }
    entries
}

fn parse(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
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
        if key.trim_end() == "Name" {
            name = Some(value.trim_start().to_owned());
        }
    }
    Some(DesktopEntry {
        path: path.to_owned(),
        name: name?,
    })
}

/// What a spoken name matched.
pub enum Found<'a> {
    One(&'a DesktopEntry),
    Several(Vec<&'a DesktopEntry>),
    None,
}

/// The entries `spoken` names.
pub fn find<'a>(entries: &'a [DesktopEntry], spoken: &str) -> Found<'a> {
    let spoken = spoken.trim().to_lowercase();
    let matches: Vec<&DesktopEntry> = entries
        .iter()
        .filter(|e| e.name.to_lowercase() == spoken)
        .collect();
    match matches.as_slice() {
        [] => Found::None,
        [one] => Found::One(one),
        _ => Found::Several(matches),
    }
}
