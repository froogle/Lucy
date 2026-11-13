//! The `claude` Thinking Provider: one persistent `claude -p` stream-json
//! process per Session, on the user's subscription (ADR 0001).

mod guard;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;

use lucy_config::Provider;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::adapters::{Relay, SessionModel, Thinking, ThinkingEvent, Workspace};
use crate::plugins::sandbox;
use crate::sentences::Sentences;

/// The `claude` tools a Delegate writes and builds a Plugin with, inside
/// its sandbox. A Session's model has none of them.
const DELEGATE_TOOLS: &[&str] = &["Bash", "Read", "Write", "Edit", "Glob", "Grep"];

/// Settings that would make `claude` bill something other than the user's
/// subscription. Everything above `CLAUDE_CODE_OAUTH_TOKEN` in Claude Code's
/// credential precedence. `apiKeyHelper` lives only in settings files, which
/// `--setting-sources ""` keeps out.
/// How long claude gets to acknowledge the control interrupt before it gets
/// SIGINT instead.
const INTERRUPT_ACK_WAIT: Duration = Duration::from_secs(1);

const SCRUBBED_ENV: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

#[derive(Clone)]
pub struct Claude {
    program: PathBuf,
    /// A Lucy-owned directory, so no project's `.mcp.json` or hooks apply.
    work_dir: PathBuf,
}

impl Claude {
    pub fn new(program: PathBuf, work_dir: PathBuf) -> Claude {
        Claude { program, work_dir }
    }

    /// `claude` run in the work dir, with the guard's env scrubbing unless
    /// the Provider opts out.
    fn base_command(&self, provider: &Provider) -> Command {
        let mut command = Command::new(&self.program);
        command.current_dir(&self.work_dir).stderr(Stdio::null());
        if !provider.allow_api_key {
            for name in SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        command
    }

    /// `claude -p` stream-json, with `relay` as its only MCP server, or none,
    /// `instructions` appended to its system prompt, and of its own tools
    /// only `own_tools`. It carries on the conversation `resume` names, if
    /// any; conversations are kept on disk so that it can.
    fn command(
        &self,
        provider: &Provider,
        relay: Option<&Relay>,
        instructions: &str,
        own_tools: &[&str],
        resume: Option<&str>,
    ) -> Command {
        let servers = match relay {
            Some(relay) => serde_json::json!({"lucy": {
                "command": relay.program,
                "args": relay.args,
            }}),
            None => serde_json::json!({}),
        };
        let mcp_config = serde_json::json!({ "mcpServers": servers });
        let mut command = self.base_command(provider);
        command
            .args(["-p", "--verbose"])
            .args(["--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .arg("--include-partial-messages")
            .arg("--strict-mcp-config")
            .arg("--mcp-config")
            .arg(mcp_config.to_string())
            .args(["--setting-sources", ""])
            .arg("--disable-slash-commands")
            .args(["--tools", &own_tools.join(",")])
            .args(["--permission-mode", "dontAsk"])
            .arg("--allowedTools")
            .arg(
                ["mcp__lucy__*"]
                    .iter()
                    .chain(own_tools)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .arg("--append-system-prompt")
            .arg(instructions);
        if let Some(model) = &provider.model {
            command.args(["--model", model]);
        }
        if let Some(session) = resume {
            command.args(["--resume", session]);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        command
    }

    /// A Delegate's `claude -p`: its own file and shell tools, working in
    /// the workspace, confined to it, the program and its config.
    fn delegate_command(
        &self,
        provider: &Provider,
        relay: &Relay,
        instructions: &str,
        workspace: &Workspace,
        resume: Option<&str>,
    ) -> std::io::Result<Command> {
        let mut command = self.command(provider, Some(relay), instructions, DELEGATE_TOOLS, resume);
        let home = sandbox::home();
        let cli_home = [home.join(".claude"), home.join(".claude.json")];
        sandbox::confine_cli(&mut command, workspace, &self.program, &cli_home)?;
        Ok(command)
    }

    /// One Session's or Delegate's process: the guard's checks, then
    /// `claude -p`, fed each Turn and interrupt from `inputs` until the
    /// SessionModel is dropped. A Turn is interrupted with the stream-json
    /// control interrupt; if claude doesn't acknowledge it, with SIGINT,
    /// which ends the process, and a new one carries on the conversation.
    fn run_session(
        &self,
        provider: &Provider,
        role: &Role,
        events: &mpsc::UnboundedSender<ThinkingEvent>,
        inputs: std_mpsc::Receiver<ToClaude>,
        process: &Arc<Mutex<Process>>,
    ) {
        if let Err(reason) = self.prepare(provider) {
            eprintln!("lucyd: refusing claude: {reason}");
            let _ = events.send(ThinkingEvent::Unavailable(reason));
            return;
        }
        // claude's id for the conversation, read from its stream.
        let session = Arc::new(Mutex::new(None));
        let spawn = || self.spawn(provider, role, events, process, &session);
        let Some(mut running) = spawn() else {
            return;
        };
        // Whether claude understands the control interrupt: until it fails
        // to, it is assumed to.
        let mut control_interrupt = true;
        let mut interrupts = 0;
        for input in inputs {
            match input {
                ToClaude::Turn(text) => {
                    running.turns.lock().unwrap().sent += 1;
                    let message = serde_json::json!(
                        {"type": "user", "message": {"role": "user", "content": text}}
                    );
                    running.write(&message);
                }
                ToClaude::Interrupt => {
                    {
                        let mut turns = running.turns.lock().unwrap();
                        if turns.sent == turns.ended {
                            continue;
                        }
                        turns.discard_until = turns.sent;
                    }
                    if control_interrupt {
                        interrupts += 1;
                        running.write(&serde_json::json!({
                            "type": "control_request",
                            "request_id": format!("interrupt-{interrupts}"),
                            "request": {"subtype": "interrupt"},
                        }));
                        if running.acks.recv_timeout(INTERRUPT_ACK_WAIT) == Ok(true) {
                            continue;
                        }
                        eprintln!("lucyd: claude didn't take the control interrupt; using SIGINT");
                        control_interrupt = false;
                    }
                    running.turns.lock().unwrap().retired = true;
                    if let Process::Running(child) = &*process.lock().unwrap() {
                        // SAFETY: kill(2) on our own child's pid.
                        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
                    }
                    running = match spawn() {
                        Some(running) => running,
                        None => return,
                    };
                }
            }
        }
    }

    /// Starts `claude -p` and a thread reading its replies into `events`.
    /// None if it couldn't start, which `events` hears, or the Session has
    /// ended.
    fn spawn(
        &self,
        provider: &Provider,
        role: &Role,
        events: &mpsc::UnboundedSender<ThinkingEvent>,
        process: &Arc<Mutex<Process>>,
        session: &Arc<Mutex<Option<String>>>,
    ) -> Option<Running> {
        let resume = session.lock().unwrap().clone();
        let resume = resume.as_deref();
        let command = match role {
            Role::Session {
                relay,
                instructions,
            } => Ok(self.command(provider, Some(relay), instructions, &[], resume)),
            Role::Delegate {
                relay,
                instructions,
                workspace,
            } => self.delegate_command(provider, relay, instructions, workspace, resume),
        };
        let mut child = match command.and_then(|mut command| command.spawn()) {
            Ok(child) => child,
            Err(e) => {
                eprintln!("lucyd: can't start {}: {e}", self.program.display());
                let reason = format!("I couldn't start Claude Code: {e}.");
                let _ = events.send(ThinkingEvent::Unavailable(reason));
                return None;
            }
        };
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        {
            let mut process = process.lock().unwrap();
            if matches!(*process, Process::Ended) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            // One that was interrupted with SIGINT ends on its own.
            if let Process::Running(mut old) =
                std::mem::replace(&mut *process, Process::Running(child))
            {
                std::thread::spawn(move || old.wait());
            }
        }
        let turns = Arc::new(Mutex::new(Turns::default()));
        let (acks_tx, acks) = std_mpsc::channel();
        let reader_turns = turns.clone();
        let reader_process = process.clone();
        let reader_events = events.clone();
        let reader_session = session.clone();
        std::thread::spawn(move || {
            let mut stream = Stream::default();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let value: Value = serde_json::from_str(&line).unwrap_or_default();
                if let Some(id) = value["session_id"].as_str() {
                    *reader_session.lock().unwrap() = Some(id.to_owned());
                }
                if value["type"] == "control_response" {
                    let _ = acks_tx.send(value["response"]["subtype"] == "success");
                    continue;
                }
                let discarding = {
                    let mut turns = reader_turns.lock().unwrap();
                    let discarding = turns.ended < turns.discard_until;
                    if value["type"] == "result" {
                        turns.ended += 1;
                    }
                    discarding
                };
                let line_events = stream.line(&line);
                if discarding {
                    // The rest of an interrupted Turn: nothing of it is said.
                    if value["type"] == "result" {
                        stream = Stream::default();
                    }
                    continue;
                }
                for event in line_events {
                    let _ = reader_events.send(event);
                }
            }
            // claude stopped on its own, not because the Session ended or
            // it was interrupted with SIGINT.
            let ended = matches!(*reader_process.lock().unwrap(), Process::Ended);
            if !ended && !reader_turns.lock().unwrap().retired {
                let _ = reader_events.send(ThinkingEvent::Died);
            }
        });
        Some(Running { stdin, turns, acks })
    }
}

/// What the SessionModel hands its process.
enum ToClaude {
    Turn(String),
    Interrupt,
}

/// One `claude -p` process of a Session.
struct Running {
    stdin: ChildStdin,
    turns: Arc<Mutex<Turns>>,
    /// Whether each control interrupt succeeded, as claude answers.
    acks: std_mpsc::Receiver<bool>,
}

impl Running {
    fn write(&mut self, message: &Value) {
        if let Err(e) = writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush()) {
            // claude is gone; the reader reports it.
            eprintln!("lucyd: can't reach claude: {e}");
        }
    }
}

/// The Turns of one process, shared by its writer and reader.
#[derive(Default)]
struct Turns {
    sent: u64,
    /// Turns whose `result` has been read.
    ended: u64,
    /// Everything up to the end of this Turn was interrupted and is dropped.
    discard_until: u64,
    /// It was interrupted with SIGINT, so it ending isn't a death.
    retired: bool,
}

/// What a `claude -p` process is for.
enum Role {
    Session {
        relay: Relay,
        instructions: String,
    },
    Delegate {
        relay: Relay,
        instructions: String,
        workspace: Workspace,
    },
}

impl Claude {
    /// Returns at once. The guard's checks and the spawn happen on a thread
    /// of their own; Turns sent meanwhile wait for the process.
    fn open_as(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        role: Role,
    ) -> Box<dyn SessionModel> {
        let (inputs, inputs_rx) = std_mpsc::channel();
        let process = Arc::new(Mutex::new(Process::Starting));
        let claude = self.clone();
        let provider = provider.clone();
        let thread_events = events.clone();
        let thread_process = process.clone();
        std::thread::spawn(move || {
            claude.run_session(&provider, &role, &thread_events, inputs_rx, &thread_process);
        });
        Box::new(ClaudeSessionModel {
            inputs,
            process,
            _events: events,
        })
    }
}

impl Thinking for Claude {
    fn open(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
    ) -> Box<dyn SessionModel> {
        let role = Role::Session {
            relay,
            instructions,
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
        let role = Role::Delegate {
            relay,
            instructions,
            workspace,
        };
        self.open_as(provider, events, role)
    }

    fn health(&self, provider: &Provider) -> Result<(), String> {
        self.signed_in(provider).map(drop)
    }

    fn check(&self, provider: &Provider) -> Result<(), String> {
        self.verify(provider)
    }
}

/// Where a Session's `claude -p` is.
enum Process {
    /// The guard is still checking.
    Starting,
    Running(Child),
    /// The Session is over; nothing may be spawned for it any more.
    Ended,
}

struct ClaudeSessionModel {
    inputs: std_mpsc::Sender<ToClaude>,
    process: Arc<Mutex<Process>>,
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl SessionModel for ClaudeSessionModel {
    fn send_turn(&mut self, text: String) {
        // Only fails once the process is gone, which has ended the Turn.
        let _ = self.inputs.send(ToClaude::Turn(text));
    }

    fn interrupt(&mut self) {
        let _ = self.inputs.send(ToClaude::Interrupt);
    }
}

impl Drop for ClaudeSessionModel {
    fn drop(&mut self) {
        let process = std::mem::replace(&mut *self.process.lock().unwrap(), Process::Ended);
        if let Process::Running(mut child) = process {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Turns stream-json lines into ThinkingEvents.
#[derive(Default)]
struct Stream {
    sentences: Sentences,
}

impl Stream {
    fn line(&mut self, line: &str) -> Vec<ThinkingEvent> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        match value["type"].as_str() {
            // Subagent text isn't spoken.
            Some("stream_event") if value["parent_tool_use_id"].is_null() => {
                let event = &value["event"];
                match event["type"].as_str() {
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => self
                        .sentences
                        .push(event["delta"]["text"].as_str().unwrap_or_default()),
                    // Speak a text block's last sentence before any tool call.
                    Some("content_block_stop") => self.sentences.finish().into_iter().collect(),
                    _ => vec![],
                }
            }
            Some("rate_limit_event") => serde_json::from_value(value["rate_limit_info"].clone())
                .map(ThinkingEvent::RateLimit)
                .into_iter()
                .collect(),
            Some("result") => {
                let mut events: Vec<_> = self.sentences.finish().into_iter().collect();
                events.push(ThinkingEvent::TurnEnd);
                events
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subagent_text_is_not_spoken() {
        let mut stream = Stream::default();
        let line = r#"{"type":"stream_event","parent_tool_use_id":"toolu_1","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"secret"}}}"#;
        assert_eq!(stream.line(line), []);
    }

    /// The env vars the child process has removed.
    fn removed_env(allow_api_key: bool) -> Vec<String> {
        let claude = Claude::new("claude".into(), "/work".into());
        let provider = Provider {
            kind: lucy_config::Kind::Claude,
            model: None,
            allow_api_key,
        };
        let relay = Relay {
            program: "lucy".into(),
            args: vec![],
        };
        let command = claude.command(&provider, Some(&relay), "", &[], None);
        command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn the_child_env_drops_billing_overrides_and_keeps_the_oauth_token() {
        let removed = removed_env(false);
        for name in [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ] {
            assert!(removed.iter().any(|r| r == name), "{name} kept");
        }
        assert!(!removed.iter().any(|r| r == "CLAUDE_CODE_OAUTH_TOKEN"));
    }

    #[test]
    fn allow_api_key_keeps_the_child_env() {
        assert_eq!(removed_env(true), Vec::<String>::new());
    }
}
