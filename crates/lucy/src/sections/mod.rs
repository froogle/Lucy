mod approvals;
mod audio;
mod plugins;
mod service;
mod speech;
mod thinking;
mod voice;
mod wake;

pub use plugins::row as plugin_row;

use std::time::Duration;

use lucy_config::credentials::CredentialStore;
use lucy_ipc::Client;
use ratatui::backend::Backend;

use crate::settings::{self, Applies, Change};
use crate::systemd::{self, Verb};
use crate::ui::{back_is_fine, Pick, Rows, Stop, Ui};
use crate::Env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Thinking,
    Speech,
    Voice,
    Audio,
    WakeWord,
    Plugins,
    Approvals,
    Service,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Thinking,
        Section::Speech,
        Section::Voice,
        Section::Audio,
        Section::WakeWord,
        Section::Plugins,
        Section::Approvals,
        Section::Service,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::Thinking => "Thinking",
            Section::Speech => "Speech-to-text",
            Section::Voice => "Voice",
            Section::Audio => "Audio devices",
            Section::WakeWord => "Wake word",
            Section::Plugins => "Plugins",
            Section::Approvals => "Approvals",
            Section::Service => "Service",
        }
    }

    fn page(self, context: &Context) -> Page {
        match self {
            Section::Thinking => thinking::page(context),
            Section::Speech => speech::page(context),
            Section::Voice => voice::page(context),
            Section::Audio => audio::page(context),
            Section::WakeWord => wake::page(),
            Section::Plugins => plugins::page(context),
            Section::Approvals => approvals::page(context),
            Section::Service => service::page(context),
        }
    }

    fn pick<B: Backend>(self, ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
        match self {
            Section::Thinking => thinking::pick(ui, context, row),
            Section::Speech => speech::pick(ui, context, row),
            Section::Voice => voice::pick(ui, context),
            Section::Audio => audio::pick(ui, context, row),
            Section::WakeWord => wake::pick(ui, context),
            Section::Plugins => plugins::pick(ui, context, row),
            Section::Approvals => approvals::revoke(ui, context, row),
            Section::Service => service::pick(ui, context),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Action {
    letter: char,
    does: &'static str,
    runs: Runs,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Runs {
    Check,
    Revoke,
    Service(Verb),
}

const CHECK: Action = Action {
    letter: 'c',
    does: "runs a real check",
    runs: Runs::Check,
};
const REVOKE: Action = Action {
    letter: 'r',
    does: "revokes",
    runs: Runs::Revoke,
};
const START: Action = Action {
    letter: 's',
    does: "starts lucyd",
    runs: Runs::Service(Verb::Start),
};
const STOP: Action = Action {
    letter: 'x',
    does: "stops it",
    runs: Runs::Service(Verb::Stop),
};
const RESTART: Action = Action {
    letter: 'r',
    does: "restarts it",
    runs: Runs::Service(Verb::Restart),
};

struct Page {
    note: Vec<String>,
    rows: Vec<String>,
    actions: Vec<Action>,
    needs_lucyd: bool,
}

impl Page {
    fn new(rows: Vec<String>) -> Page {
        Page {
            note: vec![],
            rows,
            actions: vec![],
            needs_lucyd: false,
        }
    }

    fn needing_lucyd(self) -> Page {
        Page {
            needs_lucyd: true,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opening {
    Alone,
    Walk(Option<Section>),
}

pub fn open<B: Backend>(
    ui: &mut Ui<B>,
    context: &Context,
    section: Section,
    opening: Opening,
    first: Option<usize>,
) -> Result<(), Stop> {
    let mut selected = 0;
    if let Some(row) = first {
        selected = row;
        back_is_fine(section.pick(ui, context, row))?;
    }
    loop {
        ui.screen.status = context.status();
        let mut page = section.page(context);
        if page.needs_lucyd && context.client().is_none() {
            page.note
                .push(format!("lucyd isn't running. {} starts it.", START.letter));
            page.actions.push(START);
        }
        let picked = page.rows.len();
        let mut rows = page.rows;
        if let Opening::Walk(next) = opening {
            rows.push(match next {
                Some(next) => format!("Next: {}", next.name()),
                None => "Done: go to the list of sections".to_owned(),
            });
        }
        let list = Rows {
            title: section.name().to_owned(),
            note: page.note,
            rows,
            actions: page.actions.iter().map(|a| (a.letter, a.does)).collect(),
            selected,
        };
        match ui.rows(list)? {
            Pick::Row(row) if row == picked => return Ok(()),
            Pick::Row(row) => {
                selected = row + 1;
                back_is_fine(section.pick(ui, context, row))?;
            }
            Pick::Action(letter, row) => {
                selected = row;
                let action = page
                    .actions
                    .iter()
                    .find(|action| action.letter == letter)
                    .expect("the list offers only the page's letters");
                back_is_fine(act(ui, context, action.runs, (row < picked).then_some(row)))?;
            }
        }
    }
}

fn act<B: Backend>(
    ui: &mut Ui<B>,
    context: &Context,
    runs: Runs,
    row: Option<usize>,
) -> Result<(), Stop> {
    match (runs, row) {
        (Runs::Check, Some(row)) => thinking::check(ui, context, row),
        (Runs::Revoke, Some(row)) => approvals::revoke(ui, context, row),
        (Runs::Service(verb), _) => context.service(ui, verb),
        (Runs::Check | Runs::Revoke, None) => Ok(()),
    }
}

pub struct Context<'a> {
    pub env: &'a Env,
    pub store: &'a dyn CredentialStore,
}

impl Context<'_> {
    fn client(&self) -> Option<Client> {
        self.env.connect().ok()
    }

    fn query<T>(&self, call: impl FnOnce(&mut Client) -> Result<T, lucy_ipc::Error>) -> Option<T> {
        self.client().and_then(|mut client| call(&mut client).ok())
    }

    fn lucyd<B: Backend>(&self, ui: &mut Ui<B>, needed_by: &str) -> Result<Option<Client>, Stop> {
        let client = self.client();
        if client.is_none() {
            ui.say(format!(
                "lucyd isn't running, {needed_by}. {} starts it.",
                START.letter
            ))?;
        }
        Ok(client)
    }

    pub fn status(&self) -> String {
        match self.query(Client::status) {
            None => "lucyd isn't running.".to_owned(),
            Some(status) if status.session_id.is_some() => {
                "lucyd is running. A Session is open.".to_owned()
            }
            Some(_) => "lucyd is running. No Session is open.".to_owned(),
        }
    }

    fn config(&self) -> lucy_config::Config {
        lucy_config::load(&self.env.config).unwrap_or_default()
    }

    fn save<B: Backend>(&self, ui: &mut Ui<B>, changes: &[Change]) -> Result<(), Stop> {
        match settings::write(&self.env.config, changes) {
            Ok(applies) => self.say_when(ui, applies),
            Err(e) => ui.say(format!("Not saved: {e}")),
        }
    }

    fn say_when<B: Backend>(&self, ui: &mut Ui<B>, applies: Applies) -> Result<(), Stop> {
        ui.say(applies.to_string())?;
        let offer = applies == Applies::Restart && self.client().is_some();
        if offer && ui.yes("Restart lucyd now?", true)? {
            self.service(ui, Verb::Restart)?;
        }
        Ok(())
    }

    fn service<B: Backend>(&self, ui: &mut Ui<B>, verb: Verb) -> Result<(), Stop> {
        let doing = match verb {
            Verb::Start => "Starting lucyd...",
            Verb::Stop => "Stopping lucyd...",
            Verb::Restart => "Restarting lucyd...",
        };
        ui.say(doing)?;
        if let Err(e) = systemd::run(self.env, verb) {
            return ui.say(format!("Couldn't: {e}"));
        }
        if verb == Verb::Stop {
            return ui.say("lucyd is stopped.");
        }
        match self.wait_for_lucyd() {
            true => ui.say("lucyd is running."),
            false => ui.say("lucyd didn't start listening. journalctl --user -u lucyd says why."),
        }
    }

    fn wait_for_lucyd(&self) -> bool {
        for _ in 0..50 {
            if self.client().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}
