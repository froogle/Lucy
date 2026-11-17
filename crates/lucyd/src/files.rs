use std::cmp::Reverse;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use ignore::WalkBuilder;
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde_json::{json, Value};

use crate::adapters::Launcher;
use crate::mcp::{error, text, tool};

const MAX_FOUND: usize = 20;

#[derive(Clone)]
pub struct Search {
    roots: Vec<PathBuf>,
    skip: Vec<String>,
    depth: Option<usize>,
}

#[derive(Clone)]
pub struct Words(Vec<String>);

pub struct Found {
    pub path: PathBuf,
    pub kind: Kind,
    pub modified: SystemTime,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Folder,
    File,
}

impl Words {
    pub fn new<'a>(words: impl IntoIterator<Item = &'a str>) -> Option<Words> {
        let words: Vec<String> = words
            .into_iter()
            .flat_map(str::split_whitespace)
            .map(str::to_lowercase)
            .collect();
        (!words.is_empty()).then_some(Words(words))
    }

    pub fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.0.iter().all(|word| name.contains(word))
    }
}

impl Search {
    pub fn new(roots: &[PathBuf], home: &Path, skip: &[String]) -> Search {
        let mut roots: Vec<PathBuf> = roots.iter().filter_map(|root| expand(root, home)).collect();
        roots.dedup();
        Search {
            roots,
            skip: skip.to_vec(),
            depth: None,
        }
    }

    #[expect(
        dead_code,
        reason = "the Hand-off folder search (v0.2 ticket 03) walks three levels deep"
    )]
    pub fn depth(self, depth: usize) -> Search {
        Search {
            depth: Some(depth),
            ..self
        }
    }

    pub fn find(&self, words: &Words) -> Vec<Found> {
        let Some((first, rest)) = self.roots.split_first() else {
            return vec![];
        };
        let mut walk = WalkBuilder::new(first);
        for root in rest {
            walk.add(root);
        }
        let skip = self.skip.clone();
        let inside_match = words.clone();
        walk.standard_filters(false)
            .hidden(true)
            .max_depth(self.depth)
            .filter_entry(move |entry| {
                let name = entry.file_name().to_string_lossy();
                let parent_matched = entry.depth() > 1
                    && entry
                        .path()
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|parent| inside_match.matches(&parent.to_string_lossy()));
                !skip.iter().any(|skipped| *skipped == name) && !parent_matched
            });
        let mut found: Vec<Found> = walk
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.depth() > 0)
            .filter(|entry| words.matches(&entry.file_name().to_string_lossy()))
            .filter_map(|entry| {
                let metadata = std::fs::metadata(entry.path()).ok()?;
                Some(Found {
                    kind: if metadata.is_dir() {
                        Kind::Folder
                    } else {
                        Kind::File
                    },
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    path: entry.into_path(),
                })
            })
            .collect();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found.dedup_by(|a, b| a.path == b.path);
        found
    }

    pub fn is_under(&self, found: &Path, near: &Words) -> bool {
        self.roots.iter().any(|root| {
            found
                .strip_prefix(root)
                .ok()
                .and_then(Path::parent)
                .is_some_and(|folders| {
                    folders
                        .components()
                        .any(|folder| near.matches(&folder.as_os_str().to_string_lossy()))
                })
        })
    }

    pub fn contains(&self, path: &Path) -> bool {
        let Ok(path) = std::fs::canonicalize(path) else {
            return false;
        };
        self.roots
            .iter()
            .any(|root| std::fs::canonicalize(root).is_ok_and(|root| path.starts_with(root)))
    }
}

fn expand(root: &Path, home: &Path) -> Option<PathBuf> {
    let root = match root.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => root.to_owned(),
    };
    let climbs = root.components().any(|c| c == Component::ParentDir);
    (root.is_absolute() && !climbs).then_some(root)
}

#[derive(Clone)]
pub struct Files {
    pub launcher: Arc<dyn Launcher>,
    pub search: Search,
}

impl Files {
    pub fn tools() -> Vec<Tool> {
        vec![
            tool(
                "find_files",
                "Find the user's files and folders by name. A name matches when it contains \
                 every word, in any order and any case. Pass only the words of the name: drop \
                 filler such as \"files\", \"my\" or \"the\". `near` is a folder the user named \
                 (\"documents\"); matches inside it come first. Returns `found` with at most 20 \
                 items (folders first, then newest) and the total, or `not_found`. Say what you \
                 found by name and where, not the full path.",
                json!({
                    "type": "object",
                    "properties": {
                        "words": {"type": "array", "items": {"type": "string"}, "description": "Words of the name"},
                        "near": {"type": "string", "description": "A folder the user named, if any"},
                    },
                    "required": ["words"],
                }),
            ),
            tool(
                "show_files",
                "Open the file manager with these items highlighted. Paths from `find_files`.",
                json!({
                    "type": "object",
                    "properties": {
                        "paths": {"type": "array", "items": {"type": "string"}, "description": "Absolute paths"},
                    },
                    "required": ["paths"],
                }),
            ),
            tool(
                "open_file",
                "Open one file in its default app. A path from `find_files`. Programs and \
                 desktop entries are refused.",
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "An absolute path"}},
                    "required": ["path"],
                }),
            ),
        ]
    }

    pub async fn call(&self, name: &str, arguments: &JsonObject) -> Option<CallToolResult> {
        match name {
            "find_files" => Some(self.find(arguments).await),
            "show_files" => Some(self.show(arguments).await),
            "open_file" => Some(self.open(arguments).await),
            _ => None,
        }
    }

    async fn find(&self, arguments: &JsonObject) -> CallToolResult {
        let Some(words) = Words::new(strings(arguments.get("words")).iter().map(String::as_str))
        else {
            return error("Give at least one word of the name.");
        };
        let near = arguments
            .get("near")
            .and_then(Value::as_str)
            .and_then(|near| Words::new([near]));
        let search = self.search.clone();
        let found = tokio::task::spawn_blocking(move || {
            let mut found = search.find(&words);
            found.sort_by_key(|found| {
                let elsewhere = !near
                    .as_ref()
                    .is_some_and(|near| search.is_under(&found.path, near));
                (elsewhere, found.kind, Reverse(found.modified))
            });
            found
        })
        .await;
        match found {
            Ok(found) if found.is_empty() => {
                CallToolResult::structured(json!({"result": "not_found"}))
            }
            Ok(found) => {
                let items: Vec<Value> = found
                    .iter()
                    .take(MAX_FOUND)
                    .map(|found| {
                        let kind = match found.kind {
                            Kind::Folder => "folder",
                            Kind::File => "file",
                        };
                        json!({"path": found.path, "kind": kind})
                    })
                    .collect();
                CallToolResult::structured(
                    json!({"result": "found", "total": found.len(), "items": items}),
                )
            }
            Err(e) => error(format!("Couldn't search: {e}")),
        }
    }

    async fn show(&self, arguments: &JsonObject) -> CallToolResult {
        let paths: Vec<PathBuf> = strings(arguments.get("paths"))
            .into_iter()
            .map(PathBuf::from)
            .collect();
        if paths.is_empty() || !paths.iter().all(|path| path.is_absolute()) {
            return error("Give the absolute paths `find_files` returned.");
        }
        let launcher = self.launcher.clone();
        match tokio::task::spawn_blocking(move || launcher.show_items(&paths)).await {
            Ok(Ok(())) => text("Shown."),
            Ok(Err(e)) => error(format!("Couldn't show them: {e}")),
            Err(e) => error(format!("Couldn't show them: {e}")),
        }
    }

    async fn open(&self, arguments: &JsonObject) -> CallToolResult {
        let path = PathBuf::from(
            arguments
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        if let Err(refusal) = self.openable(&path) {
            return error(refusal);
        }
        let launcher = self.launcher.clone();
        match tokio::task::spawn_blocking(move || launcher.open_file(&path)).await {
            Ok(Ok(())) => text("Opened."),
            Ok(Err(e)) => error(format!("Couldn't open it: {e}")),
            Err(e) => error(format!("Couldn't open it: {e}")),
        }
    }

    fn openable(&self, path: &Path) -> Result<(), &'static str> {
        let outside = "Only files found under the search roots can be opened.";
        if !path.is_absolute() || !self.search.contains(path) {
            return Err(outside);
        }
        let (Ok(target), Ok(metadata)) = (std::fs::canonicalize(path), std::fs::metadata(path))
        else {
            return Err(outside);
        };
        let desktop_entry = [path, &target].iter().any(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("desktop"))
        });
        let executable = metadata.is_file() && metadata.permissions().mode() & 0o111 != 0;
        if desktop_entry || executable {
            return Err("Programs and desktop entries can't be opened this way.");
        }
        Ok(())
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(one)) => vec![one.clone()],
        _ => vec![],
    }
}
