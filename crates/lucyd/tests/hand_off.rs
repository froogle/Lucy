//! Hand-off: "tell Claude to …" passes the request to the user's own Claude.
//! The Hand-off here is scripted: the test reads its Turn, calls the
//! permission tool through its real relay and ends its Turn with an answer.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::Path;
use std::time::Duration;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use lucyd::adapters::NotificationKind;
use rmcp::model::CallToolResult;
use serde_json::json;

const REQUEST: &str = "list my Linear tickets";
const CONVERSATION: &str = "5c1e0d2a-hand-off";
const ANSWER: &str = "You have three open tickets: LUC-1, LUC-2 and LUC-3. LUC-2 is due today.";

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn called(result: Result<CallToolResult, String>) -> (bool, String) {
    let result = result.unwrap();
    (result.is_error == Some(true), text(&result))
}

fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

fn hand_off(lucyd: &Harness, arguments: serde_json::Value) -> (Client, (bool, String)) {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say(&format!("tell Claude to {REQUEST}"));
    lucyd.thinking.next_turn();
    until_state(&mut events, State::Thinking);
    let called = called(lucyd.thinking.call_tool_with("hand_to_claude", arguments));
    (events, called)
}

fn handed_off(lucyd: &Harness, folder: &Path) -> Client {
    let arguments = json!({"request": REQUEST, "folder": folder.to_str().unwrap()});
    let (mut events, (failed, said)) = hand_off(lucyd, arguments);
    assert!(!failed, "{said}");
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    lucyd.thinking.hand_off.hand_off();
    events
}

#[test]
fn tell_claude_hands_the_request_to_claude_in_the_folder_given() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();

    let _events = handed_off(&lucyd, &folder);

    assert_eq!(lucyd.thinking.hand_off.next_turn(), REQUEST);
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, folder);
}

#[test]
fn when_claude_finishes_the_fast_tier_sums_it_up_and_the_whole_answer_is_notified() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    lucyd.thinking.hand_off.next_turn();

    lucyd.thinking.hand_off.answer(CONVERSATION, ANSWER);

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude has finished");
    assert!(shown.body.starts_with(ANSWER), "{}", shown.body);
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );

    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains(ANSWER), "{turn}");
    assert!(turn.contains("one to three"), "{turn}");
    lucyd
        .thinking
        .delta("Claude found three tickets, and one is due today.");
    lucyd.thinking.sentence_end();
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude found three tickets, and one is due today."
    );
}

#[test]
fn every_permission_request_is_denied_and_the_permission_tool_is_all_claude_sees() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;

    assert_eq!(hand_off.tool_names(), ["permission"]);
    let (failed, decision) = called(hand_off.call_tool_with(
        "permission",
        json!({
            "tool_name": "Bash",
            "input": {"command": "rm -rf build"},
            "tool_use_id": "toolu_1",
        }),
    ));

    assert!(!failed, "{decision}");
    let decision: serde_json::Value = serde_json::from_str(&decision).unwrap();
    assert_eq!(decision["behavior"], "deny");
    assert!(decision["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert!(lucyd.tts.sentences().is_empty(), "nothing was asked aloud");
}

#[test]
fn a_request_that_names_no_folder_runs_in_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let lucyd = Harness::with_env(&[("HOME", home.path().to_str().unwrap())]);

    let (_events, (failed, said)) = hand_off(&lucyd, json!({"request": REQUEST}));

    assert!(!failed, "{said}");
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, home.path());
}

#[test]
fn a_tilde_path_is_taken_from_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("lucy")).unwrap();
    let lucyd = Harness::with_env(&[("HOME", home.path().to_str().unwrap())]);

    let arguments = json!({"request": REQUEST, "folder": "~/lucy"});
    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(!failed, "{said}");
    let folder = lucyd.thinking.hand_off.hand_off().folder;
    assert_eq!(folder, home.path().join("lucy"));
}

#[test]
fn a_bare_folder_name_is_refused_with_say_the_full_path() {
    let lucyd = Harness::start();

    let arguments = json!({"request": REQUEST, "folder": "lucy"});
    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(failed);
    assert!(said.contains("full path"), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

#[test]
fn a_folder_that_isnt_there_is_refused() {
    let lucyd = Harness::start();
    let missing = lucyd.scratch().join("no-such-folder");

    let arguments = json!({"request": REQUEST, "folder": missing.to_str().unwrap()});
    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(failed);
    assert!(said.contains("no folder"), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

#[test]
fn the_session_stays_open_in_listening_while_claude_works() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 30\n");
    let _events = handed_off(&lucyd, &lucyd.scratch());

    lucyd.clock.advance(Duration::from_secs(60));

    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
}

#[test]
fn a_second_hand_off_is_refused_while_one_runs() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();

    lucyd.say("tell Claude to check my email");
    lucyd.thinking.next_turn();
    let arguments = json!({"request": "check my email"});
    let (failed, said) = called(lucyd.thinking.call_tool_with("hand_to_claude", arguments));

    assert!(failed);
    assert!(said.contains("still working"), "{said}");
    lucyd.thinking.hand_off.assert_no_turn();
}

#[test]
fn ending_the_session_stops_claude_with_the_resume_command_notified() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);

    lucyd.client().end_session().unwrap();

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Lucy stopped Claude");
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
}

#[test]
fn nothing_about_the_hand_off_reaches_the_public_socket() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch().join("linear-work");
    std::fs::create_dir(&folder).unwrap();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    called(hand_off.call_tool_with(
        "permission",
        json!({"tool_name": "mcp__linear__list_issues", "input": {"team": "LUC"}}),
    ));
    hand_off.answer(CONVERSATION, ANSWER);
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("Claude found three tickets.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.client().end_session().unwrap();

    lucyd.assert_nothing_private_on_bus();
    lucyd.assert_not_on_bus(&[
        REQUEST,
        "linear-work",
        "mcp__linear__list_issues",
        "LUC-2",
        CONVERSATION,
    ]);
}
