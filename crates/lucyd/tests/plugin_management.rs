//! Plugin management: enable, disable, rollback and remove by voice through
//! `manage_plugin`, and over IPC through `plugins.*` and `approvals.*`.

mod harness;

use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{error_code, Client, Event, PluginAuthor, PluginInfo, PluginStatus, State, Topic};
use rmcp::model::CallToolResult;
use serde_json::json;

/// The fixture Plugin `notes` by `author`, offering `echo` and, if `writes`,
/// `write_file`.
fn manifest(author: &str, writes: bool) -> String {
    let mut manifest = format!(
        "name = \"notes\"\n\
         version = \"0.1.0\"\n\
         author = \"{author}\"\n\
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
             handles = \"save a note to a file\"\n",
        );
    }
    manifest
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

/// lucyd with an open Session that has reached Listening.
fn in_a_session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    events
}

/// The model calls `manage_plugin`, and Lucy does it without asking.
fn manage(lucyd: &Harness, plugin: &str, action: &str) -> CallToolResult {
    lucyd
        .thinking
        .call_tool_with("manage_plugin", json!({"plugin": plugin, "action": action}))
        .unwrap()
}

/// The model calls `manage_plugin` in a Turn, in the background, for an
/// action Lucy asks about first.
fn manage_in_a_turn(
    lucyd: &Harness,
    events: &mut Client,
    plugin: &str,
    action: &str,
) -> JoinHandle<Result<CallToolResult, String>> {
    lucyd.say("please do that");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let thinking = lucyd.thinking.clone();
    let arguments = json!({"plugin": plugin, "action": action});
    std::thread::spawn(move || thinking.call_tool_with("manage_plugin", arguments))
}

fn offers(lucyd: &Harness, tool: &str) -> bool {
    lucyd.thinking.tool_names().iter().any(|name| name == tool)
}

fn plugin(lucyd: &Harness, name: &str) -> Option<PluginInfo> {
    let plugins = lucyd.client().plugins().unwrap().plugins;
    plugins.into_iter().find(|plugin| plugin.name == name)
}

fn approved_hash(lucyd: &Harness, name: &str) -> Option<String> {
    let approvals = lucyd.client().approvals().unwrap().approvals;
    let approval = approvals.into_iter().find(|a| a.plugin == name)?;
    Some(approval.hash)
}

/// Approves the Plugin `name` as it is now by the user's spoken yes, in the
/// open Session.
fn approve_by_voice(lucyd: &Harness, name: &str) {
    let approval = lucyd.request_approval(name);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    assert_eq!(approval.outcome(), Ok(true));
}

#[test]
fn manage_plugin_is_offered_to_the_session_model() {
    let lucyd = Harness::start();
    let _events = in_a_session(&lucyd);

    let tools = lucyd.thinking.tools();

    let tool = tools
        .iter()
        .find(|tool| tool.name == "manage_plugin")
        .expect("manage_plugin is offered");
    let actions = &tool.input_schema["properties"]["action"]["enum"];
    assert_eq!(*actions, json!(["enable", "disable", "rollback", "remove"]));
}

#[test]
fn disable_keeps_files_and_approval_and_enable_restores_straight_away() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let mut events = in_a_session(&lucyd);
    let watch = lucyd.thinking.watch_tools();
    assert!(offers(&lucyd, "notes__echo"));

    let result = manage(&lucyd, "notes", "disable");

    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    watch.wait_for_change();
    assert!(!offers(&lucyd, "notes__echo"));
    assert!(folder.join("manifest.toml").exists());
    assert!(approved_hash(&lucyd, "notes").is_some());
    let listed = plugin(&lucyd, "notes").unwrap();
    assert!(!listed.enabled);
    assert_eq!(listed.status, PluginStatus::Approved);
    // Still left out in the next Session.
    events.end_session().unwrap();
    events.start_session().unwrap();
    assert!(!offers(&lucyd, "notes__echo"));
    let watch = lucyd.thinking.watch_tools();

    let result = manage(&lucyd, "notes", "enable");

    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    watch.wait_for_change();
    assert!(offers(&lucyd, "notes__echo"));
    assert!(plugin(&lucyd, "notes").unwrap().enabled);
}

#[test]
fn remove_deletes_the_folder_and_the_approval_after_a_spoken_yes() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let mut events = in_a_session(&lucyd);
    let call = manage_in_a_turn(&lucyd, &mut events, "notes", "remove");

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Shall I remove the notes Plugin? Its files and its Approval will be deleted."
    );
    lucyd.tts.finish_playing();
    lucyd.say("yes");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert!(!folder.exists());
    assert_eq!(approved_hash(&lucyd, "notes"), None);
    assert_eq!(plugin(&lucyd, "notes"), None);
    assert!(!offers(&lucyd, "notes__echo"));
}

#[test]
fn anything_but_a_yes_keeps_the_plugin() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let mut events = in_a_session(&lucyd);
    let call = manage_in_a_turn(&lucyd, &mut events, "notes", "remove");
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("no, keep it");

    let said = text(&call.join().unwrap().unwrap());
    assert!(said.starts_with("declined"), "{said}");
    assert!(folder.join("manifest.toml").exists());
    assert!(approved_hash(&lucyd, "notes").is_some());
    assert!(offers(&lucyd, "notes__echo"));
}

#[test]
fn a_shipped_plugin_can_be_disabled_but_not_removed() {
    let lucyd = Harness::start();
    let folder = lucyd.install_shipped_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let _events = in_a_session(&lucyd);

    let result = manage(&lucyd, "notes", "remove");

    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("shipped"), "{}", text(&result));
    assert!(folder.exists());
    assert!(approved_hash(&lucyd, "notes").is_some());
    assert!(plugin(&lucyd, "notes").unwrap().shipped);
    let result = manage(&lucyd, "notes", "disable");
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert!(!offers(&lucyd, "notes__echo"));
}

#[test]
fn removing_a_user_copy_leaves_the_shipped_plugin_it_overrode_approved() {
    let lucyd = Harness::start();
    lucyd.install_shipped_plugin("notes", &manifest("human", false));
    let user_copy = lucyd.install_plugin("notes", &manifest("human", true));
    lucyd.approve("notes");
    let mut events = in_a_session(&lucyd);
    assert!(offers(&lucyd, "notes__write_file"));
    let call = manage_in_a_turn(&lucyd, &mut events, "notes", "remove");
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert!(!user_copy.exists());
    events.end_session().unwrap();

    events.start_session().unwrap();

    assert!(offers(&lucyd, "notes__echo"));
    assert!(!offers(&lucyd, "notes__write_file"));
    let listed = plugin(&lucyd, "notes").unwrap();
    assert!(listed.shipped);
    assert_eq!(listed.status, PluginStatus::Approved);
}

#[test]
fn rollback_restores_the_previous_approved_version_with_no_prompt() {
    let lucyd = Harness::start();
    let first = manifest("lucy", false);
    let folder = lucyd.install_plugin("notes", &first);
    let _events = in_a_session(&lucyd);
    approve_by_voice(&lucyd, "notes");
    let first_hash = approved_hash(&lucyd, "notes").unwrap();
    std::fs::write(folder.join("manifest.toml"), manifest("lucy", true)).unwrap();
    approve_by_voice(&lucyd, "notes");
    assert!(offers(&lucyd, "notes__write_file"));
    let spoken = lucyd.tts.sentences().len();
    let watch = lucyd.thinking.watch_tools();

    let result = manage(&lucyd, "notes", "rollback");

    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    watch.wait_for_change();
    assert!(offers(&lucyd, "notes__echo"));
    assert!(!offers(&lucyd, "notes__write_file"));
    assert_eq!(
        std::fs::read_to_string(folder.join("manifest.toml")).unwrap(),
        first
    );
    assert_eq!(approved_hash(&lucyd, "notes"), Some(first_hash));
    // Nothing was asked.
    assert_eq!(lucyd.tts.sentences().len(), spoken);
    let echoed = lucyd
        .thinking
        .call_tool_with("notes__echo", json!({"text": "still here"}))
        .unwrap();
    assert_eq!(text(&echoed), "still here");
}

#[test]
fn rollback_needs_a_kept_previous_version() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("lucy", false));
    lucyd.approve("notes");
    let _events = in_a_session(&lucyd);

    let result = manage(&lucyd, "notes", "rollback");

    assert_eq!(result.is_error, Some(true));
    assert!(offers(&lucyd, "notes__echo"));
}

#[test]
fn lucy_never_rolls_back_a_human_written_plugin() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("notes", &manifest("human", false));
    let _events = in_a_session(&lucyd);
    approve_by_voice(&lucyd, "notes");
    let changed = manifest("human", true);
    std::fs::write(folder.join("manifest.toml"), &changed).unwrap();
    approve_by_voice(&lucyd, "notes");

    let result = manage(&lucyd, "notes", "rollback");

    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("human"), "{}", text(&result));
    assert_eq!(
        std::fs::read_to_string(folder.join("manifest.toml")).unwrap(),
        changed
    );
    assert!(offers(&lucyd, "notes__write_file"));
}

#[test]
fn a_revoke_over_ipc_removes_the_tools_mid_session() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let _events = in_a_session(&lucyd);
    let watch = lucyd.thinking.watch_tools();
    assert!(offers(&lucyd, "notes__echo"));

    lucyd.client().revoke_approval("notes").unwrap();

    watch.wait_for_change();
    assert!(!offers(&lucyd, "notes__echo"));
    assert_eq!(approved_hash(&lucyd, "notes"), None);
    assert_eq!(
        plugin(&lucyd, "notes").unwrap().status,
        PluginStatus::AwaitingApproval
    );
}

#[test]
fn revoking_a_plugin_with_no_approval_is_refused() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("human", false));

    let refused = lucyd.client().revoke_approval("notes");

    assert!(
        matches!(refused, Err(lucy_ipc::Error::Rpc { code, .. }) if code == error_code::NOT_APPROVED),
        "{refused:?}"
    );
}

#[test]
fn set_enabled_over_ipc_disables_and_enables_mid_session() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let _events = in_a_session(&lucyd);
    let watch = lucyd.thinking.watch_tools();
    let mut client = lucyd.client();

    client.set_plugin_enabled("notes", false).unwrap();
    watch.wait_for_change();
    assert!(!offers(&lucyd, "notes__echo"));

    client.set_plugin_enabled("notes", true).unwrap();
    watch.wait_for_change();
    assert!(offers(&lucyd, "notes__echo"));

    let unknown = client.set_plugin_enabled("nothing", false);
    assert!(
        matches!(unknown, Err(lucy_ipc::Error::Rpc { code, .. }) if code == error_code::UNKNOWN_PLUGIN),
        "{unknown:?}"
    );
}

#[test]
fn plugins_list_says_what_each_plugin_is() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("lucy", false));
    lucyd.approve("notes");
    lucyd.install_plugin("broken", "name = \"broken\"\n");

    let plugins = lucyd.client().plugins().unwrap().plugins;

    let notes = plugins.iter().find(|p| p.name == "notes").unwrap();
    assert_eq!(notes.version.as_deref(), Some("0.1.0"));
    assert_eq!(notes.author, Some(PluginAuthor::Lucy));
    assert_eq!(notes.summary.as_deref(), Some("Keeps notes"));
    assert!(notes.enabled && !notes.shipped);
    assert_eq!(notes.status, PluginStatus::Approved);
    assert_eq!(notes.problem, None);
    let broken = plugins.iter().find(|p| p.name == "broken").unwrap();
    assert_eq!(broken.status, PluginStatus::Broken);
    assert!(broken.problem.is_some());
}

#[test]
fn each_change_is_sent_to_plugins_subscribers() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("human", false));
    lucyd.approve("notes");
    let mut subscriber = lucyd.client();
    subscriber.subscribe(&[Topic::Plugins]).unwrap();
    let mut client = lucyd.client();

    client.set_plugin_enabled("notes", false).unwrap();
    client.set_plugin_enabled("notes", true).unwrap();
    client.revoke_approval("notes").unwrap();

    for _ in 0..3 {
        assert!(matches!(
            subscriber.next_event().unwrap(),
            Event::PluginsChanged(_)
        ));
    }
}

#[test]
fn a_hand_installed_plugin_is_mentioned_once_at_the_next_session() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &manifest("human", false));
    assert_eq!(
        plugin(&lucyd, "notes").unwrap().status,
        PluginStatus::AwaitingApproval
    );
    let mut client = lucyd.client();

    client.start_session().unwrap();
    let instructions = lucyd.thinking.instructions();
    assert!(
        instructions.contains("The notes Plugin (Keeps notes) is awaiting the user's Approval."),
        "{instructions}"
    );
    client.end_session().unwrap();

    client.start_session().unwrap();
    let instructions = lucyd.thinking.instructions();
    assert!(!instructions.contains("notes"), "{instructions}");
}
