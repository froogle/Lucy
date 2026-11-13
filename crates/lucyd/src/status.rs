//! lucyd's published status: a snapshot for `status`, and every change
//! queued to each subscriber so none is lost or merged.

use std::sync::{Arc, Mutex};

use lucy_ipc::{State, Status};
use tokio::sync::{broadcast, mpsc};

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

/// Lucy's voice level while she speaks. A subscriber that falls behind skips
/// the levels it missed: only the latest ones matter to a meter.
#[derive(Clone)]
pub struct LoudnessBus(broadcast::Sender<f32>);

impl LoudnessBus {
    pub fn new() -> Self {
        // About a second of levels.
        LoudnessBus(broadcast::channel(32).0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<f32> {
        self.0.subscribe()
    }

    pub fn publish(&self, rms: f32) {
        // No subscribers is fine.
        let _ = self.0.send(rms);
    }
}
