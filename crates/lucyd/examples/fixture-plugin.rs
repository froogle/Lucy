//! A tiny Plugin executable for the lucyd harness: an MCP stdio server that
//! offers the tools named on its command line, from this fixed set:
//!
//! - `echo {text}`: returns `text`.
//! - `read_file {path}`: returns the file's contents.
//! - `write_file {path, text}`: writes `text` to the file.
//! - `save_state {text}`: writes `text` to `saved` in its private state dir.
//! - `env`: writes its environment to `env` in its private state dir, one
//!   `NAME=value` a line, sorted, and returns `ok`.
//! - `pid`: returns its process id.
//! - `crash`: exits without answering.
//! - `reject`: answers with an MCP error.
//! - `ask {question}`: asks the user `question` mid-call (MCP
//!   `input_required`), then returns `heard: <answer>`, or `no answer`.
//! - `wait {path}`: writes `<path>.waiting`, then returns `done` once `path`
//!   exists. If the call is cancelled first, it writes `<path>.cancelled`.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ElicitRequest,
    ElicitRequestParams, InputRequest, InputRequests, InputRequiredResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
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
        "ask" => json!({"question": string}),
        "read_file" | "wait" => json!({"path": string}),
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
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        // `input_required` needs this protocol version.
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info
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
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let outcome = match &*request.name {
            "wait" => {
                let path = std::path::PathBuf::from(arg(&request, "path"));
                let mut waiting = path.clone().into_os_string();
                waiting.push(".waiting");
                let _ = std::fs::write(waiting, "");
                loop {
                    if path.exists() {
                        break Ok("done".into());
                    }
                    if context.ct.is_cancelled() {
                        let mut cancelled = path.into_os_string();
                        cancelled.push(".cancelled");
                        let _ = std::fs::write(cancelled, "");
                        break Ok("cancelled".into());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
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
            "env" => {
                let dir = std::env::var("LUCY_PLUGIN_STATE_DIR").unwrap_or_default();
                let mut vars: Vec<String> = std::env::vars()
                    .map(|(name, value)| format!("{name}={value}\n"))
                    .collect();
                vars.sort();
                std::fs::write(std::path::Path::new(&dir).join("env"), vars.concat())
                    .map(|()| "ok".into())
            }
            "pid" => Ok(std::process::id().to_string()),
            "ask" => match &request.input_responses {
                None => return Ok(ask(&arg(&request, "question")).into()),
                Some(responses) => Ok(responses
                    .get("answer")
                    .and_then(|response| response["content"]["answer"].as_str())
                    .map(|answer| format!("heard: {answer}"))
                    .unwrap_or_else(|| "no answer".into())),
            },
            "crash" => std::process::exit(1),
            "reject" => return Err(ErrorData::invalid_params("rejected", None)),
            name => return Err(ErrorData::invalid_params(format!("no tool {name}"), None)),
        };
        Ok(result(outcome).into())
    }
}

/// Asks the user `question`, wanting a string `answer`.
fn ask(question: &str) -> InputRequiredResult {
    let request = ElicitRequest::new(ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: question.to_owned(),
        requested_schema: serde_json::from_value(json!({
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"],
        }))
        .expect("the literal is a valid form schema"),
    });
    let mut requests = InputRequests::new();
    requests.insert("answer".to_owned(), InputRequest::Elicitation(request));
    InputRequiredResult::new(Some(requests), Some("asked".to_owned()))
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
