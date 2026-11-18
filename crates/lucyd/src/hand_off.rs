use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lucy_config::{Kind, Provider};
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde_json::json;
use tokio::sync::{mpsc, watch, Notify};

use crate::adapters::{
    Adapters, Clock, Exit, HandOff, Notification, NotificationKind, Notifier, SessionModel,
    Thinking, ThinkingEvent, PERMISSION_TOOL,
};
use crate::authoring::{spoken_duration, Authoring};
use crate::input;
use crate::mcp::{error, string, text, tool, Caller, Relays, Token};
use crate::model_text::HAND_OFF;
use crate::plugins::Environment;
use crate::runner::{self, Command, When};
use crate::session::{
    which_to_cancel, Answer, Cancel, Expects, Question, BOTH_RUNNING, BOTH_STOPPED, BUILD_STOPPED,
    HAND_OFF_STOPPED, WHICH_TO_CANCEL,
};
use crate::sync::Locked;

const BRIEF: &str = "The user asked for this by voice, through Lucy, their voice assistant, and \
                     isn't at the keyboard. Work on your own. Lucy reads a short summary of your \
                     final message aloud and shows them the whole of it, so end with a final \
                     message that says plainly what you did or found.";

const CAPTURE_WAIT: Duration = Duration::from_secs(5);

const UNASKED: &str = "The user's Claude settings don't allow this, and Lucy couldn't ask them \
                       about it, so it's denied. Carry on without it, and say in your final \
                       message what you'd need.";

const REFUSED: &str = "The user didn't allow this when Lucy asked them. Carry on without it, and \
                       say in your final message what you'd need.";

pub struct HandOffs {
    thinking: Arc<dyn Thinking>,
    relays: Relays,
    env: Environment,
    commands: mpsc::UnboundedSender<Command>,
    notifier: Arc<dyn Notifier>,
    clock: Arc<dyn Clock>,
    working: watch::Sender<Working>,
    config_path: PathBuf,
    jobs: Mutex<Jobs>,
}

#[derive(Default)]
struct Jobs {
    current: Option<Arc<Job>>,
    queued: Option<Request>,
}

struct Request {
    request: String,
    folder: PathBuf,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Working {
    #[default]
    No,
    Yes,
}

impl HandOffs {
    pub fn new(
        adapters: &Adapters,
        relays: Relays,
        env: Environment,
        config_path: PathBuf,
        commands: mpsc::UnboundedSender<Command>,
    ) -> Arc<HandOffs> {
        Arc::new(HandOffs {
            thinking: adapters.thinking.clone(),
            relays,
            env,
            commands,
            notifier: adapters.notifier.clone(),
            clock: adapters.clock.clone(),
            working: watch::Sender::new(Working::No),
            config_path,
            jobs: Mutex::default(),
        })
    }

    pub fn working(&self) -> watch::Receiver<Working> {
        self.working.subscribe()
    }

    pub fn request(self: &Arc<Self>, arguments: &JsonObject) -> CallToolResult {
        let request = string(arguments, "request");
        if request.trim().is_empty() {
            return error("request must be what the user wants Claude to do, in their words.");
        }
        let folder = match self.folder(&string(arguments, "folder")) {
            Ok(folder) => folder,
            Err(refused) => return error(refused),
        };
        let request = Request { request, folder };
        let mut jobs = self.jobs.locked();
        if let Some(running) = &jobs.current {
            let running = running.request.clone();
            return Self::queue(&mut jobs, &running, request, arguments);
        }
        jobs.current = Some(self.start(request));
        drop(jobs);
        self.working.send_replace(Working::Yes);
        text(
            "Claude has started on it. Tell the user so in a few words, and carry on with \
             anything else they ask. Lucy will tell you when Claude has finished.",
        )
    }

    fn queue(
        jobs: &mut Jobs,
        running: &str,
        request: Request,
        arguments: &JsonObject,
    ) -> CallToolResult {
        if let Some(queued) = &jobs.queued {
            return error(format!(
                "Claude is still working on \"{running}\", and \"{}\" is queued after it. Tell \
                 the user Lucy can't take another until those are done.",
                queued.request
            ));
        }
        if arguments.get("queue").and_then(serde_json::Value::as_bool) != Some(true) {
            return error(format!(
                "Nothing has started: Claude is still working on \"{running}\". Ask the user \
                 whether to queue this, naming that one in a few words, such as \"Claude's \
                 still on the lucy test; want me to queue this?\" If they say yes, call \
                 hand_to_claude again with the same request and folder, and queue set to true."
            ));
        }
        jobs.queued = Some(request);
        text(
            "It's queued: Claude starts on it once it has finished the one it's on. Tell the \
             user so in a few words.",
        )
    }

    fn start(self: &Arc<Self>, request: Request) -> Arc<Job> {
        let budget = lucy_config::load(&self.config_path)
            .map(|config| config.handoff)
            .unwrap_or_default();
        let job = Arc::new(Job {
            request: request.request,
            folder: request.folder,
            budget: Duration::from_secs_f64(budget.budget_mins * 60.0),
            hand_offs: Arc::downgrade(self),
            state: Mutex::default(),
            cancelled: Notify::new(),
        });
        tokio::spawn(job.clone().run());
        job
    }

    fn folder(&self, said: &str) -> Result<PathBuf, String> {
        let said = said.trim();
        let home = || self.env.var("HOME").map(PathBuf::from).unwrap_or_default();
        let folder = if said.is_empty() || said == "~" {
            home()
        } else if let Some(below_home) = said.strip_prefix("~/") {
            home().join(below_home)
        } else if Path::new(said).is_absolute() {
            PathBuf::from(said)
        } else {
            return Err(format!(
                "\"{said}\" isn't a full path, and Lucy can't find folders by name yet. Ask \
                 the user to say the full path, such as ~/projects/{said}."
            ));
        };
        if !folder.is_dir() {
            return Err(format!(
                "There's no folder at {}. Tell the user, and ask them to say the full path.",
                folder.display()
            ));
        }
        Ok(folder)
    }

    pub fn show_still_working(&self) {
        if self.jobs.locked().current.is_some() {
            self.show(Notification {
                summary: "Claude is still working".to_owned(),
                body: "Lucy will let you know when it's done.".to_owned(),
                action: None,
                kind: NotificationKind::HandOff,
            });
        }
    }

    pub fn cancel(&self) {
        let current = {
            let mut jobs = self.jobs.locked();
            jobs.queued = None;
            jobs.current.clone()
        };
        if let Some(job) = current {
            job.cancelled.notify_one();
        }
    }

    pub fn ask_which_to_cancel(self: &Arc<Self>, authoring: Arc<Authoring>) {
        let hand_offs = self.clone();
        tokio::spawn(async move {
            let question = Question {
                prompt: WHICH_TO_CANCEL.to_owned(),
                expects: Expects::Words,
            };
            let answer = runner::ask(&hand_offs.commands, question, None).await;
            let choice = match answer {
                Some(Answer::Said(said)) => which_to_cancel(&said),
                _ => Cancel::Neither,
            };
            let said = match choice {
                Cancel::Build => {
                    authoring.cancel();
                    BUILD_STOPPED
                }
                Cancel::HandOff => {
                    hand_offs.cancel();
                    HAND_OFF_STOPPED
                }
                Cancel::Both => {
                    authoring.cancel();
                    hand_offs.cancel();
                    BOTH_STOPPED
                }
                Cancel::Neither => BOTH_RUNNING,
            };
            let _ = hand_offs.commands.send(Command::Say(said.to_owned()));
        });
    }

    fn show(&self, notification: Notification) {
        self.notifier
            .notify(notification, mpsc::unbounded_channel().0);
    }

    fn finished(self: &Arc<Self>, job: &Job) {
        let mut jobs = self.jobs.locked();
        if !jobs
            .current
            .as_deref()
            .is_some_and(|c| std::ptr::eq(c, job))
        {
            return;
        }
        jobs.current = jobs.queued.take().map(|next| self.start(next));
        if jobs.current.is_none() {
            self.working.send_replace(Working::No);
        }
    }

    async fn environment(&self) -> Vec<(OsString, OsString)> {
        match login_env(&self.env, &*self.clock).await {
            Ok(env) => env,
            Err(why) => {
                eprintln!("lucyd: the Hand-off uses lucyd's own environment: {why}");
                self.env.vars()
            }
        }
    }
}

pub fn hand_to_claude_tool() -> Tool {
    tool(
        "hand_to_claude",
        "Pass a request to the user's own Claude Code when they say \"tell Claude to …\" or \
         \"ask Claude to …\". It works on its own, with their settings and integrations, in the \
         folder they named. Lucy tells you when it has finished.",
        json!({
            "type": "object",
            "properties": {
                "request": {"type": "string", "description": "What the user wants Claude to do, in their words"},
                "folder": {"type": "string", "description": "The folder as the user said it: an absolute path or one starting with ~. Leave it out when they named none."},
                "queue": {"type": "boolean", "description": "True once the user has said yes to queueing this behind the Hand-off running"},
            },
            "required": ["request"],
        }),
    )
}

fn permission_tool() -> Tool {
    tool(
        PERMISSION_TOOL,
        "Asks the user whether Claude may use a tool their Claude settings don't allow.",
        json!({
            "type": "object",
            "properties": {
                "tool_name": {"type": "string"},
                "input": {"type": "object"},
                "tool_use_id": {"type": "string"},
            },
            "required": ["tool_name", "input"],
        }),
    )
}

pub fn hand_off_tools() -> Vec<Tool> {
    let string = |description: &str| json!({"type": "string", "description": description});
    vec![
        tool(
            "report_progress",
            "Report what you're doing, in a short line. It is shown to the user, never spoken.",
            json!({"type": "object", "properties": {"text": string("What you're doing")}, "required": ["text"]}),
        ),
        tool(
            "ask_user",
            "Ask the user something only they can answer. Lucy asks it aloud once nobody is \
             speaking and returns their answer.",
            json!({"type": "object", "properties": {"question": string("One short spoken question")}, "required": ["question"]}),
        ),
        input::ask_typed_tool(),
        permission_tool(),
    ]
}

pub struct Job {
    request: String,
    folder: PathBuf,
    budget: Duration,
    hand_offs: Weak<HandOffs>,
    state: Mutex<JobState>,
    cancelled: Notify,
}

#[derive(Default)]
struct JobState {
    model: Option<Box<dyn SessionModel>>,
    token: Option<Token>,
    conversation: Option<String>,
    answer: Option<String>,
}

impl Job {
    async fn run(self: Arc<Self>) {
        let Some(hand_offs) = self.hand_offs.upgrade() else {
            return;
        };
        let mut budget = hand_offs.clock.timer();
        budget.set(Some(hand_offs.clock.now() + self.budget));
        let env = hand_offs.environment().await;
        let (relay, token) = hand_offs
            .relays
            .issue(Caller::HandOff(Arc::downgrade(&self)));
        let (events_tx, events) = mpsc::unbounded_channel();
        let hand_off = HandOff {
            folder: self.folder.clone(),
            env,
        };
        {
            let mut state = self.state.locked();
            let mut model = hand_offs.thinking.open_hand_off(
                &claude(),
                events_tx,
                relay,
                BRIEF.to_owned(),
                hand_off,
            );
            model.send_turn(self.request.clone());
            state.model = Some(model);
            state.token = Some(token);
        }
        drop(hand_offs);
        self.follow(events, budget.fired()).await;
    }

    async fn follow(
        &self,
        mut events: mpsc::UnboundedReceiver<ThinkingEvent>,
        mut budget_spent: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) {
        loop {
            tokio::select! {
                biased;
                event = events.recv() => match event {
                    Some(ThinkingEvent::Conversation(id)) => {
                        self.state.locked().conversation = Some(id);
                    }
                    Some(ThinkingEvent::Final(answer)) => self.state.locked().answer = Some(answer),
                    Some(ThinkingEvent::TurnEnd) => return self.answered(),
                    Some(ThinkingEvent::Unavailable(reason)) => return self.failed(&reason),
                    Some(ThinkingEvent::Failed(message)) => return self.failed(&message),
                    Some(ThinkingEvent::Died(exit)) => return self.died(&exit),
                    None => return self.failed("Claude stopped unexpectedly."),
                    Some(_) => {}
                },
                () = &mut budget_spent => return self.out_of_time(),
                () = self.cancelled.notified() => return self.stopped(),
            }
        }
    }

    pub async fn call(&self, name: &str, arguments: &JsonObject) -> Option<CallToolResult> {
        Some(match name {
            "report_progress" => {
                self.show("Claude is working", string(arguments, "text"));
                text("Noted.")
            }
            "ask_user" => self.ask_user(string(arguments, "question")).await,
            "ask_typed" => {
                let Some(hand_offs) = self.hand_offs.upgrade() else {
                    return Some(error("Lucy is stopping."));
                };
                let commands = hand_offs.commands.clone();
                drop(hand_offs);
                let kept = question_kept("Claude wants you to type something");
                input::ask_typed_at(&commands, arguments, kept).await
            }
            PERMISSION_TOOL => text(self.permission(arguments).await.to_string()),
            _ => return None,
        })
    }

    async fn ask_user(&self, question: String) -> CallToolResult {
        if question.trim().is_empty() {
            return error("question must be what to ask the user.");
        }
        let Some(hand_offs) = self.hand_offs.upgrade() else {
            return error("Lucy is stopping.");
        };
        let question = Question {
            prompt: format!("Claude asks: {question}"),
            expects: Expects::Words,
        };
        let kept = question_kept("Claude has a question");
        let answer = runner::ask_at(&hand_offs.commands, question, None, kept);
        drop(hand_offs);
        match answer.await {
            Some(Answer::Said(said)) => text(format!("The user answered: \"{said}\"")),
            Some(_) => text(
                "The user didn't answer. Carry on with your best judgement, and say in your \
                 final message what you assumed.",
            ),
            None => text(
                "The user couldn't be asked. Carry on with your best judgement, and say in \
                 your final message what you assumed.",
            ),
        }
    }

    async fn permission(&self, arguments: &JsonObject) -> serde_json::Value {
        let tool_name = string(arguments, "tool_name");
        let empty = serde_json::Value::Object(JsonObject::new());
        let input = arguments.get("input").unwrap_or(&empty);
        let question = Question {
            prompt: format!("Claude wants to {}. Okay?", description(&tool_name, input)),
            expects: Expects::Yes,
        };
        let details = format!("{tool_name}\n{input:#}");
        let kept = When::QuietOrNextSession(Notification {
            summary: "Claude is waiting for your permission".into(),
            body: format!("{details}\n\nLucy will ask at your next Session."),
            action: None,
            kind: NotificationKind::HandOff,
        });
        let shown = Notification {
            summary: "Claude needs your yes".into(),
            body: details,
            action: None,
            kind: NotificationKind::Details,
        };
        let Some(hand_offs) = self.hand_offs.upgrade() else {
            return json!({"behavior": "deny", "message": UNASKED});
        };
        let commands = hand_offs.commands.clone();
        drop(hand_offs);
        match runner::ask_at(&commands, question, Some(shown), kept).await {
            Some(Answer::Yes) => json!({"behavior": "allow", "updatedInput": input}),
            Some(_) => json!({"behavior": "deny", "message": REFUSED}),
            None => json!({"behavior": "deny", "message": UNASKED}),
        }
    }

    fn answered(&self) {
        let (answer, conversation) = {
            let state = self.state.locked();
            (state.answer.clone(), state.conversation.clone())
        };
        let resume = self.resume(conversation.as_deref());
        let summary = "Claude has finished";
        match answer.filter(|answer| !answer.trim().is_empty()) {
            Some(answer) => {
                self.show(summary, format!("{answer}{resume}"));
                self.tell(
                    format!(
                        "{HAND_OFF} Claude has finished what the user handed it: \"{}\". Its \
                         final message:\n\n{answer}\n\nTell the user what it says in one to \
                         three short spoken sentences. They have the whole message in a \
                         notification.",
                        self.request
                    ),
                    summary,
                    "Lucy will tell you about it at your next Session.",
                );
            }
            None => {
                self.show(
                    summary,
                    format!("Claude left no final message, so Lucy can't say what it did.{resume}"),
                );
                self.tell(
                    format!(
                        "{HAND_OFF} Claude has finished what the user handed it: \"{}\", but \
                         left no final message, so there's nothing to report. Tell the user \
                         plainly, in a sentence.",
                        self.request
                    ),
                    summary,
                    "Lucy will tell you more at your next Session.",
                );
            }
        }
        self.finish();
    }

    fn failed(&self, reason: &str) {
        let conversation = self.state.locked().conversation.clone();
        self.show(
            "Claude couldn't finish",
            format!("{reason}{}", self.resume(conversation.as_deref())),
        );
        self.tell(
            format!(
                "{HAND_OFF} Claude couldn't finish what the user handed it: {reason}\n\nTell \
                 the user plainly, in a sentence."
            ),
            "Claude couldn't finish",
            "Lucy will tell you more at your next Session.",
        );
        self.finish();
    }

    fn died(&self, exit: &Exit) {
        eprintln!(
            "lucyd: the Hand-off's claude stopped ({}); the end of its stderr:\n{}",
            exit.status_text(),
            exit.stderr.join("\n")
        );
        self.failed(&exit.stopped("Claude"));
    }

    fn stopped(&self) {
        let conversation = self.state.locked().conversation.clone();
        self.show(
            "Lucy stopped Claude",
            format!(
                "You asked Lucy to cancel it.{}",
                self.resume(conversation.as_deref())
            ),
        );
        self.finish();
    }

    fn out_of_time(&self) {
        let conversation = self.state.locked().conversation.clone();
        let budget = spoken_duration(self.budget);
        self.show(
            "Claude ran out of time",
            format!(
                "Claude wasn't done after {budget}, so Lucy stopped it.{}",
                self.resume(conversation.as_deref())
            ),
        );
        self.tell(
            format!(
                "{HAND_OFF} Claude ran out of time on what the user handed it: \"{}\". It \
                 wasn't done after {budget}, so Lucy stopped it. Tell the user plainly, in a \
                 sentence, and that the notification says how to carry on at their keyboard.",
                self.request
            ),
            "Claude ran out of time",
            "Lucy will tell you more at your next Session.",
        );
        self.finish();
    }

    fn resume(&self, conversation: Option<&str>) -> String {
        match conversation {
            Some(id) => format!(
                "\n\nTo carry on at your keyboard, in {}: claude --resume {id}",
                self.folder.display()
            ),
            None => String::new(),
        }
    }

    fn show(&self, summary: &str, body: String) {
        if let Some(hand_offs) = self.hand_offs.upgrade() {
            hand_offs.show(Notification {
                summary: summary.to_owned(),
                body,
                action: None,
                kind: NotificationKind::HandOff,
            });
        }
    }

    fn tell(&self, text: String, summary: &str, kept: &str) {
        if let Some(hand_offs) = self.hand_offs.upgrade() {
            let kept = Notification {
                summary: summary.to_owned(),
                body: kept.to_owned(),
                action: None,
                kind: NotificationKind::HandOff,
            };
            let _ = hand_offs
                .commands
                .send(Command::TurnOrNextSession(text, kept));
        }
    }

    fn finish(&self) {
        let (model, token) = {
            let mut state = self.state.locked();
            (state.model.take(), state.token.take())
        };
        drop((model, token));
        if let Some(hand_offs) = self.hand_offs.upgrade() {
            hand_offs.finished(self);
        }
    }
}

fn question_kept(summary: &str) -> When {
    When::QuietOrNextSession(Notification {
        summary: summary.to_owned(),
        body: "Lucy will ask at your next Session.".to_owned(),
        action: None,
        kind: NotificationKind::HandOff,
    })
}

fn description(tool_name: &str, input: &serde_json::Value) -> String {
    let said = input
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(|said| said.trim().trim_end_matches('.').trim())
        .filter(|said| !said.is_empty());
    let Some(said) = said else {
        let tool = tool_name.strip_prefix("mcp__").unwrap_or(tool_name);
        return format!(
            "use {}",
            tool.replace('_', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
    };
    let mut chars = said.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && !second.is_uppercase() => {
            format!("{}{}", first.to_lowercase(), &said[first.len_utf8()..])
        }
        _ => said.to_owned(),
    }
}

fn claude() -> Provider {
    Provider {
        kind: Kind::Claude,
        model: None,
        allow_api_key: false,
        program: None,
    }
}

async fn login_env(
    own: &Environment,
    clock: &dyn Clock,
) -> Result<Vec<(OsString, OsString)>, String> {
    let shell = own.var("SHELL").ok_or("SHELL isn't set")?;
    let mut command = tokio::process::Command::new(shell);
    command
        .args(["-lic", "env -0"])
        .env_clear()
        .envs(own.vars())
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut timer = clock.timer();
    timer.set(Some(clock.now() + CAPTURE_WAIT));
    let output = tokio::select! {
        output = command.output() => output.map_err(|e| format!("the login shell couldn't run: {e}"))?,
        () = timer.fired() => return Err(format!("the login shell took over {} s", CAPTURE_WAIT.as_secs())),
    };
    if !output.status.success() {
        return Err(format!("the login shell failed ({})", output.status));
    }
    let env = parse_env(&output.stdout);
    if env.is_empty() {
        return Err("the login shell printed no environment".to_owned());
    }
    Ok(env)
}

fn parse_env(printed: &[u8]) -> Vec<(OsString, OsString)> {
    use std::os::unix::ffi::OsStrExt;
    printed
        .split(|byte| *byte == 0)
        .filter_map(|var| {
            let equals = var.iter().position(|byte| *byte == b'=')?;
            let (name, value) = (&var[..equals], &var[equals + 1..]);
            (!name.is_empty()).then(|| {
                (
                    std::ffi::OsStr::from_bytes(name).to_owned(),
                    std::ffi::OsStr::from_bytes(value).to_owned(),
                )
            })
        })
        .collect()
}
