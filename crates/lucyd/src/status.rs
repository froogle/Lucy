//! lucyd's published status: a snapshot for `status`, and every change
//! queued to each subscriber so none is lost or merged.

use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::{broadcast, mpsc};

use crate::input::Dialogs;
use crate::sync::Locked;

#[derive(Clone)]
pub struct StatusBus(Arc<Mutex<Inner>>);

struct Inner {
    status: Status,
    /// Each returns false once its subscriber has gone.
    subscribers: Vec<Box<dyn Fn(Status) -> bool + Send>>,
}

impl StatusBus {
    pub fn new() -> Self {
        StatusBus(Arc::new(Mutex::new(Inner {
            status: Status {
                state: State::Idle,
                session_id: None,
                building: false,
            },
            subscribers: vec![],
        })))
    }

    pub fn snapshot(&self) -> Status {
        self.0.locked().status.clone()
    }

    /// The current status, and a queue of every change after it.
    pub fn subscribe(&self) -> (Status, mpsc::UnboundedReceiver<Status>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let status = self.subscribe_with(move |status| tx.send(status).is_ok());
        (status, rx)
    }

    /// The current status; `deliver` is then called with every change, in
    /// order, until it returns false.
    pub fn subscribe_with(&self, deliver: impl Fn(Status) -> bool + Send + 'static) -> Status {
        let mut inner = self.0.locked();
        inner.subscribers.push(Box::new(deliver));
        inner.status.clone()
    }

    /// Sets the status and, if it changed, tells every subscriber.
    pub fn publish(&self, status: Status) {
        let mut inner = self.0.locked();
        if inner.status == status {
            return;
        }
        inner.status = status.clone();
        inner.subscribers.retain(|deliver| deliver(status.clone()));
    }
}

/// The buses lucyd publishes to its clients.
#[derive(Clone)]
pub struct Buses {
    pub status: StatusBus,
    pub levels: LevelBus,
    /// `plugins.changed`.
    pub plugins: ChangeBus,
    /// `input.open` and `input.close`, to the `input` holder.
    pub dialogs: Dialogs,
}

/// Lucy's voice level while she speaks. A subscriber that falls behind skips
/// the levels it missed: only the latest ones matter to a meter.
#[derive(Clone)]
pub struct LevelBus(broadcast::Sender<f32>);

impl LevelBus {
    pub fn new() -> Self {
        // About a second of levels.
        LevelBus(broadcast::channel(32).0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<f32> {
        self.0.subscribe()
    }

    pub fn publish(&self, rms: f32) {
        // No subscribers is fine.
        let _ = self.0.send(rms);
    }
}

/// Says that something changed, once per change, to every subscriber, in
/// order: what changed is fetched afresh.
#[derive(Clone, Default)]
pub struct ChangeBus(Arc<Mutex<Vec<mpsc::UnboundedSender<()>>>>);

impl ChangeBus {
    /// A queue of every change after now.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.locked().push(tx);
        rx
    }

    pub fn publish(&self) {
        self.0.locked().retain(|tx| tx.send(()).is_ok());
    }
}
