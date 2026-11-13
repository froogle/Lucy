//! The Plugin fixtures test harness, copied from Lucy's news Plugin. It runs
//! the Plugin's MCP server in-process, talks to it through a real MCP
//! client as lucyd would, and answers its web requests from recorded
//! responses, so no test touches the network.
//!
//! Recorded responses live in `tests/fixtures/web/<host>/<path>`: the body
//! of `https://example.com/feed.xml` is
//! `tests/fixtures/web/example.com/feed.xml`. A URL with no recording fails,
//! as an unreachable site would. A site's home page,
//! `https://example.com/`, is `example.com/index.html`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use plugin::{Plugin as Server, Web};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

/// The fixtures' web: recorded responses, and every URL asked for.
pub struct Recorded {
    dir: PathBuf,
    requested: Mutex<Vec<String>>,
}

impl Recorded {
    pub fn new() -> Recorded {
        Recorded {
            dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/web"),
            requested: Mutex::default(),
        }
    }

    /// Every URL the Plugin asked for, in order.
    pub fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }
}

impl Web for Recorded {
    fn get(&self, url: &str) -> Result<String, String> {
        self.requested.lock().unwrap().push(url.to_owned());
        let path = url
            .split_once("://")
            .map(|(_, rest)| rest)
            .ok_or_else(|| format!("not a URL: {url}"))?;
        let file = match path.strip_suffix('/') {
            Some(dir) => self.dir.join(dir).join("index.html"),
            None => self.dir.join(path),
        };
        std::fs::read_to_string(file).map_err(|_| format!("no recording of {url}"))
    }
}

/// The Plugin, running and connected.
pub struct Plugin {
    runtime: tokio::runtime::Runtime,
    client: RunningService<RoleClient, ()>,
    state: Arc<tempfile::TempDir>,
    pub web: Arc<Recorded>,
}

impl Plugin {
    /// The Plugin with a fresh state dir, reading recorded responses.
    pub fn start() -> Plugin {
        let web = Arc::new(Recorded::new());
        Plugin::launch(web, Arc::new(tempfile::tempdir().unwrap()))
    }

    /// A new run of the Plugin with this one's state dir, as the next
    /// Session would start it.
    pub fn restart(&self) -> Plugin {
        Plugin::launch(Arc::new(Recorded::new()), self.state.clone())
    }

    fn launch(web: Arc<Recorded>, state: Arc<tempfile::TempDir>) -> Plugin {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = Server::new(web.clone(), state.path().to_owned());
        let client = runtime.block_on(async {
            let (ours, theirs) = tokio::io::duplex(1 << 16);
            tokio::spawn(async move {
                let server = server.serve(theirs).await.expect("the Plugin starts");
                let _ = server.waiting().await;
            });
            ().serve(ours).await.expect("the client connects")
        });
        Plugin {
            runtime,
            client,
            state,
            web,
        }
    }

    /// The Plugin's private state dir.
    pub fn state_dir(&self) -> &Path {
        self.state.path()
    }

    /// The names of the tools it lists.
    pub fn tool_names(&self) -> Vec<String> {
        let tools = self.runtime.block_on(self.client.list_all_tools()).unwrap();
        tools
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    /// Calls `tool` with `arguments`, a JSON object.
    pub fn call_result(&self, tool: &str, arguments: Value) -> Result<CallToolResult, String> {
        let Value::Object(arguments) = arguments else {
            panic!("tool arguments are an object");
        };
        let request = CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments);
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), self.client.call_tool(request))
                .await
                .map_err(|_| "timed out".to_owned())?
                .map_err(|e| e.to_string())
        })
    }

    /// Calls `tool`, which must succeed, and returns the text it answered.
    pub fn call(&self, tool: &str, arguments: Value) -> String {
        let result = self.call_result(tool, arguments).unwrap();
        let said = text(&result);
        assert_eq!(result.is_error, Some(false), "{said}");
        said
    }
}

/// The text a tool answered with.
pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}
