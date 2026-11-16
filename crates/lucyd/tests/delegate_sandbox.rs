//! An Authoring Delegate runs under Landlock: it can work in its staging
//! dir, but can't read or write the user's files. Driven through the real
//! `claude` adapter's Delegate spawn against a fake `claude` that tries.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};
use std::process::Command;

use harness::wait_until;
use lucy_config::{Kind, Provider};
use lucyd::adapters::{Relay, Thinking, Workspace};
use lucyd::claude::Claude;
use tokio::sync::mpsc;

/// Writes an executable script from a child process, so no write fd of
/// this one leaks into a child another test forks ("Text file busy").
fn write_script(path: &Path, script: &str) {
    let written = Command::new("/bin/sh")
        .args([
            "-c",
            "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
            "sh",
        ])
        .arg(script)
        .arg(path)
        .status()
        .unwrap();
    assert!(written.success());
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(path).unwrap().trim().to_owned()
}

#[test]
fn a_delegate_works_in_its_staging_dir_but_cant_touch_the_users_files() {
    let user = tempfile::tempdir().unwrap();
    let diary = user.path().join("diary.txt");
    std::fs::write(&diary, "my diary").unwrap();
    let planted = user.path().join("planted");
    let bin = tempfile::tempdir().unwrap();
    let fake = bin.path().join("snooping-claude");
    write_script(
        &fake,
        &format!(
            "#!/bin/sh\n\
             case \"$1\" in --version) echo '2.1.323 (Claude Code)'; exit 0;; esac\n\
             cat '{diary}' > read 2>/dev/null; echo $? > read-status\n\
             echo leaked 2>/dev/null > '{planted}'; echo $? > write-status\n\
             echo \"$TMPDIR\" > tmpdir\n\
             echo ok > \"$TMPDIR/scratch\" && echo ok > wrote\n\
             touch done\n\
             while read -r line; do :; done\n",
            diary = diary.display(),
            planted = planted.display(),
        ),
    );
    let staging = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let provider = Provider {
        kind: Kind::Claude,
        model: None,
        allow_api_key: true,
    };
    let relay = Relay {
        program: "/usr/bin/true".into(),
        args: vec![],
    };
    let workspace = Workspace {
        dir: staging.path().to_owned(),
        tmp: tmp.path().to_owned(),
        writable: vec![],
        readable: vec![],
    };
    let (events, _events) = mpsc::unbounded_channel();

    let _delegate = Claude::new(fake, work.path().to_owned()).open_delegate(
        &provider,
        events,
        relay,
        String::new(),
        workspace,
    );

    let done = staging.path().join("done");
    wait_until("the Delegate never ran", || done.exists().then_some(()));
    assert_eq!(read(staging.path().join("wrote")), "ok");
    assert_eq!(
        read(staging.path().join("tmpdir")),
        tmp.path().display().to_string()
    );
    assert_ne!(read(staging.path().join("read-status")), "0");
    assert_eq!(read(staging.path().join("read")), "");
    assert_ne!(read(staging.path().join("write-status")), "0");
    assert!(!planted.exists());
}
