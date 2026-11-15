//! The sections of `lucy`, each a page: a list of rows the user picks, and
//! letters that act on them. A section that needs lucyd still opens while
//! it is stopped, says so, and offers to start it.

mod approvals;
mod audio;
mod plugins;
mod service;
mod speech;
mod thinking;
mod voice;
mod wake;

pub(crate) use audio::devices;
pub use plugins::row as plugin_row;
pub(crate) use speech::MODELS as WHISPER_MODELS;

use std::time::Duration;

use lucy_config::credentials::CredentialStore;
use lucy_ipc::Client;
use ratatui::backend::Backend;

use crate::settings::{self, Applies, Change};
use crate::systemd::{self, Verb};
use crate::ui::{Pick, Rows, Stop, Ui};
use crate::Env;

/// The sections, in the order the walk visits them.
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

    fn act<B: Backend>(
        self,
        ui: &mut Ui<B>,
        context: &Context,
        letter: char,
        row: Option<usize>,
    ) -> Result<(), Stop> {
        match (self, row) {
            (Section::Thinking, Some(row)) => thinking::check(ui, context, row),
            (Section::Approvals, Some(row)) => approvals::revoke(ui, context, row),
            (Section::Service, _) => service::act(ui, context, letter),
            _ => Ok(()),
        }
    }
}

/// What a section's page shows.
struct Page {
    note: Vec<String>,
    rows: Vec<String>,
    actions: Vec<(char, &'static str)>,
    /// Part of it works only while lucyd runs.
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

/// How a section is opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opening {
    /// From the list; Esc goes back to it.
    Alone,
    /// In the walk: a last row moves on to the next section, or ends it.
    Walk(Option<Section>),
}

/// Shows `section` until the user goes back or, in the walk, moves on.
/// `first` is a row picked as it opens.
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
        let page = section.page(context);
        let running = context.client().is_some();
        let mut rows = page.rows;
        let mut note = page.note;
        let mut actions = page.actions;
        if page.needs_lucyd && !running {
            note.push("lucyd isn't running. s starts it.".to_owned());
            actions.push(('s', "starts lucyd"));
        }
        let picked = rows.len();
        if let Opening::Walk(next) = opening {
            rows.push(match next {
                Some(next) => format!("Next: {}", next.name()),
                None => "Done: go to the list of sections".to_owned(),
            });
        }
        let list = Rows {
            title: section.name().to_owned(),
            note,
            rows,
            actions,
            selected,
        };
        match ui.rows(list)? {
            Pick::Row(row) if row == picked => return Ok(()),
            Pick::Row(row) => {
                selected = row + 1;
                back_is_fine(section.pick(ui, context, row))?;
            }
            Pick::Action('s', _) if page.needs_lucyd && !running => {
                context.service(ui, Verb::Start)?;
            }
            Pick::Action(letter, row) => {
                selected = row;
                let row = (row < picked).then_some(row);
                back_is_fine(section.act(ui, context, letter, row))?;
            }
        }
    }
}

/// Esc in the middle of a row's questions leaves that row, not the page.
fn back_is_fine(result: Result<(), Stop>) -> Result<(), Stop> {
    match result {
        Err(Stop::Back) => Ok(()),
        result => result,
    }
}

/// What the sections work with.
pub struct Context<'a> {
    pub env: &'a Env,
    pub store: &'a dyn CredentialStore,
}

impl Context<'_> {
    /// A client of lucyd, if it is running.
    fn client(&self) -> Option<Client> {
        self.env.connect().ok()
    }

    /// The status line: whether lucyd is running and a Session is open.
    pub fn status(&self) -> String {
        let status = self.client().and_then(|mut client| client.status().ok());
        match status {
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
            Ok(applies) => self.applied(ui, applies),
            Err(e) => ui.say(format!("Not saved: {e}")),
        }
    }

    /// Says when a change applies, offering to restart lucyd if it must.
    fn applied<B: Backend>(&self, ui: &mut Ui<B>, applies: Applies) -> Result<(), Stop> {
        ui.say(applies.to_string())?;
        let offer = applies == Applies::Restart && self.client().is_some();
        if offer && ui.yes("Restart lucyd now?", true)? {
            self.service(ui, Verb::Restart)?;
        }
        Ok(())
    }

    /// Starts, stops or restarts `lucyd.service`, then says how lucyd is.
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

    /// Whether lucyd is listening, once it has had a few seconds to start.
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
