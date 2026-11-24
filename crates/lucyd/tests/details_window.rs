//! The Details window: items Lucy shows beside a question for the user to read.
//!
//! A Plugin's describe tool fills it for a Confirmation, a mid-call question
//! carries its own, and Lucy writes them for a Hand-off's permission
//! requests. The `input` holder draws it.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::PathBuf;
use std::thread::JoinHandle;

use harness::{next_state, Harness};
use lucy_ipc::{Client, Event, State};
use rmcp::model::CallToolResult;
use serde_json::json;

type Call = JoinHandle<Result<CallToolResult, String>>;

const PROMPT: &str = "Write the note?";

fn mail_manifest(describe: &str) -> String {
    format!(
        "name = \"notes\"\nversion = \"0.1.0\"\nauthor = \"human\"\nsummary = \"Writes notes\"\n\
         command = [\"./fixture-plugin\", \"write_file\", \"{describe}\"]\n\n\
         [[tools]]\nname = \"write_file\"\nhandles = \"write a note\"\nconfirm = true\n\
         confirm_prompt = {PROMPT:?}\ndescribe = \"{describe}\"\n\n\
         [access]\nwrite = [\"/\"]\n"
    )
}

fn with_notes(describe: &str) -> Harness {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &mail_manifest(describe));
    lucyd.approve("notes");
    lucyd
}

fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    events
}

fn model_writes(lucyd: &Harness, events: &mut Client, text: &str) -> (PathBuf, Call) {
    lucyd.say("save a note for me");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let file = lucyd.scratch().join("note");
    let thinking = lucyd.thinking.clone();
    let arguments = json!({"path": file, "text": text});
    let call = std::thread::spawn(move || thinking.call_tool_with("notes__write_file", arguments));
    (file, call)
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn next_close(holder: &mut Client) -> String {
    loop {
        if let Event::InputClose(close) = holder.next_event().expect("an input.close") {
            return close.id;
        }
    }
}

#[test]
fn a_confirmation_with_a_describe_tool_shows_its_items_instead_of_the_notification() {
    let lucyd = with_notes("describe_write");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let (file, call) = model_writes(&lucyd, &mut events, "hello");

    let open = lucyd.next_details(&mut holder);
    assert_eq!(open.message, PROMPT);
    assert!(open.fields.is_empty(), "{open:?}");
    assert_eq!(
        open.items,
        [
            format!("## {}", file.display()),
            "It will say *hello*.".to_owned()
        ]
    );
    assert_eq!(lucyd.tts.next_sentence(), PROMPT);
    lucyd.tts.finish_playing();
    lucyd.say("Yes.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
    assert_eq!(next_close(&mut holder), open.id);
    lucyd.notifier.assert_no_notification();
}

fn asked(lucyd: &Harness, events: &mut Client, text: &str) -> (PathBuf, Call) {
    let (file, call) = model_writes(lucyd, events, text);
    assert_eq!(next_state(events).state, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), PROMPT);
    lucyd.tts.finish_playing();
    (file, call)
}

#[test]
fn a_no_declines_and_closes_the_window() {
    let lucyd = with_notes("describe_write");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");
    let open = lucyd.next_details(&mut holder);

    lucyd.say("no");

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    assert!(!file.exists());
    assert_eq!(next_close(&mut holder), open.id);
}

#[test]
fn silence_declines_and_closes_the_window() {
    let lucyd = Harness::with_config("[confirmation]\ntimeout_secs = 2\n");
    lucyd.install_plugin("notes", &mail_manifest("describe_write"));
    lucyd.approve("notes");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");
    let open = lucyd.next_details(&mut holder);
    lucyd.barrier();

    lucyd.clock.advance(std::time::Duration::from_secs(2));

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    assert!(!file.exists());
    assert_eq!(next_close(&mut holder), open.id);
}

#[test]
fn the_session_ending_closes_the_window() {
    let lucyd = with_notes("describe_write");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let (_file, _call) = asked(&lucyd, &mut events, "hello");
    let open = lucyd.next_details(&mut holder);

    events.end_session().unwrap();

    assert_eq!(next_close(&mut holder), open.id);
}

#[test]
fn closing_the_window_early_answers_nothing_and_the_question_still_waits() {
    let lucyd = with_notes("describe_write");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");
    let open = lucyd.next_details(&mut holder);

    assert!(holder.answer_input(&open.id, Default::default()).is_err());
    holder.cancel_input(&open.id).unwrap();
    lucyd.barrier();

    assert_eq!(lucyd.client().status().unwrap().state, State::Confirming);
    lucyd.say("yes");
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
}

#[test]
fn the_model_is_never_offered_the_describe_tool_and_a_call_to_it_is_refused() {
    let lucyd = with_notes("describe_write");
    let _events = session(&lucyd);

    let tools = lucyd.thinking.tool_names();
    assert!(tools.contains(&"notes__write_file".to_owned()), "{tools:?}");
    assert!(
        !tools.iter().any(|tool| tool.contains("describe")),
        "{tools:?}"
    );

    let file = lucyd.scratch().join("note");
    let called = lucyd.thinking.call_tool_with(
        "notes__describe_write",
        json!({"path": file, "text": "hello"}),
    );
    let refused = match called {
        Err(_) => true,
        Ok(result) => result.is_error == Some(true),
    };
    assert!(refused);
    assert!(lucyd.tts.sentences().is_empty());
}

fn falls_back_to_the_notification(text_argument: &str) {
    let lucyd = with_notes("describe_badly");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);

    let (file, call) = asked(&lucyd, &mut events, text_argument);

    let shown = lucyd.notifier.next_notification();
    assert_eq!(shown.summary, "Lucy needs your yes");
    assert!(shown.body.contains("notes__write_file"), "{}", shown.body);
    lucyd.say("yes");
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert!(file.exists());
    let opened = holder.next_event_within(std::time::Duration::from_millis(100));
    assert!(
        !matches!(opened, Ok(Some(Event::InputOpen(_)))),
        "{opened:?}"
    );
}

#[test]
fn a_describe_tool_that_fails_falls_back_to_the_notification() {
    falls_back_to_the_notification("fail");
}

#[test]
fn a_describe_tool_with_no_items_falls_back_to_the_notification() {
    falls_back_to_the_notification("empty");
}

#[test]
fn a_describe_tool_with_a_malformed_result_falls_back_to_the_notification() {
    falls_back_to_the_notification("malformed");
}

fn describing(file: &std::path::Path) {
    let describing = file.with_extension("describing");
    harness::wait_until("the describe tool to start", || {
        describing.exists().then_some(())
    });
}

#[test]
fn a_describe_tool_slower_than_five_seconds_falls_back_to_the_notification() {
    let lucyd = with_notes("describe_badly");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = session(&lucyd);
    let (file, call) = model_writes(&lucyd, &mut events, "stall");
    describing(&file);

    lucyd.clock.advance(std::time::Duration::from_secs(5));

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), PROMPT);
    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy needs your yes"
    );
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
}

#[test]
fn a_barge_in_while_describing_cancels_the_describe_tool_and_the_call() {
    let lucyd = with_notes("describe_badly");
    let mut events = session(&lucyd);
    let (file, call) = model_writes(&lucyd, &mut events, "stall");
    describing(&file);

    lucyd.wake();

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("cancelled"), "{}", text(&result));
    let cancelled = file.with_extension("cancelled");
    harness::wait_until("the describe call to be cancelled", || {
        cancelled.exists().then_some(())
    });
    assert!(!file.exists());
    assert!(lucyd.tts.sentences().is_empty());
}

#[test]
fn a_describe_tool_on_a_tool_without_confirm_makes_the_plugin_broken() {
    let lucyd = Harness::start();
    let unconfirmed = mail_manifest("describe_write").replace("confirm = true\n", "");
    lucyd.install_plugin("notes", &unconfirmed);

    let listed = lucyd.client().plugins().unwrap().plugins;

    let plugin = listed.iter().find(|p| p.name == "notes").unwrap();
    assert_eq!(plugin.status, lucy_ipc::PluginStatus::Broken);
    let problem = plugin.problem.as_deref().unwrap_or_default();
    assert!(problem.contains("write_file"), "{problem}");
    assert!(problem.contains("confirm"), "{problem}");
}

#[test]
fn the_approval_summary_says_lucy_describes_the_tool_before_asking() {
    let lucyd = Harness::start();
    lucyd.install_plugin("notes", &mail_manifest("describe_write"));
    let _events = session(&lucyd);

    let _approval = lucyd.request_approval("notes");

    let summary = lucyd.tts.next_sentence();
    let said = [summary, lucyd.tts.sentences().join(" ")].join(" ");
    assert!(
        said.contains(
            "Before it asks you to write a note, Lucy has it describe the details on screen."
        ),
        "{said}"
    );
}

#[test]
fn with_no_holder_the_window_opens_in_a_terminal() {
    let lucyd = with_notes("describe_write");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");

    let opened = harness::wait_until("a terminal to open", || lucyd.launcher.terminals().pop());
    let socket = lucyd.socket_path().display().to_string();
    assert_eq!(opened[1..], ["input", "--socket", &socket]);
    let mut page = lucyd.input("lucy input");
    let open = lucyd.next_details(&mut page);
    assert_eq!(open.items[1], "It will say *hello*.");
    lucyd.barrier();
    lucyd.clock.advance(std::time::Duration::from_secs(5));

    lucyd.say("yes");
    call.join().unwrap().unwrap();
    assert!(file.exists());
    assert_eq!(next_close(&mut page), open.id);
    lucyd.notifier.assert_no_notification();
}

#[test]
fn when_no_terminal_claims_the_window_soon_the_notification_is_shown_and_the_yes_still_counts() {
    let lucyd = with_notes("describe_write");
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");
    harness::wait_until("a terminal to open", || lucyd.launcher.terminals().pop());

    lucyd
        .clock
        .advance_once_set(std::time::Duration::from_secs(5));

    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy needs your yes"
    );
    lucyd.say("yes");
    call.join().unwrap().unwrap();
    assert!(file.exists());
}

#[test]
fn with_no_terminal_the_notification_is_shown_at_once() {
    let lucyd = with_notes("describe_write");
    lucyd.launcher.no_terminal();
    let _indicator = lucyd.indicator("lucy-indicator");
    let mut events = session(&lucyd);
    let (file, call) = asked(&lucyd, &mut events, "hello");

    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy needs your yes"
    );
    lucyd.say("yes");
    call.join().unwrap().unwrap();
    assert!(file.exists());
}

const ITEMS: [&str; 2] = ["**From:** Ada", "*Subject:* Lunch on Friday"];

fn with_asker() -> Harness {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "asker",
        "name = \"asker\"\nversion = \"0.1.0\"\nauthor = \"human\"\nsummary = \"Asks\"\n\
         command = [\"./fixture-plugin\", \"ask_about\", \"type_about\"]\n\n\
         [[tools]]\nname = \"ask_about\"\nhandles = \"ask about an email\"\n\n\
         [[tools]]\nname = \"type_about\"\nhandles = \"ask for a link about an email\"\n",
    );
    lucyd.approve("asker");
    lucyd
}

fn model_calls(lucyd: &Harness, events: &mut Client, tool: &str) -> Call {
    lucyd.say("deal with my email");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let thinking = lucyd.thinking.clone();
    let tool = format!("asker__{tool}");
    let arguments = json!({"question": "Reply to this one?"});
    std::thread::spawn(move || thinking.call_tool_with(&tool, arguments))
}

#[test]
fn a_spoken_mid_call_question_shows_its_items_on_their_own() {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = env_logger::try_init();
    }
    let lucyd = with_asker();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, &mut events, "ask_about");

    let open = lucyd.next_details(&mut holder);
    assert_eq!(open.message, "Reply to this one?");
    assert!(open.fields.is_empty(), "{open:?}");
    assert_eq!(open.items, ITEMS);
    assert_eq!(lucyd.tts.next_sentence(), "Reply to this one?");
    lucyd.tts.finish_playing();
    lucyd.say("Say I'll be there.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(text(&result), "heard: Say I'll be there.");
    assert_eq!(next_close(&mut holder), open.id);
    for item in ITEMS {
        assert!(!text(&result).contains(item), "{}", text(&result));
    }
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn a_typed_mid_call_question_shows_its_items_above_the_fields() {
    let lucyd = with_asker();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, &mut events, "type_about");
    assert_eq!(next_state(&mut events).state, State::Typing);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let open = lucyd.next_details(&mut holder);
    assert_eq!(open.items, ITEMS);
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].name, "url");
    lucyd.answer_fields(
        &mut holder,
        &open.id,
        &[("url", "https://example.com/reply")],
    );

    let result = call.join().unwrap().unwrap();
    assert!(
        text(&result).contains("https://example.com/reply"),
        "{}",
        text(&result)
    );
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn a_cancel_phrase_closes_a_typed_question_with_items() {
    let lucyd = with_asker();
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd, &mut events, "type_about");
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    let open = lucyd.next_details(&mut holder);

    lucyd.say("never mind");

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).contains("cancelled"), "{}", text(&result));
    assert_eq!(next_close(&mut holder), open.id);
}

const ITEMS_TEST: &str = "a_spoken_mid_call_question_shows_its_items_on_their_own";

#[test]
fn items_never_reach_lucyds_log_or_a_plugins_stderr() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([ITEMS_TEST, "--exact", "--nocapture"])
        .env("RUST_LOG", "trace")
        .output()
        .unwrap();

    let log =
        String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
    assert!(!log.contains("Lunch on Friday"), "{log}");
}
