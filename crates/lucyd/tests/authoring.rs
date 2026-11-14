//! Authoring: when no Plugin fits, the model calls `request_plugin` and
//! lucyd hands the goal to an advanced-tier Delegate. The Delegate here is
//! scripted: the test writes the Plugin into its workspace and calls the
//! Delegate tools through its real relay.

mod harness;

use std::path::Path;
use std::time::Duration;

use harness::{next_state, stage_plugin, wait_until, Harness};
use lucy_config::{Config, Tier};
use lucy_ipc::{error_code, Client, State};
use lucyd::adapters::NotificationKind;
use rmcp::model::CallToolResult;
use serde_json::json;

const A_FEW_MINUTES: &str = "This is going to take a few minutes.";
const IS_THAT_THE_ONE: &str = "Is that the one?";
const REQUEST: &str = "find my Mount Rainier photo";
const QUEUED: &str = "I'm already building the photos Plugin. I'll do this one after.";
const BUILD_STOPPED: &str = "Okay, I've stopped the build.";
const KEEP_WORKING: &str = "I'll keep working on the Plugin, and let you know when it's ready.";

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
    // The building mark may arrive before or with Speaking.
    until_state(&mut events, State::Speaking);
    lucyd.thinking.turn_end();
    lucyd.tts.finish_playing();
    until_state(&mut events, State::Listening);
    events
}

/// Reads state events until `state`.
fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

/// Ends the Session by the Indicator or `lucy end` while a build runs:
/// Lucy can't say she'll keep working on it, so she shows it.
fn end_mid_build(lucyd: &Harness) {
    lucyd.client().end_session().unwrap();
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert_eq!(shown.summary, "Lucy is still building the photos Plugin");
    assert_eq!(
        shown.body,
        "She'll keep working on it, and let you know when it's ready."
    );
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
fn the_state_carries_a_building_mark_while_a_build_runs_even_after_the_session_ends() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    assert!(lucyd.client().status().unwrap().building);

    lucyd.client().end_session().unwrap();
    let ended = next_state(&mut events);
    assert_eq!((ended.state, ended.building), (State::Idle, true));

    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    ok(delegate.call_tool_with(
        "cannot",
        json!({"reason": "no API", "fallback": "open the website"}),
    ));
    let over = next_state(&mut events);
    assert_eq!((over.state, over.building), (State::Idle, false));
}

#[test]
fn the_session_stays_open_in_listening_while_the_build_runs() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 30\n");
    let _events = request_photos(&lucyd);

    lucyd.clock.advance(Duration::from_secs(60));

    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
    let said = lucyd.tts.sentences();
    let times = said.iter().filter(|s| *s == A_FEW_MINUTES).count();
    assert_eq!(times, 1, "{said:?}");
}

#[test]
fn a_voice_preview_is_refused_while_a_build_holds_the_session_open() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);

    let refused = lucyd.client().voice_preview("hello");

    match refused {
        Err(lucy_ipc::Error::Rpc { code, .. }) => assert_eq!(code, error_code::SESSION_OPEN),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(lucyd.tts.sentences(), [A_FEW_MINUTES]);
    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
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

/// The model asks for a music Plugin.
fn request_music(lucyd: &Harness) -> CallToolResult {
    lucyd
        .thinking
        .call_tool_with(
            "request_plugin",
            json!({"name": "music", "goal": "play music", "clarifications": "", "request": "play jazz"}),
        )
        .unwrap()
}

#[test]
fn a_second_request_waits_for_the_build_running_and_a_third_is_refused() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();

    let queued = request_music(&lucyd);
    assert_eq!(queued.is_error, Some(false), "{}", text(&queued));
    assert_eq!(lucyd.tts.next_sentence(), QUEUED);
    lucyd.tts.finish_playing();
    let refused = lucyd
        .thinking
        .call_tool_with(
            "request_plugin",
            json!({"name": "maps", "goal": "find places", "clarifications": "", "request": "where's Rome?"}),
        )
        .unwrap();
    assert_eq!(refused.is_error, Some(true));
    assert!(text(&refused).contains("music"), "{}", text(&refused));
    assert_eq!(delegate.opened().len(), 1);

    ok(delegate.call_tool_with(
        "cannot",
        json!({"reason": "no API", "fallback": "open the website"}),
    ));

    let next = delegate.next_turn();
    assert!(next.contains("play jazz"), "{next}");
    assert_eq!(delegate.opened().len(), 2);
    assert!(lucyd.client().status().unwrap().building);
}

#[test]
fn cancel_it_stops_the_build_and_the_one_queued() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    ok(Ok(request_music(&lucyd)));
    assert_eq!(lucyd.tts.next_sentence(), QUEUED);
    lucyd.tts.finish_playing();

    lucyd.say("Cancel it.");

    assert_eq!(lucyd.tts.next_sentence(), BUILD_STOPPED);
    assert!(delegate
        .call_tool_with("report_progress", json!({"text": "hi"}))
        .is_err());
    wait_until("the build to stop", || {
        (!lucyd.client().status().unwrap().building).then_some(())
    });
    lucyd.thinking.assert_no_turn();
    delegate.assert_no_turn();
    assert_eq!(delegate.opened().len(), 1);
}

#[test]
fn report_progress_is_shown_as_a_notification_and_never_spoken() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();

    ok(delegate.call_tool_with(
        "report_progress",
        json!({"text": "Writing the photo search"}),
    ));

    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert!(shown.summary.contains("photos"), "{shown:?}");
    assert_eq!(shown.body, "Writing the photo search");
    lucyd.barrier();
    assert_eq!(lucyd.tts.sentences(), [A_FEW_MINUTES]);
}

#[test]
fn progress_notifications_can_be_switched_off() {
    let lucyd = Harness::with_config("[authoring]\nprogress_notifications = false\n");
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();

    ok(delegate.call_tool_with(
        "report_progress",
        json!({"text": "Writing the photo search"}),
    ));
    end_mid_build(&lucyd);
    ok(delegate.call_tool_with(
        "cannot",
        json!({"reason": "no API", "fallback": "open the website"}),
    ));

    // Everything shown up to the build's last word.
    loop {
        let shown = lucyd.notifier.next_notification();
        assert_ne!(shown.body, "Writing the photo search", "{shown:?}");
        if shown.summary == "Lucy can't build the photos Plugin" {
            break;
        }
    }
}

#[test]
fn a_build_out_of_time_stops_with_a_plain_report_and_asking_again_resumes_its_work() {
    let lucyd = Harness::with_config("[authoring]\nbudget_secs = 600\n");
    let _events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let dir = delegate.workspace().dir;
    std::fs::write(dir.join("notes.txt"), "half done").unwrap();

    lucyd.clock.advance(Duration::from_secs(599));
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
    lucyd.clock.advance(Duration::from_secs(1));

    let report = lucyd.thinking.next_turn();
    assert!(report.contains("photos"), "{report}");
    assert!(report.contains("ran out of time"), "{report}");
    assert!(report.contains("10 minutes"), "{report}");
    assert!(report.contains("try again"), "{report}");
    assert!(delegate
        .call_tool_with("report_progress", json!({"text": "hi"}))
        .is_err());
    assert!(!lucyd.client().status().unwrap().building);

    ok(lucyd.thinking.call_tool_with(
        "request_plugin",
        json!({
            "name": "photos",
            "goal": "find the user's photos by place or date",
            "clarifications": "Their photos are in ~/Pictures.",
            "request": REQUEST,
        }),
    ));
    let resumed = delegate.next_turn();
    assert!(resumed.contains("earlier attempt"), "{resumed}");
    assert_eq!(delegate.workspace().dir, dir);
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "half done"
    );
}

#[test]
fn ending_the_session_by_voice_mid_build_says_lucy_keeps_working_and_the_build_goes_on() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();

    lucyd.say("that's all, bye");
    lucyd.thinking.next_turn();
    ok(lucyd.thinking.call_tool("end_session"));
    lucyd.thinking.turn_end();

    assert_eq!(lucyd.tts.next_sentence(), KEEP_WORKING);
    lucyd.tts.finish_playing();
    until_state(&mut events, State::Idle);
    ok(delegate.call_tool_with("report_progress", json!({"text": "Still going"})));
    assert!(lucyd.client().status().unwrap().building);
}

#[test]
fn ending_the_session_by_the_indicator_mid_build_shows_that_lucy_keeps_working() {
    let lucyd = Harness::start();
    let _events = request_photos(&lucyd);
    lucyd.thinking.delegate.next_turn();

    end_mid_build(&lucyd);

    assert_eq!(lucyd.tts.sentences(), [A_FEW_MINUTES]);
    assert!(lucyd.client().status().unwrap().building);
}

#[test]
fn with_no_session_open_cannot_is_shown_plainly_and_told_once_at_the_next_session_start() {
    const NO_API: &str = "Google Photos has no API for reading your library";
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    end_mid_build(&lucyd);
    until_state(&mut events, State::Idle);

    ok(delegate.call_tool_with(
        "cannot",
        json!({
            "reason": NO_API,
            "fallback": "open Google Photos search for Mount Rainier",
        }),
    ));
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert_eq!(shown.summary, "Lucy can't build the photos Plugin");
    assert_eq!(
        shown.body,
        format!("{NO_API}.\nShe'll tell you more at your next Session.")
    );

    lucyd.client().start_session().unwrap();
    let told = lucyd.thinking.next_turn();
    assert!(told.contains(NO_API), "{told}");
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();
    lucyd.client().start_session().unwrap();
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
    let shown = lucyd.notifier.shown();
    assert!(!shown.iter().any(|n| n.body.contains(NO_API)), "{shown:?}");
}

#[test]
fn with_no_session_open_a_queued_build_that_cant_start_is_shown_plainly_and_told_once_later() {
    const TAKEN: &str = "there's already a Plugin called music that Lucy didn't write";
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    ok(Ok(request_music(&lucyd)));
    assert_eq!(lucyd.tts.next_sentence(), QUEUED);
    lucyd.tts.finish_playing();
    end_mid_build(&lucyd);
    until_state(&mut events, State::Idle);
    let theirs = photos_manifest("play music").replace("\"photos\"", "\"music\"");
    lucyd.install_plugin("music", &theirs.replace("\"lucy\"", "\"ann\""));

    ok(delegate.call_tool_with(
        "cannot",
        json!({"reason": "no API", "fallback": "open the website"}),
    ));
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert_eq!(shown.summary, "Lucy can't build the photos Plugin");
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert_eq!(shown.summary, "Lucy couldn't start the music Plugin");
    assert_eq!(
        shown.body,
        format!("I can't build it: {TAKEN}.\nShe'll tell you more at your next Session.")
    );

    lucyd.client().start_session().unwrap();
    lucyd.thinking.next_turn();
    lucyd.thinking.turn_end();
    let told = lucyd.thinking.next_turn();
    assert!(told.contains(TAKEN), "{told}");
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();
    lucyd.client().start_session().unwrap();
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
}

#[test]
fn the_building_mark_is_off_while_a_build_waits_on_the_user() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    end_mid_build(&lucyd);
    until_state(&mut events, State::Idle);

    ok(delegate.call_tool_with(
        "ask_user",
        json!({"question": "Which folder are your photos in?"}),
    ));
    let waiting = next_state(&mut events);
    assert_eq!((waiting.state, waiting.building), (State::Idle, false));

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("the Pictures folder");
    delegate.next_turn();
    wait_until("the building mark", || {
        lucyd.client().status().unwrap().building.then_some(())
    });
}

#[test]
fn a_kept_question_is_shown_once_however_many_sessions_end_before_it_is_asked() {
    const QUESTION: &str = "Which folder are your photos in?";
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    end_mid_build(&lucyd);
    ok(delegate.call_tool_with("ask_user", json!({"question": QUESTION})));
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert!(shown.body.contains(QUESTION), "{shown:?}");

    // The user is still speaking, so it isn't asked before this Session ends.
    lucyd.start_speaking();
    lucyd.client().start_session().unwrap();
    lucyd.client().end_session().unwrap();
    lucyd.mumble();
    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), QUESTION);

    let shown = lucyd.notifier.shown();
    assert!(
        !shown.iter().any(|n| n.body.contains(QUESTION)),
        "{shown:?}"
    );
}

#[test]
fn a_question_parked_with_no_session_open_is_shown_then_asked_at_the_next_session_start() {
    let lucyd = Harness::with_config("[authoring]\nbudget_secs = 600\n");
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    end_mid_build(&lucyd);

    ok(delegate.call_tool_with(
        "ask_user",
        json!({"question": "Which folder are your photos in?"}),
    ));
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert!(
        shown.body.contains("Which folder are your photos in?"),
        "{shown:?}"
    );
    // The budget doesn't count while the question waits.
    lucyd.clock.advance(Duration::from_secs(3600));
    delegate.assert_no_turn();

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
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
    lucyd.barrier();
    lucyd.thinking.turn_end();
    lucyd.barrier();
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

    lucyd.barrier();
    delegate.assert_no_turn();
    // The build is over: its token is revoked.
    assert!(delegate
        .call_tool_with("report_progress", json!({"text": "hi"}))
        .is_err());
}

#[test]
fn an_approval_whose_session_ends_before_the_answer_is_shown_plainly_and_answered_once_later() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    submit(&lucyd, &delegate.workspace().dir, "find photos by place");
    until_state(&mut events, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    // The user goes on speaking, so the answer waits; then the Session ends.
    lucyd.say_and_go_on("yes");
    until_state(&mut events, State::Listening);
    lucyd.client().end_session().unwrap();
    let shown = loop {
        let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
        if shown.summary != "Lucy is still building the photos Plugin" {
            break shown;
        }
    };
    assert_eq!(shown.summary, "The photos Plugin is approved");
    assert_eq!(
        shown.body,
        format!("Lucy will answer \"{REQUEST}\" with it at your next Session.")
    );
    lucyd.mumble();

    lucyd.client().start_session().unwrap();
    let told = lucyd.thinking.next_turn();
    assert!(told.contains(REQUEST), "{told}");
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();
    lucyd.client().start_session().unwrap();
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
}

#[test]
fn a_build_that_passes_after_the_session_ended_is_notified_and_offered_at_the_next_session_start() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let dir = delegate.workspace().dir;
    end_mid_build(&lucyd);
    until_state(&mut events, State::Idle);

    submit(&lucyd, &dir, "find photos by place");
    let shown = lucyd.notifier.next_notification_of(NotificationKind::Build);
    assert_eq!(shown.summary, "The photos Plugin is ready");
    // It waits on the user, so the mark is off.
    wait_until("the building mark to go", || {
        (!lucyd.client().status().unwrap().building).then_some(())
    });

    lucyd.client().start_session().unwrap();
    approve_and_answer(
        &lucyd,
        &mut events,
        "The photos Plugin needs your Approval. It handles find photos by place.",
    );
    lucyd.say("yes");
    wait_until("the build to end", || {
        (!lucyd.client().status().unwrap().building).then_some(())
    });

    // Offered once: the next Session doesn't ask again.
    lucyd.client().end_session().unwrap();
    lucyd.client().start_session().unwrap();
    lucyd.barrier();
    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
}

#[test]
fn a_plugin_built_after_the_session_ended_and_approved_at_the_next_gets_its_credential() {
    let lucyd = Harness::start();
    let mut events = request_photos(&lucyd);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    let dir = delegate.workspace().dir;
    end_mid_build(&lucyd);
    until_state(&mut events, State::Idle);
    lucyd.credentials.enter("photos", "PHOTOS_KEY", "p-123");

    let manifest = photos_manifest("find photos by place")
        .replace("\"echo\"]", "\"env\"]")
        .replace("name = \"echo\"", "name = \"env\"")
        + "\n[access]\ncredentials = [\"PHOTOS_KEY\"]\n";
    stage_plugin(&dir, &manifest, "exit 0", "exit 0");
    ok(delegate.call_tool_with("submit_plugin", json!({})));
    delegate.turn_end();
    lucyd.notifier.next_notification_of(NotificationKind::Build);

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    lucyd.thinking.next_turn();
    ok(lucyd.thinking.call_tool_with("photos__env", json!({})));

    let env = std::fs::read_to_string(lucyd.state_dir().join("plugins/photos/env")).unwrap();
    assert!(env.lines().any(|line| line == "PHOTOS_KEY=p-123"), "{env}");
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
