//! Service: whether `lucyd.service` starts at login, and starting, stopping
//! or restarting it.

use ratatui::backend::Backend;

use super::{Context, Page};
use crate::systemd::{self, Verb};
use crate::ui::{Stop, Ui};

pub(super) fn page(context: &Context) -> Page {
    let enabled = match systemd::is_enabled(context.env) {
        true => "yes",
        false => "no",
    };
    Page {
        actions: vec![
            ('s', "starts lucyd"),
            ('x', "stops it"),
            ('r', "restarts it"),
        ],
        ..Page::new(vec![format!("Start Lucy whenever you log in: {enabled}")])
    }
}

/// Turns starting at login on or off.
pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    match systemd::is_enabled(context.env) {
        true => {
            ui.say("Disabling lucyd.service...")?;
            match systemd::disable(context.env) {
                Ok(()) => ui.say("Lucy no longer starts when you log in."),
                Err(e) => ui.say(format!("Couldn't disable lucyd.service: {e}")),
            }
        }
        false => {
            ui.say("Enabling lucyd.service...")?;
            match systemd::enable(context.env) {
                Ok(()) => ui.say("Lucy starts whenever you log in."),
                Err(e) => ui.say(format!("Couldn't enable lucyd.service: {e}")),
            }
        }
    }
}

pub(super) fn act<B: Backend>(ui: &mut Ui<B>, context: &Context, letter: char) -> Result<(), Stop> {
    match letter {
        's' => context.service(ui, Verb::Start),
        'x' => context.service(ui, Verb::Stop),
        'r' => context.service(ui, Verb::Restart),
        _ => Ok(()),
    }
}
