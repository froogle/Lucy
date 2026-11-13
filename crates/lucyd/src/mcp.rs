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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::adapters::Relay;
use crate::persona::Persona;
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

    /// A relay command for `caller`. Its token works until the returned
    /// `Token` is dropped.
    pub fn issue(&self, caller: Caller) -> (Relay, Token) {
        let value = random_token();
        self.tokens.0.lock().unwrap().insert(value.clone(), caller);
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
    fn caller(&self, token: &str) -> Option<Caller> {
        self.0.lock().unwrap().get(token).copied()
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
    if tokens.caller(&token).is_none() {
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
    /// Who is calling, or an error once the token has been revoked.
    fn caller(&self) -> Result<Caller, ErrorData> {
        self.tokens
            .caller(&self.token)
            .ok_or_else(|| ErrorData::invalid_request("this relay's token has been revoked", None))
    }

    async fn end_session(&self) -> CallToolResult {
        let (reply, ended) = oneshot::channel();
        let _ = self.commands.send(Command::EndAfterSpeech(reply));
        match ended.await {
            Ok(Ok(())) => CallToolResult::success(vec![ContentBlock::text(
                "The Session will end once your reply has been spoken.",
            )]),
            _ => CallToolResult::error(vec![ContentBlock::text("No Session is open.")]),
        }
    }

    /// Needs no Approval: the Persona is the user's to shape by voice.
    fn update_persona(&self, request: &CallToolRequestParams) -> CallToolResult {
        let text = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("persona"))
            .and_then(|persona| persona.as_str())
            .filter(|persona| !persona.trim().is_empty());
        let Some(text) = text else {
            return CallToolResult::error(vec![ContentBlock::text(
                "persona must be the whole new Persona text.",
            )]);
        };
        match self.persona.save(text) {
            Ok(()) => CallToolResult::success(vec![ContentBlock::text(format!(
                "Saved. Your Persona now reads:\n\n{text}\n\nTell the user what changed, in \
                 one short sentence."
            ))]),
            Err(e) => {
                eprintln!("lucyd: can't save {}: {e}", self.persona.path().display());
                CallToolResult::error(vec![ContentBlock::text(
                    "The Persona couldn't be saved. Tell the user it didn't change.",
                )])
            }
        }
    }
}

fn end_session_tool() -> Tool {
    let schema = serde_json::json!({"type": "object", "properties": {}});
    let serde_json::Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "end_session",
        "End the Session once your reply has been spoken. Call it when the user says goodbye \
         or that they're done.",
        Arc::new(schema),
    )
}

fn update_persona_tool() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {"persona": {
            "type": "string",
            "description": "The whole new Persona text, keeping everything the user didn't \
                            ask to change.",
        }},
        "required": ["persona"],
    });
    let serde_json::Value::Object(schema) = schema else {
        unreachable!()
    };
    Tool::new(
        "update_persona",
        "Rewrite your Persona: the instructions you were given about who you are, how you \
         speak and what to call the user. Call it when the user asks you to change your manner \
         (\"be a bit more sarcastic\") or tells you what to call them. It applies fully from \
         the next Session.",
        Arc::new(schema),
    )
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
        let tools = match self.caller()? {
            Caller::SessionModel => vec![end_session_tool(), update_persona_tool()],
            Caller::Delegate => vec![],
        };
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        match (self.caller()?, &*request.name) {
            (Caller::SessionModel, "end_session") => Ok(self.end_session().await.into()),
            (Caller::SessionModel, "update_persona") => Ok(self.update_persona(&request).into()),
            (_, name) => Err(ErrorData::invalid_params(
                format!("no tool named {name}"),
                None,
            )),
        }
    }
}
