//! The Plugin fixtures test harness. It runs the Plugin's MCP server
//! in-process, talks to it through a real MCP client as lucyd would, and
//! answers its web requests from recorded responses, so no test touches the
//! network.
//!
//! Recorded responses live in `tests/fixtures/web/<host>/<path>`: the body
//! of `https://feeds.npr.org/1001/rss.xml` is
//! `tests/fixtures/web/feeds.npr.org/1001/rss.xml`. A URL with no recording
//! fails, as an unreachable site would.
//!
//! The Plugin template copies this file; a new Plugin swaps `News` for its
//! own server and keeps its one way out to the web behind a trait like
//! `Web`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lucy_news::{News, Web};
use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, RunningService};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

/// The hour the Plugin's clock reads in tests: 8 in the morning.
pub const HOUR: u32 = 8;

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
        std::fs::read_to_string(self.dir.join(path)).map_err(|_| format!("no recording of {url}"))
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
    /// The Plugin with a fresh state dir, reading recorded feeds.
    pub fn start() -> Plugin {
        let web = Arc::new(Recorded::new());
        Plugin::launch(web.clone(), web, Arc::new(tempfile::tempdir().unwrap()))
    }

    /// The Plugin with a fresh state dir, reaching the web through `web`.
    pub fn with_web(web: Arc<dyn Web>) -> Plugin {
        let recorded = Arc::new(Recorded::new());
        Plugin::launch(web, recorded, Arc::new(tempfile::tempdir().unwrap()))
    }

    /// A new run of the Plugin with this one's state dir, as the next
    /// Session would start it.
    pub fn restart(&self) -> Plugin {
        let web = Arc::new(Recorded::new());
        Plugin::launch(web.clone(), web, self.state.clone())
    }

    fn launch(web: Arc<dyn Web>, recorded: Arc<Recorded>, state: Arc<tempfile::TempDir>) -> Plugin {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let news = News::new(web, state.path().to_owned(), || HOUR);
        let client = runtime.block_on(async {
            let (ours, theirs) = tokio::io::duplex(1 << 16);
            tokio::spawn(async move {
                let server = news.serve(theirs).await.expect("the Plugin starts");
                let _ = server.waiting().await;
            });
            let lifecycle = ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: None,
            };
            serve_client_with_lifecycle((), ours, lifecycle)
                .await
                .expect("the client connects")
        });
        Plugin {
            runtime,
            client,
            state,
            web: recorded,
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

    /// Calls `tool`, which must succeed, and returns its JSON answer.
    pub fn call(&self, tool: &str, arguments: Value) -> Value {
        let result = self.call_result(tool, arguments).unwrap();
        let text = text(&result);
        assert_eq!(result.is_error, Some(false), "{text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
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
