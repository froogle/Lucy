//! Lucy's MCP tool server (ADR 0005), on a private socket beside the public
//! one. A model process reaches it through `lucy mcp-relay`, which opens each
//! connection with a line holding its per-spawn token. lucyd answers `ok` and
//! then speaks MCP, or answers `refused` and hangs up.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::adapters::Relay;
use crate::authoring::{self, Authoring, Build};
use crate::desktop::Desktop;
use crate::input;
use crate::persona::Persona;
use crate::plugins::{self, SessionTools};
use crate::runner::Command;
use crate::sync::Locked;

/// Who a token was issued to, and what it lets them reach. Each sees a
/// different set of tools.
#[derive(Clone)]
pub enum Caller {
    /// The model process of the open Session: the Built-in tools, the
    /// Session's `plugins` and its `desktop` tools.
    SessionModel {
        plugins: Arc<SessionTools>,
        desktop: Desktop,
    },
    /// The agent working on an Authoring build: only the Delegate tools.
    Delegate(Weak<Build>),
}

/// Issues relay commands, each with a fresh token for one caller.
#[derive(Clone)]
pub struct Relays {
    /// The `lucy` binary.
    program: PathBuf,
    socket: PathBuf,
    tokens: Tokens,
}

impl Relays {
    pub fn new(program: PathBuf, socket: PathBuf) -> Self {
        Relays {
            program,
            socket,
            tokens: Tokens::default(),
        }
    }

    /// The `lucy` binary each relay runs.
    pub fn program(&self) -> PathBuf {
        self.program.clone()
    }

    /// A relay command for `caller`. Its token works until the returned
    /// `Token` is dropped.
    pub fn issue(&self, caller: Caller) -> (Relay, Token) {
        let value = random_token();
        self.tokens.0.locked().insert(value.clone(), caller);
        let relay = Relay {
            program: self.program.clone(),
            args: vec![
                "mcp-relay".to_owned(),
                "--socket".to_owned(),
                self.socket.display().to_string(),
                value.clone(),
            ],
        };
        let token = Token {
            value,
            tokens: self.tokens.clone(),
        };
        (relay, token)
    }
}

/// The tokens currently issued.
#[derive(Clone, Default)]
struct Tokens(Arc<Mutex<HashMap<String, Caller>>>);

impl Tokens {
    fn grant(&self, token: &str) -> Option<Caller> {
        self.0.locked().get(token).cloned()
    }
}

/// An issued token. Dropping it revokes it.
pub struct Token {
    value: String,
    tokens: Tokens,
}

impl Drop for Token {
    fn drop(&mut self) {
        self.tokens.0.locked().remove(&self.value);
    }
}

/// 128 random bits, as hex.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("/dev/urandom is readable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn serve(
    listener: UnixListener,
    relays: Relays,
    persona: Persona,
    commands: mpsc::UnboundedSender<Command>,
    authoring: Arc<Authoring>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(
                    stream,
                    relays.tokens.clone(),
                    persona.clone(),
                    commands.clone(),
                    authoring.clone(),
                ));
            }
            Err(e) => eprintln!("lucyd: MCP accept failed: {e}"),
        }
    }
}

async fn connection(
    mut stream: UnixStream,
    tokens: Tokens,
    persona: Persona,
    commands: mpsc::UnboundedSender<Command>,
    authoring: Arc<Authoring>,
) {
    let Some(token) = read_token(&mut stream).await else {
        return;
    };
    let Some(grant) = tokens.grant(&token) else {
        let _ = stream.write_all(b"refused\n").await;
        return;
    };
    if stream.write_all(b"ok\n").await.is_err() {
        return;
    }
    let tools = Tools {
        token,
        tokens,
        persona,
        commands,
        authoring,
    };
    if let Ok(service) = tools.serve(stream).await {
        if let Caller::SessionModel { plugins, .. } = grant {
            plugins.watch(service.peer().clone());
        }
        let _ = service.waiting().await;
    }
}

/// Reads the token line byte by byte, so nothing after it is consumed.
async fn read_token(stream: &mut UnixStream) -> Option<String> {
    let mut line = Vec::new();
    loop {
        match stream.read_u8().await.ok()? {
            b'\n' => return String::from_utf8(line).ok(),
            _ if line.len() >= 128 => return None,
            byte => line.push(byte),
        }
    }
}

/// The MCP server for one relay connection.
struct Tools {
    token: String,
    tokens: Tokens,
    persona: Persona,
    commands: mpsc::UnboundedSender<Command>,
    authoring: Arc<Authoring>,
}

impl Tools {
    /// Who is calling and what they may reach, or an error once the token
    /// has been revoked.
    fn grant(&self) -> Result<Caller, ErrorData> {
        self.tokens.grant(&self.token).ok_or_else(revoked)
    }

    async fn end_session(&self) -> CallToolResult {
        let (reply, ended) = oneshot::channel();
        let _ = self.commands.send(Command::EndAfterSpeech(reply));
        match ended.await {
            Ok(Ok(())) => text("The Session will end once your reply has been spoken."),
            _ => error("No Session is open."),
        }
    }

    /// Needs no Approval: the Persona is the user's to shape by voice.
    fn update_persona(&self, request: &CallToolRequestParams) -> CallToolResult {
        let persona = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("persona"))
            .and_then(|persona| persona.as_str())
            .filter(|persona| !persona.trim().is_empty());
        let Some(persona) = persona else {
            return error("persona must be the whole new Persona text.");
        };
        match self.persona.save(persona) {
            Ok(()) => text(format!(
                "Saved. Your Persona now reads:\n\n{persona}\n\nTell the user what changed, in \
                 one short sentence."
            )),
            Err(e) => {
                eprintln!("lucyd: can't save {}: {e}", self.persona.path().display());
                error("The Persona couldn't be saved. Tell the user it didn't change.")
            }
        }
    }
}

fn revoked() -> ErrorData {
    ErrorData::invalid_request("this relay's token has been revoked", None)
}

fn no_tool(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("no tool named {name}"), None)
}

fn end_session_tool() -> Tool {
    tool(
        "end_session",
        "End the Session once your reply has been spoken. Call it when the user says goodbye \
         or that they're done.",
        serde_json::json!({"type": "object", "properties": {}}),
    )
}

fn update_persona_tool() -> Tool {
    tool(
        "update_persona",
        "Rewrite your Persona: the instructions you were given about who you are, how you \
         speak and what to call the user. Call it when the user asks you to change your manner \
         (\"be a bit more sarcastic\") or tells you what to call them. It applies fully from \
         the next Session.",
        serde_json::json!({
            "type": "object",
            "properties": {"persona": {
                "type": "string",
                "description": "The whole new Persona text, keeping everything the user didn't \
                                ask to change.",
            }},
            "required": ["persona"],
        }),
    )
}

/// A tool whose input schema is `schema`, a JSON object.
pub fn tool(name: &'static str, description: &'static str, schema: Value) -> Tool {
    let Value::Object(schema) = schema else {
        unreachable!("a tool's input schema is an object")
    };
    Tool::new(name, description, Arc::new(schema))
}

/// A tool result: `text` for the model.
pub fn text(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text.into())])
}

/// The text of a tool result.
pub fn result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
        .collect()
}

/// A failed tool result: `text` tells the model why.
pub fn error(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text.into())])
}

impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = match self.grant()? {
            Caller::SessionModel { plugins, .. } => [
                vec![
                    end_session_tool(),
                    update_persona_tool(),
                    plugins::manage_plugin_tool(),
                    authoring::request_plugin_tool(),
                    input::ask_typed_tool(),
                ],
                Desktop::tools(),
                plugins.list(),
            ]
            .concat(),
            Caller::Delegate(_) => authoring::delegate_tools(),
        };
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.clone().unwrap_or_default();
        let (plugins, desktop) = match self.grant()? {
            Caller::SessionModel { plugins, desktop } => (plugins, desktop),
            Caller::Delegate(build) => {
                let build = build.upgrade().ok_or_else(revoked)?;
                let result = build.call(&request.name, &arguments).await;
                return result.map(Into::into).ok_or_else(|| no_tool(&request.name));
            }
        };
        match &*request.name {
            "end_session" => Ok(self.end_session().await.into()),
            "update_persona" => Ok(self.update_persona(&request).into()),
            "manage_plugin" => Ok(plugins.manage(&arguments).await.into()),
            "request_plugin" => Ok(self.authoring.request(&arguments).into()),
            "ask_typed" => Ok(input::ask_typed(&self.commands, &arguments).await.into()),
            name => {
                if let Some(result) = desktop.call(name, &arguments).await {
                    return Ok(result.into());
                }
                match plugins.call(name, request.arguments).await {
                    Some(result) => Ok(result.into()),
                    None => Err(no_tool(name)),
                }
            }
        }
    }
}
