use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use lucy_config::Provider;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::adapters::{Exit, Place, Relay, Role, SessionModel, Thinking, ThinkingEvent, Workspace};
use crate::plugins::sandbox;
use crate::sentences::Sentences;
use crate::sync::Locked;

const REQUEST_WAIT: Duration = Duration::from_secs(30);

const ROUND_TRIP_WAIT: Duration = Duration::from_secs(60);

const VERIFY_PROMPT: &str = "Reply with just OK.";

const OPT_OUT: &str = "or set allow_api_key = true for this Provider in config.toml";

#[derive(Clone)]
pub struct Codex {
    program: PathBuf,
    work_dir: PathBuf,
}

impl Codex {
    pub fn new(program: PathBuf, work_dir: PathBuf) -> Codex {
        Codex { program, work_dir }
    }

    fn program<'a>(&'a self, provider: &'a Provider) -> &'a Path {
        provider.program.as_deref().unwrap_or(&self.program)
    }

    fn command(
        &self,
        provider: &Provider,
        relay: Option<&Relay>,
        workspace: Option<&Workspace>,
    ) -> std::io::Result<Command> {
        let program = match workspace {
            Some(_) => sandbox::delegate_program(provider, &self.program)?,
            None => self.program(provider).to_owned(),
        };
        let mut command = Command::new(&program);
        command
            .arg("app-server")
            .current_dir(workspace.map_or(&self.work_dir, |w| &w.dir))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut config = vec![r#"web_search="disabled""#.to_owned()];
        match workspace {
            Some(workspace) => {
                let codex_home = std::env::var_os("CODEX_HOME")
                    .map_or_else(|| sandbox::home().join(".codex"), PathBuf::from);
                sandbox::confine_cli(&mut command, workspace, &program, &[codex_home])?;
            }
            None => config.push("features.shell_tool=false".to_owned()),
        }
        if !provider.allow_api_key {
            config.push(r#"forced_login_method="chatgpt""#.to_owned());
        }
        if let Some(relay) = relay {
            let program_as_toml = Value::from(relay.program.to_string_lossy());
            let args_as_toml = json!(relay.args);
            config.push(format!("mcp_servers.lucy.command={program_as_toml}"));
            config.push(format!("mcp_servers.lucy.args={args_as_toml}"));
            config.push(r#"mcp_servers.lucy.default_tools_approval_mode="approve""#.to_owned());
        }
        for setting in config {
            command.arg("-c").arg(setting);
        }
        Ok(command)
    }

    fn connect(
        &self,
        provider: &Provider,
        relay: Option<&Relay>,
        workspace: Option<&Workspace>,
        inbox: Inbox,
    ) -> Result<(AppServer, Child), String> {
        let Inbox { tx, inputs } = inbox;
        std::fs::create_dir_all(&self.work_dir)
            .map_err(|e| format!("I couldn't make my Codex work folder: {e}."))?;
        let mut child = self
            .command(provider, relay, workspace)
            .and_then(|mut command| command.spawn())
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => format!(
                    "I can't find Codex at {}. Please install it, or put codex on my PATH.",
                    self.program(provider).display()
                ),
                _ => format!("I couldn't start Codex: {e}."),
            })?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(message) = serde_json::from_str(&line) {
                    let _ = tx.send(Input::Message(message));
                }
            }
            let _ = tx.send(Input::StdoutClosed);
        });
        let mut server = AppServer {
            stdin,
            inputs,
            next_id: 0,
            held: VecDeque::new(),
        };
        let signed_in = server.signed_in(provider);
        if let Err(reason) = signed_in {
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason);
        }
        Ok((server, child))
    }

    fn probe(
        &self,
        provider: &Provider,
        check: impl FnOnce(&mut AppServer) -> Result<(), String>,
    ) -> Result<(), String> {
        let (tx, inputs) = std_mpsc::channel();
        let (mut server, mut child) = self.connect(provider, None, None, Inbox { tx, inputs })?;
        let result = check(&mut server);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    fn run_session(
        &self,
        provider: &Provider,
        role: &Role,
        events: &mpsc::UnboundedSender<ThinkingEvent>,
        inbox: Inbox,
        process: &Arc<Mutex<Process>>,
    ) {
        let Role {
            relay,
            instructions,
            place,
        } = role;
        let workspace = match place {
            Place::Workspace(workspace) => Some(workspace),
            Place::WorkDir | Place::HandOff(_) => None,
        };
        let started = self
            .connect(provider, Some(relay), workspace, inbox)
            .and_then(|(mut server, child)| {
                {
                    let mut process = process.locked();
                    if matches!(*process, Process::Ended) {
                        let mut child = child;
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(None);
                    }
                    *process = Process::Running(child);
                }
                let thread = match workspace {
                    Some(workspace) => {
                        server.start_delegate_thread(provider, workspace, instructions)?
                    }
                    None => server.start_thread(provider, &self.work_dir, instructions)?,
                };
                Ok(Some((server, thread)))
            });
        match started {
            Ok(Some((server, thread))) => Session {
                server,
                thread,
                events,
                turn: None,
                queued: VecDeque::new(),
                sentences: Sentences::default(),
            }
            .run(process),
            Ok(None) => {}
            Err(reason) => {
                eprintln!("lucyd: refusing codex: {reason}");
                let _ = events.send(ThinkingEvent::Unavailable(reason));
            }
        }
    }
}

impl Codex {
    fn open_as(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        role: Role,
    ) -> Box<dyn SessionModel> {
        let (tx, inputs) = std_mpsc::channel();
        let process = Arc::new(Mutex::new(Process::Starting));
        let codex = self.clone();
        let provider = provider.clone();
        let thread_events = events.clone();
        let thread_tx = tx.clone();
        let thread_process = process.clone();
        std::thread::spawn(move || {
            codex.run_session(
                &provider,
                &role,
                &thread_events,
                Inbox {
                    tx: thread_tx,
                    inputs,
                },
                &thread_process,
            );
        });
        Box::new(CodexSessionModel {
            inputs: tx,
            process,
            _events: events,
        })
    }
}

impl Thinking for Codex {
    fn open(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
    ) -> Box<dyn SessionModel> {
        let role = Role {
            relay,
            instructions,
            place: Place::WorkDir,
        };
        self.open_as(provider, events, role)
    }

    fn open_delegate(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
        workspace: Workspace,
    ) -> Box<dyn SessionModel> {
        let role = Role {
            relay,
            instructions,
            place: Place::Workspace(workspace),
        };
        self.open_as(provider, events, role)
    }

    fn health(&self, provider: &Provider) -> Result<(), String> {
        self.probe(provider, |_| Ok(()))
    }

    fn check(&self, provider: &Provider) -> Result<(), String> {
        let work_dir = self.work_dir.clone();
        self.probe(provider, |server| {
            let thread = server.start_thread(provider, &work_dir, "")?;
            server.reply_ok(&thread)
        })
    }

    fn executable(&self, provider: &Provider) -> Option<PathBuf> {
        sandbox::delegate_program(provider, &self.program).ok()
    }
}

enum Input {
    Turn(String),
    Interrupt,
    Message(Value),
    StdoutClosed,
}

struct Inbox {
    tx: std_mpsc::Sender<Input>,
    inputs: std_mpsc::Receiver<Input>,
}

enum Process {
    Starting,
    Running(Child),
    Ended,
}

struct CodexSessionModel {
    inputs: std_mpsc::Sender<Input>,
    process: Arc<Mutex<Process>>,
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl SessionModel for CodexSessionModel {
    fn send_turn(&mut self, text: String) {
        let _fails_once_the_turn_has_ended = self.inputs.send(Input::Turn(text));
    }

    fn interrupt(&mut self) {
        let _ = self.inputs.send(Input::Interrupt);
    }
}

impl Drop for CodexSessionModel {
    fn drop(&mut self) {
        let process = std::mem::replace(&mut *self.process.locked(), Process::Ended);
        if let Process::Running(mut child) = process {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct AppServer {
    stdin: ChildStdin,
    inputs: std_mpsc::Receiver<Input>,
    next_id: u64,
    held: VecDeque<Input>,
}

impl AppServer {
    fn send(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"id": id, "jsonrpc": "2.0", "method": method, "params": params}));
        id
    }

    fn write(&mut self, message: &Value) {
        if let Err(e) = writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush()) {
            eprintln!("lucyd: can't reach codex: {e}");
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.send(method, params);
        let deadline = Instant::now() + REQUEST_WAIT;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            let input = self.inputs.recv_timeout(wait).map_err(|_| {
                "Codex stopped answering. Run codex in a terminal to see what's wrong.".to_owned()
            })?;
            match input {
                Input::Message(message) if is_response_to(&message, id) => {
                    return match message.get("error") {
                        Some(error) => Err(format!(
                            "Codex couldn't {method}: {}",
                            error["message"].as_str().unwrap_or("an unknown error")
                        )),
                        None => Ok(message["result"].clone()),
                    };
                }
                Input::Message(message) => self.refuse(&message),
                Input::StdoutClosed => {
                    return Err(
                        "Codex stopped as it started. Run codex in a terminal to see \
                                why, or reinstall it."
                            .to_owned(),
                    )
                }
                held => self.held.push_back(held),
            }
        }
    }

    fn refuse(&mut self, message: &Value) {
        let (Some(id), Some(method)) = (message.get("id"), message["method"].as_str()) else {
            return;
        };
        let reply = match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                json!({"id": id, "result": {"decision": "decline"}})
            }
            "mcpServer/elicitation/request" => {
                json!({"id": id, "result": {"action": "decline", "content": null, "_meta": null}})
            }
            _ => json!({"id": id, "error": {
                "code": -32601,
                "message": format!("Lucy doesn't handle {method}"),
            }}),
        };
        self.write(&reply);
    }

    fn signed_in(&mut self, provider: &Provider) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {"name": "lucy", "title": "Lucy", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": null,
            }),
        )?;
        self.write(&json!({"jsonrpc": "2.0", "method": "initialized"}));
        if provider.allow_api_key {
            return Ok(());
        }
        let account = self.request("account/read", json!({}))?;
        match account["account"]["type"].as_str() {
            Some("chatgpt") => Ok(()),
            None => Err(
                "Codex isn't signed in. Run codex login in a terminal and sign in \
                         with ChatGPT, then wake me again."
                    .to_owned(),
            ),
            Some(other) => Err(format!(
                "Codex is signed in with {other}, not a ChatGPT subscription, so I won't use \
                 it in case it bills you. Run codex login and sign in with ChatGPT, {OPT_OUT}."
            )),
        }
    }

    fn start_thread(
        &mut self,
        provider: &Provider,
        work_dir: &std::path::Path,
        instructions: &str,
    ) -> Result<String, String> {
        let result = self.request(
            "thread/start",
            json!({
                "model": provider.model,
                "cwd": work_dir,
                "approvalPolicy": "untrusted",
                "sandbox": "read-only",
                "developerInstructions": instructions,
                "ephemeral": true,
            }),
        )?;
        result["thread"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                "Codex didn't start a conversation. Run codex in a terminal to see \
                           what's wrong."
                    .to_owned()
            })
    }

    fn start_delegate_thread(
        &mut self,
        provider: &Provider,
        workspace: &Workspace,
        instructions: &str,
    ) -> Result<String, String> {
        let result = self.request(
            "thread/start",
            json!({
                "model": provider.model,
                "cwd": workspace.dir,
                "approvalPolicy": "never",
                "sandbox": "danger-full-access",
                "developerInstructions": instructions,
                "ephemeral": true,
            }),
        )?;
        result["thread"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Codex didn't start the build.".to_owned())
    }

    fn start_turn(&mut self, thread: &str, text: &str) -> u64 {
        self.send(
            "turn/start",
            json!({
                "threadId": thread,
                "input": [{"type": "text", "text": text, "text_elements": []}],
            }),
        )
    }

    fn reply_ok(&mut self, thread: &str) -> Result<(), String> {
        self.start_turn(thread, VERIFY_PROMPT);
        let deadline = Instant::now() + ROUND_TRIP_WAIT;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            let Ok(Input::Message(message)) = self.inputs.recv_timeout(wait) else {
                return Err(
                    "Codex didn't answer a test message. Run codex in a terminal to \
                            see what's wrong."
                        .to_owned(),
                );
            };
            if message["method"] != "turn/completed" {
                self.refuse(&message);
                continue;
            }
            let turn = &message["params"]["turn"];
            return match turn["status"].as_str() {
                Some("completed") => Ok(()),
                _ => Err(format!(
                    "Codex couldn't answer a test message: {}",
                    turn["error"]["message"]
                        .as_str()
                        .unwrap_or("an unknown error")
                )),
            };
        }
    }
}

fn is_response_to(message: &Value, id: u64) -> bool {
    message.get("method").is_none() && message["id"] == id
}

struct Turn {
    start_request: u64,
    id: Option<String>,
    interrupted: bool,
}

struct Session<'a> {
    server: AppServer,
    thread: String,
    events: &'a mpsc::UnboundedSender<ThinkingEvent>,
    turn: Option<Turn>,
    queued: VecDeque<String>,
    sentences: Sentences,
}

impl Session<'_> {
    fn run(mut self, process: &Arc<Mutex<Process>>) {
        loop {
            let input = match self.server.held.pop_front() {
                Some(input) => input,
                None => match self.server.inputs.recv() {
                    Ok(input) => input,
                    Err(_) => return,
                },
            };
            match input {
                Input::Turn(text) if self.turn.is_some() => self.queued.push_back(text),
                Input::Turn(text) => self.start(&text),
                Input::Interrupt => self.interrupt(),
                Input::Message(message) => self.message(&message),
                Input::StdoutClosed => {
                    let stopped_on_its_own = !matches!(*process.locked(), Process::Ended);
                    if stopped_on_its_own {
                        let _ = self.events.send(ThinkingEvent::Died(Exit::default()));
                    }
                    return;
                }
            }
        }
    }

    fn start(&mut self, text: &str) {
        let request = self.server.start_turn(&self.thread, text);
        self.turn = Some(Turn {
            start_request: request,
            id: None,
            interrupted: false,
        });
    }

    fn interrupt(&mut self) {
        if let Some(turn) = &mut self.turn {
            turn.interrupted = true;
        }
        self.send_interrupt();
    }

    fn send_interrupt(&mut self) {
        let Some(Turn {
            id: Some(id),
            interrupted: true,
            ..
        }) = &self.turn
        else {
            return;
        };
        let params = json!({"threadId": self.thread, "turnId": id});
        self.server.send("turn/interrupt", params);
    }

    fn learn_id(&mut self, id: &Value) {
        let Some(turn) = &mut self.turn else { return };
        if turn.id.is_some() {
            return;
        }
        turn.id = id.as_str().map(str::to_owned);
        self.send_interrupt();
    }

    fn speaking(&self) -> bool {
        self.turn.as_ref().is_some_and(|turn| !turn.interrupted)
    }

    fn message(&mut self, message: &Value) {
        if message.get("method").is_none() {
            return self.response(message);
        }
        if message.get("id").is_some() {
            return self.server.refuse(message);
        }
        let params = &message["params"];
        match message["method"].as_str() {
            Some("turn/started") => self.learn_id(&params["turn"]["id"]),
            Some("item/agentMessage/delta") if self.speaking() => {
                for event in self
                    .sentences
                    .push(params["delta"].as_str().unwrap_or_default())
                {
                    let _ = self.events.send(event);
                }
            }
            Some("item/completed") if params["item"]["type"] == "agentMessage" => {
                self.finish_sentence();
            }
            Some("turn/completed") => {
                let turn = &params["turn"];
                if turn["status"] == "failed" {
                    eprintln!("lucyd: codex's turn failed: {}", turn["error"]);
                }
                self.end_turn();
            }
            _ => {}
        }
    }

    fn response(&mut self, message: &Value) {
        let Some(turn) = &self.turn else { return };
        if message["id"] != turn.start_request {
            return;
        }
        match message.get("error") {
            Some(refused_so_no_completion_comes) => {
                eprintln!("lucyd: codex refused a turn: {refused_so_no_completion_comes}");
                self.end_turn();
            }
            None => self.learn_id(&message["result"]["turn"]["id"]),
        }
    }

    fn finish_sentence(&mut self) {
        let event = self.sentences.finish();
        if self.speaking() {
            if let Some(event) = event {
                let _ = self.events.send(event);
            }
        }
    }

    fn end_turn(&mut self) {
        self.finish_sentence();
        if self.speaking() {
            let _ = self.events.send(ThinkingEvent::TurnEnd);
        }
        self.turn = None;
        if let Some(text) = self.queued.pop_front() {
            self.start(&text);
        }
    }
}
