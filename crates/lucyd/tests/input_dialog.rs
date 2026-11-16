//! The Input dialog: `ask_typed` puts a box on screen through the client
//! holding the `input` role, Lucy waits in Typing, and the typed answer goes
//! back to the model.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::collections::BTreeMap;
use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, wait_until, Harness};
use lucy_ipc::{error_code, Client, Event, FieldKind, InputOpen, Role, State};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

type Call = JoinHandle<Result<CallToolResult, String>>;

/// The model, in a Turn, calls `ask_typed` with `arguments`.
fn model_asks(lucyd: &Harness, events: &mut Client, arguments: Value) -> Call {
    lucyd.say("I'll type a URL for you");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let thinking = lucyd.thinking.clone();
    std::thread::spawn(move || thinking.call_tool_with("ask_typed", arguments))
}

fn url_question() -> Value {
    json!({"question": "Which page shall I open?", "kind": "url"})
}

/// The next `input.open` the holder is sent.
fn next_open(holder: &mut Client) -> InputOpen {
    loop {
        match holder.next_event().expect("an input.open event") {
            Event::InputOpen(open) => return open,
            _ => continue,
        }
    }
}

fn answer(value: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("answer".to_owned(), value.to_owned())])
}

fn structured(result: &CallToolResult) -> Value {
    result
        .structured_content
        .clone()
        .expect("a structured result")
}

/// A Session open, with a client watching its state.
fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    events
}

#[test]
fn ask_typed_opens_the_box_on_the_holder_and_returns_what_was_typed() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());

    let open = next_open(&mut holder);
    assert_eq!(open.message, "Which page shall I open?");
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].kind, FieldKind::Url);
    lucyd.answer_input(&mut holder, &open.id, "https://example.com/a/long/path");

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        structured(&result),
        json!({"answer": "https://example.com/a/long/path"})
    );
}

fn secs(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

/// The next `input.close` the holder is sent, as its dialog's id.
fn next_close(holder: &mut Client) -> String {
    loop {
        match holder.next_event().expect("an input.close event") {
            Event::InputClose(close) => return close.id,
            _ => continue,
        }
    }
}

#[test]
fn ask_typed_refuses_a_secret() {
    let lucyd = Harness::start();
    let _holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(
        &lucyd,
        &mut events,
        json!({"question": "Your Linear API key?", "kind": "secret"}),
    );

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(lucyd.tts.sentences().is_empty());
}

#[test]
fn ask_typed_offers_a_choice_and_returns_the_one_picked() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(
        &lucyd,
        &mut events,
        json!({"question": "Which one?", "kind": "choice", "choices": ["red", "blue"]}),
    );

    let open = next_open(&mut holder);
    assert_eq!(open.fields[0].kind, FieldKind::Choice);
    assert_eq!(open.fields[0].choices, ["red", "blue"]);
    lucyd.answer_input(&mut holder, &open.id, "blue");

    let result = call.join().unwrap().unwrap();
    assert_eq!(structured(&result), json!({"answer": "blue"}));
}

#[test]
fn lucy_says_she_has_put_a_box_on_screen_and_waits_in_typing() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let _call = model_asks(&lucyd, &mut events, url_question());

    assert_eq!(next_state(&mut events).state, State::Typing);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "I've put a box on screen for the URL."
    );
    next_open(&mut holder);
    assert_eq!(lucyd.client().status().unwrap().state, State::Typing);
}

#[test]
fn the_silence_timer_is_paused_while_typing() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 5\n");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    assert_eq!(next_state(&mut events).state, State::Typing);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    let open = next_open(&mut holder);

    lucyd.clock.advance_once_stopped(secs(60));

    assert_eq!(lucyd.client().status().unwrap().state, State::Typing);
    lucyd.answer_input(&mut holder, &open.id, "https://example.com");
    call.join().unwrap().unwrap();
}

#[test]
fn a_cancel_phrase_closes_the_box_and_returns_cancelled() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("Never mind.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(structured(&result), json!({"result": "cancelled"}));
    assert_eq!(next_close(&mut holder), open.id);
}

#[test]
fn other_speech_is_ignored_while_typing() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("Sorry, I'm on the phone, can you cancel my dentist?");
    lucyd.mumble_handled();

    assert_eq!(lucyd.client().status().unwrap().state, State::Typing);
    lucyd.thinking.assert_no_turn();
    lucyd.answer_input(&mut holder, &open.id, "https://example.com");
    let result = call.join().unwrap().unwrap();
    assert_eq!(
        structured(&result),
        json!({"answer": "https://example.com"})
    );
}

#[test]
fn a_barge_in_does_not_close_the_box() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    lucyd.tts.next_sentence();

    lucyd.wake();
    lucyd.barrier();

    assert_eq!(lucyd.client().status().unwrap().state, State::Typing);
    lucyd.answer_input(&mut holder, &open.id, "https://example.com");
    call.join().unwrap().unwrap();
}

#[test]
fn the_box_gives_up_after_its_timeout() {
    let lucyd = Harness::with_config("[input]\ntimeout_secs = 30\n");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.clock.advance_once_set(secs(30));

    let result = call.join().unwrap().unwrap();
    assert_eq!(structured(&result), json!({"result": "timed_out"}));
    assert_eq!(next_close(&mut holder), open.id);
}

#[test]
fn an_indicator_click_ends_the_session_and_closes_the_box() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    assert_eq!(next_state(&mut events).state, State::Typing);

    lucyd.client().end_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Idle);
    assert_eq!(next_close(&mut holder), open.id);
    let result = call.join().unwrap().unwrap();
    assert_eq!(structured(&result), json!({"result": "cancelled"}));
}

#[test]
fn a_second_client_cannot_claim_the_input_role() {
    let lucyd = Harness::start();
    let _holder = lucyd.input("a widget");

    let second = Client::connect(lucyd.socket_path(), "another", Some(Role::Input));

    match second {
        Err(lucy_ipc::Error::Rpc { code, .. }) => assert_eq!(code, error_code::ROLE_TAKEN),
        Err(other) => panic!("refused for the wrong reason: {other}"),
        Ok(_) => panic!("a second client claimed the input role"),
    }
}

#[test]
fn the_role_is_free_again_once_its_holder_goes() {
    let lucyd = Harness::start();
    drop(lucyd.input("a widget"));

    wait_until("the role to be free", || {
        Client::connect(lucyd.socket_path(), "another", Some(Role::Input)).ok()
    });
}

#[test]
fn answers_from_a_client_without_the_role_are_refused() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);

    let refused = lucyd
        .client()
        .answer_input(&open.id, answer("https://evil.example"));

    match refused {
        Err(lucy_ipc::Error::Rpc { code, .. }) => assert_eq!(code, error_code::NOT_INPUT_HOLDER),
        other => panic!("not refused: {other:?}"),
    }
    lucyd.answer_input(&mut holder, &open.id, "https://example.com");
    let result = call.join().unwrap().unwrap();
    assert_eq!(
        structured(&result),
        json!({"answer": "https://example.com"})
    );
}

#[test]
fn with_no_holder_lucy_input_is_opened_in_a_terminal_and_answers() {
    let lucyd = Harness::start();
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    assert_eq!(next_state(&mut events).state, State::Typing);

    let opened = wait_until("a terminal to open", || lucyd.launcher.terminals().pop());
    let socket = lucyd.socket_path().display().to_string();
    assert_eq!(opened[1..], ["input", "--socket", &socket]);
    let mut page = lucyd.input("lucy input");
    let open = next_open(&mut page);
    lucyd.answer_input(&mut page, &open.id, "https://example.com");

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        structured(&result),
        json!({"answer": "https://example.com"})
    );
}

#[test]
fn with_no_terminal_the_model_gets_cancelled_and_lucy_says_why() {
    let lucyd = Harness::start();
    lucyd.launcher.no_terminal();
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());

    let result = call.join().unwrap().unwrap();
    assert_eq!(structured(&result), json!({"result": "cancelled"}));
    let said = lucyd.tts.next_sentence();
    assert!(said.contains("couldn't put a box on screen"), "{said}");
}

#[test]
fn a_typed_answer_never_reaches_the_bus() {
    let lucyd = Harness::start();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_asks(&lucyd, &mut events, url_question());
    let open = next_open(&mut holder);
    lucyd.answer_input(&mut holder, &open.id, "https://example.com/private");
    call.join().unwrap().unwrap();

    lucyd.client().end_session().unwrap();

    lucyd.assert_nothing_private_on_bus();
}
