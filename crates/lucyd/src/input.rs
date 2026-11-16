use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use lucy_ipc::{Event, Field, FieldKind, InputClose, InputOpen};
use rmcp::model::{CallToolResult, JsonObject, Tool};
use serde_json::json;
use tokio::sync::{mpsc, oneshot, watch};

use crate::mcp::{error, tool};
use crate::runner::{self, Command};
use crate::session::{Answer, Expects, Question};
use crate::sync::Locked;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub message: String,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub enum Typed {
    Values(BTreeMap<String, String>),
    Cancelled,
}

/// How a question put in the Input dialog was answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedAnswer {
    Answer(String),
    /// The user closed it, said a cancel phrase, or it couldn't be shown.
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub enum Refusal {
    NoDialog,
    Fields,
}

#[derive(Clone)]
pub struct Dialogs {
    inner: Arc<Mutex<Inner>>,
    held: Arc<watch::Sender<bool>>,
    page: Vec<String>,
}

#[derive(Default)]
struct Inner {
    holder: Option<mpsc::UnboundedSender<Event>>,
    open: Option<Open>,
    opened: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DialogId(u64);

impl DialogId {
    fn parse(wire: &str) -> Option<DialogId> {
        wire.parse().ok().map(DialogId)
    }
}

impl std::fmt::Display for DialogId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

struct Open {
    id: DialogId,
    request: InputOpen,
    reply: oneshot::Sender<Typed>,
}

impl Inner {
    fn send(&self, event: Event) {
        if let Some(holder) = &self.holder {
            let _ = holder.send(event);
        }
    }

    fn take(&mut self, id: DialogId) -> Option<Open> {
        match &self.open {
            Some(open) if open.id == id => self.open.take(),
            _ => None,
        }
    }
}

impl Dialogs {
    pub fn new(page: Vec<String>) -> Self {
        Dialogs {
            inner: Arc::default(),
            held: Arc::new(watch::Sender::new(false)),
            page,
        }
    }

    pub fn page(&self) -> &[String] {
        &self.page
    }

    pub fn claimed(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut held = self.held.subscribe();
        async move {
            let _ = held.wait_for(|held| *held).await;
        }
    }

    pub fn claim(&self) -> Option<InputClaim> {
        let mut inner = self.inner.locked();
        if inner.holder.is_some() {
            return None;
        }
        let (holder, events) = mpsc::unbounded_channel();
        inner.holder = Some(holder);
        self.held.send_replace(true);
        if let Some(open) = &inner.open {
            inner.send(Event::InputOpen(open.request.clone()));
        }
        Some(InputClaim {
            dialogs: self.clone(),
            events,
        })
    }

    pub fn open(&self, dialog: Dialog) -> (OpenDialog, bool) {
        let mut inner = self.inner.locked();
        inner.opened += 1;
        let id = DialogId(inner.opened);
        let request = InputOpen {
            id: id.to_string(),
            message: dialog.message,
            fields: dialog.fields,
        };
        let (reply, typed) = oneshot::channel();
        inner.send(Event::InputOpen(request.clone()));
        inner.open = Some(Open { id, request, reply });
        let held = inner.holder.is_some();
        let open = OpenDialog {
            id,
            dialogs: self.clone(),
            typed,
        };
        (open, held)
    }

    fn release(&self) {
        let mut inner = self.inner.locked();
        inner.holder = None;
        self.held.send_replace(false);
        let closed_by_user = inner.open.take();
        if let Some(open) = closed_by_user {
            let _ = open.reply.send(Typed::Cancelled);
        }
    }
}

pub struct InputClaim {
    dialogs: Dialogs,
    pub events: mpsc::UnboundedReceiver<Event>,
}

impl InputClaim {
    pub fn answer(&self, id: &str, values: BTreeMap<String, String>) -> Result<(), Refusal> {
        let mut inner = self.dialogs.inner.locked();
        let id = DialogId::parse(id).ok_or(Refusal::NoDialog)?;
        let open = inner.open.as_ref().filter(|open| open.id == id);
        let Some(open) = open else {
            return Err(Refusal::NoDialog);
        };
        let names = open.request.fields.iter().map(|field| &field.name);
        if !names.eq(values.keys()) {
            return Err(Refusal::Fields);
        }
        let open = inner.take(id).expect("checked above");
        let _ = open.reply.send(Typed::Values(values));
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<(), Refusal> {
        let mut inner = self.dialogs.inner.locked();
        let id = DialogId::parse(id).ok_or(Refusal::NoDialog)?;
        let open = inner.take(id).ok_or(Refusal::NoDialog)?;
        let _ = open.reply.send(Typed::Cancelled);
        Ok(())
    }
}

impl Drop for InputClaim {
    fn drop(&mut self) {
        self.dialogs.release();
    }
}

pub struct OpenDialog {
    id: DialogId,
    dialogs: Dialogs,
    typed: oneshot::Receiver<Typed>,
}

impl OpenDialog {
    pub async fn typed(&mut self) -> Typed {
        (&mut self.typed).await.unwrap_or(Typed::Cancelled)
    }
}

impl Drop for OpenDialog {
    fn drop(&mut self) {
        let mut inner = self.dialogs.inner.locked();
        if inner.take(self.id).is_some() {
            inner.send(Event::InputClose(InputClose {
                id: self.id.to_string(),
            }));
        }
    }
}

pub fn ask_typed_tool() -> Tool {
    tool(
        "ask_typed",
        "Ask the user to type something rather than say it: a long URL, an exact spelling, a \
         number or a choice. Lucy puts a box on screen and tells the user. Returns `answer`, or \
         `result` cancelled or timed_out. Never for passwords or other secrets.",
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "What to type, shown in the box"},
                "kind": {"type": "string", "enum": ["text", "url", "number", "choice"]},
                "choices": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "The options, for kind choice",
                },
            },
            "required": ["question", "kind"],
        }),
    )
}

pub async fn ask_typed(
    commands: &mpsc::UnboundedSender<Command>,
    arguments: &JsonObject,
) -> CallToolResult {
    let question = arguments
        .get("question")
        .and_then(|question| question.as_str())
        .filter(|question| !question.trim().is_empty());
    let Some(question) = question else {
        return error("question must say what to type.");
    };
    let kind = match arguments.get("kind").and_then(|kind| kind.as_str()) {
        Some("text") => FieldKind::Text,
        Some("url") => FieldKind::Url,
        Some("number") => FieldKind::Number,
        Some("choice") => FieldKind::Choice,
        Some("secret") => {
            return error(
                "ask_typed can't ask for a secret. Passwords and keys go into a Plugin's \
                 credentials, never to you.",
            )
        }
        _ => return error("kind must be one of text, url, number or choice."),
    };
    let choices: Vec<String> = arguments
        .get("choices")
        .and_then(|choices| choices.as_array())
        .into_iter()
        .flatten()
        .filter_map(|choice| choice.as_str().map(str::to_owned))
        .collect();
    if kind == FieldKind::Choice && choices.is_empty() {
        return error("A choice needs its choices.");
    }
    match ask(commands, question, kind, choices).await {
        TypedAnswer::Answer(answer) => CallToolResult::structured(json!({"answer": answer})),
        TypedAnswer::Cancelled => CallToolResult::structured(json!({"result": "cancelled"})),
        TypedAnswer::TimedOut => CallToolResult::structured(json!({"result": "timed_out"})),
    }
}

pub async fn ask(
    commands: &mpsc::UnboundedSender<Command>,
    question: &str,
    kind: FieldKind,
    choices: Vec<String>,
) -> TypedAnswer {
    let dialog = Dialog {
        message: question.to_owned(),
        fields: vec![Field {
            name: ANSWER.to_owned(),
            description: None,
            kind,
            choices,
        }],
    };
    let question = Question {
        prompt: format!("I've put a box on screen for {}.", what(kind)),
        expects: Expects::Typed(dialog),
    };
    match runner::ask(commands, question, None).await {
        Some(Answer::Typed(mut values)) => {
            TypedAnswer::Answer(values.remove(ANSWER).unwrap_or_default())
        }
        Some(Answer::TimedOut) => TypedAnswer::TimedOut,
        _ => TypedAnswer::Cancelled,
    }
}

const ANSWER: &str = "answer";

fn what(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Text => "your answer",
        FieldKind::Url => "the URL",
        FieldKind::Number => "the number",
        FieldKind::Choice => "your choice",
    }
}
