//! Approval: Lucy asks the user's spoken yes to a new or changed Plugin, and
//! hears it herself. Nothing else can grant one.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::adapters::NotificationKind;

/// The fixture Plugin offering `echo`, plus `write_file` (which needs
/// Confirmation) if `writes`, with `access` as its `[access]` body.
fn manifest(writes: bool, access: &str) -> String {
    let mut manifest = format!(
        "name = \"notes\"\n\
         version = \"0.1.0\"\n\
         author = \"human\"\n\
         summary = \"Keeps notes\"\n\
         command = [\"./fixture-plugin\", \"echo\"{}]\n\
         \n\
         [[tools]]\n\
         name = \"echo\"\n\
         handles = \"repeat a note\"\n",
        if writes { ", \"write_file\"" } else { "" }
    );
    if writes {
        manifest.push_str(
            "\n[[tools]]\n\
             name = \"write_file\"\n\
             handles = \"save a note to a file\"\n\
             confirm = true\n",
        );
    }
    manifest.push_str(&format!("\n[access]\n{access}\n"));
    manifest
}

/// lucyd with an open Session that has reached Listening.
fn in_a_session(lucyd: &Harness) -> lucy_ipc::Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    events
}

#[test]
fn a_new_plugin_gets_the_full_summary_and_a_yes_offers_its_tools_straight_away() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin(
        "notes",
        &manifest(
            true,
            "read = [\"~/Notes\"]\nnetwork = [\"notes.example.com\"]",
        ),
    );
    let mut events = in_a_session(&lucyd);
    let watch = lucyd.thinking.watch_tools();
    assert!(!lucyd.thinking.tool_names().contains(&"notes__echo".into()));

    let approval = lucyd.request_approval("notes");

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "The notes Plugin needs your Approval. It handles repeat a note and save a note to a \
         file. It can read ~/Notes and reach notes.example.com. It asks you before it will \
         save a note to a file. Do you approve it?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("yes");

    assert_eq!(approval.outcome(), Ok(true));
    watch.wait_for_change();
    let tools = lucyd.thinking.tool_names();
    assert!(tools.contains(&"notes__echo".into()), "{tools:?}");
    assert!(tools.contains(&"notes__write_file".into()), "{tools:?}");
    assert_eq!(next_state(&mut events).state, State::Listening);
    // Recorded in Lucy's own state, never in the Plugin's folder.
    assert!(lucyd.state_dir().join("approvals.toml").exists());
    assert!(!folder.join("approvals.toml").exists());
    events.end_session().unwrap();
    events.start_session().unwrap();
    assert!(lucyd.thinking.tool_names().contains(&"notes__echo".into()));
}

#[test]
fn anything_but_a_yes_leaves_the_plugin_unapproved() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest(false, ""));
    let mut events = in_a_session(&lucyd);
    let approval = lucyd.request_approval("notes");
    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert!(lucyd
        .tts
        .next_sentence()
        .contains("It needs no access to your files, the network or other programs."));
    lucyd.tts.finish_playing();

    lucyd.say("not now");

    assert_eq!(approval.outcome(), Ok(false));
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
    assert!(!lucyd.thinking.tool_names().contains(&"notes__echo".into()));
    assert!(!lucyd.state_dir().join("approvals.toml").exists());
}

#[test]
fn an_update_gets_a_changes_only_summary_with_the_full_diff_on_screen() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("notes", &manifest(false, "read = [\"~/Notes\"]"));
    let mut events = in_a_session(&lucyd);
    let first = lucyd.request_approval("notes");
    lucyd.tts.next_sentence();
    lucyd.say("yes");
    assert_eq!(first.outcome(), Ok(true));
    events.end_session().unwrap();
    std::fs::write(
        folder.join("manifest.toml"),
        manifest(true, "read = [\"~/Documents\"]"),
    )
    .unwrap();
    // The changed Plugin isn't offered until it is approved again.
    events.start_session().unwrap();
    assert!(!lucyd.thinking.tool_names().contains(&"notes__echo".into()));

    let update = lucyd.request_approval("notes");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "The notes Plugin has changed and needs your Approval again. It now handles save a \
         note to a file. It can now read ~/Documents. It can no longer read ~/Notes. The full \
         diff is on screen. Do you approve the update?"
    );
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::Details);
    assert_eq!(shown.summary, "Changes to the notes Plugin");
    assert!(
        shown.body.contains("- read = [\"~/Notes\"]")
            && shown.body.contains("+ read = [\"~/Documents\"]")
            && shown.body.contains("+ name = \"write_file\"")
            && shown.body.contains("  name = \"echo\""),
        "{}",
        shown.body
    );
    lucyd.say("yep");
    assert_eq!(update.outcome(), Ok(true));
    assert!(lucyd
        .thinking
        .tool_names()
        .contains(&"notes__write_file".into()));
}

#[test]
fn a_shipped_plugin_is_offered_in_the_first_session_without_asking() {
    let lucyd = Harness::start();
    lucyd.install_shipped_plugin("notes", &manifest(false, ""));

    let _events = in_a_session(&lucyd);

    assert!(lucyd.thinking.tool_names().contains(&"notes__echo".into()));
    let instructions = lucyd.thinking.instructions();
    assert!(!instructions.contains("awaiting"), "{instructions}");
    assert_eq!(lucyd.tts.sentences(), Vec::<String>::new());
    let approvals = lucyd.client().approvals().unwrap().approvals;
    assert!(
        approvals.iter().any(|a| a.plugin == "notes"),
        "{approvals:?}"
    );
}

#[test]
fn an_approval_needs_an_open_session_to_ask_in() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest(false, ""));

    let outcome = lucyd.request_approval("notes").outcome();

    assert!(outcome.is_err(), "{outcome:?}");
    assert!(!lucyd.state_dir().join("approvals.toml").exists());
    // The Plugin wasn't started: starting it makes its state dir.
    assert!(!lucyd.state_dir().join("plugins").join("notes").exists());
}

#[test]
fn an_update_to_a_plugin_approved_without_its_manifest_kept_gets_the_full_summary() {
    let lucyd = Harness::start();
    let _indicator = lucyd.indicator("lucy-indicator");
    let folder = lucyd.install_plugin("notes", &manifest(false, ""));
    // Approved before Approvals kept the Manifest.
    lucyd.approve("notes");
    std::fs::write(folder.join("manifest.toml"), manifest(true, "")).unwrap();
    let _events = in_a_session(&lucyd);

    let _update = lucyd.request_approval("notes");

    assert!(lucyd
        .tts
        .next_sentence()
        .starts_with("The notes Plugin needs your Approval. It handles repeat a note and save"));
    // No diff against nothing on screen.
    lucyd.notifier.assert_no_notification();
}

#[test]
fn no_model_tool_and_no_ipc_request_can_grant_an_approval() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest(false, ""));
    let _events = in_a_session(&lucyd);

    let tools = lucyd.thinking.tool_names();
    assert!(
        !tools.iter().any(|tool| tool.contains("approv")),
        "{tools:?}"
    );
    let schema = lucy_ipc::schema();
    let methods: Vec<&String> = schema["x-methods"].as_object().unwrap().keys().collect();
    assert!(
        !methods.iter().any(|method| method.contains("approv")
            && !method.ends_with(".list")
            && !method.ends_with(".revoke")),
        "{methods:?}"
    );
}
