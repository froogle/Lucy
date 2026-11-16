use ratatui::backend::Backend;

use super::{Context, Page, RESTART, START, STOP};
use crate::panel;
use crate::systemd;
use crate::ui::{Stop, Ui};

pub(super) fn page(context: &Context) -> Page {
    let enabled = match systemd::is_enabled(context.env) {
        true => "yes",
        false => "no",
    };
    Page {
        actions: vec![START, STOP, RESTART],
        ..Page::new(vec![format!("Start Lucy whenever you log in: {enabled}")])
    }
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    if !ui.yes("Start Lucy whenever you log in?", true)? {
        ui.say("Disabling lucyd.service...")?;
        return match systemd::disable(context.env) {
            Ok(()) => ui.say("Lucy no longer starts when you log in."),
            Err(e) => ui.say(format!("Couldn't disable lucyd.service: {e}")),
        };
    }
    ui.say("Enabling lucyd.service...")?;
    if let Err(e) = systemd::enable(context.env) {
        return ui.say(format!("Couldn't enable lucyd.service: {e}"));
    }
    ui.say("Lucy starts whenever you log in.")?;
    match panel::link(context.env) {
        Ok(()) => Ok(()),
        Err(e) => ui.say(format!("Couldn't link the bar panel into Omarchy: {e}")),
    }
}
