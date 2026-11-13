//! The Indicator: a separate client of lucyd's public socket that claims the
//! `indicator` role and draws Lucy's state in the bottom-right corner.
//! Clicking it ends the Session. Without layer-shell it exits cleanly, and
//! lucyd falls back to notifications.

mod look;
mod theme;
mod wayland;

use std::process::ExitCode;

use lucy_ipc::{Client, Event, Role, Topic};
use smithay_client_toolkit::reexports::calloop::channel;

use crate::wayland::{Display, Update};

fn main() -> ExitCode {
    let display = match Display::connect() {
        Ok(display) => display,
        Err(why) => {
            eprintln!("lucy-indicator: {why}; lucyd will show notifications instead");
            return ExitCode::SUCCESS;
        }
    };
    let Some(socket_path) = lucy_ipc::default_socket_path() else {
        eprintln!("lucy-indicator: XDG_RUNTIME_DIR is not set");
        return ExitCode::FAILURE;
    };
    let mut client = match Client::connect(&socket_path, "lucy-indicator", Some(Role::Indicator))
        .and_then(|mut client| client.subscribe(&[Topic::State]).map(|()| client))
    {
        Ok(client) => client,
        Err(e) => {
            eprintln!("lucy-indicator: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (updates_tx, updates) = channel::channel();
    std::thread::spawn(move || loop {
        let update = match client.next_event() {
            Ok(Event::State(status)) => Update::State(status.state),
            Err(e) => Update::Lost(e.to_string()),
        };
        let lost = matches!(update, Update::Lost(_));
        if updates_tx.send(update).is_err() || lost {
            return;
        }
    });

    let why = display.run(updates, theme::omarchy_colors_path(), end_session);
    eprintln!("lucy-indicator: {why}");
    ExitCode::FAILURE
}

/// The marker was clicked: end the Session, from any state.
fn end_session() {
    let Some(socket_path) = lucy_ipc::default_socket_path() else {
        return;
    };
    let ended = Client::connect(&socket_path, "lucy-indicator", None)
        .and_then(|mut client| client.end_session());
    if let Err(e) = ended {
        eprintln!("lucy-indicator: can't end the Session: {e}");
    }
}
