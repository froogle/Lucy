//! lucyd's published status: a snapshot for `status`, and every change
//! queued to each subscriber so none is lost or merged.

use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::mpsc;

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
            },
            subscribers: vec![],
        })))
    }

    pub fn snapshot(&self) -> Status {
        self.0.lock().unwrap().status.clone()
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
        let mut inner = self.0.lock().unwrap();
        inner.subscribers.push(Box::new(deliver));
        inner.status.clone()
    }

    /// Sets the status and, if it changed, tells every subscriber.
    pub fn publish(&self, status: Status) {
        let mut inner = self.0.lock().unwrap();
        if inner.status == status {
            return;
        }
        inner.status = status.clone();
        inner.subscribers.retain(|deliver| deliver(status.clone()));
    }
}
