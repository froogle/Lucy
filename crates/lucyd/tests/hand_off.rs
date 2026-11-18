//! Hand-off: "tell Claude to …" passes the request to the user's own Claude.
//! The Hand-off here is scripted: the test reads its Turn, calls the
//! permission tool through its real relay and ends its Turn with an answer.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::Path;
use std::thread::JoinHandle;
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

fn asks_permission(
    lucyd: &Harness,
    request: serde_json::Value,
) -> JoinHandle<Result<CallToolResult, String>> {
    let hand_off = lucyd.thinking.hand_off.clone();
    std::thread::spawn(move || hand_off.call_tool_with("permission", request))
}

fn run_the_tests() -> serde_json::Value {
    json!({
        "tool_name": "Bash",
        "input": {"command": "cargo test -p lucyd", "description": "Run lucyd's tests."},
        "tool_use_id": "toolu_1",
    })
}

fn decision(call: JoinHandle<Result<CallToolResult, String>>) -> serde_json::Value {
    let (failed, decision) = called(call.join().unwrap());
    assert!(!failed, "{decision}");
    serde_json::from_str(&decision).unwrap()
}

fn assert_denied(decision: &serde_json::Value) {
    assert_eq!(decision["behavior"], "deny", "{decision}");
    assert!(
        decision["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{decision}"
    );
}

#[test]
fn a_permission_request_is_asked_with_claudes_description_and_the_full_input_notified() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();

    let call = asks_permission(&lucyd, run_the_tests());

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude wants to run lucyd's tests. Okay?"
    );
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::Details);
    assert!(
        shown.body.contains("Bash") && shown.body.contains("cargo test -p lucyd"),
        "{}",
        shown.body
    );
    lucyd.tts.finish_playing();
    lucyd.say("Yes.");

    let decision = decision(call);
    assert_eq!(decision["behavior"], "allow", "{decision}");
    assert_eq!(
        decision["updatedInput"],
        json!({"command": "cargo test -p lucyd", "description": "Run lucyd's tests."})
    );
}

#[test]
fn a_request_with_no_description_names_the_tool() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();

    let call = asks_permission(
        &lucyd,
        json!({"tool_name": "mcp__linear__create_issue", "input": {"title": "Fix it"}}),
    );

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude wants to use linear create issue. Okay?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("no");
    assert_denied(&decision(call));
}

#[test]
fn a_no_denies_the_request() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("no, don't");

    assert_denied(&decision(call));
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
}

#[test]
fn silence_denies_the_request() {
    let lucyd = Harness::with_config("[confirmation]\ntimeout_secs = 2\n");
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.barrier();

    lucyd.clock.advance(Duration::from_secs(2));

    assert_denied(&decision(call));
}

#[test]
fn an_indicator_click_while_asked_denies_and_ends_the_session() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    assert_eq!(next_state(&mut events).state, State::Confirming);

    lucyd.client().end_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Idle);
    let unless_the_relay_was_cut_off = call.join().unwrap();
    if let Ok(result) = unless_the_relay_was_cut_off {
        let decision: serde_json::Value = serde_json::from_str(&text(&result)).unwrap();
        assert_denied(&decision);
    }
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

fn hand_to_claude(lucyd: &Harness, arguments: serde_json::Value) -> (bool, String) {
    called(lucyd.thinking.call_tool_with("hand_to_claude", arguments))
}

#[test]
fn a_second_request_offers_to_queue_and_a_third_is_refused_while_one_waits() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    let (failed, offer) = hand_to_claude(&lucyd, json!({"request": "check my email"}));
    assert!(failed, "nothing starts before the user says to queue it");
    assert!(offer.contains(REQUEST), "{offer}");
    assert!(offer.contains("want me to queue this?"), "{offer}");

    let (failed, queued) =
        hand_to_claude(&lucyd, json!({"request": "check my email", "queue": true}));
    assert!(!failed, "{queued}");
    let (failed, refused) =
        hand_to_claude(&lucyd, json!({"request": "tidy my desktop", "queue": true}));
    assert!(failed, "{refused}");
    assert!(refused.contains("check my email"), "{refused}");
    hand_off.assert_no_turn();
    assert_eq!(hand_off.opened().len(), 1);

    hand_off.answer(CONVERSATION, ANSWER);

    assert_eq!(hand_off.next_turn(), "check my email");
    assert_eq!(hand_off.opened().len(), 2);
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
    let call = asks_permission(
        &lucyd,
        json!({"tool_name": "mcp__linear__list_issues", "input": {"team": "LUC"}}),
    );
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    decision(call);
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

#[test]
fn a_failure_kept_for_the_next_session_is_not_notified_as_finished() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    lucyd.say("what time is it");
    lucyd.thinking.next_turn();

    hand_off.fail("Linear's MCP server isn't connected");
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude couldn't finish");
    lucyd.client().end_session().unwrap();

    let kept = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(kept.summary, "Claude couldn't finish");
}

#[test]
fn claude_finishing_without_a_final_message_says_so() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    hand_off.conversation(CONVERSATION);
    hand_off.turn_end();

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert!(
        shown.body.starts_with("Claude left no final message"),
        "{}",
        shown.body
    );
    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains("left no final message"), "{turn}");
    assert!(!turn.contains("Its final message"), "{turn}");
}

#[test]
fn at_its_budget_claude_is_stopped_on_the_clock_with_the_resume_command_and_folder_notified() {
    let lucyd = Harness::with_config("[handoff]\nbudget_mins = 30\n");
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);

    lucyd.clock.advance(Duration::from_secs(30 * 60 - 1));
    lucyd.barrier();
    let shown = lucyd.notifier.shown();
    assert!(
        shown
            .iter()
            .all(|shown| shown.kind != NotificationKind::HandOff),
        "{shown:?}"
    );
    lucyd.clock.advance(Duration::from_secs(1));

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude ran out of time");
    assert!(shown.body.contains("30 minutes"), "{}", shown.body);
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    let report = lucyd.thinking.next_turn();
    assert!(report.contains("ran out of time"), "{report}");
}

const HAND_OFF_STOPPED: &str = "Okay, I've stopped Claude.";

#[test]
fn cancel_it_stops_claude_with_the_resume_command_notified() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);

    lucyd.say("Cancel it.");

    assert_eq!(lucyd.tts.next_sentence(), HAND_OFF_STOPPED);
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
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
}

fn request_a_build(lucyd: &Harness) {
    let (failed, said) = called(lucyd.thinking.call_tool_with(
        "request_plugin",
        json!({
            "name": "photos",
            "goal": "find the user's photos",
            "clarifications": "",
            "request": "find my photos of Rome",
        }),
    ));
    assert!(!failed, "{said}");
    assert_eq!(
        lucyd.tts.next_sentence(),
        "This is going to take a few minutes."
    );
    lucyd.tts.finish_playing();
}

#[test]
fn a_hand_off_and_a_build_run_together_and_cancel_it_asks_which_one() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    assert_eq!(hand_off.next_turn(), REQUEST);
    lucyd.say("make a Plugin to find my photos");
    lucyd.thinking.next_turn();
    request_a_build(&lucyd);
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    let build = &lucyd.thinking.delegate;
    assert!(build.next_turn().contains("photos"));

    lucyd.say("cancel it");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Do you mean the Plugin build, or Claude?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("Claude");
    assert_eq!(lucyd.tts.next_sentence(), HAND_OFF_STOPPED);
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    assert!(build
        .call_tool_with("report_progress", json!({"text": "still going"}))
        .is_ok());
    assert!(lucyd.client().status().unwrap().building);
    lucyd.thinking.assert_no_turn();
}
