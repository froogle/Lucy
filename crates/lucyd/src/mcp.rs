//! Lucy's MCP tool server (ADR 0005), on a private socket beside the public
//! one. A model process reaches it through `lucy mcp-relay`, which opens each
//! connection with a line holding its per-spawn token. lucyd answers `ok` and
//! then speaks MCP, or answers `refused` and hangs up.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
use crate::desktop::Desktop;
use crate::persona::Persona;
use crate::plugins::SessionTools;
use crate::runner::Command;

/// Who a token was issued to. Each sees a different set of tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The model process of the open Session.
    SessionModel,
    /// An agent working on a goal, such as an Authoring build.
    #[allow(dead_code)] // Issued once Delegates land.
    Delegate,
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

    /// A relay command for `caller`, offering the Session's `plugins` and
    /// `desktop` tools. Its token works until the returned `Token` is
    /// dropped.
    pub fn issue(
        &self,
        caller: Caller,
        plugins: Arc<SessionTools>,
        desktop: Desktop,
    ) -> (Relay, Token) {
        let value = random_token();
        self.tokens.0.lock().unwrap().insert(
            value.clone(),
            Grant {
                caller,
                plugins,
                desktop,
            },
        );
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

/// What a token lets its holder reach.
#[derive(Clone)]
struct Grant {
    caller: Caller,
    plugins: Arc<SessionTools>,
    desktop: Desktop,
}

/// The tokens currently issued.
#[derive(Clone, Default)]
struct Tokens(Arc<Mutex<HashMap<String, Grant>>>);

impl Tokens {
    fn grant(&self, token: &str) -> Option<Grant> {
        self.0.lock().unwrap().get(token).cloned()
    }
}

/// An issued token. Dropping it revokes it.
pub struct Token {
    value: String,
    tokens: Tokens,
}

impl Drop for Token {
    fn drop(&mut self) {
        self.tokens.0.lock().unwrap().remove(&self.value);
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
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(connection(
                    stream,
                    relays.tokens.clone(),
                    persona.clone(),
                    commands.clone(),
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
) {
    let Some(token) = read_token(&mut stream).await else {
        return;
    };
    if tokens.grant(&token).is_none() {
        let _ = stream.write_all(b"refused\n").await;
        return;
    }
    if stream.write_all(b"ok\n").await.is_err() {
        return;
    }
    let tools = Tools {
        token,
        tokens,
        persona,
        commands,
    };
    if let Ok(service) = tools.serve(stream).await {
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
}

impl Tools {
    /// Who is calling and what they may reach, or an error once the token
    /// has been revoked.
    fn grant(&self) -> Result<Grant, ErrorData> {
        self.tokens
            .grant(&self.token)
            .ok_or_else(|| ErrorData::invalid_request("this relay's token has been revoked", None))
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

/// A failed tool result: `text` tells the model why.
pub fn error(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text.into())])
}

impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let grant = self.grant()?;
        let tools = match grant.caller {
            Caller::SessionModel => [
                vec![end_session_tool(), update_persona_tool()],
                Desktop::tools(),
                grant.plugins.list(),
            ]
            .concat(),
            Caller::Delegate => vec![],
        };
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.clone().unwrap_or_default();
        let grant = self.grant()?;
        match (grant.caller, &*request.name) {
            (Caller::SessionModel, "end_session") => Ok(self.end_session().await.into()),
            (Caller::SessionModel, "update_persona") => Ok(self.update_persona(&request).into()),
            (Caller::SessionModel, name) => {
                if let Some(result) = grant.desktop.call(name, &arguments).await {
                    return Ok(result.into());
                }
                match grant.plugins.call(name, request.arguments).await {
                    Some(result) => Ok(result.into()),
                    None => Err(no_tool(name)),
                }
            }
            (_, name) => Err(no_tool(name)),
        }
    }
}
