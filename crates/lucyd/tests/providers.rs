//! `providers.status` and `provider.check` on the public socket, for the
//! TUI's Provider status page.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness::{write_script, Harness};
use lucy_ipc::{error_code, Error, ProviderKind, ProviderStatus, Tier};
use lucyd::claude::Claude;

const TWO_PROVIDERS: &str = r#"
[providers.quick]
kind = "claude"
model = "haiku"

[providers.deep]
kind = "claude"
model = "opus"

[tiers]
fast = "quick"
advanced = "deep"
"#;

#[test]
fn providers_status_lists_each_provider_with_its_tier_and_health() {
    let lucyd = Harness::with_config(TWO_PROVIDERS);
    lucyd
        .thinking
        .unhealthy("opus", "Claude Code isn't signed in.");
    let mut client = lucyd.client();

    let status = client.providers_status().unwrap();

    assert_eq!(
        status.providers,
        [
            ProviderStatus {
                name: "deep".into(),
                kind: ProviderKind::Claude,
                model: Some("opus".into()),
                tiers: vec![Tier::Advanced],
                problem: Some("Claude Code isn't signed in.".into()),
            },
            ProviderStatus {
                name: "quick".into(),
                kind: ProviderKind::Claude,
                model: Some("haiku".into()),
                tiers: vec![Tier::Fast],
                problem: None,
            },
        ]
    );
}

#[test]
fn provider_check_reports_the_round_trip() {
    let lucyd = Harness::with_config(TWO_PROVIDERS);
    lucyd
        .thinking
        .fails_check("haiku", "Claude Code couldn't answer a test message.");
    let mut client = lucyd.client();

    assert_eq!(client.check_provider("deep").unwrap().problem, None);
    assert_eq!(
        client.check_provider("quick").unwrap().problem.as_deref(),
        Some("Claude Code couldn't answer a test message.")
    );
}

#[test]
fn checking_a_provider_not_in_config_is_an_error_naming_it() {
    let lucyd = Harness::with_config(TWO_PROVIDERS);
    let mut client = lucyd.client();

    let checked = client.check_provider("nope");

    assert!(
        matches!(&checked, Err(Error::Rpc { code, message })
            if *code == error_code::UNKNOWN_PROVIDER && message.contains("nope")),
        "{checked:?}"
    );
}

#[test]
fn a_config_that_does_not_parse_is_an_error_naming_the_file() {
    let lucyd = Harness::with_config("this is = = not toml");
    let mut client = lucyd.client();

    let status = client.providers_status();

    assert!(
        matches!(&status, Err(Error::Rpc { code, message })
            if *code == error_code::INVALID_CONFIG && message.contains("config.toml")),
        "{status:?}"
    );
}

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude/fake-claude")
        .canonicalize()
        .unwrap()
}

#[test]
fn provider_check_names_the_real_binary_behind_a_mise_shim() {
    let bin = tempfile::tempdir().unwrap();
    let mise = bin.path().join("mise");
    write_script(
        &mise,
        &format!(
            "#!/bin/sh\n[ \"$1 $2\" = 'which claude' ] && echo '{}'\n",
            fake_claude().display()
        ),
    );
    let shim = bin.path().join("claude");
    std::os::unix::fs::symlink(&mise, &shim).unwrap();
    let claude = Claude::new(shim, bin.path().join("work"));
    let lucyd = Harness::with_thinking(Some(TWO_PROVIDERS), Arc::new(claude));

    let checked = lucyd.client().check_provider("quick").unwrap();

    assert_eq!(checked.program, Some(fake_claude().display().to_string()));
}

#[test]
fn provider_check_names_a_providers_program_as_given() {
    let work = tempfile::tempdir().unwrap();
    let config = format!(
        "[providers.quick]\nkind = \"claude\"\nprogram = \"{}\"\n",
        fake_claude().display()
    );
    let claude = Claude::new("/nonexistent/claude".into(), work.path().to_owned());
    let lucyd = Harness::with_thinking(Some(&config), Arc::new(claude));

    let checked = lucyd.client().check_provider("quick").unwrap();

    assert_eq!(checked.program, Some(fake_claude().display().to_string()));
}
