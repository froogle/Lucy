use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lucy_config::{Kind, Provider};
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, watch, Notify};

use crate::adapters::{
    Adapters, Clock, Exit, HandOff, Notification, NotificationKind, Notifier, SessionModel,
    Thinking, ThinkingEvent, PERMISSION_TOOL,
};
use crate::authoring::{spoken_duration, Authoring};
use crate::files::{self, Search, Words};
use crate::input;
use crate::mcp::{error, string, text, tool, Caller, Relays, Token};
use crate::model_text::HAND_OFF;
use crate::plugins::{self, Environment};
use crate::runner::{self, Beside, Command, When};
use crate::session::{
    self, Answer, Expects, Question, BUILD_STOPPED, HAND_OFF_STOPPED, QUEUED_DROPPED,
};
use crate::sync::Locked;

const BRIEF: &str = "The user asked for this by voice, through Lucy, their voice assistant, and \
                     isn't at the keyboard. Work on your own. Lucy reads a short summary of your \
                     final message aloud and shows them the whole of it, so end with a final \
                     message that says plainly what you did or found.";

const CAPTURE_WAIT: Duration = Duration::from_secs(5);

const FOLDER_DEPTH: usize = 3;

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
    state_dir: PathBuf,
    jobs: Mutex<Jobs>,
}

#[derive(Default)]
struct Jobs {
    current: Option<Arc<Job>>,
    queued: Option<Request>,
}

struct Request {
    task: String,
    start: Start,
    budget: Duration,
}

enum Start {
    Fresh(PathBuf),
    Continue,
}

#[derive(Serialize, Deserialize)]
struct Last {
    conversation: String,
    folder: PathBuf,
}

const NOTHING_TO_CONTINUE: &str = "There's nothing to continue: Claude's last Hand-off left \
                                   no conversation to carry on, so nothing has started. Tell \
                                   the user there's nothing to continue.";

const UNREADABLE_LAST: &str = "Nothing has started: Lucy couldn't read her record of Claude's \
                               last Hand-off, so it can't be continued. Tell the user Lucy \
                               couldn't read it, and that a new request starts afresh.";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Working {
    #[default]
    No,
    Yes,
    YesAndQueued,
}

const BOTH_STOPPED: &str = "Okay, I've stopped them both.";

const BOTH_RUNNING: &str = "Okay, I'll leave them both running.";

const WHICH_TO_CANCEL: &str = "Do you mean the Plugin build, or Claude?";

const ALREADY_FINISHED: &str = "Claude had already finished.";

#[derive(Debug, PartialEq, Eq)]
enum Cancel {
    Build,
    HandOff,
    Both,
    Unclear,
}

fn which_to_cancel(said: &str) -> Cancel {
    let said = session::words(said);
    let named = |names: &[&str]| said.iter().any(|word| names.contains(&word.as_str()));
    let both = named(&["both", "all"]);
    let build = both || named(&["build", "plugin"]);
    let hand_off = both || named(&["claude", "hand", "handoff"]);
    match (build, hand_off) {
        (true, true) => Cancel::Both,
        (true, false) => Cancel::Build,
        (false, true) => Cancel::HandOff,
        (false, false) => Cancel::Unclear,
    }
}

enum Stopped {
    Ended,
    Alone,
    AndQueued,
}

impl Stopped {
    fn said(&self, stopped: &str) -> String {
        match self {
            Stopped::AndQueued => format!("{stopped} {QUEUED_DROPPED}"),
            _ => stopped.to_owned(),
        }
    }
}

impl HandOffs {
    pub fn new(
        adapters: &Adapters,
        relays: Relays,
        env: Environment,
        config_path: PathBuf,
        state_dir: &Path,
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
            state_dir: state_dir.to_owned(),
            jobs: Mutex::default(),
        })
    }

    pub fn working(&self) -> watch::Receiver<Working> {
        self.working.subscribe()
    }

    pub async fn request(
        self: &Arc<Self>,
        arguments: &JsonObject,
        folders: &Search,
    ) -> CallToolResult {
        let task = string(arguments, "request");
        if task.trim().is_empty() {
            return error("request must be what the user wants Claude to do, in their words.");
        }
        let start = if arguments
            .get("continue")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            Start::Continue
        } else {
            match self.folder(&string(arguments, "folder"), folders).await {
                Ok(folder) => Start::Fresh(folder),
                Err(refused) => return error(refused),
            }
        };
        let budget = match lucy_config::load(&self.config_path) {
            Ok(config) => config.handoff.budget_mins.to_duration(),
            Err(e) => {
                return error(format!(
                    "Nothing has started: Lucy {}. Tell the user, and that they can fix the file \
                     and ask again.",
                    e.to_string().lines().next().unwrap_or_default()
                ))
            }
        };
        let request = Request {
            task,
            start,
            budget,
        };
        let mut jobs = self.jobs.locked();
        if let Some(running) = &jobs.current {
            let running = running.request.clone();
            return self.queue(&mut jobs, &running, request, arguments);
        }
        let job = match self.start(request) {
            Ok(job) => job,
            Err(refused) => return error(refused),
        };
        jobs.current = Some(job);
        drop(jobs);
        self.working.send_replace(Working::Yes);
        text(
            "Claude has started on it. Tell the user so in a few words, and carry on with \
             anything else they ask. Lucy will tell you when Claude has finished.",
        )
    }

    fn queue(
        &self,
        jobs: &mut Jobs,
        running: &str,
        request: Request,
        arguments: &JsonObject,
    ) -> CallToolResult {
        if let Some(queued) = &jobs.queued {
            return error(format!(
                "Claude is still working on \"{running}\", and \"{}\" is queued after it. Tell \
                 the user Lucy can't take another until those are done.",
                queued.task
            ));
        }
        if arguments.get("queue").and_then(serde_json::Value::as_bool) != Some(true) {
            return error(format!(
                "Nothing has started: Claude is still working on \"{running}\". Ask the user \
                 whether to queue this, naming that one in a few words, such as \"Claude's \
                 still on the lucy test; want me to queue this?\" If they say yes, call \
                 hand_to_claude again with the same arguments, and queue set to true."
            ));
        }
        jobs.queued = Some(request);
        self.working.send_replace(Working::YesAndQueued);
        text(
            "It's queued: Claude starts on it once it has finished the one it's on. Tell the \
             user so in a few words.",
        )
    }

    fn start(self: &Arc<Self>, request: Request) -> Result<Arc<Job>, &'static str> {
        let (folder, resume) = match request.start {
            Start::Fresh(folder) => (folder, None),
            Start::Continue => {
                let last = self.last()?.ok_or(NOTHING_TO_CONTINUE)?;
                (last.folder, Some(last.conversation))
            }
        };
        let job = Arc::new(Job {
            request: request.task,
            folder,
            budget: request.budget,
            hand_offs: Arc::downgrade(self),
            state: Mutex::new(JobState {
                conversation: resume.clone(),
                ..JobState::default()
            }),
            resume,
            cancelled: Notify::new(),
            ended: watch::Sender::new(false),
        });
        tokio::spawn(job.clone().run());
        Ok(job)
    }

    fn last_path(&self) -> PathBuf {
        self.state_dir.join("hand-off.toml")
    }

    fn last(&self) -> Result<Option<Last>, &'static str> {
        let text = match std::fs::read_to_string(self.last_path()) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                eprintln!(
                    "lucyd: the last Hand-off's record can't be read: {}",
                    e.kind()
                );
                return Err(UNREADABLE_LAST);
            }
        };
        toml::from_str(&text).map(Some).map_err(|_| {
            eprintln!("lucyd: the last Hand-off's record doesn't read as one");
            UNREADABLE_LAST
        })
    }

    fn save_last(&self, last: Option<&Last>) {
        let saved = match last {
            Some(last) => toml::to_string(last)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    plugins::save_state(&self.state_dir, &self.last_path(), &text)
                        .map_err(|e| e.to_string())
                }),
            None => match std::fs::remove_file(self.last_path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.kind().to_string()),
                _ => Ok(()),
            },
        };
        if let Err(e) = saved {
            eprintln!("lucyd: the last Hand-off couldn't be saved, so it can't be continued: {e}");
        }
    }

    async fn folder(&self, said: &str, folders: &Search) -> Result<PathBuf, String> {
        let said = said.trim();
        let home = || self.env.var("HOME").map(PathBuf::from).unwrap_or_default();
        let folder = if said.is_empty() || said == "~" {
            home()
        } else if let Some(below_home) = said.strip_prefix("~/") {
            home().join(below_home)
        } else if Path::new(said).is_absolute() {
            PathBuf::from(said)
        } else {
            return by_name(said, folders).await;
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
        let current = self.jobs.locked().current.clone();
        if let Some(job) = current {
            self.stop(&job);
        }
    }

    fn stop(&self, job: &Arc<Job>) -> Stopped {
        let mut jobs = self.jobs.locked();
        if !jobs.current.as_ref().is_some_and(|c| Arc::ptr_eq(c, job)) {
            return Stopped::Ended;
        }
        job.cancelled.notify_one();
        if jobs.queued.take().is_none() {
            return Stopped::Alone;
        }
        self.working.send_replace(Working::Yes);
        Stopped::AndQueued
    }

    pub fn ask_which_to_cancel(self: &Arc<Self>, authoring: Arc<Authoring>) {
        let hand_offs = self.clone();
        let asked_about = self.jobs.locked().current.clone();
        tokio::spawn(async move {
            let question = Question {
                prompt: WHICH_TO_CANCEL.to_owned(),
                expects: Expects::Words,
            };
            let answer = runner::ask(&hand_offs.commands, question, None).await;
            let choice = match answer {
                Some(Answer::Said(said)) => which_to_cancel(&said),
                _ => Cancel::Unclear,
            };
            let stop = || match &asked_about {
                Some(job) => hand_offs.stop(job),
                None => Stopped::Ended,
            };
            let said = match choice {
                Cancel::Build => {
                    authoring.cancel();
                    BUILD_STOPPED.to_owned()
                }
                Cancel::HandOff => match stop() {
                    Stopped::Ended => ALREADY_FINISHED.to_owned(),
                    stopped => stopped.said(HAND_OFF_STOPPED),
                },
                Cancel::Both => {
                    authoring.cancel();
                    match stop() {
                        Stopped::Ended => format!("{BUILD_STOPPED} {ALREADY_FINISHED}"),
                        stopped => stopped.said(BOTH_STOPPED),
                    }
                }
                Cancel::Unclear => BOTH_RUNNING.to_owned(),
            };
            let _ = hand_offs.commands.send(Command::Say(said));
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
        jobs.current = jobs.queued.take().and_then(|next| {
            let started = self.start(next).ok();
            if started.is_none() {
                self.show(Notification {
                    summary: "Claude didn't start".to_owned(),
                    body: "There was nothing for it to continue.".to_owned(),
                    action: None,
                    kind: NotificationKind::HandOff,
                });
            }
            started
        });
        let working = match jobs.current {
            Some(_) => Working::Yes,
            None => Working::No,
        };
        self.working.send_replace(working);
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

async fn by_name(name: &str, folders: &Search) -> Result<PathBuf, String> {
    let words = Words::new([name]).expect("a name that isn't blank has a word");
    let wanted = name.to_lowercase();
    let folders = folders.clone().depth(FOLDER_DEPTH);
    let found = tokio::task::spawn_blocking(move || named(&folders, &words, &wanted))
        .await
        .map_err(|e| format!("Couldn't search for the folder: {e}"))?;
    match found.as_slice() {
        [] => Err(format!(
            "No folder called \"{name}\" was found, so nothing has started. Tell the user \
             plainly, and that they can say the full path instead."
        )),
        [one] => Ok(one.clone()),
        several => {
            let listed: Vec<String> = several.iter().map(|p| p.display().to_string()).collect();
            Err(format!(
                "Several folders are called \"{name}\", so nothing has started: {}. Ask the \
                 user which one they mean, naming each by where it is, then call \
                 hand_to_claude again with that one's full path.",
                listed.join("; ")
            ))
        }
    }
}

fn named(folders: &Search, words: &Words, wanted: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = folders
        .find(words)
        .into_iter()
        .filter(|found| found.kind == files::Kind::Folder)
        .map(|found| found.path)
        .collect();
    let exact: Vec<PathBuf> = found
        .iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().to_lowercase() == wanted)
        })
        .cloned()
        .collect();
    if !exact.is_empty() {
        found = exact;
    }
    found.sort_by_key(|path| !path.join(".git").exists());
    found
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
                "folder": {"type": "string", "description": "The folder as the user said it: a name (\"lucy\"), an absolute path or one starting with ~. Leave it out when they named none."},
                "queue": {"type": "boolean", "description": "True once the user has said yes to queueing this behind the Hand-off running"},
                "continue": {"type": "boolean", "description": "True when the user is replying to Claude's last result, such as \"yes, go ahead\" to its offer. Claude carries on that conversation, in its folder."},
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
    resume: Option<String>,
    hand_offs: Weak<HandOffs>,
    state: Mutex<JobState>,
    cancelled: Notify,
    ended: watch::Sender<bool>,
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
            resume: self.resume.clone(),
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
        let mut ended = self.ended.subscribe();
        tokio::select! {
            result = self.answer(name, arguments) => result,
            _ = ended.wait_for(|ended| *ended) => Some(error("Lucy has stopped this Hand-off.")),
        }
    }

    async fn answer(&self, name: &str, arguments: &JsonObject) -> Option<CallToolResult> {
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
        let beside = Beside {
            shown: Some(shown),
            items: permission_items(&tool_name, input),
        };
        match runner::ask_beside(&commands, question, beside, kept).await {
            Some(Answer::Yes) => json!({"behavior": "allow", "updatedInput": input}),
            Some(_) => json!({"behavior": "deny", "message": REFUSED}),
            None => json!({"behavior": "deny", "message": UNASKED}),
        }
    }

    fn answered(&self) {
        let answer = self.state.locked().answer.clone();
        let conversation = self.save_last_conversation();
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
        let conversation = self.save_last_conversation();
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
        let conversation = self.save_last_conversation();
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
        let conversation = self.save_last_conversation();
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

    fn save_last_conversation(&self) -> Option<String> {
        let conversation = self.state.locked().conversation.clone();
        let last = conversation.clone().map(|conversation| Last {
            conversation,
            folder: self.folder.clone(),
        });
        if let Some(hand_offs) = self.hand_offs.upgrade() {
            hand_offs.save_last(last.as_ref());
        }
        conversation
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
                .send(Command::ShownTurnOrNextSession(text, kept));
        }
    }

    fn finish(&self) {
        let (model, token) = {
            let mut state = self.state.locked();
            (state.model.take(), state.token.take())
        };
        drop((model, token));
        self.ended.send_replace(true);
        if let Some(hand_offs) = self.hand_offs.upgrade() {
            hand_offs.finished(self);
        }
    }
}

fn permission_items(tool_name: &str, input: &serde_json::Value) -> Vec<String> {
    let field = |name: &str| input.get(name).and_then(serde_json::Value::as_str);
    let at = |path: &str, block: String| format!("`{path}`\n\n{block}");
    let item = match tool_name {
        "Bash" => field("command").map(|command| fenced("sh", command)),
        "Edit" => field("file_path")
            .zip(field("old_string"))
            .map(|(path, old)| {
                let new = field("new_string").unwrap_or_default();
                at(path, fenced("diff", &change(old, new)))
            }),
        "Write" => field("file_path")
            .zip(field("content"))
            .map(|(path, content)| at(path, fenced("", content))),
        _ => None,
    };
    let item = item.unwrap_or_else(|| at(tool_name, fenced("json", &format!("{input:#}"))));
    vec![item]
}

fn change(old: &str, new: &str) -> String {
    let removed = old.lines().map(|line| format!("-{line}"));
    let added = new.lines().map(|line| format!("+{line}"));
    removed.chain(added).collect::<Vec<_>>().join("\n")
}

fn fenced(language: &str, code: &str) -> String {
    let longest_run = code
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest_run.max(2) + 1);
    format!("{fence}{language}\n{code}\n{fence}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_to_which_one_names_the_build_claude_or_both() {
        assert_eq!(which_to_cancel("The Plugin build."), Cancel::Build);
        assert_eq!(which_to_cancel("claude"), Cancel::HandOff);
        assert_eq!(which_to_cancel("the hand-off"), Cancel::HandOff);
        assert_eq!(which_to_cancel("both of them"), Cancel::Both);
        assert_eq!(which_to_cancel("neither"), Cancel::Unclear);
    }
}
