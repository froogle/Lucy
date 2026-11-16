//! A Plugin's mid-call question: one plain string field is asked aloud; a
//! `uri` field, more than one field or Lucy's `secret` field opens the Input
//! dialog. A secret goes to the Secret Service, never to the Plugin or model.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, Event, FieldKind, InputOpen, State};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

type Call = JoinHandle<Result<CallToolResult, String>>;

const TOOLS: [&str; 5] = ["ask", "ask_url", "ask_two", "ask_secret", "env"];

fn asker(credentials: &[&str]) -> Harness {
    let lucyd = Harness::start();
    let command: Vec<String> = TOOLS.iter().map(|tool| format!(", \"{tool}\"")).collect();
    let tools: Vec<String> = TOOLS
        .iter()
        .map(|tool| format!("\n[[tools]]\nname = \"{tool}\"\nhandles = \"{tool} things\"\n"))
        .collect();
    let credentials: Vec<String> = credentials.iter().map(|c| format!("\"{c}\"")).collect();
    lucyd.install_plugin(
        "asker",
        &format!(
            "name = \"asker\"\nversion = \"0.1.0\"\nauthor = \"human\"\nsummary = \"Asks\"\n\
             command = [\"./fixture-plugin\"{}]\n{}\n[access]\ncredentials = [{}]\n",
            command.concat(),
            tools.concat(),
            credentials.join(", ")
        ),
    );
    lucyd.approve("asker");
    lucyd
}

fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("find my album");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    events
}

fn model_calls(lucyd: &Harness, tool: &str, arguments: Value) -> Call {
    let thinking = lucyd.thinking.clone();
    let tool = format!("asker__{tool}");
    std::thread::spawn(move || thinking.call_tool_with(&tool, arguments))
}

fn next_open(holder: &mut Client) -> InputOpen {
    loop {
        if let Event::InputOpen(open) = holder.next_event().expect("an input.open event") {
            return open;
        }
    }
}

fn line_said(lucyd: &Harness) -> String {
    let line = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    line
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn answered(result: &CallToolResult) -> Value {
    let text = text(result);
    let json = text.strip_prefix("answered: ").expect(&text);
    serde_json::from_str(json).unwrap()
}

#[test]
fn a_uri_question_opens_the_box_and_the_answer_goes_back_to_the_plugin() {
    let lucyd = asker(&[]);
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "ask_url", json!({"question": "Which page?"}));

    assert_eq!(next_state(&mut events).state, State::Typing);
    line_said(&lucyd);
    let open = next_open(&mut holder);
    assert_eq!(open.message, "Which page?");
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].name, "url");
    assert_eq!(open.fields[0].kind, FieldKind::Url);
    lucyd.answer_fields(&mut holder, &open.id, &[("url", "https://example.com/a")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        answered(&result),
        json!({"action": "accept", "content": {"url": "https://example.com/a"}})
    );
}

#[test]
fn a_question_with_two_fields_opens_the_box_and_each_answer_keeps_its_type() {
    let lucyd = asker(&[]);
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "ask_two", json!({"question": "Who and how many?"}));

    assert_eq!(next_state(&mut events).state, State::Typing);
    assert_eq!(
        line_said(&lucyd),
        "I've put a box on screen for your answers."
    );
    let open = next_open(&mut holder);
    let fields: Vec<(&str, &FieldKind)> = open
        .fields
        .iter()
        .map(|field| (&*field.name, &field.kind))
        .collect();
    assert_eq!(
        fields,
        [("count", &FieldKind::Number), ("name", &FieldKind::Text)]
    );
    assert_eq!(open.fields[1].description.as_deref(), Some("Name"));
    lucyd.answer_fields(&mut holder, &open.id, &[("name", "Ada"), ("count", "3")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        answered(&result),
        json!({"action": "accept", "content": {"name": "Ada", "count": 3}})
    );
}

#[test]
fn a_question_with_one_plain_field_is_still_asked_aloud() {
    let lucyd = asker(&[]);
    let _holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "ask", json!({"question": "Which album?"}));

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(line_said(&lucyd), "Which album?");
    lucyd.say("The summer one.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(text(&result), "heard: The summer one.");
}

fn env_of(lucyd: &Harness) -> String {
    let path = lucyd.state_dir().join("plugins").join("asker").join("env");
    std::fs::read_to_string(path).unwrap()
}

const SECRET_TEST: &str =
    "a_secret_goes_to_the_keyring_and_the_plugins_next_start_while_the_model_sees_only_provided";

#[test]
fn a_secret_goes_to_the_keyring_and_the_plugins_next_start_while_the_model_sees_only_provided() {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = env_logger::try_init();
    }
    let lucyd = asker(&["LINEAR_KEY"]);
    lucyd.credentials.enter("asker", "LINEAR_KEY", "lin-old");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "ask_secret", json!({"name": "LINEAR_KEY"}));

    assert_eq!(next_state(&mut events).state, State::Typing);
    assert_eq!(
        line_said(&lucyd),
        "I've put a box on screen for your secret."
    );
    let open = next_open(&mut holder);
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].name, "LINEAR_KEY");
    assert_eq!(open.fields[0].kind, FieldKind::Secret);
    lucyd.answer_fields(&mut holder, &open.id, &[("LINEAR_KEY", "lin-n3w-s3cret")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        answered(&result),
        json!({"action": "accept", "content": {"LINEAR_KEY": "provided"}})
    );
    assert_eq!(lucyd.credentials.values(), ["lin-n3w-s3cret".to_owned()]);
    let shown = format!("{:?}", lucyd.notifier.shown());
    assert!(!shown.contains("lin-n3w-s3cret"), "{shown}");

    events.end_session().unwrap();
    while next_state(&mut events).state != State::Idle {}
    session(&lucyd);
    let result = lucyd
        .thinking
        .call_tool_with("asker__env", json!({}))
        .unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert!(
        env_of(&lucyd).contains("LINEAR_KEY=lin-n3w-s3cret\n"),
        "{}",
        env_of(&lucyd)
    );
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn a_secret_never_reaches_lucyds_log_or_a_plugins_stderr() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([SECRET_TEST, "--exact", "--nocapture"])
        .env("RUST_LOG", "trace")
        .output()
        .unwrap();

    let log =
        String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
    assert!(!log.contains("lin-n3w-s3cret"), "{log}");
}

#[test]
fn a_secret_on_a_name_the_manifest_does_not_declare_is_declined_unasked() {
    let lucyd = asker(&[]);
    let _holder = lucyd.input("a widget");
    let _events = session(&lucyd);
    let call = model_calls(&lucyd, "ask_secret", json!({"name": "OTHER_KEY"}));

    let result = call.join().unwrap().unwrap();
    assert_eq!(answered(&result), json!({"action": "decline"}));
    assert!(lucyd.tts.sentences().is_empty());
    assert!(lucyd.credentials.values().is_empty());
}

#[test]
fn a_secret_left_untyped_tells_the_plugin_it_timed_out() {
    let lucyd = asker(&["LINEAR_KEY"]);
    lucyd.credentials.enter("asker", "LINEAR_KEY", "lin-old");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, "ask_secret", json!({"name": "LINEAR_KEY"}));
    assert_eq!(next_state(&mut events).state, State::Typing);
    line_said(&lucyd);
    next_open(&mut holder);

    lucyd
        .clock
        .advance_once_set(std::time::Duration::from_secs(120));

    let result = call.join().unwrap().unwrap();
    assert_eq!(
        answered(&result),
        json!({"action": "cancel", "_meta": {"lucy/result": "timed_out"}})
    );
    assert_eq!(lucyd.credentials.values(), ["lin-old".to_owned()]);
}
