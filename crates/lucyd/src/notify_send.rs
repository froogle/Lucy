//! Desktop notifications through libnotify's `notify-send`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tokio::sync::mpsc;

use crate::adapters::{Notification, NotificationAction, Notifier};

pub struct NotifySend {
    program: PathBuf,
}

impl NotifySend {
    pub fn new(program: PathBuf) -> Self {
        NotifySend { program }
    }
}

/// The action key `notify-send` prints when the user picks the action.
const END_KEY: &str = "end";

impl Notifier for NotifySend {
    fn notify(
        &self,
        notification: Notification,
        actions: mpsc::UnboundedSender<NotificationAction>,
    ) {
        let mut command = Command::new(&self.program);
        command.arg("--app-name=Lucy");
        if let Some(NotificationAction::EndSession) = notification.action {
            // `--wait` keeps it running until the notification closes, and
            // it prints the picked action's key.
            command.arg(format!("--action={END_KEY}=End")).arg("--wait");
        }
        command
            .arg("--")
            .arg(&notification.summary)
            .arg(&notification.body)
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("lucyd: can't run {}: {e}", self.program.display());
                return;
            }
        };
        let stdout = child.stdout.take().expect("stdout is piped");
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.trim() == END_KEY {
                    let _ = actions.send(NotificationAction::EndSession);
                }
            }
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    /// A stand-in `notify-send` that records its arguments, one per line,
    /// and prints `output`.
    fn fake_notify_send(dir: &Path, output: &str) -> PathBuf {
        let program = dir.join("notify-send");
        let args = dir.join("args");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '{output}'\n",
                args.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    fn shown_args(dir: &Path) -> Vec<String> {
        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        args.lines().map(str::to_owned).collect()
    }

    #[tokio::test]
    async fn picking_end_sends_the_end_action() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), "end\\n"));
        let (actions, mut picked) = mpsc::unbounded_channel();

        notifier.notify(
            Notification {
                summary: "Lucy is listening".into(),
                body: String::new(),
                action: Some(NotificationAction::EndSession),
            },
            actions,
        );

        assert_eq!(picked.recv().await, Some(NotificationAction::EndSession));
        assert_eq!(
            shown_args(dir.path()),
            [
                "--app-name=Lucy",
                "--action=end=End",
                "--wait",
                "--",
                "Lucy is listening",
                ""
            ]
        );
    }

    #[tokio::test]
    async fn a_notification_without_an_action_does_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), ""));
        let (actions, mut picked) = mpsc::unbounded_channel();

        notifier.notify(
            Notification {
                summary: "Lucy has stopped listening".into(),
                body: String::new(),
                action: None,
            },
            actions,
        );

        assert_eq!(picked.recv().await, None);
        assert_eq!(
            shown_args(dir.path()),
            ["--app-name=Lucy", "--", "Lucy has stopped listening", ""]
        );
    }
}
