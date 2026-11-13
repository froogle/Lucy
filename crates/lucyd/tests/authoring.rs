//! Authoring: when no Plugin fits, the model calls `request_plugin` and
//! lucyd hands the goal to an advanced-tier Delegate. The Delegate here is
//! scripted: the test writes the Plugin into its workspace and calls the
//! Delegate tools through its real relay.

mod harness;

use std::path::Path;
use std::time::Duration;

use harness::{next_state, stage_plugin, Harness};
use lucy_config::{Config, Tier};
use lucy_ipc::{Client, State};
use rmcp::model::CallToolResult;
use serde_json::json;

const A_FEW_MINUTES: &str = "This is going to take a few minutes.";
const IS_THAT_THE_ONE: &str = "Is that the one?";
const REQUEST: &str = "find my Mount Rainier photo";

/// The Plugin the scripted Delegate writes: the fixture Plugin's `echo`,
/// handling `handles`.
fn photos_manifest(handles: &str) -> String {
    format!(
        "name = \"photos\"\n\
         version = \"0.1.0\"\n\
         author = \"lucy\"\n\
         summary = \"Finds photos\"\n\
         command = [\"./fixture-plugin\", \"echo\"]\n\
         \n\
         [[tools]]\n\
         name = \"echo\"\n\
         handles = \"{handles}\"\n"
    )
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn ok(result: Result<CallToolResult, String>) -> String {
    let result = result.unwrap();
    let said = text(&result);
    assert_eq!(result.is_error, Some(false), "{said}");
    said
}

/// Opens a Session in which the user asks for something no Plugin does,
/// and the model calls `request_plugin` for it. Returns the state client,
/// with Lucy's line about the wait heard and the model's Turn ended, so
/// Lucy is back in Listening.
fn request_photos(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say(REQUEST);
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    ok(lucyd.thinking.call_tool_with(
        "request_plugin",
        json!({
            "name": "photos",
            "goal": "find the user's photos by place or date",
            "clarifications": "Their photos are in ~/Pictures.",
            "request": REQUEST,
        }),
    ));
    assert_eq!(lucyd.tts.next_sentence(), A_FEW_MINUTES);
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.thinking.turn_end();
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Listening);
    events
}

/// Reads state events until `state`.
fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

/// Makes every command sent to the Session before this one take effect
/// first: lucyd handles them in order, and refuses this one.
fn barrier(lucyd: &Harness) {
    assert!(lucyd.client().start_session().is_err());
}

#[test]
fn request_plugin_starts_an_advanced_tier_delegate_with_the_brief_and_the_template() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);

    let advanced = Config::default().provider(Tier::Advanced).unwrap().clone();
    assert_eq!(lucyd.thinking.delegate.opened(), vec![advanced]);
    let brief = lucyd.thinking.delegate.instructions();
    for tool in ["submit_plugin", "ask_user", "cannot", "report_progress"] {
        assert!(brief.contains(tool), "the brief doesn't mention {tool}");
    }
    let turn = lucyd.thinking.delegate.next_turn();
    assert!(
        turn.contains("find the user's photos by place or date"),
        "{turn}"
    );
    assert!(turn.contains("Their photos are in ~/Pictures."), "{turn}");

    let workspace = lucyd.thinking.delegate.workspace();
    assert_eq!(workspace.dir, lucyd.plugins_dir().join(".build/photos"));
    let manifest = std::fs::read_to_string(workspace.dir.join("manifest.toml")).unwrap();
    assert!(manifest.contains("name = \"photos\""), "{manifest}");
    assert!(manifest.contains("author = \"lucy\""), "{manifest}");
    for file in [
        "Cargo.toml",
        "src/main.rs",
        "tests/support/mod.rs",
        "build.sh",
        "test.sh",
    ] {
        assert!(
            workspace.dir.join(file).is_file(),
            "the template has no {file}"
        );
    }
}

#[test]
fn the_session_stays_open_in_listening_while_the_build_runs() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 0.3\n");
    let _events = request_photos(&lucyd);

    // sleep-ok: timer test; lucyd has no clock seam yet
    std::thread::sleep(Duration::from_secs(1));

    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
    let said = lucyd.tts.sentences();
    let times = said.iter().filter(|s| *s == A_FEW_MINUTES).count();
    assert_eq!(times, 1, "{said:?}");
}

#[test]
fn the_delegate_sees_only_the_delegate_tools_and_the_model_none_of_them() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;

    let mut names = delegate.tool_names();
    names.sort();
    assert_eq!(
        names,
        ["ask_user", "cannot", "report_progress", "submit_plugin"]
    );
    assert!(delegate
        .call_tool_with("open_url", json!({"url": "https://example.com"}))
        .is_err());
    assert!(lucyd.launcher.opened().is_empty());

    let model = lucyd.thinking.tool_names();
    assert!(model.iter().any(|name| name == "request_plugin"));
    for tool in ["ask_user", "cannot", "report_progress", "submit_plugin"] {
        assert!(!model.iter().any(|name| name == tool), "{model:?}");
        assert!(lucyd.thinking.call_tool_with(tool, json!({})).is_err());
    }
}

#[test]
fn a_second_request_while_building_is_refused() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with(
            "request_plugin",
            json!({"name": "music", "goal": "play music", "clarifications": "", "request": "play jazz"}),
        )
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("photos"), "{}", text(&result));
    assert_eq!(lucyd.thinking.delegate.opened().len(), 1);
}

#[test]
fn cannot_has_lucy_state_the_limit_and_offer_the_fallback_and_ends_the_build() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();

    ok(delegate.call_tool_with(
        "cannot",
        json!({
            "reason": "Google Photos has no API for reading your library",
            "fallback": "open Google Photos search for Mount Rainier",
        }),
    ));

    let turn = lucyd.thinking.next_turn();
    assert!(
        turn.contains("Google Photos has no API for reading your library"),
        "{turn}"
    );
    assert!(
        turn.contains("open Google Photos search for Mount Rainier"),
        "{turn}"
    );
    // The Delegate's token is revoked once the build is over.
    assert!(delegate
        .call_tool_with("report_progress", json!({"text": "hi"}))
        .is_err());
}

#[test]
fn ask_user_waits_until_neither_side_is_speaking_and_the_answer_goes_to_the_delegate() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    // Lucy is answering something else, and still speaking.
    lucyd.say("what's the weather?");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    lucyd.thinking.delta("It's sunny.");
    lucyd.thinking.sentence_end();
    assert_eq!(lucyd.tts.next_sentence(), "It's sunny.");
    assert_eq!(next_state(&mut events).state, State::Speaking);

    ok(delegate.call_tool_with(
        "ask_user",
        json!({"question": "Which folder are your photos in?"}),
    ));
    delegate.turn_end();
    barrier(&lucyd);
    lucyd.thinking.turn_end();
    barrier(&lucyd);
    assert_eq!(lucyd.tts.sentences(), [A_FEW_MINUTES, "It's sunny."]);
    lucyd.tts.finish_playing();

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Which folder are your photos in?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("the Pictures folder");
    let answer = delegate.next_turn();
    assert!(answer.contains("the Pictures folder"), "{answer}");
    lucyd.thinking.assert_no_turn();
}

#[test]
fn a_failed_build_goes_back_to_the_delegate_and_lucy_says_what_failed_when_it_gives_up() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let dir = delegate.workspace().dir;
    stage_plugin(
        &dir,
        &photos_manifest("find photos by place"),
        "exit 0",
        "echo 'test finds_rainier ... FAILED'; exit 1",
    );

    let result = delegate.call_tool_with("submit_plugin", json!({})).unwrap();
    assert_eq!(result.is_error, Some(true));
    let said = text(&result);
    assert!(said.contains("test finds_rainier ... FAILED"), "{said}");

    stage_plugin(
        &dir,
        &photos_manifest("find photos by place"),
        "echo 'error[E0425]: oops' >&2; exit 1",
        "exit 0",
    );
    let said = text(&delegate.call_tool_with("submit_plugin", json!({})).unwrap());
    assert!(said.contains("error[E0425]: oops"), "{said}");
    delegate.turn_end();

    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains("photos"), "{turn}");
    assert!(turn.contains("error[E0425]: oops"), "{turn}");
    assert!(!lucyd.plugins_dir().join("photos").exists());
}

#[test]
fn a_mismatched_manifest_fails_the_submit() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let manifest = photos_manifest("find photos").replace("\"echo\"]", "\"pid\"]");
    stage_plugin(&delegate.workspace().dir, &manifest, "exit 0", "exit 0");

    let result = delegate.call_tool_with("submit_plugin", json!({})).unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("echo"), "{}", text(&result));
}

/// The Delegate submits the Plugin in `dir`, handling `handles`, and it
/// passes.
fn submit(lucyd: &Harness, dir: &Path, handles: &str) {
    stage_plugin(dir, &photos_manifest(handles), "exit 0", "exit 0");
    ok(lucyd
        .thinking
        .delegate
        .call_tool_with("submit_plugin", json!({})));
    lucyd.thinking.delegate.turn_end();
}

/// The user approves, the model answers the original request with the new
/// Plugin, and Lucy asks whether that was the one.
fn approve_and_answer(lucyd: &Harness, events: &mut Client, approval: &str) {
    until_state(events, State::Confirming);
    let asked = lucyd.tts.next_sentence();
    assert!(asked.starts_with(approval), "{asked}");
    lucyd.tts.finish_playing();
    lucyd.say("yes");

    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains(REQUEST), "{turn}");
    assert!(lucyd
        .thinking
        .tool_names()
        .iter()
        .any(|n| n == "photos__echo"));
    lucyd.thinking.delta("Here's your Mount Rainier photo.");
    lucyd.thinking.turn_end();
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Here's your Mount Rainier photo."
    );
    lucyd.tts.finish_playing();
    assert_eq!(lucyd.tts.next_sentence(), IS_THAT_THE_ONE);
    lucyd.tts.finish_playing();
}

#[test]
fn a_passing_build_is_approved_answered_and_a_no_to_is_that_the_one_is_fixed() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let dir = delegate.workspace().dir;

    submit(&lucyd, &dir, "find photos by place");
    approve_and_answer(
        &lucyd,
        &mut events,
        "The photos Plugin needs your Approval. It handles find photos by place.",
    );
    lucyd.say("no, the one from 2023");

    let fix = delegate.next_turn();
    assert!(fix.contains("no, the one from 2023"), "{fix}");
    lucyd.thinking.assert_no_turn();

    submit(&lucyd, &dir, "find photos by place or year");
    approve_and_answer(
        &lucyd,
        &mut events,
        "The photos Plugin has changed and needs your Approval again.",
    );
    lucyd.say("yes");

    barrier(&lucyd);
    delegate.assert_no_turn();
    // The build is over: its token is revoked.
    assert!(delegate
        .call_tool_with("report_progress", json!({"text": "hi"}))
        .is_err());
}

#[test]
fn the_submitted_build_and_tests_run_sandboxed_away_from_the_users_files() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let diary = lucyd.scratch().join("diary.txt");
    std::fs::write(&diary, "my diary").unwrap();
    let test = format!("cat '{}'", diary.display());
    stage_plugin(
        &delegate.workspace().dir,
        &photos_manifest("find photos"),
        "exit 0",
        &test,
    );

    let result = delegate.call_tool_with("submit_plugin", json!({})).unwrap();

    assert_eq!(result.is_error, Some(true));
    let said = text(&result);
    assert!(!said.contains("my diary"), "{said}");
    assert!(said.contains("Permission denied"), "{said}");
}
