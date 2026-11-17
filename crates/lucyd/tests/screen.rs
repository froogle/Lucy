//! Screen access (ADR 0008): a Plugin tool marked `screen = "look"` shows the
//! model the screen. The first look in a Session needs a spoken yes; the image
//! reaches the model and nothing else.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::Path;
use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, State, StateEvent};
use rmcp::model::CallToolResult;
use serde_json::json;

const SCREEN: &str = "iVBORw0KGgpMdWN5U2NyZWVuU2VjcmV0";
const SCREEN_TEXT: &str = "LucyScreenSecret";
const ASKED: &str = "I need to look at your screen. Okay?";

const EYES: &str = "name = \"eyes\"\n\
     version = \"0.1.0\"\n\
     author = \"human\"\n\
     summary = \"Looks at the screen\"\n\
     command = [\"./fixture-plugin\", \"look\", \"echo\"]\n\
     \n\
     [[tools]]\n\
     name = \"look\"\n\
     handles = \"looking at the screen\"\n\
     screen = \"look\"\n\
     \n\
     [[tools]]\n\
     name = \"echo\"\n\
     handles = \"saying something back\"\n";

type Call = JoinHandle<Result<CallToolResult, String>>;

fn with_eyes(config: &str) -> Harness {
    let lucyd = Harness::with_config(config);
    lucyd.install_plugin("eyes", EYES);
    lucyd.approve("eyes");
    lucyd
}

fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's on my screen?");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    events
}

fn model_looks(lucyd: &Harness) -> Call {
    let thinking = lucyd.thinking.clone();
    std::thread::spawn(move || thinking.call_tool("eyes__look"))
}

fn images(result: &CallToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|block| block.as_image())
        .map(|image| image.data.clone())
        .collect()
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn asked_and_answered(lucyd: &Harness, events: &mut Client, answer: &str) {
    assert_eq!(next_state(events).state, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), ASKED);
    lucyd.tts.finish_playing();
    lucyd.say(answer);
}

fn first_look_allowed(lucyd: &Harness, events: &mut Client) {
    let call = model_looks(lucyd);
    asked_and_answered(lucyd, events, "yes");
    let result = call.join().unwrap().unwrap();
    assert_eq!(images(&result), [SCREEN], "{result:?}");
}

fn states_until_listening(events: &mut Client) -> Vec<StateEvent> {
    let mut seen = vec![];
    loop {
        let state = next_state(events);
        seen.push(state.clone());
        if state.state == State::Listening {
            return seen;
        }
    }
}

fn times_asked(lucyd: &Harness) -> usize {
    lucyd
        .tts
        .sentences()
        .iter()
        .filter(|said| *said == ASKED)
        .count()
}

fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = vec![];
    for entry in std::fs::read_dir(dir).into_iter().flatten() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn after_a_yes_the_plugins_image_reaches_the_model() {
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);
    let call = model_looks(&lucyd);

    asked_and_answered(&lucyd, &mut events, "yes");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(images(&result), [SCREEN]);
    assert_eq!(text(&result), "a fake screen");
}

const UNLOGGED_TEST: &str = "the_image_reaches_nothing_but_the_model";

#[test]
fn the_image_reaches_nothing_but_the_model() {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = lucyd::start_log();
    }
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);
    first_look_allowed(&lucyd, &mut events);
    let again = lucyd.thinking.call_tool("eyes__look").unwrap();
    assert_eq!(images(&again), [SCREEN]);
    lucyd.thinking.delta("You've got a terminal open.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.turn_end();
    lucyd.tts.finish_playing();

    events.end_session().unwrap();

    lucyd.assert_not_on_bus(SCREEN);
    lucyd.assert_not_on_bus(SCREEN_TEXT);
    for shown in lucyd.notifier.shown() {
        let shown = format!("{shown:?}");
        assert!(!shown.contains(SCREEN), "{shown}");
    }
    let config_dir = lucyd.config_path().parent().unwrap().to_owned();
    for file in [files_under(lucyd.state_dir()), files_under(&config_dir)].concat() {
        let kept = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
        assert!(!kept.contains(SCREEN), "{} holds the image", file.display());
        assert!(
            !kept.contains(SCREEN_TEXT),
            "{} holds the image",
            file.display()
        );
    }
}

#[test]
fn the_image_never_reaches_lucyds_log() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([UNLOGGED_TEST, "--exact", "--nocapture"])
        .env("RUST_LOG", "trace")
        .output()
        .unwrap();

    let log =
        String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
    assert!(!log.contains(SCREEN), "{log}");
    assert!(!log.contains(SCREEN_TEXT), "{log}");
}

#[test]
fn a_declined_first_look_holds_for_the_session_unasked() {
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);
    let call = model_looks(&lucyd);
    asked_and_answered(&lucyd, &mut events, "no");
    let declined = call.join().unwrap().unwrap();
    assert!(text(&declined).starts_with("declined"), "{declined:?}");
    assert!(images(&declined).is_empty());
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let again = lucyd.thinking.call_tool("eyes__look").unwrap();

    assert!(text(&again).starts_with("declined"), "{again:?}");
    assert!(images(&again).is_empty());
    assert_eq!(times_asked(&lucyd), 1);
}

#[test]
fn a_decline_lasts_only_for_its_session() {
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);
    let call = model_looks(&lucyd);
    asked_and_answered(&lucyd, &mut events, "no");
    call.join().unwrap().unwrap();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    events.end_session().unwrap();
    while next_state(&mut events).state != State::Idle {}

    let mut events = session(&lucyd);
    let call = model_looks(&lucyd);

    asked_and_answered(&lucyd, &mut events, "yes");
    assert_eq!(images(&call.join().unwrap().unwrap()), [SCREEN]);
}

#[test]
fn a_later_look_is_unasked_and_shows_the_looking_mark() {
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);
    first_look_allowed(&lucyd, &mut events);
    lucyd.thinking.turn_end();
    states_until_listening(&mut events);
    lucyd.say("and what about now?");
    lucyd.thinking.next_turn();

    let again = lucyd.thinking.call_tool("eyes__look").unwrap();

    assert_eq!(images(&again), [SCREEN]);
    lucyd.thinking.turn_end();
    let seen = states_until_listening(&mut events);
    assert!(
        seen.iter().all(|s| s.state != State::Confirming),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.looking && s.state == State::Thinking),
        "{seen:?}"
    );
    assert!(!seen.last().unwrap().looking, "{seen:?}");
    assert_eq!(times_asked(&lucyd), 1);
}

#[test]
fn an_unmarked_tool_is_never_asked_about_or_marked() {
    let lucyd = with_eyes("");
    let mut events = session(&lucyd);

    let echoed = lucyd
        .thinking
        .call_tool_with("eyes__echo", json!({"text": "hi"}))
        .unwrap();

    assert_eq!(text(&echoed), "hi");
    assert_eq!(times_asked(&lucyd), 0);
    lucyd.thinking.turn_end();
    let mut state = next_state(&mut events);
    while state.state != State::Listening {
        assert!(!state.looking, "{state:?}");
        state = next_state(&mut events);
    }
}

#[test]
fn screen_access_turned_off_drops_the_marked_tools() {
    let lucyd = with_eyes("[screen]\nenabled = false\n");
    let _events = session(&lucyd);

    let names = lucyd.thinking.tool_names();

    assert!(names.contains(&"eyes__echo".to_owned()), "{names:?}");
    assert!(!names.contains(&"eyes__look".to_owned()), "{names:?}");
    let refused = lucyd.thinking.call_tool("eyes__look");
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(times_asked(&lucyd), 0);
}
