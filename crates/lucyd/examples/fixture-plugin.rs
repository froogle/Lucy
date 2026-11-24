//! A tiny Plugin executable for the lucyd harness, an MCP stdio server.
//! It offers the tools named on its command line, from this fixed set:
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
//! - `ask_url {question}`, `ask_two {question}` and `ask_secret {name}` ask a
//!   form: one `uri` field `url`; a string `name` and an integer `count`; or
//!   Lucy's `secret` field on the credential `name`. Each returns `answered:`
//!   and the response it got, as JSON.
//! - `describe_write {path, text}`: describes a `write_file` call for the
//!   Details window: `structuredContent` `{"items": ["## <path>", "It will
//!   say *<text>*."]}`.
//! - `describe_badly {path, text}`: a describe tool that goes wrong as `text`
//!   says: `fail` answers with an error result, `empty` with no items,
//!   `malformed` with items that aren't a list, and `stall` writes
//!   `<path>.describing` and never answers; if that call is cancelled, it
//!   writes `<path>.cancelled`.
//! - `ask_about {question}` and `type_about {question}` ask `question` with
//!   the Details window's items `ITEMS` in the elicitation's `_meta`
//!   (`lucy/items`): aloud, as one plain field `answer`, returning `heard:
//!   <answer>`; or typed, as one `uri` field `url`, returning `answered:` and
//!   the response, as JSON.
//! - `wait {path}`: writes `<path>.waiting`, then returns `done` once `path`
//!   exists. If the call is cancelled first, it writes `<path>.cancelled`.
//! - `look`: returns an `image/png` image, base64 of a PNG signature then
//!   `LucyScreenSecret`, and the text `a fake screen`.
//! - `click`, `key`, `type` and `scroll`: return their name, `: ` and the
//!   arguments they got, as JSON.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ElicitRequest,
    ElicitRequestParams, InputRequest, InputRequests, InputRequiredResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};

const ITEMS: [&str; 2] = ["**From:** Ada", "*Subject:* Lunch on Friday"];

const SCREEN: &str = "iVBORw0KGgpMdWN5U2NyZWVuU2VjcmV0";

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
        "ask" | "ask_url" | "ask_two" | "ask_about" | "type_about" => {
            json!({"question": string})
        }
        "ask_secret" => json!({"name": string}),
        "read_file" | "wait" => json!({"path": string}),
        "write_file" | "describe_write" | "describe_badly" => {
            json!({"path": string, "text": string})
        }
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

const INPUT_REQUIRED_VERSION: ProtocolVersion = ProtocolVersion::V_2026_07_28;

impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.protocol_version = INPUT_REQUIRED_VERSION;
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
            "describe_write" => {
                let items = [
                    format!("## {}", arg(&request, "path")),
                    format!("It will say *{}*.", arg(&request, "text")),
                ];
                return Ok(CallToolResult::structured(json!({ "items": items })).into());
            }
            "describe_badly" => return Ok(describe_badly(&request, &context).await.into()),
            "ask" | "ask_about" => match &request.input_responses {
                None => return Ok(ask(&request).into()),
                Some(responses) => Ok(responses
                    .get("answer")
                    .and_then(|response| response["content"]["answer"].as_str())
                    .map(|answer| format!("heard: {answer}"))
                    .unwrap_or_else(|| "no answer".into())),
            },
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
            "look" => {
                let looked = CallToolResult::success(vec![
                    ContentBlock::image(SCREEN, "image/png"),
                    ContentBlock::text("a fake screen"),
                ]);
                return Ok(looked.into());
            }
            "ask_url" | "ask_two" | "ask_secret" | "type_about" => match &request.input_responses {
                None => return Ok(ask_form(&request).into()),
                Some(responses) => Ok(format!("answered: {}", responses["answer"])),
            },
            "click" | "key" | "type" | "scroll" => {
                let arguments = Value::Object(request.arguments.clone().unwrap_or_default());
                Ok(format!("{}: {arguments}", request.name))
            }
            "crash" => std::process::exit(1),
            "reject" => return Err(ErrorData::invalid_params("rejected", None)),
            name => return Err(ErrorData::invalid_params(format!("no tool {name}"), None)),
        };
        Ok(result(outcome).into())
    }
}

async fn describe_badly(
    request: &CallToolRequestParams,
    context: &RequestContext<RoleServer>,
) -> CallToolResult {
    match &*arg(request, "text") {
        "fail" => CallToolResult::error(vec![ContentBlock::text("can't describe it")]),
        "empty" => CallToolResult::structured(json!({"items": []})),
        "malformed" => CallToolResult::structured(json!({"items": "not a list"})),
        _ => {
            let path = arg(request, "path");
            let _ = std::fs::write(format!("{path}.describing"), "");
            context.ct.cancelled().await;
            let _ = std::fs::write(format!("{path}.cancelled"), "");
            CallToolResult::error(vec![ContentBlock::text("cancelled")])
        }
    }
}

fn items_meta(request: &CallToolRequestParams) -> Option<Value> {
    request
        .name
        .ends_with("_about")
        .then(|| json!({"lucy/items": ITEMS}))
}

fn ask(request: &CallToolRequestParams) -> InputRequiredResult {
    elicit(
        arg(request, "question"),
        json!({"answer": {"type": "string"}}),
        items_meta(request),
    )
}

fn ask_form(request: &CallToolRequestParams) -> InputRequiredResult {
    let question = arg(request, "question");
    match &*request.name {
        "ask_url" | "type_about" => elicit(
            question,
            json!({"url": {"type": "string", "format": "uri"}}),
            items_meta(request),
        ),
        "ask_two" => elicit(
            question,
            json!({
                "name": {"type": "string", "title": "Name"},
                "count": {"type": "integer"},
            }),
            None,
        ),
        _ => {
            let name = arg(request, "name");
            let meta = json!({"lucy/secret": [name]});
            elicit(
                format!("Your {name}?"),
                json!({ name: {"type": "string"} }),
                Some(meta),
            )
        }
    }
}

fn elicit(message: String, properties: Value, meta: Option<Value>) -> InputRequiredResult {
    let request = ElicitRequest::new(ElicitRequestParams::FormElicitationParams {
        meta: meta.map(|meta| serde_json::from_value(meta).expect("the literal is an object")),
        message,
        requested_schema: serde_json::from_value(json!({
            "type": "object",
            "properties": properties,
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
