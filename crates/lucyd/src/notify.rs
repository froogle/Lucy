//! Desktop notifications through `notify-send`.

use std::process::{Command, Stdio};

use crate::adapters::Notifier;

pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify(&self, body: &str) {
        let shown = Command::new("notify-send")
            .args(["--app-name=Lucy", "Lucy", body])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match shown {
            // Reaped on its own thread so it never becomes a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(e) => eprintln!("lucyd: can't show a notification: {e}"),
        }
    }
}
