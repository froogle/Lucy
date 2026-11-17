//! The `claude` Thinking Provider: one persistent `claude -p` process per Session.
//! It speaks stream-json, on the user's subscription (ADR 0001).

mod guard;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, Weak};
use std::time::Duration;

use lucy_config::Provider;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::adapters::{
    HandOff, Place, Relay, Role, SessionModel, Thinking, ThinkingEvent, Workspace, PERMISSION_TOOL,
};
use crate::plugins::sandbox;
use crate::process::{self, Process, StderrTail};
use crate::sentences::Sentences;
use crate::sync::Locked;

const DELEGATE_TOOLS: &[&str] = &["Bash", "Read", "Write", "Edit", "Glob", "Grep"];

const HAND_OFF_SETTINGS: &str = "user,project,local";

const INTERRUPT_ACK_WAIT: Duration = Duration::from_secs(1);

const BILLING_ENV: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

#[derive(Clone)]
pub struct Claude {
    program: PathBuf,
    work_dir: PathBuf,
    config_dir: Option<PathBuf>,
    live: Arc<Mutex<Vec<Weak<Run>>>>,
}

impl Claude {
    pub fn new(program: PathBuf, work_dir: PathBuf) -> Claude {
        Claude {
            program,
            work_dir,
            config_dir: None,
            live: Arc::default(),
        }
    }

    /// Runs claude with `config_dir` as its config dir (`CLAUDE_CONFIG_DIR`).
    /// claude keeps its login and conversations there.
    pub fn with_config_dir(self, config_dir: PathBuf) -> Claude {
        Claude {
            config_dir: Some(config_dir),
            ..self
        }
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir
            .clone()
            .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
            .unwrap_or_else(|| sandbox::home().join(".claude"))
    }

    fn base_command(&self, program: &Path, provider: &Provider) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.work_dir).stderr(Stdio::null());
        self.guard_env(&mut command, provider);
        command
    }

    fn guard_env(&self, command: &mut Command, provider: &Provider) {
        if let Some(config_dir) = &self.config_dir {
            command.env("CLAUDE_CONFIG_DIR", config_dir);
        }
        if !provider.allow_api_key {
            for name in BILLING_ENV {
                command.env_remove(name);
            }
        }
    }

    fn hand_off_command(
        &self,
        provider: &Provider,
        relay: &Relay,
        instructions: &str,
        hand_off: &HandOff,
        resume: Option<&str>,
    ) -> Command {
        let mcp_config = serde_json::json!({"mcpServers": {"lucy": {
            "command": relay.program,
            "args": relay.args,
        }}});
        let mut command = Command::new(sandbox::program(provider, &self.program));
        command
            .current_dir(&hand_off.folder)
            .stderr(Stdio::piped())
            .env_clear()
            .envs(hand_off.env.iter().map(|(name, value)| (name, value)));
        self.guard_env(&mut command, provider);
        command
            .args(["-p", "--verbose"])
            .args(["--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .arg("--include-partial-messages")
            .arg("--mcp-config")
            .arg(mcp_config.to_string())
            .args(["--setting-sources", HAND_OFF_SETTINGS])
            .arg("--permission-prompt-tool")
            .arg(format!("mcp__lucy__{PERMISSION_TOOL}"))
            .args(["--allowedTools", "mcp__lucy__*"])
            .arg("--append-system-prompt")
            .arg(instructions);
        if let Some(session) = resume {
            command.args(["--resume", session]);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        command
    }

    fn command(
        &self,
        program: &Path,
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
        let mut command = self.base_command(program, provider);
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

    fn delegate_command(
        &self,
        provider: &Provider,
        relay: &Relay,
        instructions: &str,
        workspace: &Workspace,
        resume: Option<&str>,
    ) -> std::io::Result<Command> {
        let program = sandbox::delegate_program(provider, &self.program)?;
        let mut command = self.command(
            &program,
            provider,
            Some(relay),
            instructions,
            DELEGATE_TOOLS,
            resume,
        );
        let home = sandbox::home();
        let cli_home = [home.join(".claude"), home.join(".claude.json")];
        sandbox::confine_cli(&mut command, workspace, &program, &cli_home)?;
        command.stderr(Stdio::piped());
        Ok(command)
    }

    fn run_session(
        &self,
        provider: &Provider,
        role: &Role,
        events: &mpsc::UnboundedSender<ThinkingEvent>,
        inputs: std_mpsc::Receiver<ToClaude>,
        run: &Arc<Run>,
    ) {
        if let Err(reason) = self.prepare(provider) {
            eprintln!("lucyd: refusing claude: {reason}");
            let _ = events.send(ThinkingEvent::Unavailable(reason));
            return;
        }
        let spawn = || self.spawn(provider, role, events, run);
        let Some(mut running) = spawn() else {
            return;
        };
        let mut control_interrupt_works = true;
        let mut interrupts = 0;
        for input in inputs {
            match input {
                ToClaude::Turn(text) => {
                    running.turns.locked().sent += 1;
                    let message = serde_json::json!(
                        {"type": "user", "message": {"role": "user", "content": text}}
                    );
                    running.write(&message);
                }
                ToClaude::Interrupt => {
                    {
                        let mut turns = running.turns.locked();
                        if turns.sent == turns.ended {
                            continue;
                        }
                        turns.discard_until = turns.sent;
                    }
                    if control_interrupt_works {
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
                        control_interrupt_works = false;
                    }
                    running.turns.locked().sigint_sent = true;
                    if let Process::Running(child) = &*run.process.locked() {
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

    fn spawn(
        &self,
        provider: &Provider,
        role: &Role,
        events: &mpsc::UnboundedSender<ThinkingEvent>,
        run: &Arc<Run>,
    ) -> Option<Running> {
        let resume = run.conversations.locked().last().cloned();
        let resume = resume.as_deref();
        let Role {
            relay,
            instructions,
            place,
        } = role;
        let command = match place {
            Place::WorkDir => Ok(self.command(
                sandbox::program(provider, &self.program),
                provider,
                Some(relay),
                instructions,
                &[],
                resume,
            )),
            Place::Workspace(workspace) => {
                self.delegate_command(provider, relay, instructions, workspace, resume)
            }
            Place::HandOff(hand_off) => {
                Ok(self.hand_off_command(provider, relay, instructions, hand_off, resume))
            }
        };
        let mut child = match command.and_then(|mut command| command.spawn()) {
            Ok(child) => child,
            Err(e) => {
                let program = sandbox::program(provider, &self.program).display();
                eprintln!("lucyd: can't start {program}: {e}");
                let reason = format!("I couldn't start Claude Code: {e}.");
                let _ = events.send(ThinkingEvent::Unavailable(reason));
                return None;
            }
        };
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().map(StderrTail::follow);
        {
            let mut process = run.process.locked();
            if matches!(*process, Process::Ended) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            if let Process::Running(mut interrupted) =
                std::mem::replace(&mut *process, Process::Running(child))
            {
                std::thread::spawn(move || interrupted.wait());
            }
        }
        let turns = Arc::new(Mutex::new(Turns::default()));
        let (acks_tx, acks) = std_mpsc::channel();
        let reader_turns = turns.clone();
        let reader_run = run.clone();
        let reader_events = events.clone();
        std::thread::spawn(move || {
            let mut stream = Stream::default();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let value: Value = serde_json::from_str(&line).unwrap_or_default();
                if let Some(id) = value["session_id"].as_str() {
                    if reader_run.kept(id) {
                        let _ = reader_events.send(ThinkingEvent::Conversation(id.to_owned()));
                    }
                }
                if value["type"] == "control_response" {
                    let _ = acks_tx.send(value["response"]["subtype"] == "success");
                    continue;
                }
                let in_interrupted_turn = {
                    let mut turns = reader_turns.locked();
                    let in_interrupted_turn = turns.ended < turns.discard_until;
                    if value["type"] == "result" {
                        turns.ended += 1;
                    }
                    in_interrupted_turn
                };
                let line_events = stream.line(&line);
                if in_interrupted_turn {
                    if value["type"] == "result" {
                        stream = Stream::default();
                    }
                    continue;
                }
                for event in line_events {
                    let _ = reader_events.send(event);
                }
            }
            let session_ended = matches!(*reader_run.process.locked(), Process::Ended);
            let stopped_on_its_own = !session_ended && !reader_turns.locked().sigint_sent;
            if stopped_on_its_own {
                let exit = process::exit(&reader_run.process, stderr);
                let _ = reader_events.send(ThinkingEvent::Died(exit));
            }
        });
        Some(Running { stdin, turns, acks })
    }
}

enum ToClaude {
    Turn(String),
    Interrupt,
}

struct Running {
    stdin: ChildStdin,
    turns: Arc<Mutex<Turns>>,
    acks: std_mpsc::Receiver<bool>,
}

impl Running {
    fn write(&mut self, message: &Value) {
        if let Err(e) = writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush()) {
            eprintln!("lucyd: can't reach claude: {e}");
        }
    }
}

#[derive(Default)]
struct Turns {
    sent: u64,
    ended: u64,
    discard_until: u64,
    sigint_sent: bool,
}

impl Claude {
    fn open_as(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        role: Role,
    ) -> Box<dyn SessionModel> {
        let (inputs, inputs_rx) = std_mpsc::channel();
        let (cwd, transcripts) = match &role.place {
            Place::WorkDir => (&self.work_dir, Transcripts::Deleted),
            Place::Workspace(workspace) => (&workspace.dir, Transcripts::Deleted),
            Place::HandOff(hand_off) => (&hand_off.folder, Transcripts::Kept),
        };
        let run = Arc::new(Run {
            process: Mutex::new(Process::Starting),
            conversations: Mutex::default(),
            transcripts,
            config_dir: self.config_dir(),
            cwd: cwd.clone(),
        });
        {
            let mut live = self.live.locked();
            live.retain(|run| run.strong_count() > 0);
            live.push(Arc::downgrade(&run));
        }
        let claude = self.clone();
        let provider = provider.clone();
        let thread_events = events.clone();
        let thread_run = run.clone();
        std::thread::spawn(move || {
            claude.run_session(&provider, &role, &thread_events, inputs_rx, &thread_run);
        });
        Box::new(ClaudeSessionModel {
            inputs,
            run,
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

    fn open_hand_off(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
        hand_off: HandOff,
    ) -> Box<dyn SessionModel> {
        let role = Role {
            relay,
            instructions,
            place: Place::HandOff(hand_off),
        };
        self.open_as(provider, events, role)
    }

    fn health(&self, provider: &Provider) -> Result<(), String> {
        self.signed_in(provider).map(drop)
    }

    fn check(&self, provider: &Provider) -> Result<(), String> {
        self.verify(provider)
    }

    fn executable(&self, provider: &Provider) -> Option<PathBuf> {
        sandbox::delegate_program(provider, &self.program).ok()
    }

    fn shutdown(&self) {
        let live = std::mem::take(&mut *self.live.locked());
        for run in live.iter().filter_map(Weak::upgrade) {
            run.end();
        }
    }

    fn sweep(&self) {
        let project = self
            .config_dir()
            .join("projects")
            .join(project_dir(&self.work_dir));
        for entry in std::fs::read_dir(project).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !is_conversation_id(name.strip_suffix(".jsonl").unwrap_or(&name)) {
                continue;
            }
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
        }
    }
}

struct Run {
    process: Mutex<Process>,
    conversations: Mutex<Vec<String>>,
    transcripts: Transcripts,
    config_dir: PathBuf,
    cwd: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transcripts {
    Deleted,
    Kept,
}

impl Run {
    fn kept(&self, id: &str) -> bool {
        let mut conversations = self.conversations.locked();
        let new = !conversations.iter().any(|kept| kept == id);
        if new {
            conversations.push(id.to_owned());
        }
        new
    }

    fn end(&self) {
        let process = std::mem::replace(&mut *self.process.locked(), Process::Ended);
        if let Process::Running(mut child) = process {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.transcripts == Transcripts::Kept {
            return;
        }
        let project = self
            .config_dir
            .join("projects")
            .join(project_dir(&self.cwd));
        for id in std::mem::take(&mut *self.conversations.locked()) {
            let _ = std::fs::remove_file(project.join(format!("{id}.jsonl")));
            let _ = std::fs::remove_dir_all(project.join(&id));
        }
    }
}

fn is_conversation_id(name: &str) -> bool {
    const DASHES: [usize; 4] = [8, 13, 18, 23];
    name.len() == 36
        && name.char_indices().all(|(i, c)| {
            if DASHES.contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn project_dir(cwd: &Path) -> String {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

struct ClaudeSessionModel {
    inputs: std_mpsc::Sender<ToClaude>,
    run: Arc<Run>,
    _events: mpsc::UnboundedSender<ThinkingEvent>,
}

impl SessionModel for ClaudeSessionModel {
    fn send_turn(&mut self, text: String) {
        let _fails_once_the_turn_has_ended = self.inputs.send(ToClaude::Turn(text));
    }

    fn interrupt(&mut self) {
        let _ = self.inputs.send(ToClaude::Interrupt);
    }
}

impl Drop for ClaudeSessionModel {
    fn drop(&mut self) {
        self.run.end();
    }
}

#[derive(Default)]
struct Stream {
    sentences: Sentences,
}

impl Stream {
    fn finish_block(&mut self) -> Vec<ThinkingEvent> {
        self.sentences.finish().into_iter().collect()
    }

    fn line(&mut self, line: &str) -> Vec<ThinkingEvent> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        match value["type"].as_str() {
            Some("stream_event") if !is_subagent(&value) => {
                let event = &value["event"];
                match event["type"].as_str() {
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => self
                        .sentences
                        .push(event["delta"]["text"].as_str().unwrap_or_default()),
                    Some("content_block_stop") => self.finish_block(),
                    _ => vec![],
                }
            }
            Some("rate_limit_event") => serde_json::from_value(value["rate_limit_info"].clone())
                .map(ThinkingEvent::RateLimit)
                .into_iter()
                .collect(),
            Some("result") => {
                let mut events: Vec<_> = self.sentences.finish().into_iter().collect();
                match failure(&value) {
                    Some(message) => events.push(ThinkingEvent::Failed(message)),
                    None => {
                        if let Some(answer) = value["result"].as_str() {
                            events.push(ThinkingEvent::Final(answer.to_owned()));
                        }
                        events.push(ThinkingEvent::TurnEnd);
                    }
                }
                events
            }
            _ => vec![],
        }
    }
}

fn failure(result: &Value) -> Option<String> {
    let subtype = result["subtype"].as_str().unwrap_or_default();
    if result["is_error"] != true && !subtype.starts_with("error") {
        return None;
    }
    let errors = result["errors"].as_array().map(|errors| {
        errors
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; ")
    });
    let message = result["result"]
        .as_str()
        .map(str::to_owned)
        .or(errors)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("claude ended the Turn with {subtype}"));
    Some(message)
}

fn is_subagent(value: &Value) -> bool {
    !value["parent_tool_use_id"].is_null()
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

    fn removed_env(allow_api_key: bool) -> Vec<String> {
        let claude = Claude::new("claude".into(), "/work".into());
        let provider = Provider {
            kind: lucy_config::Kind::Claude,
            model: None,
            allow_api_key,
            program: None,
        };
        let relay = Relay {
            program: "lucy".into(),
            args: vec![],
        };
        let command = claude.command(Path::new("claude"), &provider, Some(&relay), "", &[], None);
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
