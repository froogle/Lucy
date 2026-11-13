//! The notification fallback: whenever no client holds the `indicator` role,
//! lucyd shows Session start, Confirming and end as desktop notifications.

use std::future::Future;
use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::{mpsc, oneshot};

use crate::adapters::{Notification, NotificationAction, NotificationKind, Notifier};
use crate::audio::Direction;
use crate::runner::Command;
use crate::status::StatusBus;

/// A status change or a change in how many clients hold the role, queued in
/// the order they happened.
enum Change {
    Status(Status),
    Indicators(usize),
}

/// Counts the connected clients that hold the `indicator` role.
#[derive(Clone)]
pub struct Indicators {
    count: Arc<Mutex<usize>>,
    changes: mpsc::UnboundedSender<Change>,
}

impl Indicators {
    /// Counts a client as holding the role until the claim is dropped.
    pub fn claim(&self) -> IndicatorClaim {
        self.add(1);
        IndicatorClaim(self.clone())
    }

    fn add(&self, delta: isize) {
        let mut count = self.count.lock().unwrap();
        *count = count.checked_add_signed(delta).expect("claims balance");
        let _ = self.changes.send(Change::Indicators(*count));
    }
}

pub struct IndicatorClaim(Indicators);

impl Drop for IndicatorClaim {
    fn drop(&mut self) {
        self.0.add(-1);
    }
}

/// The role count, and the task that notifies while no Indicator is there.
/// It sees every change made after this returns.
pub fn fallback(
    status: &StatusBus,
    notifier: Arc<dyn Notifier>,
    sessions: mpsc::UnboundedSender<Command>,
) -> (Indicators, impl Future<Output = ()>) {
    let (changes_tx, changes) = mpsc::unbounded_channel();
    let status_changes = changes_tx.clone();
    let current =
        status.subscribe_with(move |status| status_changes.send(Change::Status(status)).is_ok());
    let indicators = Indicators {
        count: Arc::default(),
        changes: changes_tx,
    };
    (indicators, notify(current, changes, notifier, sessions))
}

async fn notify(
    mut current: Status,
    mut changes: mpsc::UnboundedReceiver<Change>,
    notifier: Arc<dyn Notifier>,
    sessions: mpsc::UnboundedSender<Command>,
) {
    let mut indicators = 0;
    let (actions_tx, mut actions) = mpsc::unbounded_channel();
    loop {
        let notification = tokio::select! {
            Some(change) = changes.recv() => match change {
                Change::Status(next) => {
                    let previous = std::mem::replace(&mut current, next);
                    on_change(previous.state, current.state).filter(|_| indicators == 0)
                }
                Change::Indicators(count) => {
                    let lost = indicators > 0 && count == 0;
                    indicators = count;
                    on_indicator_lost(current.state).filter(|_| lost)
                }
            },
            Some(action) = actions.recv() => {
                match action {
                    NotificationAction::EndSession => {
                        let (reply, _) = oneshot::channel();
                        let _ = sessions.send(Command::End(reply));
                    }
                }
                None
            }
        };
        if let Some(notification) = notification {
            notifier.notify(notification, actions_tx.clone());
        }
    }
}

/// What to show for a state change, with no Indicator.
fn on_change(previous: State, next: State) -> Option<Notification> {
    match (previous, next) {
        (State::Idle, State::Idle) => None,
        (State::Idle, _) => Some(listening()),
        (_, State::Idle) => Some(ended()),
        (State::Confirming, State::Confirming) => None,
        (_, State::Confirming) => Some(confirming()),
        _ => None,
    }
}

/// What to show when the last Indicator goes away, so an open Session still
/// has a way to end it.
fn on_indicator_lost(state: State) -> Option<Notification> {
    match state {
        State::Idle => None,
        State::Confirming => Some(confirming()),
        _ => Some(listening()),
    }
}

pub fn listening() -> Notification {
    Notification {
        summary: "Lucy is listening".into(),
        body: String::new(),
        action: Some(NotificationAction::EndSession),
        kind: NotificationKind::Status,
    }
}

pub fn confirming() -> Notification {
    Notification {
        summary: "Lucy is waiting for your yes".into(),
        body: "Say yes to go ahead, or End to decline.".into(),
        action: Some(NotificationAction::EndSession),
        kind: NotificationKind::Status,
    }
}

pub fn ended() -> Notification {
    Notification {
        summary: "Lucy has stopped listening".into(),
        body: String::new(),
        action: None,
        kind: NotificationKind::Status,
    }
}

/// A reply Lucy couldn't speak, shown instead: the sentences of one Turn.
pub fn unspoken(sentences: &[String]) -> Notification {
    Notification {
        summary: "Lucy".into(),
        body: sentences.join(" "),
        action: None,
        kind: NotificationKind::Reply,
    }
}

/// Pinned devices that weren't there at Session start, so the defaults
/// stand in. The pins stay.
pub fn devices_missing(missing: &[(Direction, String)]) -> Notification {
    let names: Vec<String> = missing
        .iter()
        .map(|(direction, name)| format!("{} {name}", device(*direction)))
        .collect();
    Notification {
        summary: "Lucy is using the default device".into(),
        body: format!(
            "Can't find the pinned {}. She'll use it again once it's back.",
            names.join(" or the pinned ")
        ),
        action: None,
        kind: NotificationKind::Problem,
    }
}

/// A device went away mid-Session, so the Session ended.
pub fn device_lost(direction: Direction) -> Notification {
    Notification {
        summary: format!("Lucy lost the {}", device(direction)),
        body: "It went away mid-Session, so the Session ended.".into(),
        action: None,
        kind: NotificationKind::Problem,
    }
}

fn device(direction: Direction) -> &'static str {
    match direction {
        Direction::Input => "microphone",
        Direction::Output => "speaker",
    }
}

/// The model process died twice running, so the Session ended.
pub fn thinking_lost(provider: &str) -> Notification {
    Notification {
        summary: format!("Lucy lost her connection to {provider}"),
        body: format!("The {provider} process stopped twice, so the Session ended."),
        action: None,
        kind: NotificationKind::Problem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirming_without_an_indicator_is_notified_with_an_end_action() {
        let shown = on_change(State::Thinking, State::Confirming).expect("a notification");

        assert_eq!(shown.summary, "Lucy is waiting for your yes");
        assert_eq!(shown.action, Some(NotificationAction::EndSession));
    }

    #[test]
    fn a_transient_error_mid_session_is_not_notified() {
        assert_eq!(on_change(State::Listening, State::Error), None);
        assert_eq!(on_change(State::Error, State::Listening), None);
    }
}
