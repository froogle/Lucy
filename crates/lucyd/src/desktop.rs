//! The desktop Built-in tools: `open_url`, `launch_app` and `system_status`.
//! None of them needs Confirmation.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
use serde_json::json;

use crate::adapters::Launcher;
use crate::desktop_entries::{self, Found};
use crate::system_status;

/// What the desktop tools act on.
#[derive(Clone)]
pub struct Desktop {
    pub launcher: Arc<dyn Launcher>,
    /// Where to look for desktop entries, most important first.
    pub applications: Vec<PathBuf>,
}

impl Desktop {
    pub fn tools() -> Vec<Tool> {
        vec![
            tool(
                "open_url",
                "Open a web page in the user's browser. Only http and https URLs.",
                serde_json::json!({
                    "type": "object",
                    "properties": {"url": {"type": "string", "description": "An http or https URL"}},
                    "required": ["url"],
                }),
            ),
            tool(
                "launch_app",
                "Start an installed app by its name, generic name (\"web browser\") or a keyword. \
                 Returns `launched`, `candidates` to ask the user to choose from, or `not_found` \
                 when no installed app matches.",
                serde_json::json!({
                    "type": "object",
                    "properties": {"name": {"type": "string", "description": "The app, as the user said it"}},
                    "required": ["name"],
                }),
            ),
            tool(
                "system_status",
                "Facts about this machine: battery, disk space, memory and CPU load, network, \
                 volume, the date and time, or uptime.",
                serde_json::json!({
                    "type": "object",
                    "properties": {"topic": {"type": "string", "enum": system_status::TOPICS}},
                    "required": ["topic"],
                }),
            ),
        ]
    }

    /// Runs the desktop tool `name`, or returns `None` if there is none.
    pub async fn call(&self, name: &str, arguments: &JsonObject) -> Option<CallToolResult> {
        match name {
            "open_url" => Some(self.open_url(string(arguments, "url")).await),
            "launch_app" => Some(self.launch_app(string(arguments, "name")).await),
            "system_status" => Some(system_status(string(arguments, "topic")).await),
            _ => None,
        }
    }

    async fn open_url(&self, url: String) -> CallToolResult {
        if !is_web_url(&url) {
            return error("Only http and https URLs can be opened.");
        }
        let launcher = self.launcher.clone();
        let opened = tokio::task::spawn_blocking(move || launcher.open_url(&url)).await;
        match opened {
            Ok(Ok(())) => text("Opened."),
            Ok(Err(e)) => error(format!("Couldn't open it: {e}")),
            Err(e) => error(format!("Couldn't open it: {e}")),
        }
    }

    async fn launch_app(&self, name: String) -> CallToolResult {
        let launcher = self.launcher.clone();
        let applications = self.applications.clone();
        let launched = tokio::task::spawn_blocking(move || {
            let entries = desktop_entries::read_all(&applications);
            match desktop_entries::find(&entries, &name) {
                Found::One(entry) => launcher
                    .launch(&entry.path)
                    .map(|()| json!({"result": "launched", "app": entry.name})),
                Found::Several(entries) => {
                    let mut names: Vec<&str> = entries.iter().map(|e| &*e.name).collect();
                    names.sort_unstable();
                    Ok(json!({"result": "candidates", "candidates": names}))
                }
                Found::None => Ok(json!({"result": "not_found"})),
            }
        })
        .await;
        match launched {
            Ok(Ok(result)) => CallToolResult::structured(result),
            Ok(Err(e)) => error(format!("Couldn't launch it: {e}")),
            Err(e) => error(format!("Couldn't launch it: {e}")),
        }
    }
}

async fn system_status(topic: String) -> CallToolResult {
    match tokio::task::spawn_blocking(move || system_status::facts(&topic)).await {
        Ok(Some(facts)) => text(facts),
        Ok(None) => error(format!(
            "The topic must be one of {}.",
            system_status::TOPICS.join(", ")
        )),
        Err(e) => error(format!("Couldn't read it: {e}")),
    }
}

/// An http or https URL with a host, and no whitespace or control characters.
fn is_web_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    web && !host.is_empty() && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn tool(name: &'static str, description: &'static str, schema: serde_json::Value) -> Tool {
    let serde_json::Value::Object(schema) = schema else {
        unreachable!("a tool's input schema is an object")
    };
    Tool::new(name, description, Arc::new(schema))
}

/// The string argument `key`, or an empty string.
fn string(arguments: &JsonObject, key: &str) -> String {
    arguments
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}

fn text(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text.into())])
}

fn error(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text.into())])
}
