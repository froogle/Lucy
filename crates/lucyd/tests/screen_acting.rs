//! Acting on the screen (ADR 0008): before a `click` or `key` reaches the
//! Plugin, Lucy matches its `describe` against `[screen] risky_words`, and a
//! match needs a spoken yes. A `type` ending in a newline counts as a key.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

const HANDS: &str = "name = \"hands\"\n\
     version = \"0.1.0\"\n\
     author = \"human\"\n\
     summary = \"Acts on the screen\"\n\
     command = [\"./fixture-plugin\", \"click\", \"key\", \"type\", \"scroll\"]\n\
     \n\
     [[tools]]\n\
     name = \"click\"\n\
     handles = \"clicking on the screen\"\n\
     screen = \"click\"\n\
     \n\
     [[tools]]\n\
     name = \"key\"\n\
     handles = \"pressing keys\"\n\
     screen = \"key\"\n\
     \n\
     [[tools]]\n\
     name = \"type\"\n\
     handles = \"typing text\"\n\
     screen = \"type\"\n\
     \n\
     [[tools]]\n\
     name = \"scroll\"\n\
     handles = \"scrolling\"\n\
     screen = \"scroll\"\n";

type Call = JoinHandle<Result<CallToolResult, String>>;

fn with_hands(config: &str) -> Harness {
    let lucyd = Harness::with_config(config);
    lucyd.install_plugin("hands", HANDS);
    lucyd.approve("hands");
    lucyd
}

fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("play the third video");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    events
}

fn model_calls(lucyd: &Harness, tool: &str, arguments: Value) -> Call {
    let thinking = lucyd.thinking.clone();
    let tool = format!("hands__{tool}");
    std::thread::spawn(move || thinking.call_tool_with(&tool, arguments))
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn forwarded(result: &CallToolResult, tool: &str) -> Value {
    let said = text(result);
    let Some(arguments) = said.strip_prefix(&format!("{tool}: ")) else {
        panic!("{tool} wasn't forwarded: {said}");
    };
    serde_json::from_str(arguments).unwrap()
}

fn asked(lucyd: &Harness, events: &mut Client) -> String {
    assert_eq!(next_state(events).state, State::Confirming);
    let prompt = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    prompt
}

fn unasked(lucyd: &Harness, tool: &str, arguments: Value) -> CallToolResult {
    let result = lucyd
        .thinking
        .call_tool_with(&format!("hands__{tool}"), arguments)
        .unwrap();
    assert!(
        lucyd.tts.sentences().is_empty(),
        "{:?}",
        lucyd.tts.sentences()
    );
    result
}

#[test]
fn a_risky_click_asks_in_its_own_words_and_a_yes_clicks() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let click = json!({"x": 10, "y": 20, "describe": "the Send button"});
    let call = model_calls(&lucyd, "click", click.clone());

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Send button?");
    lucyd.say("yes");

    let result = call.join().unwrap().unwrap();
    assert_eq!(forwarded(&result, "click"), click);
}

#[test]
fn a_declined_risky_click_never_reaches_the_plugin() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let call = model_calls(
        &lucyd,
        "click",
        json!({"x": 10, "y": 20, "describe": "Delete this video"}),
    );
    asked(&lucyd, &mut events);

    lucyd.say("no");

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
}

#[test]
fn a_plain_click_goes_straight_through() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);
    let click = json!({"x": 10, "y": 20, "describe": "the Play button on the third video"});

    let result = unasked(&lucyd, "click", click.clone());

    assert_eq!(forwarded(&result, "click"), click);
}

#[test]
fn a_risky_word_is_matched_at_the_start_of_a_word_in_any_case() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);

    unasked(
        &lucyd,
        "click",
        json!({"x": 1, "y": 1, "describe": "the Replay button, on display"}),
    );
    let call = model_calls(
        &lucyd,
        "click",
        json!({"x": 1, "y": 1, "describe": "PAYMENT options"}),
    );

    assert_eq!(lucyd.tts.next_sentence(), "Shall I click PAYMENT options?");
    lucyd.tts.finish_playing();
    lucyd.say("no");
    call.join().unwrap().unwrap();
}

#[test]
fn a_risky_key_asks_in_its_own_words() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let key = json!({"combo": "Return", "describe": "Enter, to post the comment"});
    let call = model_calls(&lucyd, "key", key.clone());

    assert_eq!(
        asked(&lucyd, &mut events),
        "Shall I press Enter, to post the comment?"
    );
    lucyd.say("yes");

    assert_eq!(forwarded(&call.join().unwrap().unwrap(), "key"), key);
}

#[test]
fn a_plain_key_goes_straight_through() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);
    let key = json!({"combo": "ctrl+l", "describe": "Ctrl+L, to focus the address bar"});

    let result = unasked(&lucyd, "key", key.clone());

    assert_eq!(forwarded(&result, "key"), key);
}

#[test]
fn a_click_or_key_without_a_describe_is_refused_unsent() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);

    for (tool, arguments) in [
        ("click", json!({"x": 1, "y": 1})),
        ("click", json!({"x": 1, "y": 1, "describe": " "})),
        ("key", json!({"combo": "Return"})),
    ] {
        let result = unasked(&lucyd, tool, arguments);
        assert_eq!(result.is_error, Some(true), "{}", text(&result));
        assert!(text(&result).contains("describe"), "{}", text(&result));
    }
}

#[test]
fn a_risky_type_ending_in_a_newline_asks_like_a_key() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let typed = json!({"text": "see you at 6\n", "describe": "Enter, to send the message"});
    let call = model_calls(&lucyd, "type", typed.clone());

    assert_eq!(
        asked(&lucyd, &mut events),
        "Shall I type that and press Enter, to send the message?"
    );
    lucyd.say("yes");

    assert_eq!(forwarded(&call.join().unwrap().unwrap(), "type"), typed);
}

#[test]
fn a_type_with_no_newline_is_never_checked() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);
    let typed = json!({"text": "send me the delete form"});

    let result = unasked(&lucyd, "type", typed.clone());

    assert_eq!(forwarded(&result, "type"), typed);
}

#[test]
fn a_type_ending_in_a_newline_without_a_describe_is_refused_unsent() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);

    let result = unasked(&lucyd, "type", json!({"text": "hello\n"}));

    assert_eq!(result.is_error, Some(true), "{}", text(&result));
    assert!(text(&result).contains("describe"), "{}", text(&result));
}

#[test]
fn a_custom_risky_words_list_replaces_the_default() {
    let lucyd = with_hands("[screen]\nrisky_words = [\"play\"]\n");
    let mut events = session(&lucyd);

    unasked(
        &lucyd,
        "click",
        json!({"x": 1, "y": 1, "describe": "the Send button"}),
    );
    let call = model_calls(
        &lucyd,
        "click",
        json!({"x": 1, "y": 1, "describe": "the Play button"}),
    );

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
    lucyd.say("no");
    call.join().unwrap().unwrap();
}

#[test]
fn scrolling_is_never_checked() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);
    let scroll = json!({"x": 1, "y": 1, "direction": "down", "amount": 3});

    let result = unasked(&lucyd, "scroll", scroll.clone());

    assert_eq!(forwarded(&result, "scroll"), scroll);
}

#[test]
fn screen_access_turned_off_drops_every_acting_tool() {
    let lucyd = with_hands("[screen]\nenabled = false\n");
    let _events = session(&lucyd);

    let names = lucyd.thinking.tool_names();

    assert!(
        names.iter().all(|name| !name.starts_with("hands__")),
        "{names:?}"
    );
}
