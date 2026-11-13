//! Confirmation: a Plugin tool with `confirm = true` waits in Confirming
//! while Lucy asks, and only a yes she hears herself lets it run.

mod harness;

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use harness::{next_state, Harness};
use lucy_ipc::{Client, State};
use lucyd::adapters::NotificationKind;
use serde_json::json;

/// The fixture Plugin offering `write_file`, which needs Confirmation, with
/// `prompt` as its `confirm_prompt` if given.
fn manifest(prompt: Option<&str>) -> String {
    let prompt = prompt
        .map(|prompt| format!("confirm_prompt = {prompt:?}\n"))
        .unwrap_or_default();
    format!(
        "name = \"files\"\n\
         version = \"0.1.0\"\n\
         author = \"human\"\n\
         summary = \"Writes files\"\n\
         command = [\"./fixture-plugin\", \"write_file\"]\n\
         \n\
         [[tools]]\n\
         name = \"write_file\"\n\
         handles = \"write a note to a file\"\n\
         confirm = true\n\
         {prompt}\
         \n\
         [access]\n\
         write = [{:?}]\n",
        "/"
    )
}

/// lucyd with the `files` Plugin approved, its Confirmation prompt
/// "Write {text} to {path}?", and `config` as its config.
fn with_files_plugin(config: &str) -> Harness {
    let lucyd = Harness::with_config(config);
    lucyd.install_plugin("files", &manifest(Some("Write {text} to {path}?")));
    lucyd.approve("files");
    lucyd
}

/// The model, in a Turn, calls `files__write_file` to write "hello" to a new
/// file in the scratch dir. Returns the file and the waiting call.
fn model_writes(
    lucyd: &Harness,
    events: &mut Client,
) -> (
    std::path::PathBuf,
    JoinHandle<Result<rmcp::model::CallToolResult, String>>,
) {
    lucyd.say("save hello for me");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let file = lucyd.scratch().join("note");
    let thinking = lucyd.thinking.clone();
    let path = file.clone();
    let call = std::thread::spawn(move || {
        thinking.call_tool_with("files__write_file", json!({"path": path, "text": "hello"}))
    });
    (file, call)
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

#[test]
fn a_yes_after_the_prompt_runs_the_call() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);

    assert_eq!(next_state(&mut events).state, State::Confirming);
    let prompt = lucyd.tts.next_sentence();
    assert_eq!(prompt, format!("Write hello to {}?", file.display()));
    lucyd.tts.finish_playing();
    lucyd.say("Yes.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
    assert_eq!(next_state(&mut events).state, State::Thinking);
}

#[test]
fn anything_but_a_yes_declines_says_so_and_tells_the_model_only_declined() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("yes but write it to my desktop");

    let result = call.join().unwrap().unwrap();
    let said = text(&result);
    assert!(said.starts_with("declined"), "{said}");
    assert!(
        !said.contains("desktop"),
        "the model heard the reply: {said}"
    );
    assert!(!file.exists());
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
}

#[test]
fn a_yes_said_during_the_prompt_counts() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();

    // The prompt is still playing.
    lucyd.say("go ahead");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
}

#[test]
fn silence_after_the_prompt_declines() {
    let lucyd = with_files_plugin("[confirmation]\ntimeout_secs = 0.3\n");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    // The wait starts only once the prompt has been said.
    std::thread::sleep(Duration::from_millis(600));
    assert!(!call.is_finished(), "declined while the prompt played");
    let prompted = Instant::now();
    lucyd.tts.finish_playing();

    let result = call.join().unwrap().unwrap();
    assert!(prompted.elapsed() >= Duration::from_millis(250));
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    assert!(!file.exists());
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
}

#[test]
fn the_silence_timer_is_paused_while_confirming() {
    let lucyd = with_files_plugin("[session]\nsilence_timeout_secs = 0.3\n");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (_file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    std::thread::sleep(Duration::from_secs(1));

    assert_eq!(events.status().unwrap().state, State::Confirming);
    lucyd.say("yes");
    assert_eq!(call.join().unwrap().unwrap().is_error, Some(false));
}

#[test]
fn an_indicator_click_while_confirming_declines_and_ends_the_session() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);

    // The Indicator's click is `session.end`.
    lucyd.client().end_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Idle);
    // The relay may be cut off with the Session; either way nothing ran.
    if let Ok(result) = call.join().unwrap() {
        assert!(text(&result).starts_with("declined"), "{}", text(&result));
    }
    assert!(!file.exists());
}

#[test]
fn the_full_arguments_are_shown_and_a_generic_prompt_is_built_from_handles() {
    let lucyd = Harness::start();
    let indicator = lucyd.indicator("lucy-indicator");
    lucyd.install_plugin("files", &manifest(None));
    lucyd.approve("files");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, _call) = model_writes(&lucyd, &mut events);

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Shall I use the files Plugin to write a note to a file?"
    );
    let shown = lucyd.notifier.next_notification();
    assert_eq!(shown.kind, NotificationKind::Details);
    assert!(
        shown.body.contains("files__write_file")
            && shown.body.contains(&format!("path: {}", file.display()))
            && shown.body.contains("text: hello"),
        "{}",
        shown.body
    );
    drop(indicator);
}

#[test]
fn without_an_indicator_confirming_is_notified_and_its_end_action_declines() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy is listening"
    );
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);

    // The details and the Confirming notification come from different
    // tasks, in either order.
    let mut shown = [
        lucyd.notifier.next_notification(),
        lucyd.notifier.next_notification(),
    ];
    shown.sort_by_key(|notification| notification.kind == NotificationKind::Details);
    assert_eq!(shown[0].summary, "Lucy is waiting for your yes");
    lucyd.notifier.pick_action();

    assert_eq!(next_state(&mut events).state, State::Idle);
    if let Ok(result) = call.join().unwrap() {
        assert!(text(&result).starts_with("declined"), "{}", text(&result));
    }
    assert!(!file.exists());
}

/// lucyd with the `asker` Plugin, whose `ask` tool asks the user a question
/// mid-call, approved and a Session open in Thinking.
fn with_asker(config: &str) -> (Harness, Client) {
    let lucyd = Harness::with_config(config);
    lucyd.install_plugin(
        "asker",
        "name = \"asker\"\nversion = \"0.1.0\"\nauthor = \"human\"\nsummary = \"Asks\"\n\
         command = [\"./fixture-plugin\", \"ask\"]\n\n\
         [[tools]]\nname = \"ask\"\nhandles = \"ask something\"\n",
    );
    lucyd.approve("asker");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("find my album");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    (lucyd, events)
}

fn model_calls_ask(lucyd: &Harness) -> JoinHandle<Result<rmcp::model::CallToolResult, String>> {
    let thinking = lucyd.thinking.clone();
    std::thread::spawn(move || {
        thinking.call_tool_with(
            "asker__ask",
            json!({"question": "Which album, the summer one?"}),
        )
    })
}

#[test]
fn a_plugins_mid_call_question_is_asked_and_the_answer_relayed_to_it() {
    let (lucyd, mut events) = with_asker("");
    let call = model_calls_ask(&lucyd);

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), "Which album, the summer one?");
    lucyd.tts.finish_playing();
    lucyd.say("The one from Rainier.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(text(&result), "heard: The one from Rainier.");
    assert_eq!(next_state(&mut events).state, State::Thinking);
}

#[test]
fn a_mid_call_question_met_with_silence_goes_unanswered() {
    let (lucyd, mut events) = with_asker("[confirmation]\ntimeout_secs = 0.3\n");
    let call = model_calls_ask(&lucyd);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let result = call.join().unwrap().unwrap();

    assert_eq!(text(&result), "no answer");
    // Only a declined Confirmation is answered "Okay, I won't".
    assert!(!lucyd.tts.sentences().contains(&"Okay, I won't.".to_owned()));
}

#[test]
fn in_a_session_opened_by_the_wake_word_a_yes_after_the_wake_phrase_counts() {
    let lucyd = with_files_plugin("[wake_word]\nmodel = \"/models/hey_lucy.onnx\"\n");
    let mut events = lucyd.state_client();
    lucyd.wake();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.wake();
    lucyd.say("Hey Lucy, yes.");

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
}

// Tickets 11 and 14 together: an STT failure while Confirming.
#[test]
fn a_reply_stt_fails_on_declines_with_an_error_flash() {
    let lucyd = with_files_plugin("");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let (file, call) = model_writes(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say_but_stt_fails();

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    assert!(!file.exists());
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
    assert_eq!(next_state(&mut events).state, State::Error);
    assert_eq!(next_state(&mut events).state, State::Speaking);
}
