use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::{broadcast, mpsc};

use crate::input::Dialogs;
use crate::sync::Locked;

#[derive(Clone)]
pub struct StatusBus(Arc<Mutex<Inner>>);

struct Inner {
    status: Status,
    subscribers: Vec<Box<dyn Fn(Status) -> bool + Send>>,
}

impl StatusBus {
    pub fn new() -> Self {
        StatusBus(Arc::new(Mutex::new(Inner {
            status: Status {
                state: State::Idle,
                session_id: None,
                building: false,
                looking: false,
            },
            subscribers: vec![],
        })))
    }

    pub fn snapshot(&self) -> Status {
        self.0.locked().status.clone()
    }

    pub fn subscribe(&self) -> (Status, mpsc::UnboundedReceiver<Status>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let status = self.subscribe_with(move |status| tx.send(status).is_ok());
        (status, rx)
    }

    pub fn subscribe_with(&self, deliver: impl Fn(Status) -> bool + Send + 'static) -> Status {
        let mut inner = self.0.locked();
        inner.subscribers.push(Box::new(deliver));
        inner.status.clone()
    }

    pub fn publish(&self, status: Status) {
        let mut inner = self.0.locked();
        if inner.status == status {
            return;
        }
        inner.status = status.clone();
        inner.subscribers.retain(|deliver| deliver(status.clone()));
    }
}

#[derive(Clone)]
pub struct Buses {
    pub status: StatusBus,
    pub levels: LevelBus,
    pub plugins: ChangeBus,
    pub dialogs: Dialogs,
}

#[derive(Clone)]
pub struct LevelBus(broadcast::Sender<f32>);

impl LevelBus {
    pub fn new() -> Self {
        const ABOUT_A_SECOND_OF_LEVELS: usize = 32;
        LevelBus(broadcast::channel(ABOUT_A_SECOND_OF_LEVELS).0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<f32> {
        self.0.subscribe()
    }

    pub fn publish(&self, rms: f32) {
        let _none_listening = self.0.send(rms);
    }
}

#[derive(Clone, Default)]
pub struct ChangeBus(Arc<Mutex<Vec<mpsc::UnboundedSender<()>>>>);

impl ChangeBus {
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.locked().push(tx);
        rx
    }

    pub fn publish(&self) {
        self.0.locked().retain(|tx| tx.send(()).is_ok());
    }
}
