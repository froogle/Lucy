//! Acting on the screen (ADR 0008): the first screen tool call after lucyd
//! starts, whatever its kind, asks the one spoken question; after a yes every
//! click, key, typing and scroll reaches the Plugin unasked.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

const ASKED: &str = "I need to look at and use your screen. That lasts until Lucy restarts. Okay?";

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

const SEND: &str = r#"{"x": 610, "y": 410}"#;

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
    lucyd.say("send the message");
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
    while next_state(events).state != State::Confirming {}
    let prompt = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    prompt
}

fn screen_allowed(lucyd: &Harness, events: &mut Client) {
    let scroll = json!({"x": 1, "y": 1, "direction": "down", "amount": 3});
    let call = model_calls(lucyd, "scroll", scroll.clone());
    assert_eq!(asked(lucyd, events), ASKED);
    lucyd.say("yes");
    assert_eq!(forwarded(&call.join().unwrap().unwrap(), "scroll"), scroll);
}

fn unasked(lucyd: &Harness, tool: &str, arguments: Value) -> Value {
    let said = lucyd.tts.sentences().len();
    let result = lucyd
        .thinking
        .call_tool_with(&format!("hands__{tool}"), arguments)
        .unwrap();
    assert_eq!(
        lucyd.tts.sentences().len(),
        said,
        "{:?}",
        lucyd.tts.sentences()
    );
    forwarded(&result, tool)
}

#[test]
fn a_click_with_no_look_before_it_asks_the_one_question() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let click: Value = serde_json::from_str(SEND).unwrap();
    let call = model_calls(&lucyd, "click", click.clone());

    assert_eq!(asked(&lucyd, &mut events), ASKED);
    lucyd.say("yes");

    assert_eq!(forwarded(&call.join().unwrap().unwrap(), "click"), click);
}

#[test]
fn a_no_reaches_nothing_and_the_next_call_asks_again() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "key", json!({"combo": "Return"}));
    assert_eq!(asked(&lucyd, &mut events), ASKED);
    lucyd.say("no");
    let declined = call.join().unwrap().unwrap();
    assert!(text(&declined).starts_with("declined"), "{declined:?}");
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let again = model_calls(&lucyd, "key", json!({"combo": "Return"}));

    assert_eq!(asked(&lucyd, &mut events), ASKED);
    lucyd.say("yes");
    assert_eq!(
        forwarded(&again.join().unwrap().unwrap(), "key"),
        json!({"combo": "Return"})
    );
}

#[test]
fn after_a_yes_clicking_send_typing_and_keys_run_unasked() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    screen_allowed(&lucyd, &mut events);

    let typed = json!({"text": "see you at 6\nthanks"});
    assert_eq!(unasked(&lucyd, "type", typed.clone()), typed);
    let enter = json!({"combo": "Return"});
    assert_eq!(unasked(&lucyd, "key", enter.clone()), enter);
    let send: Value = serde_json::from_str(SEND).unwrap();
    assert_eq!(unasked(&lucyd, "click", send.clone()), send);
}

#[test]
fn a_config_still_naming_risky_words_loads_and_asks_nothing_more() {
    let lucyd = with_hands("[screen]\nrisky_words = [\"send\"]\n");
    let mut events = session(&lucyd);
    screen_allowed(&lucyd, &mut events);

    let send: Value = serde_json::from_str(SEND).unwrap();

    assert_eq!(unasked(&lucyd, "click", send.clone()), send);
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

#[test]
fn a_screen_tool_that_always_confirms_still_asks_after_the_screen_yes() {
    let lucyd = Harness::start();
    let confirming = HANDS.replacen(
        "screen = \"click\"\n",
        "screen = \"click\"\nconfirm = true\nconfirm_prompt = \"Shall I click there?\"\n",
        1,
    );
    lucyd.install_plugin("hands", &confirming);
    lucyd.approve("hands");
    let mut events = session(&lucyd);
    screen_allowed(&lucyd, &mut events);
    let click: Value = serde_json::from_str(SEND).unwrap();

    let call = model_calls(&lucyd, "click", click.clone());

    assert_eq!(asked(&lucyd, &mut events), "Shall I click there?");
    lucyd.say("yes");
    assert_eq!(forwarded(&call.join().unwrap().unwrap(), "click"), click);
    assert_eq!(lucyd.tts.sentences(), [ASKED, "Shall I click there?"]);
}
