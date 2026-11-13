//! lucyd's published status: a snapshot for `status`, and every change
//! queued to each subscriber so none is lost or merged.

use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct StatusBus(Arc<Mutex<Inner>>);

struct Inner {
    status: Status,
    subscribers: Vec<mpsc::UnboundedSender<Status>>,
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
        let mut inner = self.0.lock().unwrap();
        inner.subscribers.push(tx);
        (inner.status.clone(), rx)
    }

    /// Sets the status and, if it changed, tells every subscriber.
    pub fn publish(&self, status: Status) {
        let mut inner = self.0.lock().unwrap();
        if inner.status == status {
            return;
        }
        inner.status = status.clone();
        inner
            .subscribers
            .retain(|subscriber| subscriber.send(status.clone()).is_ok());
    }
}
