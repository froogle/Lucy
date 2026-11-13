//! The `claude` Thinking Provider: one persistent `claude -p` stream-json
//! process per Session, on the user's subscription (ADR 0001).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::adapters::{Relay, SessionModel, Thinking, ThinkingEvent};

/// Until the Persona lands (ticket 16).
const SYSTEM_PROMPT: &str = "You are Lucy, a voice assistant. Everything you write is spoken \
aloud. Answer in one to three short spoken sentences. No markdown, no lists, no emoji. \
Before calling a tool that may be slow, first say \"Let me check.\"";

/// Settings that would make `claude` bill something other than the user's
/// subscription. Everything above `CLAUDE_CODE_OAUTH_TOKEN` in Claude Code's
/// credential precedence. `apiKeyHelper` lives only in settings files, which
/// `--setting-sources ""` keeps out.
const SCRUBBED_ENV: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

pub struct Claude {
    program: PathBuf,
    /// A Lucy-owned directory, so no project's `.mcp.json` or hooks apply.
    work_dir: PathBuf,
}

impl Claude {
    pub fn new(program: PathBuf, work_dir: PathBuf) -> Claude {
        Claude { program, work_dir }
    }

    fn command(&self, relay: &Relay) -> Command {
        let mcp_config = serde_json::json!({"mcpServers": {"lucy": {
            "command": relay.program,
            "args": relay.args,
        }}});
        let mut command = Command::new(&self.program);
        command
            .args(["-p", "--verbose"])
            .args(["--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .args(["--include-partial-messages", "--no-session-persistence"])
            .arg("--strict-mcp-config")
            .arg("--mcp-config")
            .arg(mcp_config.to_string())
            .args(["--setting-sources", ""])
            .arg("--disable-slash-commands")
            .args(["--tools", ""])
            .args(["--permission-mode", "dontAsk"])
            .args(["--allowedTools", "mcp__lucy__*"])
            .args(["--append-system-prompt", SYSTEM_PROMPT])
            .current_dir(&self.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in SCRUBBED_ENV {
            command.env_remove(name);
        }
        command
    }
}

impl Thinking for Claude {
    fn open(
        &self,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
    ) -> Box<dyn SessionModel> {
        let child =
            std::fs::create_dir_all(&self.work_dir).and_then(|()| self.command(&relay).spawn());
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                eprintln!("lucyd: can't start {}: {e}", self.program.display());
                return Box::new(ClaudeSessionModel {
                    process: None,
                    events,
                    in_turn: Arc::default(),
                });
            }
        };
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let reader_events = events.clone();
        let in_turn = Arc::new(AtomicBool::new(false));
        let reader_in_turn = in_turn.clone();
        std::thread::spawn(move || {
            let mut stream = Stream::default();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                for event in stream.line(&line) {
                    if event == ThinkingEvent::TurnEnd {
                        reader_in_turn.store(false, Ordering::SeqCst);
                    }
                    let _ = reader_events.send(event);
                }
            }
            // claude died: don't leave the Session waiting on the Turn.
            if reader_in_turn.swap(false, Ordering::SeqCst) {
                let _ = reader_events.send(ThinkingEvent::TurnEnd);
            }
        });
        Box::new(ClaudeSessionModel {
            process: Some((child, stdin)),
            events,
            in_turn,
        })
    }
}

struct ClaudeSessionModel {
    process: Option<(Child, ChildStdin)>,
    events: mpsc::UnboundedSender<ThinkingEvent>,
    /// A Turn was sent and its `TurnEnd` hasn't been.
    in_turn: Arc<AtomicBool>,
}

impl SessionModel for ClaudeSessionModel {
    fn send_turn(&mut self, text: String) {
        let message =
            serde_json::json!({"type": "user", "message": {"role": "user", "content": text}});
        self.in_turn.store(true, Ordering::SeqCst);
        let sent = match &mut self.process {
            Some((_, stdin)) => writeln!(stdin, "{message}").and_then(|()| stdin.flush()),
            None => Err(std::io::ErrorKind::NotFound.into()),
        };
        if let Err(e) = sent {
            // Don't leave the Session waiting on a reply that can't come.
            eprintln!("lucyd: can't reach claude: {e}");
            if self.in_turn.swap(false, Ordering::SeqCst) {
                let _ = self.events.send(ThinkingEvent::TurnEnd);
            }
        }
    }
}

impl Drop for ClaudeSessionModel {
    fn drop(&mut self) {
        if let Some((mut child, _)) = self.process.take() {
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

/// Splits streamed text at sentence ends: a newline, or a `.`, `!` or `?`
/// followed by whitespace. Each sentence is closed as soon as the text that
/// proves it complete arrives.
#[derive(Default)]
struct Sentences {
    /// Non-blank text has been passed on since the last `SentenceEnd`.
    open: bool,
    /// The last character passed on ends a sentence if whitespace follows.
    after_terminator: bool,
}

impl Sentences {
    fn push(&mut self, text: &str) -> Vec<ThinkingEvent> {
        let mut events = vec![];
        let mut start = 0;
        for (i, c) in text.char_indices() {
            let ends = c == '\n' || (self.after_terminator && c.is_whitespace());
            if ends && (self.open || !text[start..i].trim().is_empty()) {
                if i > start {
                    events.push(ThinkingEvent::Delta(text[start..i].to_owned()));
                }
                events.push(ThinkingEvent::SentenceEnd);
                self.open = false;
                start = i;
            }
            self.after_terminator = matches!(c, '.' | '!' | '?');
        }
        if start < text.len() {
            events.push(ThinkingEvent::Delta(text[start..].to_owned()));
            self.open |= !text[start..].trim().is_empty();
        }
        events
    }

    /// Ends the sentence in progress, if any.
    fn finish(&mut self) -> Option<ThinkingEvent> {
        self.after_terminator = false;
        std::mem::take(&mut self.open).then_some(ThinkingEvent::SentenceEnd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ThinkingEvent::{Delta, SentenceEnd};

    fn delta(text: &str) -> ThinkingEvent {
        Delta(text.to_owned())
    }

    #[test]
    fn a_sentence_ends_as_soon_as_whitespace_follows_its_terminator() {
        let mut s = Sentences::default();
        assert_eq!(s.push("Hello"), [delta("Hello")]);
        assert_eq!(s.push(" there."), [delta(" there.")]);
        assert_eq!(s.push(" How"), [SentenceEnd, delta(" How")]);
    }

    #[test]
    fn one_delta_can_end_several_sentences() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("Yes! Sure? Fine.\nOK"),
            [
                delta("Yes!"),
                SentenceEnd,
                delta(" Sure?"),
                SentenceEnd,
                delta(" Fine."),
                SentenceEnd,
                delta("\nOK"),
            ]
        );
    }

    #[test]
    fn a_newline_ends_a_sentence() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("Hi\nThere"),
            [delta("Hi"), SentenceEnd, delta("\nThere")]
        );
        // Across deltas too, and a blank line doesn't end an empty sentence.
        assert_eq!(s.push("\n"), [SentenceEnd, delta("\n")]);
        assert_eq!(s.push("\nOK"), [delta("\nOK")]);
    }

    #[test]
    fn a_terminator_inside_a_word_or_number_does_not_end_a_sentence() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("It costs 3.50 at example.com today"),
            [delta("It costs 3.50 at example.com today")]
        );
    }

    #[test]
    fn finishing_ends_the_open_sentence_once() {
        let mut s = Sentences::default();
        s.push("Let me check.");
        assert_eq!(s.finish(), Some(SentenceEnd));
        assert_eq!(s.finish(), None);
        // The next block's first space doesn't end an empty sentence.
        assert_eq!(s.push(" Done"), [delta(" Done")]);
    }

    #[test]
    fn subagent_text_is_not_spoken() {
        let mut stream = Stream::default();
        let line = r#"{"type":"stream_event","parent_tool_use_id":"toolu_1","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"secret"}}}"#;
        assert_eq!(stream.line(line), []);
    }

    #[test]
    fn the_child_env_drops_billing_overrides_and_keeps_the_oauth_token() {
        let claude = Claude::new("claude".into(), "/work".into());
        let relay = Relay {
            program: "lucy".into(),
            args: vec![],
        };
        let command = claude.command(&relay);
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_str().unwrap().to_owned())
            .collect();
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
}
