//! A tiny Plugin executable for the lucyd harness: an MCP stdio server that
//! offers the tools named on its command line, from this fixed set:
//!
//! - `echo {text}`: returns `text`.
//! - `read_file {path}`: returns the file's contents.
//! - `write_file {path, text}`: writes `text` to the file.
//! - `save_state {text}`: writes `text` to `saved` in its private state dir.
//! - `pid`: returns its process id.
//! - `crash`: exits without answering.
//! - `reject`: answers with an MCP error.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};

struct Fixture {
    tools: Vec<String>,
}

fn schema(properties: Value) -> Arc<serde_json::Map<String, Value>> {
    let Value::Object(schema) = json!({"type": "object", "properties": properties}) else {
        unreachable!()
    };
    Arc::new(schema)
}

fn tool(name: &str) -> Tool {
    let string = json!({"type": "string"});
    let properties = match name {
        "echo" | "save_state" => json!({"text": string}),
        "read_file" => json!({"path": string}),
        "write_file" => json!({"path": string, "text": string}),
        _ => json!({}),
    };
    Tool::new(name.to_owned(), name.to_owned(), schema(properties))
}

fn arg(request: &CallToolRequestParams, name: &str) -> String {
    request
        .arguments
        .as_ref()
        .and_then(|args| args.get(name))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn result(outcome: std::io::Result<String>) -> CallToolResult {
    match outcome {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
    }
}

impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            self.tools.iter().map(|name| tool(name)).collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let outcome = match &*request.name {
            "echo" => Ok(arg(&request, "text")),
            "read_file" => std::fs::read_to_string(arg(&request, "path")),
            "write_file" => {
                std::fs::write(arg(&request, "path"), arg(&request, "text")).map(|()| "ok".into())
            }
            "save_state" => {
                let dir = std::env::var("LUCY_PLUGIN_STATE_DIR").unwrap_or_default();
                std::fs::write(
                    std::path::Path::new(&dir).join("saved"),
                    arg(&request, "text"),
                )
                .map(|()| "ok".into())
            }
            "pid" => Ok(std::process::id().to_string()),
            "crash" => std::process::exit(1),
            "reject" => return Err(ErrorData::invalid_params("rejected", None)),
            name => return Err(ErrorData::invalid_params(format!("no tool {name}"), None)),
        };
        Ok(result(outcome).into())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let fixture = Fixture {
        tools: std::env::args().skip(1).collect(),
    };
    let service = fixture
        .serve(rmcp::transport::stdio())
        .await
        .expect("MCP starts");
    let _ = service.waiting().await;
}
