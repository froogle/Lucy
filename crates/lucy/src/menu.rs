//! `lucy`'s list of sections under lucyd's status. Enter opens a section as
//! its own page and Esc comes back. With no `config.toml`, and for
//! `lucy setup`, it first walks the sections in order.

use lucy_config::credentials::CredentialStore;
use ratatui::backend::Backend;

use crate::sections::{self, Context, Opening, Section};
use crate::ui::{back_is_fine, Pick, Rows, Stop, Ui};
use crate::Env;

/// What `lucy` opens at.
#[derive(Debug, Clone, PartialEq)]
pub enum Open {
    /// `lucy`: the list, after the walk if there is no `config.toml` yet.
    Lucy,
    /// `lucy setup`: the walk, then the list.
    Setup,
    /// `lucy credentials <plugin>`: the Plugins section, entering that
    /// Plugin's credentials.
    Credentials(String),
}

/// Shows `lucy` until the user quits.
///
/// # Errors
///
/// If the terminal can't be drawn on or read from.
pub fn run<B: Backend>(
    ui: &mut Ui<B>,
    env: &Env,
    store: &dyn CredentialStore,
    open: Open,
) -> Result<(), String> {
    let context = Context { env, store };
    match show(ui, &context, open) {
        Ok(()) | Err(Stop::Quit | Stop::Back) => Ok(()),
        Err(Stop::Io(e)) => Err(e.to_string()),
    }
}

fn show<B: Backend>(ui: &mut Ui<B>, context: &Context, open: Open) -> Result<(), Stop> {
    let mut selected = 0;
    match open {
        Open::Lucy if context.env.config.exists() => {}
        Open::Lucy | Open::Setup => walk(ui, context)?,
        Open::Credentials(plugin) => {
            selected = position(Section::Plugins);
            ui.screen.lines.clear();
            let row = sections::plugin_row(context, &plugin);
            if row.is_none() {
                ui.say(format!("There's no Plugin called {plugin}."))?;
            }
            back_is_fine(sections::open(
                ui,
                context,
                Section::Plugins,
                Opening::Alone,
                row,
            ))?;
        }
    }
    loop {
        ui.screen.status = context.status();
        ui.screen.lines.clear();
        let list = Rows {
            title: "Sections".to_owned(),
            note: vec![],
            rows: Section::ALL.iter().map(|s| s.name().to_owned()).collect(),
            actions: vec![],
            selected,
        };
        let Pick::Row(row) = ui.rows(list)? else {
            continue;
        };
        selected = row;
        ui.screen.lines.clear();
        back_is_fine(sections::open(
            ui,
            context,
            Section::ALL[row],
            Opening::Alone,
            None,
        ))?;
    }
}

fn walk<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    for (i, &section) in Section::ALL.iter().enumerate() {
        ui.screen.lines.clear();
        let next = Section::ALL.get(i + 1).copied();
        match sections::open(ui, context, section, Opening::Walk(next), None) {
            Ok(()) => {}
            Err(Stop::Back) => break,
            Err(stop) => return Err(stop),
        }
    }
    if !context.env.config.exists() {
        if let Err(e) = write_header(&context.env.config) {
            ui.say(format!("Can't write {}: {e}", context.env.config.display()))?;
        }
    }
    Ok(())
}

fn write_header(config: &std::path::Path) -> std::io::Result<()> {
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        config,
        "# Lucy's settings. Each is optional; `lucy` writes the ones you pick there.\n",
    )
}

fn position(section: Section) -> usize {
    Section::ALL
        .iter()
        .position(|s| *s == section)
        .expect("every section is listed")
}
