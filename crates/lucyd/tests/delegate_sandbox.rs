//! An Authoring Delegate runs under Landlock, confined to its staging dir.
//! It can't read or write the user's files. Driven through the real
//! `claude` adapter's Delegate spawn against a fake `claude` that tries.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};

use harness::{wait_until, write_script};
use lucy_config::{Kind, Provider};
use lucyd::adapters::{Relay, SessionModel, Thinking, Workspace};
use lucyd::claude::Claude;
use tokio::sync::mpsc;

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
        program: None,
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

fn delegate(program: PathBuf, provider: &Provider, staging: &Path) -> Box<dyn SessionModel> {
    let tmp = staging.join(".tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let relay = Relay {
        program: "/usr/bin/true".into(),
        args: vec![],
    };
    let workspace = Workspace {
        dir: staging.to_owned(),
        tmp,
        writable: vec![],
        readable: vec![],
    };
    let (events, _events) = mpsc::unbounded_channel();
    Claude::new(program, staging.join(".work")).open_delegate(
        provider,
        events,
        relay,
        String::new(),
        workspace,
    )
}

fn provider(program: Option<PathBuf>) -> Provider {
    Provider {
        kind: Kind::Claude,
        model: None,
        allow_api_key: true,
        program,
    }
}

fn real_claude(dir: &Path, peek: &Path) -> PathBuf {
    let real = dir.join("claude");
    write_script(
        &real,
        &format!(
            "#!/bin/sh\n\
             case \"$1\" in --version) echo '2.1.323 (Claude Code)'; exit 0;; esac\n\
             cat '{peek}' > peeked 2>/dev/null; echo $? > peek-status\n\
             touch ran\n\
             while read -r line; do :; done\n",
            peek = peek.display(),
        ),
    );
    real
}

#[test]
fn a_delegate_reached_through_a_mise_shim_runs_the_real_binary_with_only_its_folder_readable() {
    let mise_dir = tempfile::tempdir().unwrap();
    let mise_config = mise_dir.path().join("config.toml");
    std::fs::write(&mise_config, "mise's own config").unwrap();
    let real_dir = tempfile::tempdir().unwrap();
    let real = real_claude(real_dir.path(), &mise_config);
    write_script(
        &mise_dir.path().join("mise"),
        &format!(
            "#!/bin/sh\n\
             if [ \"$1 $2\" = 'which claude' ]; then echo '{real}'; exit 0; fi\n\
             cat '{config}' > /dev/null || exit 1\n\
             exec '{real}' \"$@\"\n",
            real = real.display(),
            config = mise_config.display(),
        ),
    );
    let shims = tempfile::tempdir().unwrap();
    let shim = shims.path().join("claude");
    std::os::unix::fs::symlink(mise_dir.path().join("mise"), &shim).unwrap();
    let staging = tempfile::tempdir().unwrap();

    let _delegate = delegate(shim, &provider(None), staging.path());

    let ran = staging.path().join("ran");
    wait_until("the real claude never ran", || ran.exists().then_some(()));
    assert_ne!(read(staging.path().join("peek-status")), "0");
    assert_eq!(read(staging.path().join("peeked")), "");
}

#[test]
fn a_providers_program_is_run_as_given() {
    let real_dir = tempfile::tempdir().unwrap();
    let real = real_claude(real_dir.path(), Path::new("/nonexistent"));
    let staging = tempfile::tempdir().unwrap();

    let _delegate = delegate(
        "/nonexistent/claude".into(),
        &provider(Some(real)),
        staging.path(),
    );

    let ran = staging.path().join("ran");
    wait_until("the Provider's program never ran", || {
        ran.exists().then_some(())
    });
}
