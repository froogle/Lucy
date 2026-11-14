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
//! A build outlives the Session it started in: its question or Approval
//! waits for the next one. One build runs at a time, with one more queued;
//! each has a time budget, and only "cancel it" stops one early.

mod template;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lucy_config::Tier;
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde_json::json;
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use crate::adapters::{
    Adapters, Clock, Notification, NotificationKind, Notifier, SessionModel, Thinking,
    ThinkingEvent, Timer, Workspace,
};
use crate::mcp::{error, text, tool, Caller, Relays, Token};
use crate::model_text::PLUGIN_BUILD;
use crate::plugins::{sandbox, Host};
use crate::runner::{self, Command, When};
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
    /// For what a build shows rather than says.
    notifier: Arc<dyn Notifier>,
    /// What each build's budget counts on.
    clock: Arc<dyn Clock>,
    building: watch::Sender<Building>,
    builds: Mutex<Builds>,
}

/// What Authoring is doing, for the building mark and the Session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Building {
    #[default]
    No,
    /// The Delegate is working on a build, or lucyd is checking it.
    Working,
    /// A build waits on the user: for an answer, or their Approval.
    Waiting,
}

#[derive(Default)]
struct Builds {
    current: Option<Arc<Build>>,
    /// The one request that waits for the current build to end.
    queued: Option<Request>,
}

/// What `request_plugin` asked for.
struct Request {
    name: String,
    goal: String,
    clarifications: String,
    /// The user's words, answered once the Plugin is approved.
    request: String,
}

impl Authoring {
    /// Builds through `adapters`' Thinking, notifier and clock.
    pub fn new(
        adapters: &Adapters,
        relays: Relays,
        config_path: PathBuf,
        plugins_dir: PathBuf,
        host: Arc<Host>,
        commands: mpsc::UnboundedSender<Command>,
    ) -> Arc<Authoring> {
        Arc::new(Authoring {
            thinking: adapters.thinking.clone(),
            relays,
            config_path,
            plugins_dir,
            host,
            commands,
            notifier: adapters.notifier.clone(),
            clock: adapters.clock.clone(),
            building: watch::Sender::new(Building::No),
            builds: Mutex::default(),
        })
    }

    /// What Authoring is doing, as it changes.
    pub fn building(&self) -> watch::Receiver<Building> {
        self.building.subscribe()
    }

    /// Publishes what the current build in `builds` is doing.
    fn show_building(&self, builds: &Builds) {
        let building = match &builds.current {
            Some(build) => build.state.lock().unwrap().stage.building(),
            None => Building::No,
        };
        self.building.send_replace(building);
    }

    /// Shows `notification`, which offers no action.
    fn show(&self, notification: Notification) {
        self.notifier
            .notify(notification, mpsc::unbounded_channel().0);
    }

    /// Shows a `report_progress` line from the build of `plugin`.
    fn show_progress(&self, plugin: &str, progress: String) {
        self.show(build_notification(
            format!("Building the {plugin} Plugin"),
            progress,
        ));
    }

    /// The Session ended without Lucy saying she'll keep working on the
    /// build running, if one is: she shows it instead.
    pub fn show_still_building(&self) {
        let building = self.builds.lock().unwrap().current.clone();
        if let Some(build) = building {
            self.show(build_notification(
                format!("Lucy is still building the {} Plugin", build.name),
                "She'll keep working on it, and let you know when it's ready.".to_owned(),
            ));
        }
    }

    /// `request_plugin`: starts a build, and Lucy says it will take a few
    /// minutes; or, while one runs, queues it, and she says so.
    pub fn request(self: &Arc<Self>, arguments: &JsonObject) -> CallToolResult {
        let [name, goal, clarifications, request] =
            ["name", "goal", "clarifications", "request"].map(|key| string(arguments, key));
        if !is_plugin_name(&name) {
            return error("name must be a short lowercase name for the Plugin, such as photos.");
        }
        if goal.trim().is_empty() || request.trim().is_empty() {
            return error("goal and request must say what the Plugin is for.");
        }
        let request = Request {
            name,
            goal,
            clarifications,
            request,
        };
        let mut builds = self.builds.lock().unwrap();
        if let Some(build) = &builds.current {
            let building = build.name.clone();
            return self.queue(&mut builds, &building, request);
        }
        match self.start(request) {
            Ok(build) => builds.current = Some(build),
            Err(reason) => return error(reason),
        }
        self.show_building(&builds);
        drop(builds);
        let _ = self.commands.send(Command::Say(A_FEW_MINUTES.to_owned()));
        text(
            "The build has started, and Lucy has told the user it will take a few minutes. \
             Don't say anything more about it; carry on with anything else they ask. Lucy will \
             tell them when it's ready.",
        )
    }

    /// Queues `request` to start once the `building` Plugin is done, unless
    /// one is queued already.
    fn queue(&self, builds: &mut Builds, building: &str, request: Request) -> CallToolResult {
        if let Some(queued) = &builds.queued {
            return error(format!(
                "I'm already building the {building} Plugin, and the {} Plugin is next. Tell the \
                 user, and offer to build this one once those are done.",
                queued.name
            ));
        }
        if request.name == building {
            return error(format!("I'm already building the {building} Plugin."));
        }
        if let Err(reason) = self.host.may_author(&request.name) {
            return error(format!("I can't build it: {reason}."));
        }
        builds.queued = Some(request);
        let _ = self.commands.send(Command::Say(format!(
            "I'm already building the {building} Plugin. I'll do this one after."
        )));
        text(
            "It's queued, and Lucy has told the user she'll build it after the one she's \
             building. Don't say anything more about it.",
        )
    }

    /// Starts the Delegate on `request`, or says why it can't start.
    fn start(self: &Arc<Self>, request: Request) -> Result<Arc<Build>, String> {
        let Request {
            name,
            goal,
            clarifications,
            request,
        } = request;
        self.host
            .may_author(&name)
            .map_err(|reason| format!("I can't build it: {reason}."))?;
        let config = lucy_config::load(&self.config_path).map_err(|e| e.to_string());
        let provider = config
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|config| config.provider(Tier::Advanced).cloned())
            .map_err(|reason| format!("I can't start a build: {reason}"))?;
        let settings = config.map(|config| config.authoring).unwrap_or_default();
        let (workspace, resumed) = self
            .workspace(&name)
            .map_err(|e| format!("I can't make the build folder: {e}"))?;

        let allowance = Duration::from_secs_f64(settings.budget_secs);
        let build = Arc::new(Build {
            name: name.clone(),
            request,
            workspace: workspace.clone(),
            authoring: Arc::downgrade(self),
            progress_notifications: settings.progress_notifications,
            allowance,
            state: Mutex::new(BuildState {
                stage: Stage::Working,
                turns: 0,
                failure: None,
                model: None,
                token: None,
                budget: Budget::new(allowance, self.clock.clone()),
                waiting: Vec::new(),
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
        let earlier = if resumed {
            "\n\nThe folder already holds an earlier attempt at it. Carry on from there."
        } else {
            ""
        };
        build.send_turn(format!(
            "Build the {name} Plugin.\n\nGoal: {goal}\n\nWhat the user has told Lucy: \
             {clarifications}\n\nThe request it's for: \"{}\"{earlier}",
            build.request
        ));
        tokio::spawn(build.clone().follow(events));
        Ok(build)
    }

    /// "cancel it": stops the build that is running, and the one queued.
    pub fn cancel(&self) {
        let current = {
            let mut builds = self.builds.lock().unwrap();
            builds.queued = None;
            builds.current.clone()
        };
        if let Some(build) = current {
            build.finish();
        }
    }

    /// The staging dir for the Plugin `name`, made from the template if it
    /// isn't there from an earlier build, and what else it may touch; and
    /// whether it was there.
    fn workspace(&self, name: &str) -> std::io::Result<(Workspace, bool)> {
        let build_dir = self.plugins_dir.join(".build");
        let dir = build_dir.join(name);
        let tmp = build_dir.join(".tmp").join(name);
        let resumed = dir.exists();
        if !resumed {
            template::write(&dir, name)?;
        }
        std::fs::create_dir_all(&tmp)?;
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let toolchain_home = |var: &str, default: &str| {
            std::env::var_os(var).map_or_else(|| home.join(default), PathBuf::from)
        };
        let relay = self.relays.program();
        let workspace = Workspace {
            dir,
            tmp,
            writable: vec![toolchain_home("CARGO_HOME", ".cargo")],
            readable: [toolchain_home("RUSTUP_HOME", ".rustup")]
                .into_iter()
                .chain(sandbox::program_dir(&relay))
                .collect(),
        };
        Ok((workspace, resumed))
    }

    /// The build is over: the queued one, if any, starts.
    fn finished(self: &Arc<Self>, build: &Build) {
        let mut builds = self.builds.lock().unwrap();
        if !builds
            .current
            .as_deref()
            .is_some_and(|c| std::ptr::eq(c, build))
        {
            return;
        }
        builds.current = None;
        if let Some(next) = builds.queued.take() {
            let name = next.name.clone();
            match self.start(next) {
                Ok(next) => builds.current = Some(next),
                Err(reason) => {
                    let kept = build_notification(
                        format!("Lucy couldn't start the {name} Plugin"),
                        format!("{reason}\nShe'll tell you more at your next Session."),
                    );
                    let _ = self.commands.send(Command::TurnOrNextSession(
                        format!(
                            "{PLUGIN_BUILD} The {name} Plugin, which was queued, couldn't be \
                             started: {reason}\n\nTell the user plainly, in a sentence."
                        ),
                        kept,
                    ));
                }
            }
        }
        self.show_building(&builds);
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
    /// Show `report_progress` lines as notifications.
    progress_notifications: bool,
    /// How long it may work.
    allowance: Duration,
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
    /// What is left of its time, which counts only while it works.
    budget: Budget,
    /// Tasks waiting on the user for it, stopped when the build is over.
    waiting: Vec<AbortHandle>,
}

impl BuildState {
    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.budget
            .set_running(matches!(stage, Stage::Working | Stage::Checking));
    }
}

/// A countdown that can be paused and resumed.
struct Budget {
    remaining: Duration,
    /// When it last resumed, while running.
    resumed: Option<Instant>,
    clock: Arc<dyn Clock>,
    /// Set to when it runs out, while running.
    timer: Box<dyn Timer>,
}

impl Budget {
    /// A running countdown from `allowance`.
    fn new(allowance: Duration, clock: Arc<dyn Clock>) -> Self {
        let mut budget = Budget {
            remaining: allowance,
            resumed: Some(clock.now()),
            timer: clock.timer(),
            clock,
        };
        budget.timer.set(budget.deadline());
        budget
    }

    fn set_running(&mut self, running: bool) {
        let now = self.clock.now();
        match (running, self.resumed) {
            (true, None) => self.resumed = Some(now),
            (false, Some(resumed)) => {
                let worked = now.saturating_duration_since(resumed);
                self.remaining = self.remaining.saturating_sub(worked);
                self.resumed = None;
            }
            _ => return,
        }
        self.timer.set(self.deadline());
    }

    /// When it runs out, while running.
    fn deadline(&self) -> Option<Instant> {
        self.resumed.map(|resumed| resumed + self.remaining)
    }

    /// Whether it has run out.
    fn is_spent(&self) -> bool {
        self.deadline()
            .is_some_and(|deadline| deadline <= self.clock.now())
    }

    /// Resolves once it may have run out.
    fn fired(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        self.timer.fired()
    }
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

impl Stage {
    fn building(self) -> Building {
        match self {
            Stage::Working | Stage::Checking => Building::Working,
            Stage::Asking | Stage::Approving => Building::Waiting,
            Stage::Over => Building::No,
        }
    }
}

impl Build {
    /// Follows the Delegate's model process until the build is over.
    async fn follow(self: Arc<Self>, mut events: mpsc::UnboundedReceiver<ThinkingEvent>) {
        loop {
            // Dropped only once its wake-up is handled, which a test's clock
            // waits for.
            let mut budget_fired = self.state.lock().unwrap().budget.fired();
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
                () = &mut budget_fired => if self.out_of_time() {
                    return;
                },
                () = self.over.notified() => return,
            }
        }
    }

    /// If the budget ran out, the build stops, its staging kept, and Lucy
    /// says so. Returns whether it did.
    fn out_of_time(&self) -> bool {
        // It may have stopped counting just now.
        if !self.state.lock().unwrap().budget.is_spent() {
            return false;
        }
        let allowance = spoken_duration(self.allowance);
        self.tell(
            format!(
                "{PLUGIN_BUILD} The {} Plugin ran out of time: it wasn't done after \
                 {allowance}. Its work so far is kept, and asking for it again carries on from \
                 there. Tell the user plainly, in a sentence, and that they can say \"try \
                 again\".",
                self.name
            ),
            build_notification(
                format!("The {} Plugin ran out of time", self.name),
                format!(
                    "It wasn't done after {allowance}. Its work so far is kept: ask Lucy to try \
                     again."
                ),
            ),
        );
        self.finish();
        true
    }

    /// A call to one of the Delegate tools.
    pub async fn call(
        self: &Arc<Self>,
        name: &str,
        arguments: &JsonObject,
    ) -> Option<CallToolResult> {
        Some(match name {
            "report_progress" => {
                let progress = string(arguments, "text");
                eprintln!("lucyd: building {}: {progress}", self.name);
                if let Some(authoring) = self.authoring.upgrade() {
                    if self.progress_notifications {
                        authoring.show_progress(&self.name, progress);
                    }
                }
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
        state.enter(to);
        drop(state);
        if let Some(authoring) = self.authoring.upgrade() {
            authoring.show_building(&authoring.builds.lock().unwrap());
        }
        Ok(())
    }

    /// Stops `task`, which waits on the user, when the build is over.
    fn waits(&self, task: AbortHandle) {
        let mut state = self.state.lock().unwrap();
        if state.stage == Stage::Over {
            task.abort();
        } else {
            state.waiting.push(task);
        }
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
        let kept = build_notification(
            format!("Lucy has a question about the {} Plugin", self.name),
            format!("{question}\nShe'll ask at your next Session."),
        );
        let question = Question {
            prompt: question,
            expects: Expects::Words,
        };
        let answer =
            runner::ask_when_quiet_or_next_session(&authoring.commands, question, None, kept);
        let build = self.clone();
        let task = tokio::spawn(async move {
            let said = match answer.await {
                Some(Answer::Said(said)) => format!("The user answered: \"{said}\""),
                Some(_) => "The user didn't answer. Carry on with your best judgement, or call \
                            cannot."
                    .to_owned(),
                None => "The user couldn't be asked: the Session ended. Carry on with your \
                         best judgement."
                    .to_owned(),
            };
            if build.advance(Stage::Asking, Stage::Working).is_ok() {
                build.send_turn(said);
            }
        });
        self.waits(task.abort_handle());
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
        let checked = self.check(&authoring).await;
        // It ran out of time, or was cancelled, while being checked.
        if self.state.lock().unwrap().stage == Stage::Over {
            return error("The build is over.");
        }
        match checked {
            Ok(()) => {
                let _ = self.advance(Stage::Checking, Stage::Approving);
                let task = tokio::spawn(self.clone().approve(authoring));
                self.waits(task.abort_handle());
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
        let kept = build_notification(
            format!("The {} Plugin is ready", self.name),
            "Lucy will ask for your Approval at the start of your next Session.".to_owned(),
        );
        match authoring
            .host
            .request_approval_when_quiet(&self.name, kept)
            .await
        {
            Ok(true) => {}
            Ok(false) => return self.finish(),
            Err(reason) => {
                eprintln!("lucyd: the {} Plugin wasn't approved: {reason}", self.name);
                return self.finish();
            }
        }
        let kept = build_notification(
            format!("The {} Plugin is approved", self.name),
            format!(
                "Lucy will answer \"{}\" with it at your next Session.",
                self.request
            ),
        );
        let _ = authoring.commands.send(Command::TurnOrNextSession(
            format!(
                "{PLUGIN_BUILD} The user approved the new {} Plugin, and its tools are \
                 yours now. Use it to answer the request it was built for, as if they'd just \
                 asked: \"{}\". Don't ask whether that's what they wanted; Lucy asks that next.",
                self.name, self.request
            ),
            kept,
        ));
        let question = Question {
            prompt: IS_THAT_THE_ONE.to_owned(),
            expects: Expects::YesOrWords,
        };
        // Kept with the answer's Turn, which has shown that it waits.
        match runner::ask_at(&authoring.commands, question, None, When::Kept).await {
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
        self.tell(
            format!(
                "{PLUGIN_BUILD} The {} Plugin can't be built with permitted access: \
                 {reason}. The best permitted fallback: {fallback}. Tell the user that limit \
                 plainly, in a sentence, and offer the fallback.",
                self.name
            ),
            build_notification(
                format!("Lucy can't build the {} Plugin", self.name),
                format!("{reason}.\nShe'll tell you more at your next Session."),
            ),
        );
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
        self.tell(
            format!(
                "{PLUGIN_BUILD} The {} Plugin couldn't be built: {reason}\n\nTell the \
                 user plainly, in a sentence or two, what failed.",
                self.name
            ),
            build_notification(
                format!("Lucy couldn't build the {} Plugin", self.name),
                format!("{reason}.\nShe'll tell you more at your next Session."),
            ),
        );
        self.finish();
    }

    /// Starts a Turn of the Session model once neither side is speaking,
    /// at the next Session if none is open, showing `kept` meanwhile.
    fn tell(&self, text: String, kept: Notification) {
        if let Some(authoring) = self.authoring.upgrade() {
            let _ = authoring
                .commands
                .send(Command::TurnOrNextSession(text, kept));
        }
    }

    /// Ends the build: the Delegate stops and its token is revoked. The
    /// staging dir is kept.
    fn finish(&self) {
        let (model, token, waiting) = {
            let mut state = self.state.lock().unwrap();
            state.enter(Stage::Over);
            let waiting = std::mem::take(&mut state.waiting);
            (state.model.take(), state.token.take(), waiting)
        };
        drop((model, token));
        for task in waiting {
            task.abort();
        }
        if let Some(authoring) = self.authoring.upgrade() {
            authoring.finished(self);
        }
        self.over.notify_one();
    }
}

/// What a build shows rather than says.
fn build_notification(summary: String, body: String) -> Notification {
    Notification {
        summary,
        body,
        action: None,
        kind: NotificationKind::Build,
    }
}

/// `duration` as Lucy would say it: in minutes, or seconds if shorter.
fn spoken_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs >= 60.0 {
        format!("{} minutes", (secs / 60.0).round())
    } else {
        format!("{} seconds", secs.round())
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
