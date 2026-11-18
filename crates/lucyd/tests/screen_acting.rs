//! Acting on the screen (ADR 0008): before a `click` or `key` reaches the
//! Plugin, Lucy matches its `describe` against `[screen] risky_words`, and a
//! match needs a spoken yes. A `type` with a newline in it counts as a key.
//! Before a `click`, Lucy also reads the text under it through the Plugin's
//! probe and matches that; a click on nothing readable asks.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

const HANDS: &str = "name = \"hands\"\n\
     version = \"0.1.0\"\n\
     author = \"human\"\n\
     summary = \"Acts on the screen\"\n\
     command = [\"./fixture-plugin\", \"click\", \"key\", \"type\", \"scroll\", \"text_at\"]\n\
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
     screen = \"scroll\"\n\
     \n\
     [[tools]]\n\
     name = \"text_at\"\n\
     handles = \"reading the screen at a point\"\n\
     screen = \"probe\"\n";

const A_VIDEO: &str = r#"{"name": "Video", "text": "", "role": "link"}"#;

type Call = JoinHandle<Result<CallToolResult, String>>;

fn with_hands(config: &str) -> Harness {
    let lucyd = Harness::with_config(config);
    lucyd.install_plugin("hands", HANDS);
    lucyd.approve("hands");
    show(&lucyd, A_VIDEO);
    lucyd
}

fn show(lucyd: &Harness, under: &str) {
    let dir = lucyd.state_dir().join("plugins/hands");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("under"), under).unwrap();
}

fn probed(lucyd: &Harness) -> Vec<Value> {
    let probed =
        std::fs::read_to_string(lucyd.state_dir().join("plugins/hands/probed")).unwrap_or_default();
    probed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
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
fn a_risky_type_with_a_newline_in_it_asks_like_a_key() {
    for text in ["see you at 6\n", "see you at 6\nthanks"] {
        let lucyd = with_hands("");
        let mut events = session(&lucyd);
        let typed = json!({"text": text, "describe": "Enter, to send the message"});
        let call = model_calls(&lucyd, "type", typed.clone());

        assert_eq!(
            asked(&lucyd, &mut events),
            "Shall I type that and press Enter, to send the message?"
        );
        lucyd.say("yes");

        assert_eq!(forwarded(&call.join().unwrap().unwrap(), "type"), typed);
    }
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
fn a_type_with_a_newline_in_it_without_a_describe_is_refused_unsent() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);

    for typed in ["hello\n", "hello\nthere"] {
        let result = unasked(&lucyd, "type", json!({"text": typed}));

        assert_eq!(result.is_error, Some(true), "{}", text(&result));
        assert!(text(&result).contains("describe"), "{}", text(&result));
    }
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

#[test]
fn a_risky_click_on_a_tool_that_always_confirms_asks_once() {
    let lucyd = Harness::start();
    let confirming = HANDS.replacen(
        "screen = \"click\"\n",
        "screen = \"click\"\nconfirm = true\n",
        1,
    );
    lucyd.install_plugin("hands", &confirming);
    lucyd.approve("hands");
    show(&lucyd, A_VIDEO);
    let mut events = session(&lucyd);
    let click = json!({"x": 10, "y": 20, "describe": "the Send button"});
    let call = model_calls(&lucyd, "click", click.clone());

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Send button?");
    lucyd.say("yes");

    let result = call.join().unwrap().unwrap();
    assert_eq!(forwarded(&result, "click"), click);
    assert_eq!(lucyd.tts.sentences(), ["Shall I click the Send button?"]);
}

#[test]
fn the_text_under_a_click_matching_a_risky_word_asks_even_when_describe_is_plain() {
    let lucyd = with_hands("");
    show(
        &lucyd,
        r#"{"name": "Send", "text": "Send", "role": "push button"}"#,
    );
    let mut events = session(&lucyd);
    let click = json!({"x": 10, "y": 20, "describe": "the Play button"});
    let call = model_calls(&lucyd, "click", click.clone());

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
    lucyd.say("yes");

    let result = call.join().unwrap().unwrap();
    assert_eq!(forwarded(&result, "click"), click);
    assert_eq!(probed(&lucyd), [json!({"x": 10, "y": 20})]);
}

#[test]
fn a_click_on_nothing_readable_asks_in_its_own_words() {
    for under in ["null", "fail"] {
        let lucyd = with_hands("");
        show(&lucyd, under);
        let mut events = session(&lucyd);
        let call = model_calls(
            &lucyd,
            "click",
            json!({"x": 10, "y": 20, "describe": "the Play button"}),
        );

        assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
        lucyd.say("no");

        let result = call.join().unwrap().unwrap();
        assert!(text(&result).starts_with("declined"), "{}", text(&result));
    }
}

fn probe_hangs(lucyd: &Harness) -> Call {
    show(lucyd, "hang");
    let call = model_calls(
        lucyd,
        "click",
        json!({"x": 10, "y": 20, "describe": "the Play button"}),
    );
    harness::wait_until("the probe never ran", || {
        (!probed(lucyd).is_empty()).then_some(())
    });
    call
}

#[test]
fn a_probe_that_doesnt_answer_in_time_counts_as_unreadable_and_asks() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let call = probe_hangs(&lucyd);

    lucyd.clock.advance_once_set(Duration::from_secs(3));

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
    lucyd.say("no");
    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
}

#[test]
fn a_barge_in_cuts_off_a_probe_that_doesnt_answer() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let call = probe_hangs(&lucyd);

    lucyd.start_speaking();

    assert_eq!(next_state(&mut events).state, State::Listening);
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(true), "{}", text(&result));
    assert!(text(&result).contains("cut in"), "{}", text(&result));
}

#[test]
fn a_click_on_a_plugin_with_no_probe_asks() {
    let lucyd = Harness::start();
    let unprobed = HANDS
        .split("\n[[tools]]\nname = \"text_at\"")
        .next()
        .unwrap()
        .replace(", \"text_at\"]", "]");
    lucyd.install_plugin("hands", &unprobed);
    lucyd.approve("hands");
    let mut events = session(&lucyd);
    let call = model_calls(
        &lucyd,
        "click",
        json!({"x": 10, "y": 20, "describe": "the Play button"}),
    );

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
    lucyd.say("no");
    call.join().unwrap().unwrap();
}

#[test]
fn a_risky_describe_asks_without_probing() {
    let lucyd = with_hands("");
    let mut events = session(&lucyd);
    let call = model_calls(
        &lucyd,
        "click",
        json!({"x": 10, "y": 20, "describe": "the Send button"}),
    );

    assert_eq!(asked(&lucyd, &mut events), "Shall I click the Send button?");
    lucyd.say("no");
    call.join().unwrap().unwrap();
    assert!(probed(&lucyd).is_empty());
}

#[test]
fn the_probe_is_never_offered_to_the_model_nor_called_by_it() {
    let lucyd = with_hands("");
    let _events = session(&lucyd);

    let names = lucyd.thinking.tool_names();
    let called = lucyd
        .thinking
        .call_tool_with("hands__text_at", json!({"x": 1, "y": 1}));

    assert!(names.contains(&"hands__click".to_owned()), "{names:?}");
    assert!(!names.contains(&"hands__text_at".to_owned()), "{names:?}");
    assert!(
        called
            .as_ref()
            .map_or(true, |result| result.is_error == Some(true)),
        "{called:?}"
    );
    assert!(probed(&lucyd).is_empty());
}

const OFFER: &str = "I can't read the page in Brave, so I'll ask before each click. Its \
                     accessibility switch turns off when Brave restarts; the \
                     force-renderer-accessibility flag in brave-flags.conf keeps it on. Want me \
                     to open Brave's accessibility settings?";

fn accessibility_off(lucyd: &Harness, browser: &str) {
    show(lucyd, "null");
    let dir = lucyd.state_dir().join("plugins/hands");
    std::fs::write(dir.join("accessibility_off"), browser).unwrap();
}

fn asked_next(lucyd: &Harness, events: &mut Client) -> String {
    while next_state(events).state != State::Confirming {}
    let prompt = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    prompt
}

fn plain_click(lucyd: &Harness) -> Call {
    model_calls(
        lucyd,
        "click",
        json!({"x": 10, "y": 20, "describe": "the Play button"}),
    )
}

#[test]
fn a_browser_with_accessibility_off_gets_the_offer_once_and_a_yes_opens_its_settings() {
    let lucyd = with_hands("");
    accessibility_off(&lucyd, "brave");
    let mut events = session(&lucyd);
    let call = plain_click(&lucyd);

    assert_eq!(asked(&lucyd, &mut events), OFFER);
    lucyd.say("yes");
    assert_eq!(
        asked_next(&lucyd, &mut events),
        "Shall I click the Play button?"
    );
    lucyd.say("yes");
    call.join().unwrap().unwrap();
    assert_eq!(
        lucyd.launcher.opened_in_browser(),
        [("brave".to_owned(), "brave://accessibility".to_owned())]
    );

    let again = plain_click(&lucyd);
    assert_eq!(
        asked_next(&lucyd, &mut events),
        "Shall I click the Play button?"
    );
    lucyd.say("no");
    again.join().unwrap().unwrap();
    assert_eq!(lucyd.launcher.opened_in_browser().len(), 1);
}

#[test]
fn a_no_to_the_offer_opens_nothing_and_the_click_still_asks() {
    let lucyd = with_hands("");
    accessibility_off(&lucyd, "chrome");
    let mut events = session(&lucyd);
    let call = plain_click(&lucyd);

    let offer = asked(&lucyd, &mut events);
    assert!(
        offer.starts_with("I can't read the page in Chrome"),
        "{offer}"
    );
    assert!(offer.contains("chrome-flags.conf"), "{offer}");
    lucyd.say("no");
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
    lucyd.tts.finish_playing();
    assert_eq!(
        asked_next(&lucyd, &mut events),
        "Shall I click the Play button?"
    );
    lucyd.say("no");

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    assert!(lucyd.launcher.opened_in_browser().is_empty());
    assert!(lucyd.launcher.opened().is_empty());
}

#[test]
fn an_app_that_returns_nothing_or_an_unknown_browser_gets_no_offer() {
    for off in [None, Some("netscape")] {
        let lucyd = with_hands("");
        show(&lucyd, "null");
        if let Some(browser) = off {
            accessibility_off(&lucyd, browser);
        }
        let mut events = session(&lucyd);
        let call = plain_click(&lucyd);

        assert_eq!(asked(&lucyd, &mut events), "Shall I click the Play button?");
        lucyd.say("no");
        call.join().unwrap().unwrap();
        assert!(lucyd.launcher.opened_in_browser().is_empty());
    }
}
