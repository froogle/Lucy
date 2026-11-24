//! A yes to a Hand-off's request for an integration's tool lasts the Session.
//!
//! The scripted Hand-off calls the permission tool through its real relay. A
//! yes to an `mcp__` tool covers later requests for that exact tool until the
//! Session ends; Claude's own tools (`Bash`, `Write`, …) ask every time.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use lucyd::adapters::NotificationKind;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

const REQUEST: &str = "add the turkey to my Christmas list";
const EDIT: &str = "mcp__x__edit";
const ASKED: &str = "Claude needs your yes";
const ALLOWED: &str = "Claude is using a tool you allowed";
const REMEMBERED: &str = "I won't ask again for that this Session.";
const KEPT: &str = "Claude is waiting for your permission";
const DECLINED: &str = "Okay, I won't.";

type Call = JoinHandle<Result<CallToolResult, String>>;

fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

fn handed_off(lucyd: &Harness, events: &mut Client, request: &str) {
    lucyd.say(&format!("tell Claude to {request}"));
    lucyd.thinking.next_turn();
    until_state(events, State::Thinking);
    let folder = lucyd.scratch();
    let arguments = json!({"request": request, "folder": folder.to_str().unwrap()});
    let result = lucyd
        .thinking
        .call_tool_with("hand_to_claude", arguments)
        .unwrap();
    assert_ne!(result.is_error, Some(true));
    lucyd.thinking.turn_end();
    until_state(events, State::Listening);
    assert_eq!(lucyd.thinking.hand_off.next_turn(), request);
}

fn session_with_a_hand_off(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    handed_off(lucyd, &mut events, REQUEST);
    events
}

fn add(tool: &str, item: &str) -> Value {
    json!({
        "tool_name": tool,
        "input": {"item": item, "description": format!("Add {item} to the list.")},
        "tool_use_id": format!("toolu_{item}"),
    })
}

fn run_ls() -> Value {
    json!({
        "tool_name": "Bash",
        "input": {"command": "ls", "description": "List the folder."},
        "tool_use_id": "toolu_ls",
    })
}

fn asks_permission(lucyd: &Harness, request: Value) -> Call {
    let hand_off = lucyd.thinking.hand_off.clone();
    std::thread::spawn(move || hand_off.call_tool_with("permission", request))
}

fn decision(call: Call) -> Value {
    let result = call.join().unwrap().unwrap();
    assert_ne!(result.is_error, Some(true));
    let text: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect();
    serde_json::from_str(&text).unwrap()
}

fn asked(lucyd: &Harness, request: Value) -> (Call, String) {
    let call = asks_permission(lucyd, request);
    let question = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    (call, question)
}

fn answered(lucyd: &Harness, request: Value, answer: &str) -> Value {
    let (call, _) = asked(lucyd, request);
    lucyd.say(answer);
    decision(call)
}

fn refused(lucyd: &Harness, request: Value) {
    let decision = answered(lucyd, request, "no");
    assert_eq!(decision["behavior"], "deny", "{decision}");
    assert_eq!(lucyd.tts.next_sentence(), DECLINED);
    lucyd.tts.finish_playing();
}

fn question(description: &str) -> String {
    format!("Claude wants to {description}. Okay? {REMEMBERED}")
}

fn details(request: &Value) -> String {
    let tool = request["tool_name"].as_str().unwrap();
    let input = serde_json::to_string_pretty(&request["input"]).unwrap();
    format!("{tool}\n{input}")
}

fn assert_only_allowed_notified(lucyd: &Harness, request: &Value) {
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::Details);
    assert_eq!(shown.summary, ALLOWED);
    assert_eq!(shown.body, details(request));
    lucyd.barrier();
    let asked: Vec<_> = lucyd
        .notifier
        .shown()
        .into_iter()
        .filter(|shown| shown.summary == ASKED)
        .collect();
    assert!(asked.is_empty(), "{asked:?}");
}

fn assert_allowed_unasked(lucyd: &Harness, request: Value) {
    lucyd.barrier();
    let spoken = lucyd.tts.sentences();
    lucyd.notifier.shown();

    let decision = decision(asks_permission(lucyd, request.clone()));

    assert_eq!(decision["behavior"], "allow", "{decision}");
    assert_eq!(decision["updatedInput"], request["input"]);
    assert_only_allowed_notified(lucyd, &request);
    assert_eq!(lucyd.tts.sentences(), spoken, "nothing was asked aloud");
}

#[test]
fn a_yes_to_an_integration_tool_lets_its_next_request_through_unasked_and_notified() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);

    let turkey = add(EDIT, "the turkey");
    let first = answered(&lucyd, turkey.clone(), "yes");

    assert_eq!(first["behavior"], "allow", "{first}");
    let asked = lucyd
        .notifier
        .next_notification_of(NotificationKind::Details);
    assert_eq!(
        (asked.summary.as_str(), asked.body),
        (ASKED, details(&turkey))
    );
    assert_allowed_unasked(&lucyd, add(EDIT, "the cranberry sauce"));
}

#[test]
fn an_integration_tools_question_says_it_wont_ask_again_and_claudes_own_tools_doesnt() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);

    let (call, integration) = asked(&lucyd, add(EDIT, "the turkey"));
    lucyd.say("no");
    decision(call);
    assert_eq!(lucyd.tts.next_sentence(), DECLINED);
    lucyd.tts.finish_playing();
    let (call, claudes_own) = asked(&lucyd, run_ls());
    lucyd.say("no");
    decision(call);

    assert_eq!(integration, question("add the turkey to the list"));
    assert_eq!(claudes_own, "Claude wants to list the folder. Okay?");
}

#[test]
fn claudes_own_tool_is_asked_again_after_a_yes() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);
    assert_eq!(answered(&lucyd, run_ls(), "yes")["behavior"], "allow");

    let (call, again) = asked(&lucyd, run_ls());

    assert_eq!(again, "Claude wants to list the folder. Okay?");
    lucyd.say("yes");
    assert_eq!(decision(call)["behavior"], "allow");
}

#[test]
fn after_a_no_the_next_request_for_the_tool_is_asked_again() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);
    refused(&lucyd, add(EDIT, "the turkey"));

    let (call, again) = asked(&lucyd, add(EDIT, "the cranberry sauce"));

    assert_eq!(again, question("add the cranberry sauce to the list"));
    lucyd.say("yes");
    assert_eq!(decision(call)["behavior"], "allow");
}

#[test]
fn after_silence_the_next_request_for_the_tool_is_asked_again() {
    let lucyd = Harness::with_config("[confirmation]\ntimeout_secs = 2\n");
    let _events = session_with_a_hand_off(&lucyd);
    let (call, _) = asked(&lucyd, add(EDIT, "the turkey"));
    lucyd.barrier();
    lucyd.clock.advance(Duration::from_secs(2));
    assert_eq!(decision(call)["behavior"], "deny");
    assert_eq!(lucyd.tts.next_sentence(), DECLINED);
    lucyd.tts.finish_playing();

    let (call, again) = asked(&lucyd, add(EDIT, "the cranberry sauce"));

    assert_eq!(again, question("add the cranberry sauce to the list"));
    lucyd.say("yes");
    assert_eq!(decision(call)["behavior"], "allow");
}

#[test]
fn a_yes_covers_only_the_exact_tool_named() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);
    let first = answered(&lucyd, add(EDIT, "the turkey"), "yes");
    assert_eq!(first["behavior"], "allow");

    let (call, other_tool) = asked(&lucyd, add("mcp__x__delete", "the turkey"));
    lucyd.say("no");
    decision(call);
    assert_eq!(lucyd.tts.next_sentence(), DECLINED);
    lucyd.tts.finish_playing();
    let (call, other_server) = asked(&lucyd, add("mcp__y__edit", "the turkey"));
    lucyd.say("no");
    decision(call);

    assert_eq!(other_tool, question("add the turkey to the list"));
    assert_eq!(other_server, question("add the turkey to the list"));
}

fn kept(lucyd: &Harness) {
    while lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff)
        .summary
        != KEPT
    {}
}

#[test]
fn a_request_waiting_behind_the_yes_for_its_tool_runs_without_being_asked() {
    let lucyd = Harness::start();
    let mut events = session_with_a_hand_off(&lucyd);
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);
    let first = asks_permission(&lucyd, add(EDIT, "the turkey"));
    kept(&lucyd);
    let cranberry_sauce = add(EDIT, "the cranberry sauce");
    let second = asks_permission(&lucyd, cranberry_sauce.clone());
    kept(&lucyd);

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    let asked = lucyd.tts.next_sentence();
    lucyd.notifier.shown();
    lucyd.tts.finish_playing();
    lucyd.say("yes");

    assert_eq!(asked, question("add the turkey to the list"));
    assert_eq!(decision(first)["behavior"], "allow");
    assert_eq!(decision(second)["behavior"], "allow");
    assert_only_allowed_notified(&lucyd, &cranberry_sauce);
    assert_eq!(lucyd.tts.sentences(), [asked], "the second wasn't asked");
}

#[test]
fn a_yes_in_one_hand_off_covers_the_tool_in_the_next_hand_off_of_the_session() {
    let lucyd = Harness::start();
    let mut events = session_with_a_hand_off(&lucyd);
    let first = answered(&lucyd, add(EDIT, "the turkey"), "yes");
    assert_eq!(first["behavior"], "allow");
    lucyd
        .thinking
        .hand_off
        .answer("5c1e0d2a-hand-off", "I added the turkey.");
    lucyd.thinking.next_turn();
    lucyd.thinking.turn_end();

    handed_off(&lucyd, &mut events, "add the stuffing too");

    assert_allowed_unasked(&lucyd, add(EDIT, "the stuffing"));
}

#[test]
fn after_the_session_ends_the_tool_is_asked_at_the_next_session_and_a_yes_there_lasts_it() {
    let lucyd = Harness::start();
    let mut events = session_with_a_hand_off(&lucyd);
    let first = answered(&lucyd, add(EDIT, "the turkey"), "yes");
    assert_eq!(first["behavior"], "allow");
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);
    let spoken = lucyd.tts.sentences();

    let call = asks_permission(&lucyd, add(EDIT, "the cranberry sauce"));
    kept(&lucyd);
    assert_eq!(lucyd.tts.sentences(), spoken, "nothing was asked aloud");
    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);

    assert_eq!(
        lucyd.tts.next_sentence(),
        question("add the cranberry sauce to the list")
    );
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    assert_eq!(decision(call)["behavior"], "allow");
    assert_allowed_unasked(&lucyd, add(EDIT, "the stuffing"));
}

fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for path in entries.map(|entry| entry.unwrap().path()) {
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            let contents = std::fs::read(&path).unwrap();
            files.push((path, contents));
        }
    }
    files.sort();
    files
}

#[test]
fn nothing_about_allowed_tools_is_written_under_the_state_dir() {
    let lucyd = Harness::start();
    let _events = session_with_a_hand_off(&lucyd);
    let before = files_under(lucyd.state_dir());

    let first = answered(&lucyd, add(EDIT, "the turkey"), "yes");
    assert_eq!(first["behavior"], "allow");
    assert_allowed_unasked(&lucyd, add(EDIT, "the cranberry sauce"));

    assert_eq!(files_under(lucyd.state_dir()), before);
}

#[test]
fn an_indicator_click_while_an_integration_tool_is_asked_isnt_remembered() {
    let lucyd = Harness::start();
    let mut events = session_with_a_hand_off(&lucyd);
    let (call, _) = asked(&lucyd, add(EDIT, "the turkey"));

    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);
    let unless_the_relay_was_cut_off = call.join().unwrap();
    if let Ok(result) = unless_the_relay_was_cut_off {
        let text = result.content[0].as_text().unwrap().text.clone();
        let decision: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(decision["behavior"], "deny", "{decision}");
    }
    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Listening);

    let (call, again) = asked(&lucyd, add(EDIT, "the cranberry sauce"));
    assert_eq!(again, question("add the cranberry sauce to the list"));
    lucyd.say("no");
    decision(call);
}

#[test]
fn a_waiting_request_of_a_hand_off_out_of_time_is_dropped_though_its_tool_was_allowed() {
    let lucyd = Harness::with_config(
        "[handoff]\nbudget_mins = 30\n\n[confirmation]\ntimeout_secs = 7200\n",
    );
    let mut events = session_with_a_hand_off(&lucyd);
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);
    let first = asks_permission(&lucyd, add(EDIT, "the turkey"));
    kept(&lucyd);
    let second = asks_permission(&lucyd, add(EDIT, "the cranberry sauce"));
    kept(&lucyd);
    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.clock.advance(Duration::from_secs(30 * 60));
    for call in [first, second] {
        let stopped = call.join().unwrap().unwrap();
        assert_eq!(stopped.is_error, Some(true), "{stopped:?}");
    }
    lucyd.notifier.shown();
    lucyd.say("yes");
    lucyd.barrier();
    let allowed: Vec<_> = lucyd
        .notifier
        .shown()
        .into_iter()
        .filter(|shown| shown.summary == ALLOWED)
        .collect();
    assert!(allowed.is_empty(), "{allowed:?}");
}
