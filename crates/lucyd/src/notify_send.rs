use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::adapters::{Notification, NotificationAction, NotificationKind, Notifier};
use crate::sync::Locked;

pub struct NotifySend {
    program: PathBuf,
    with_action: Arc<Mutex<Option<String>>>,
}

impl NotifySend {
    pub fn new(program: PathBuf) -> Self {
        NotifySend {
            program,
            with_action: Arc::default(),
        }
    }
}

const END_KEY: &str = "end";

const PRINT_ID_FIRST: &str = "--print-id";

const WAIT_AND_PRINT_PICKED_KEY: &str = "--wait";

impl Notifier for NotifySend {
    fn notify(
        &self,
        notification: Notification,
        actions: mpsc::UnboundedSender<NotificationAction>,
    ) {
        let mut command = Command::new(&self.program);
        command.arg("--app-name=Lucy").arg(PRINT_ID_FIRST);
        if notification.kind == NotificationKind::Status {
            if let Some(id) = self.with_action.locked().take() {
                command.arg(format!("--replace-id={id}"));
            }
        }
        let has_action = notification.action.is_some();
        if let Some(NotificationAction::EndSession) = notification.action {
            command
                .arg(format!("--action={END_KEY}=End"))
                .arg(WAIT_AND_PRINT_PICKED_KEY);
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
        let with_action = self.with_action.clone();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines().map_while(Result::ok);
            if let Some(id) = lines.next().filter(|_| has_action) {
                *with_action.locked() = Some(id.trim().to_owned());
            }
            for line in lines {
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
    use std::path::Path;

    use super::*;
    use crate::notifications::{ended, listening, unspoken};

    fn fake_notify_send(dir: &Path, output: &str) -> PathBuf {
        let program = dir.join("notify-send");
        let calls = dir.join("calls");
        let script = format!(
            "#!/bin/sh\n(IFS='|'; echo \"$*\") >> '{}'\nprintf '{output}'\n",
            calls.display()
        );
        let written = Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
                "sh",
            ])
            .arg(script)
            .arg(&program)
            .status()
            .unwrap();
        assert!(written.success());
        program
    }

    fn calls(dir: &Path) -> Vec<String> {
        let calls = std::fs::read_to_string(dir.join("calls")).unwrap();
        calls.lines().map(str::to_owned).collect()
    }

    #[tokio::test]
    async fn picking_end_sends_the_end_action() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), "7\\nend\\n"));
        let (actions, mut picked) = mpsc::unbounded_channel();

        notifier.notify(listening(), actions);

        assert_eq!(picked.recv().await, Some(NotificationAction::EndSession));
        assert_eq!(
            calls(dir.path()),
            ["--app-name=Lucy|--print-id|--action=end=End|--wait|--|Lucy is listening|"]
        );
    }

    #[tokio::test]
    async fn the_next_notification_replaces_one_with_an_action() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), "7\\nend\\n"));
        let (actions, mut picked) = mpsc::unbounded_channel();
        notifier.notify(listening(), actions.clone());
        let _action_read_after_the_id = picked.recv().await;

        notifier.notify(ended(), actions);

        while picked.recv().await.is_some() {}
        assert_eq!(
            calls(dir.path())[1],
            "--app-name=Lucy|--print-id|--replace-id=7|--|Lucy has stopped listening|"
        );
    }

    #[tokio::test]
    async fn an_unspoken_reply_leaves_the_end_action_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), "7\\nend\\n"));
        let (actions, mut picked) = mpsc::unbounded_channel();
        notifier.notify(listening(), actions.clone());
        let _action_read_after_the_id = picked.recv().await;

        notifier.notify(unspoken(&["It's three o'clock.".into()]), actions.clone());
        notifier.notify(ended(), actions);

        while picked.recv().await.is_some() {}
        let mut later_in_either_order = calls(dir.path())[1..].to_vec();
        later_in_either_order.sort();
        assert_eq!(
            later_in_either_order,
            [
                "--app-name=Lucy|--print-id|--replace-id=7|--|Lucy has stopped listening|",
                "--app-name=Lucy|--print-id|--|Lucy|It's three o'clock."
            ]
        );
    }

    #[tokio::test]
    async fn a_notification_without_an_action_does_not_wait_or_get_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let notifier = NotifySend::new(fake_notify_send(dir.path(), "7\\n"));
        let (actions, mut picked) = mpsc::unbounded_channel();

        notifier.notify(ended(), actions.clone());
        notifier.notify(ended(), actions);

        assert_eq!(picked.recv().await, None);
        let calls = calls(dir.path());
        assert_eq!(
            calls,
            [
                "--app-name=Lucy|--print-id|--|Lucy has stopped listening|",
                "--app-name=Lucy|--print-id|--|Lucy has stopped listening|"
            ]
        );
    }
}
