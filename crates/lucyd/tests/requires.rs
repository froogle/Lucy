//! A Manifest's `requires`: where a Plugin applies. A Plugin whose required
//! env vars or commands lucyd's environment lacks is left out quietly, never
//! called broken or mentioned, and listed as not for this desktop.

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::{PluginInfo, PluginStatus, State};
use serde_json::json;

/// The fixture Plugin `desk`, offering `echo` and `env`, with `requires` as
/// its `requires` value.
fn manifest(requires: &str) -> String {
    format!(
        "name = \"desk\"\n\
         version = \"0.1.0\"\n\
         author = \"human\"\n\
         summary = \"Arranges the desk\"\n\
         command = [\"./fixture-plugin\", \"echo\", \"env\"]\n\
         requires = {requires}\n\
         \n\
         [[tools]]\n\
         name = \"echo\"\n\
         handles = \"repeat a word\"\n\
         \n\
         [[tools]]\n\
         name = \"env\"\n\
         handles = \"note its environment\"\n"
    )
}

fn listed(lucyd: &Harness) -> PluginInfo {
    let plugins = lucyd.client().plugins().unwrap().plugins;
    plugins.into_iter().find(|p| p.name == "desk").unwrap()
}

/// Opens a Session and returns whether its model is offered `desk__echo`.
fn offered_in_a_session(lucyd: &Harness) -> bool {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    let offered = lucyd.thinking.tool_names().contains(&"desk__echo".into());
    events.end_session().unwrap();
    offered
}

#[test]
fn a_plugin_whose_required_env_var_is_unset_is_left_out_unmentioned_and_listed_as_not_for_this_desktop(
) {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "desk",
        &manifest(r#"{ env = ["FIXTURE_DESK_SURELY_UNSET"] }"#),
    );
    lucyd.approve("desk");

    assert!(!offered_in_a_session(&lucyd));

    let instructions = lucyd.thinking.instructions();
    assert!(!instructions.contains("desk"), "{instructions}");
    let desk = listed(&lucyd);
    assert_eq!(desk.status, PluginStatus::NotForThisDesktop);
    assert_eq!(desk.problem, None);
    assert_eq!(desk.summary.as_deref(), Some("Arranges the desk"));
}

#[test]
fn a_plugin_whose_required_command_is_not_on_path_is_left_out() {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "desk",
        &manifest(r#"{ commands = ["fixture-desk-surely-missing"] }"#),
    );
    lucyd.approve("desk");

    assert!(!offered_in_a_session(&lucyd));
    assert_eq!(listed(&lucyd).status, PluginStatus::NotForThisDesktop);
}

#[test]
fn an_unmet_plugin_awaiting_approval_is_never_mentioned() {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "desk",
        &manifest(r#"{ env = ["FIXTURE_DESK_SURELY_UNSET"] }"#),
    );

    offered_in_a_session(&lucyd);

    let instructions = lucyd.thinking.instructions();
    assert!(!instructions.contains("desk"), "{instructions}");
}

#[test]
fn an_unmet_shipped_plugin_is_not_approved_at_scan() {
    let lucyd = Harness::start();
    lucyd.install_shipped_plugin(
        "desk",
        &manifest(r#"{ env = ["FIXTURE_DESK_SURELY_UNSET"] }"#),
    );

    assert!(!offered_in_a_session(&lucyd));

    assert!(lucyd.client().approvals().unwrap().approvals.is_empty());
}

#[test]
fn a_met_plugin_is_offered_and_gets_the_env_vars_it_requires() {
    let lucyd = Harness::with_env(&[("FIXTURE_DESK", "oak")]);
    lucyd.install_plugin(
        "desk",
        &manifest(r#"{ env = ["FIXTURE_DESK"], commands = ["sh"] }"#),
    );
    lucyd.approve("desk");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    let result = lucyd
        .thinking
        .call_tool_with("desk__env", json!({}))
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    let env = std::fs::read_to_string(lucyd.state_dir().join("plugins/desk/env")).unwrap();
    assert!(env.lines().any(|line| line == "FIXTURE_DESK=oak"), "{env}");
    assert_eq!(listed(&lucyd).status, PluginStatus::Approved);
}

#[test]
fn changing_requires_needs_a_fresh_approval() {
    let lucyd = Harness::with_env(&[("FIXTURE_DESK", "oak")]);
    let folder = lucyd.install_plugin("desk", &manifest(r#"{ env = ["FIXTURE_DESK"] }"#));
    lucyd.approve("desk");

    std::fs::write(
        folder.join("manifest.toml"),
        manifest(r#"{ env = ["FIXTURE_DESK"], commands = ["sh"] }"#),
    )
    .unwrap();

    assert!(!offered_in_a_session(&lucyd));
    assert_eq!(listed(&lucyd).status, PluginStatus::AwaitingApproval);
}

#[test]
fn a_manifest_requiring_an_env_var_lucy_sets_or_a_command_path_is_broken() {
    let lucyd = Harness::start();
    for requires in [
        r#"{ env = ["LUCY_PLUGIN_STATE_DIR"] }"#,
        r#"{ env = ["LD_PRELOAD"] }"#,
        r#"{ commands = ["/usr/bin/sh"] }"#,
    ] {
        lucyd.install_plugin("desk", &manifest(requires));

        assert_eq!(listed(&lucyd).status, PluginStatus::Broken, "{requires}");
    }
}

#[test]
fn the_approval_summary_says_which_env_vars_the_plugin_will_see() {
    let lucyd = Harness::with_env(&[("FIXTURE_DESK", "oak")]);
    lucyd.install_plugin("desk", &manifest(r#"{ env = ["FIXTURE_DESK"] }"#));
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    let _approval = lucyd.request_approval("desk");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "The desk Plugin needs your Approval. It handles repeat a word and note its \
         environment. It can see the environment variables FIXTURE_DESK. It never asks you \
         before acting. Do you approve it?"
    );
}
