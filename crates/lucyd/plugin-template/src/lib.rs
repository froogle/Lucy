//! The Plugin's MCP server. lucyd starts it in its sandbox, lists its tools
//! and calls them for the user. Its only way out to the web is `Web`, so
//! tests can answer from recorded responses instead.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{json, Value};

/// How the Plugin reaches the web. Replace the body of `get` in the real
/// implementation with an HTTP client if the Plugin needs the network.
pub trait Web: Send + Sync {
    /// The body of `url`.
    fn get(&self, url: &str) -> Result<String, String>;
}

/// The real web. This template doesn't use the network.
pub struct NoWeb;

impl Web for NoWeb {
    fn get(&self, url: &str) -> Result<String, String> {
        Err(format!("no network access: {url}"))
    }
}

pub struct Plugin {
    pub web: Arc<dyn Web>,
    /// Its private, writable state folder.
    pub state_dir: PathBuf,
}

impl Plugin {
    pub fn new(web: Arc<dyn Web>, state_dir: PathBuf) -> Plugin {
        Plugin { web, state_dir }
    }
}

fn example_tool() -> Tool {
    let Value::Object(schema) = json!({
        "type": "object",
        "properties": {"text": {"type": "string", "description": "What to echo"}},
        "required": ["text"],
    }) else {
        unreachable!("a schema is an object")
    };
    Tool::new("example", "Echoes its text", Arc::new(schema))
}

impl ServerHandler for Plugin {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![example_tool()]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        match &*request.name {
            "example" => {
                let text = request
                    .arguments
                    .as_ref()
                    .and_then(|arguments| arguments.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
            }
            name => Err(ErrorData::invalid_params(format!("no tool {name}"), None)),
        }
    }
}
