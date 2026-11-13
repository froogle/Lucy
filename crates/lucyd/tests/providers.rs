//! `providers.status` and `provider.check` on the public socket, for the
//! TUI's Provider status page.

mod harness;

use harness::Harness;
use lucy_ipc::{error_code, Error, ProviderStatus, Tier};

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
                kind: "claude".into(),
                model: Some("opus".into()),
                tiers: vec![Tier::Advanced],
                problem: Some("Claude Code isn't signed in.".into()),
            },
            ProviderStatus {
                name: "quick".into(),
                kind: "claude".into(),
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
