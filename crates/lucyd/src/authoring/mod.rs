//! Authoring: when no Plugin fits a request, the model calls
//! `request_plugin` and lucyd hands the goal to an advanced-tier Delegate
//! (ADR 0002 amendment). The Delegate writes the Plugin in a staging dir,
//! `.build/<name>/` in the Plugins folder, starting from the shipped
//! template and confined to it by Landlock. It sees only the Delegate
//! tools. Once its build and tests pass, Lucy asks the user's Approval,
//! answers the request it was built for, and asks "Is that the one?"; a no
//! goes back to the Delegate as a fix.
//!
//! Lucy speaks for a build only once neither she nor the user is speaking.

mod template;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use lucy_config::Tier;
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde_json::json;
use tokio::sync::{mpsc, watch, Notify};

use crate::adapters::{SessionModel, Thinking, ThinkingEvent, Workspace};
use crate::mcp::{error, text, tool, Caller, Relays, Token};
use crate::plugins::{sandbox, Host};
use crate::runner::{self, Command};
use crate::session::{Answer, Expects, Question};

/// The Delegate's system prompt.
const BRIEF: &str = include_str!("brief.md");

/// What Lucy says once when a build starts.
const A_FEW_MINUTES: &str = "This is going to take a few minutes.";

/// What Lucy asks after answering the request a new Plugin was built for.
const IS_THAT_THE_ONE: &str = "Is that the one?";

/// How many lines of a failed build's or test run's output the Delegate,
/// and then the user, hear.
const OUTPUT_LINES: usize = 20;

/// Lives as long as lucyd: starts builds, one at a time.
pub struct Authoring {
    thinking: Arc<dyn Thinking>,
    relays: Relays,
    /// `config.toml`, for the advanced tier's Provider.
    config_path: PathBuf,
    /// User and Lucy-written Plugins; staging is under `.build/`.
    plugins_dir: PathBuf,
    host: Arc<Host>,
    commands: mpsc::UnboundedSender<Command>,
    /// Whether a build is running.
    building: watch::Sender<bool>,
    current: Mutex<Option<Arc<Build>>>,
}

impl Authoring {
    pub fn new(
        thinking: Arc<dyn Thinking>,
        relays: Relays,
        config_path: PathBuf,
        plugins_dir: PathBuf,
        host: Arc<Host>,
        commands: mpsc::UnboundedSender<Command>,
    ) -> Arc<Authoring> {
        Arc::new(Authoring {
            thinking,
            relays,
            config_path,
            plugins_dir,
            host,
            commands,
            building: watch::Sender::new(false),
            current: Mutex::default(),
        })
    }

    /// Whether a build is running, as it changes.
    pub fn building(&self) -> watch::Receiver<bool> {
        self.building.subscribe()
    }

    /// `request_plugin`: starts a build, and Lucy says it will take a few
    /// minutes.
    pub fn request(self: &Arc<Self>, arguments: &JsonObject) -> CallToolResult {
        let [name, goal, clarifications, request] =
            ["name", "goal", "clarifications", "request"].map(|key| string(arguments, key));
        if !is_plugin_name(&name) {
            return error("name must be a short lowercase name for the Plugin, such as photos.");
        }
        if goal.trim().is_empty() || request.trim().is_empty() {
            return error("goal and request must say what the Plugin is for.");
        }
        let mut current = self.current.lock().unwrap();
        if let Some(build) = &*current {
            return error(format!(
                "I'm already building the {} Plugin. Tell the user, and offer to build this one \
                 once it's done.",
                build.name
            ));
        }
        if let Err(reason) = self.host.may_author(&name) {
            return error(format!("I can't build it: {reason}."));
        }
        let provider = lucy_config::load(&self.config_path)
            .map_err(|e| e.to_string())
            .and_then(|config| config.provider(Tier::Advanced).cloned());
        let provider = match provider {
            Ok(provider) => provider,
            Err(reason) => return error(format!("I can't start a build: {reason}")),
        };
        let workspace = match self.workspace(&name) {
            Ok(workspace) => workspace,
            Err(e) => return error(format!("I can't make the build folder: {e}")),
        };

        let build = Arc::new(Build {
            name: name.clone(),
            request,
            workspace: workspace.clone(),
            authoring: Arc::downgrade(self),
            state: Mutex::new(BuildState {
                stage: Stage::Working,
                turns: 0,
                failure: None,
                model: None,
                token: None,
            }),
            over: Notify::new(),
        });
        let (relay, token) = self.relays.issue(Caller::Delegate(Arc::downgrade(&build)));
        let (events_tx, events) = mpsc::unbounded_channel();
        let model =
            self.thinking
                .open_delegate(&provider, events_tx, relay, BRIEF.to_owned(), workspace);
        {
            let mut state = build.state.lock().unwrap();
            state.model = Some(model);
            state.token = Some(token);
        }
        build.send_turn(format!(
            "Build the {name} Plugin.\n\nGoal: {goal}\n\nWhat the user has told Lucy: \
             {clarifications}\n\nThe request it's for: \"{}\"",
            build.request
        ));
        *current = Some(build.clone());
        drop(current);
        self.building.send_replace(true);
        tokio::spawn(build.follow(events));
        let _ = self.commands.send(Command::Say(A_FEW_MINUTES.to_owned()));
        text(
            "The build has started, and Lucy has told the user it will take a few minutes. \
             Don't say anything more about it; carry on with anything else they ask. Lucy will \
             tell them when it's ready.",
        )
    }

    /// The staging dir for the Plugin `name`, made from the template if it
    /// isn't there from an earlier build, and what else it may touch.
    fn workspace(&self, name: &str) -> std::io::Result<Workspace> {
        let build_dir = self.plugins_dir.join(".build");
        let dir = build_dir.join(name);
        let tmp = build_dir.join(".tmp").join(name);
        if !dir.exists() {
            template::write(&dir, name)?;
        }
        std::fs::create_dir_all(&tmp)?;
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let toolchain_home = |var: &str, default: &str| {
            std::env::var_os(var).map_or_else(|| home.join(default), PathBuf::from)
        };
        let relay = self.relays.program();
        Ok(Workspace {
            dir,
            tmp,
            writable: vec![toolchain_home("CARGO_HOME", ".cargo")],
            readable: [toolchain_home("RUSTUP_HOME", ".rustup")]
                .into_iter()
                .chain(sandbox::program_dir(&relay))
                .collect(),
        })
    }

    /// The build is over: another may start.
    fn finished(&self, build: &Build) {
        let mut current = self.current.lock().unwrap();
        if current.as_deref().is_some_and(|c| std::ptr::eq(c, build)) {
            *current = None;
            self.building.send_replace(false);
        }
    }
}

/// The Session model's tool that starts Authoring.
pub fn request_plugin_tool() -> Tool {
    tool(
        "request_plugin",
        "Build a new Plugin when none of your tools fits what the user asked. First ask them \
         only what only they can answer (where their files are, which platform they use), \
         never how to build or connect it. Lucy tells them it will take a few minutes, and \
         answers their request once they've approved the Plugin.",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "A short lowercase name for the Plugin, such as photos"},
                "goal": {"type": "string", "description": "What the Plugin should do, in general"},
                "clarifications": {"type": "string", "description": "Everything the user told you that the builder needs"},
                "request": {"type": "string", "description": "The user's request, in their words, to answer once it's built"},
            },
            "required": ["name", "goal", "clarifications", "request"],
        }),
    )
}

/// The tools a Delegate sees, and no others.
pub fn delegate_tools() -> Vec<Tool> {
    let string = |description: &str| json!({"type": "string", "description": description});
    vec![
        tool(
            "report_progress",
            "Report what you're doing, in a short line. It is shown, never spoken.",
            json!({"type": "object", "properties": {"text": string("What you're doing")}, "required": ["text"]}),
        ),
        tool(
            "ask_user",
            "Ask the user something only they can answer. Their answer comes as your next \
             message.",
            json!({"type": "object", "properties": {"question": string("One short spoken question")}, "required": ["question"]}),
        ),
        tool(
            "submit_plugin",
            "Build the Plugin, run its tests and check its Manifest. On a pass the user is \
             asked to approve it.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "cannot",
            "Give up because the goal can't be met with permitted access.",
            json!({
                "type": "object",
                "properties": {
                    "reason": string("Why, plainly"),
                    "fallback": string("The best permitted alternative"),
                },
                "required": ["reason", "fallback"],
            }),
        ),
    ]
}

/// One Plugin being built by its Delegate.
pub struct Build {
    name: String,
    /// The request it is built for, in the user's words.
    request: String,
    workspace: Workspace,
    authoring: Weak<Authoring>,
    state: Mutex<BuildState>,
    /// Told when the build is over.
    over: Notify,
}

struct BuildState {
    stage: Stage,
    /// Turns sent to the Delegate that haven't ended.
    turns: usize,
    /// Why its last submission failed.
    failure: Option<String>,
    /// The Delegate's model process, until the build is over.
    model: Option<Box<dyn SessionModel>>,
    /// Lets the Delegate reach its tools, until the build is over.
    token: Option<Token>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The Delegate is working on it.
    Working,
    /// lucyd is building and testing what it submitted.
    Checking,
    /// Waiting for the user's answer to the Delegate's question.
    Asking,
    /// It passed; the user is being asked about it.
    Approving,
    Over,
}

impl Build {
    /// Follows the Delegate's model process until the build is over.
    async fn follow(self: Arc<Self>, mut events: mpsc::UnboundedReceiver<ThinkingEvent>) {
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(ThinkingEvent::TurnEnd) => self.turn_ended(),
                    Some(ThinkingEvent::Unavailable(reason)) => self.give_up(reason),
                    Some(ThinkingEvent::Died) | None => {
                        self.give_up("the builder stopped unexpectedly".to_owned());
                        return;
                    }
                    Some(_) => {}
                },
                () = self.over.notified() => return,
            }
        }
    }

    /// A call to one of the Delegate tools.
    pub async fn call(
        self: &Arc<Self>,
        name: &str,
        arguments: &JsonObject,
    ) -> Option<CallToolResult> {
        Some(match name {
            "report_progress" => {
                eprintln!(
                    "lucyd: building {}: {}",
                    self.name,
                    string(arguments, "text")
                );
                text("Noted.")
            }
            "ask_user" => self.ask_user(string(arguments, "question")),
            "submit_plugin" => self.submit().await,
            "cannot" => self.cannot(string(arguments, "reason"), string(arguments, "fallback")),
            _ => return None,
        })
    }

    /// Sends the Delegate a Turn.
    fn send_turn(&self, text: String) {
        let mut state = self.state.lock().unwrap();
        if let Some(model) = &mut state.model {
            model.send_turn(text);
            state.turns += 1;
        }
    }

    /// Moves from `from` to `to`, or says why the Delegate can't do that
    /// now.
    fn advance(&self, from: Stage, to: Stage) -> Result<(), CallToolResult> {
        let mut state = self.state.lock().unwrap();
        if state.stage != from {
            return Err(error(match state.stage {
                Stage::Asking => "Wait for the user's answer, which comes as your next message.",
                Stage::Checking => "Your last submission is still being checked.",
                Stage::Approving => "The user is being asked about it; end your turn.",
                Stage::Working | Stage::Over => "The build is over.",
            }));
        }
        state.stage = to;
        Ok(())
    }

    fn ask_user(self: &Arc<Self>, question: String) -> CallToolResult {
        if question.trim().is_empty() {
            return error("question must be what to ask the user.");
        }
        if let Err(refused) = self.advance(Stage::Working, Stage::Asking) {
            return refused;
        }
        let Some(authoring) = self.authoring.upgrade() else {
            return error("Lucy is stopping.");
        };
        let question = Question {
            prompt: question,
            expects: Expects::Words,
        };
        let answer = runner::ask_when_quiet(&authoring.commands, question, None);
        let build = self.clone();
        tokio::spawn(async move {
            let said = match answer.await {
                Some(Answer::Said(said)) => format!("The user answered: \"{said}\""),
                Some(_) => "The user didn't answer. Carry on with your best judgement, or call \
                            cannot."
                    .to_owned(),
                None => "The user couldn't be asked: no Session is open. Carry on with your \
                         best judgement."
                    .to_owned(),
            };
            if build.advance(Stage::Asking, Stage::Working).is_ok() {
                build.send_turn(said);
            }
        });
        text(
            "Lucy will ask the user once neither of you is speaking. Their answer comes as your \
             next message; end your turn unless you have other work.",
        )
    }

    async fn submit(self: &Arc<Self>) -> CallToolResult {
        if let Err(refused) = self.advance(Stage::Working, Stage::Checking) {
            return refused;
        }
        let Some(authoring) = self.authoring.upgrade() else {
            return error("Lucy is stopping.");
        };
        match self.check(&authoring).await {
            Ok(()) => {
                let _ = self.advance(Stage::Checking, Stage::Approving);
                tokio::spawn(self.clone().approve(authoring));
                text(
                    "It passed. Lucy is asking the user to approve it; end your turn. If they \
                     want changes, their words come as your next message.",
                )
            }
            Err(reason) => {
                let _ = self.advance(Stage::Checking, Stage::Working);
                self.state.lock().unwrap().failure = Some(reason.clone());
                error(format!(
                    "{reason}\n\nFix it and call submit_plugin again, call ask_user if only the \
                     user can say, or cannot if it can't be done with permitted access."
                ))
            }
        }
    }

    /// Builds and tests what the Delegate submitted, in its sandbox, then
    /// installs it as awaiting Approval and checks its Manifest against the
    /// tools it lists.
    async fn check(&self, authoring: &Authoring) -> Result<(), String> {
        self.run("build.sh", "Its build").await?;
        self.run("test.sh", "Its tests").await?;
        authoring
            .host
            .install_authored(&self.name, &self.workspace.dir)?;
        authoring.host.check_authored(&self.name).await
    }

    /// Runs `script` in the staging dir, confined as the Delegate is.
    async fn run(&self, script: &str, what: &str) -> Result<(), String> {
        let workspace = &self.workspace;
        let mut command = tokio::process::Command::new("sh");
        command
            .arg(script)
            .current_dir(&workspace.dir)
            .env("TMPDIR", &workspace.tmp)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let writable: Vec<PathBuf> = [workspace.dir.clone(), workspace.tmp.clone()]
            .into_iter()
            .chain(workspace.writable.iter().cloned())
            .collect();
        sandbox::confine_delegate(command.as_std_mut(), &workspace.readable, &writable)
            .map_err(|e| format!("{what} couldn't be sandboxed: {e}"))?;
        let output = command
            .output()
            .await
            .map_err(|e| format!("{what} couldn't run: {e}"))?;
        if output.status.success() {
            return Ok(());
        }
        let printed = [output.stdout, output.stderr].concat();
        let printed = String::from_utf8_lossy(&printed);
        let lines: Vec<&str> = printed.lines().collect();
        let tail = lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n");
        Err(format!("{what} failed ({}):\n{tail}", output.status))
    }

    /// Asks the user's Approval; on a yes, answers the request the Plugin
    /// was built for and asks whether that was the one.
    async fn approve(self: Arc<Self>, authoring: Arc<Authoring>) {
        match authoring.host.request_approval_when_quiet(&self.name).await {
            Ok(true) => {}
            Ok(false) => return self.finish(),
            Err(reason) => {
                eprintln!("lucyd: the {} Plugin wasn't approved: {reason}", self.name);
                return self.finish();
            }
        }
        let _ = authoring.commands.send(Command::Turn(format!(
            "[Lucy's Plugin build] The user approved the new {} Plugin, and its tools are \
             yours now. Use it to answer the request it was built for, as if they'd just \
             asked: \"{}\". Don't ask whether that's what they wanted; Lucy asks that next.",
            self.name, self.request
        )));
        let question = Question {
            prompt: IS_THAT_THE_ONE.to_owned(),
            expects: Expects::YesOrWords,
        };
        match runner::ask_when_quiet(&authoring.commands, question, None).await {
            Some(Answer::Said(said)) => {
                if self.advance(Stage::Approving, Stage::Working).is_ok() {
                    self.send_turn(format!(
                        "The user tried the {} Plugin on \"{}\" and said it isn't right: \
                         \"{said}\". Fix it in the same folder, then call submit_plugin again.",
                        self.name, self.request
                    ));
                }
            }
            _ => self.finish(),
        }
    }

    fn cannot(&self, reason: String, fallback: String) -> CallToolResult {
        if reason.trim().is_empty() {
            return error("reason must say why it can't be done.");
        }
        if let Err(refused) = self.advance(Stage::Working, Stage::Over) {
            return refused;
        }
        self.tell(format!(
            "[Lucy's Plugin build] The {} Plugin can't be built with permitted access: \
             {reason}. The best permitted fallback: {fallback}. Tell the user that limit \
             plainly, in a sentence, and offer the fallback.",
            self.name
        ));
        self.finish();
        text("Lucy will tell the user. End your turn.")
    }

    /// The Delegate ended a Turn. If it was working and has nothing more to
    /// do, it has given up.
    fn turn_ended(&self) {
        let reason = {
            let mut state = self.state.lock().unwrap();
            state.turns = state.turns.saturating_sub(1);
            if state.turns > 0 || state.stage != Stage::Working {
                return;
            }
            state.failure.take()
        };
        self.give_up(reason.unwrap_or_else(|| "the builder stopped without submitting it".into()));
    }

    /// The build failed for `reason`: Lucy says so.
    fn give_up(&self, reason: String) {
        if self.state.lock().unwrap().stage == Stage::Over {
            return;
        }
        self.tell(format!(
            "[Lucy's Plugin build] The {} Plugin couldn't be built: {reason}\n\nTell the user \
             plainly, in a sentence or two, what failed.",
            self.name
        ));
        self.finish();
    }

    /// Starts a Turn of the Session model once neither side is speaking.
    fn tell(&self, text: String) {
        if let Some(authoring) = self.authoring.upgrade() {
            let _ = authoring.commands.send(Command::Turn(text));
        }
    }

    /// Ends the build: the Delegate stops and its token is revoked. The
    /// staging dir is kept.
    fn finish(&self) {
        let (model, token) = {
            let mut state = self.state.lock().unwrap();
            state.stage = Stage::Over;
            (state.model.take(), state.token.take())
        };
        drop((model, token));
        if let Some(authoring) = self.authoring.upgrade() {
            authoring.finished(self);
        }
        self.over.notify_one();
    }
}

/// A Plugin name: lowercase letters, digits and dashes, starting with a
/// letter.
fn is_plugin_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The string argument `key`, or an empty string.
fn string(arguments: &JsonObject, key: &str) -> String {
    arguments
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}
