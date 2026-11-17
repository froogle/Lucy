//! Claude's questions during a Hand-off: `ask_user` is spoken once nobody is
//! speaking, `ask_typed` opens the Input dialog, and `report_progress` is only
//! shown. The scripted Hand-off calls them through its real relay; each answer
//! comes back as the tool's result, and Claude carries on.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, Event, FieldKind, InputOpen, State};
use lucyd::adapters::NotificationKind;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

const REQUEST: &str = "fix the failing test";
const QUESTION: &str = "Should I update the snapshot or the code?";

type Call = JoinHandle<Result<CallToolResult, String>>;

fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

fn handed_off(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say(&format!("tell Claude to {REQUEST}"));
    lucyd.thinking.next_turn();
    until_state(&mut events, State::Thinking);
    let folder = lucyd.scratch();
    let arguments = json!({"request": REQUEST, "folder": folder.to_str().unwrap()});
    let result = lucyd
        .thinking
        .call_tool_with("hand_to_claude", arguments)
        .unwrap();
    assert_ne!(result.is_error, Some(true));
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    assert_eq!(lucyd.thinking.hand_off.next_turn(), REQUEST);
    events
}

fn claude_calls(lucyd: &Harness, tool: &'static str, arguments: Value) -> Call {
    let hand_off = lucyd.thinking.hand_off.clone();
    std::thread::spawn(move || hand_off.call_tool_with(tool, arguments))
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn next_open(holder: &mut Client) -> InputOpen {
    loop {
        if let Event::InputOpen(open) = holder.next_event().expect("an input.open event") {
            return open;
        }
    }
}

#[test]
fn claude_sees_the_hand_off_tools_and_nothing_of_authoring() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd);

    let mut tools = lucyd.thinking.hand_off.tool_names();
    tools.sort();

    assert_eq!(
        tools,
        ["ask_typed", "ask_user", "permission", "report_progress"]
    );
}

#[test]
fn ask_user_waits_until_nobody_is_speaking_and_the_answer_goes_back_to_claude() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd);
    lucyd.say("what's the weather?");
    lucyd.thinking.next_turn();
    until_state(&mut events, State::Thinking);
    lucyd.thinking.delta("It's sunny.");
    lucyd.thinking.sentence_end();
    assert_eq!(lucyd.tts.next_sentence(), "It's sunny.");

    let call = claude_calls(&lucyd, "ask_user", json!({"question": QUESTION}));
    lucyd.barrier();
    lucyd.thinking.turn_end();
    lucyd.barrier();
    assert_eq!(lucyd.tts.sentences(), ["It's sunny."]);
    lucyd.tts.finish_playing();

    assert_eq!(
        lucyd.tts.next_sentence(),
        format!("Claude asks: {QUESTION}")
    );
    lucyd.tts.finish_playing();
    lucyd.say("update the snapshot");

    let result = call.join().unwrap().unwrap();
    assert_ne!(result.is_error, Some(true));
    let said = text(&result);
    assert!(said.contains("update the snapshot"), "{said}");
    lucyd.thinking.assert_no_turn();
    lucyd.thinking.hand_off.assert_no_turn();
}

#[test]
fn ask_user_left_unanswered_tells_claude_to_carry_on() {
    let lucyd = Harness::with_config("[confirmation]\ntimeout_secs = 2\n");
    let _events = handed_off(&lucyd);

    let call = claude_calls(&lucyd, "ask_user", json!({"question": QUESTION}));
    assert_eq!(
        lucyd.tts.next_sentence(),
        format!("Claude asks: {QUESTION}")
    );
    lucyd.tts.finish_playing();
    lucyd
        .clock
        .advance_once_set(std::time::Duration::from_secs(2));

    let said = text(&call.join().unwrap().unwrap());
    assert!(said.contains("didn't answer"), "{said}");
}

#[test]
fn ask_typed_opens_the_input_dialog_and_returns_what_was_typed() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let _events = handed_off(&lucyd);

    let call = claude_calls(
        &lucyd,
        "ask_typed",
        json!({"question": "Which branch should I push to?", "kind": "text"}),
    );
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let open = next_open(&mut holder);
    assert_eq!(open.message, "Which branch should I push to?");
    assert_eq!(open.fields[0].kind, FieldKind::Text);
    lucyd.answer_input(&mut holder, &open.id, "fix/flaky-test");

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({"answer": "fix/flaky-test"}))
    );
    lucyd.thinking.hand_off.assert_no_turn();
}

#[test]
fn ask_typed_refuses_a_secret() {
    let lucyd = Harness::start();
    let _holder = lucyd.input("a widget");
    let _events = handed_off(&lucyd);

    let result = lucyd
        .thinking
        .hand_off
        .call_tool_with(
            "ask_typed",
            json!({"question": "Your GitHub token?", "kind": "secret"}),
        )
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    lucyd.barrier();
    assert!(lucyd.tts.sentences().is_empty());
}

#[test]
fn report_progress_is_notified_and_never_spoken() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd);

    let result = lucyd
        .thinking
        .hand_off
        .call_tool_with("report_progress", json!({"text": "Running the test suite"}))
        .unwrap();

    assert_ne!(result.is_error, Some(true));
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude is working");
    assert_eq!(shown.body, "Running the test suite");
    lucyd.barrier();
    assert!(lucyd.tts.sentences().is_empty());
    lucyd.thinking.assert_no_turn();
}
