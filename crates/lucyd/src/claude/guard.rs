use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use lucy_config::Provider;
use serde_json::Value;

use super::Claude;
use crate::plugins::sandbox;

const ROUND_TRIP_WAIT: Duration = Duration::from_secs(60);

const VERIFY_PROMPT: &str = "Reply with just OK.";

const OPT_OUT: &str = "or set allow_api_key = true for this Provider in config.toml";

impl Claude {
    pub(super) fn prepare(&self, provider: &Provider) -> Result<(), String> {
        let version = self.signed_in(provider)?;
        if provider.allow_api_key || self.verified_version().as_deref() == Some(&version) {
            return Ok(());
        }
        self.verify(provider)
    }

    pub(super) fn verify(&self, provider: &Provider) -> Result<(), String> {
        let version = self.signed_in(provider)?;
        self.round_trip(provider)?;
        if !provider.allow_api_key {
            self.record_verified(&version);
        }
        Ok(())
    }

    pub(super) fn signed_in(&self, provider: &Provider) -> Result<String, String> {
        let version = self.version(provider)?;
        if provider.allow_api_key {
            return Ok(version);
        }
        let output = self
            .base_command(sandbox::program(provider, &self.program), provider)
            .args(["auth", "status", "--json"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("I couldn't ask Claude Code who is signed in: {e}."))?;
        let status: Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
        if status["loggedIn"] != true {
            return Err(
                "Claude Code isn't signed in. Run claude in a terminal and sign in \
                        with /login, then wake me again."
                    .to_owned(),
            );
        }
        match status["authMethod"].as_str() {
            Some("claude.ai") => Ok(version),
            method => Err(format!(
                "Claude Code is signed in with {}, not a Claude subscription, so I won't \
                 use it in case it bills you. Sign in with /login, {OPT_OUT}.",
                method.unwrap_or("something unknown")
            )),
        }
    }

    fn version(&self, provider: &Provider) -> Result<String, String> {
        let output = std::fs::create_dir_all(&self.work_dir).and_then(|()| {
            self.base_command(sandbox::program(provider, &self.program), provider)
                .arg("--version")
                .stdin(Stdio::null())
                .output()
        });
        match output {
            Ok(output) if output.status.success() => {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
            }
            Ok(_) => Err("Claude Code is installed but won't start. Run claude in a \
                          terminal to see why, or reinstall it."
                .to_owned()),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(format!(
                "I can't find Claude Code at {}. Please install it, or put claude on \
                 my PATH.",
                sandbox::program(provider, &self.program).display()
            )),
            Err(e) => Err(format!("I couldn't run Claude Code: {e}.")),
        }
    }

    fn round_trip(&self, provider: &Provider) -> Result<(), String> {
        let reply = self.reply_ok(provider)?;
        match reply.result {
            None => {
                return Err("Claude Code didn't answer a test message. Run claude in a \
                            terminal to see what's wrong."
                    .to_owned())
            }
            Some(Err(error)) => {
                return Err(format!(
                    "Claude Code couldn't answer a test message: {error}"
                ))
            }
            Some(Ok(())) => {}
        }
        if provider.allow_api_key {
            return Ok(());
        }
        let source = reply.api_key_source.as_deref().unwrap_or("unknown");
        if source != "none" || !reply.rate_limited {
            return Err(format!(
                "This version of Claude Code may bill an API key instead of your Claude \
                 subscription: its key source is {source}. So I won't use it. Remove \
                 that key from my environment, {OPT_OUT}."
            ));
        }
        Ok(())
    }

    fn reply_ok(&self, provider: &Provider) -> Result<Reply, String> {
        let mut child = self
            .command(
                sandbox::program(provider, &self.program),
                provider,
                None,
                "",
                &[],
                None,
            )
            .arg("--no-session-persistence")
            .spawn()
            .map_err(|e| format!("I couldn't start Claude Code: {e}."))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let message = serde_json::json!(
            {"type": "user", "message": {"role": "user", "content": VERIFY_PROMPT}}
        );
        let _shows_as_a_missing_result = writeln!(stdin, "{message}");
        drop(stdin);
        let (lines_tx, lines) = std_mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if lines_tx.send(line).is_err() {
                    return;
                }
            }
        });
        let deadline = Instant::now() + ROUND_TRIP_WAIT;
        let mut reply = Reply::default();
        while reply.result.is_none() {
            let wait = deadline.saturating_duration_since(Instant::now());
            let Ok(line) = lines.recv_timeout(wait) else {
                break;
            };
            reply.line(&line);
        }
        let _ = child.kill();
        let _ = child.wait();
        Ok(reply)
    }

    fn verified_version(&self) -> Option<String> {
        std::fs::read_to_string(self.verified_path()).ok()
    }

    fn record_verified(&self, version: &str) {
        if let Err(e) = std::fs::write(self.verified_path(), version) {
            eprintln!("lucyd: can't record the verified claude version: {e}");
        }
    }

    fn verified_path(&self) -> PathBuf {
        self.work_dir.join(".verified-claude-version")
    }
}

#[derive(Default)]
struct Reply {
    api_key_source: Option<String>,
    rate_limited: bool,
    result: Option<Result<(), String>>,
}

impl Reply {
    fn line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        match value["type"].as_str() {
            Some("system") if value["subtype"] == "init" => {
                self.api_key_source = value["apiKeySource"].as_str().map(str::to_owned);
            }
            Some("rate_limit_event") => self.rate_limited = true,
            Some("result") if value["is_error"] == true => {
                let error = value["result"].as_str().unwrap_or("an unknown error");
                self.result = Some(Err(error.to_owned()));
            }
            Some("result") => self.result = Some(Ok(())),
            _ => {}
        }
    }
}
