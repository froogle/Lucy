//! Hand-off: "tell Claude to …" passes the request to the user's own Claude.
//! The Hand-off here is scripted: the test reads its Turn, calls the
//! permission tool through its real relay and ends its Turn with an answer.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::Path;
use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, wait_until, Harness};
use lucy_ipc::{Client, State};
use lucyd::adapters::NotificationKind;
use rmcp::model::CallToolResult;
use serde_json::json;

const REQUEST: &str = "list my Linear tickets";
const CONVERSATION: &str = "5c1e0d2a-hand-off";
const ANSWER: &str = "You have three open tickets: LUC-1, LUC-2 and LUC-3. LUC-2 is due today.";

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn called(result: Result<CallToolResult, String>) -> (bool, String) {
    let result = result.unwrap();
    (result.is_error == Some(true), text(&result))
}

fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

fn hand_off(lucyd: &Harness, arguments: serde_json::Value) -> (Client, (bool, String)) {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say(&format!("tell Claude to {REQUEST}"));
    lucyd.thinking.next_turn();
    until_state(&mut events, State::Thinking);
    let called = called(lucyd.thinking.call_tool_with("hand_to_claude", arguments));
    (events, called)
}

fn handed_off(lucyd: &Harness, folder: &Path) -> Client {
    let arguments = json!({"request": REQUEST, "folder": folder.to_str().unwrap()});
    let (mut events, (failed, said)) = hand_off(lucyd, arguments);
    assert!(!failed, "{said}");
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    lucyd.thinking.hand_off.hand_off();
    events
}

#[test]
fn tell_claude_hands_the_request_to_claude_in_the_folder_given() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();

    let _events = handed_off(&lucyd, &folder);

    assert_eq!(lucyd.thinking.hand_off.next_turn(), REQUEST);
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, folder);
}

#[test]
fn when_claude_finishes_the_fast_tier_sums_it_up_and_the_whole_answer_is_notified() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    lucyd.thinking.hand_off.next_turn();

    lucyd.thinking.hand_off.answer(CONVERSATION, ANSWER);

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude has finished");
    assert!(shown.body.starts_with(ANSWER), "{}", shown.body);
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );

    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains(ANSWER), "{turn}");
    assert!(turn.contains("one to three"), "{turn}");
    lucyd
        .thinking
        .delta("Claude found three tickets, and one is due today.");
    lucyd.thinking.sentence_end();
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude found three tickets, and one is due today."
    );
}

fn asks_permission(
    lucyd: &Harness,
    request: serde_json::Value,
) -> JoinHandle<Result<CallToolResult, String>> {
    let hand_off = lucyd.thinking.hand_off.clone();
    std::thread::spawn(move || hand_off.call_tool_with("permission", request))
}

fn run_the_tests() -> serde_json::Value {
    json!({
        "tool_name": "Bash",
        "input": {"command": "cargo test -p lucyd", "description": "Run lucyd's tests."},
        "tool_use_id": "toolu_1",
    })
}

fn decision(call: JoinHandle<Result<CallToolResult, String>>) -> serde_json::Value {
    let (failed, decision) = called(call.join().unwrap());
    assert!(!failed, "{decision}");
    serde_json::from_str(&decision).unwrap()
}

fn assert_denied(decision: &serde_json::Value) {
    assert_eq!(decision["behavior"], "deny", "{decision}");
    assert!(
        decision["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{decision}"
    );
}

#[test]
fn a_permission_request_is_asked_with_claudes_description_and_the_full_input_notified() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();

    let call = asks_permission(&lucyd, run_the_tests());

    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude wants to run lucyd's tests. Okay?"
    );
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::Details);
    assert!(
        shown.body.contains("Bash") && shown.body.contains("cargo test -p lucyd"),
        "{}",
        shown.body
    );
    lucyd.tts.finish_playing();
    lucyd.say("Yes.");

    let decision = decision(call);
    assert_eq!(decision["behavior"], "allow", "{decision}");
    assert_eq!(
        decision["updatedInput"],
        json!({"command": "cargo test -p lucyd", "description": "Run lucyd's tests."})
    );
}

#[test]
fn a_request_with_no_description_names_the_tool() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();

    let call = asks_permission(
        &lucyd,
        json!({"tool_name": "mcp__linear__create_issue", "input": {"title": "Fix it"}}),
    );

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude wants to use linear create issue. Okay? I won't ask again for that this Session."
    );
    lucyd.tts.finish_playing();
    lucyd.say("no");
    assert_denied(&decision(call));
}

#[test]
fn a_no_denies_the_request() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    lucyd.say("no, don't");

    assert_denied(&decision(call));
    assert_eq!(lucyd.tts.next_sentence(), "Okay, I won't.");
}

#[test]
fn silence_denies_the_request() {
    let lucyd = Harness::with_config("[confirmation]\ntimeout_secs = 2\n");
    let _events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.barrier();

    lucyd.clock.advance(Duration::from_secs(2));

    assert_denied(&decision(call));
}

#[test]
fn an_indicator_click_while_asked_denies_and_ends_the_session() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    let call = asks_permission(&lucyd, run_the_tests());
    assert_eq!(next_state(&mut events).state, State::Confirming);

    lucyd.client().end_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Idle);
    let unless_the_relay_was_cut_off = call.join().unwrap();
    if let Ok(result) = unless_the_relay_was_cut_off {
        let decision: serde_json::Value = serde_json::from_str(&text(&result)).unwrap();
        assert_denied(&decision);
    }
}

#[test]
fn a_request_that_names_no_folder_runs_in_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let lucyd = Harness::with_env(&[("HOME", home.path().to_str().unwrap())]);

    let (_events, (failed, said)) = hand_off(&lucyd, json!({"request": REQUEST}));

    assert!(!failed, "{said}");
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, home.path());
}

#[test]
fn a_tilde_path_is_taken_from_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("lucy")).unwrap();
    let lucyd = Harness::with_env(&[("HOME", home.path().to_str().unwrap())]);

    let arguments = json!({"request": REQUEST, "folder": "~/lucy"});
    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(!failed, "{said}");
    let folder = lucyd.thinking.hand_off.hand_off().folder;
    assert_eq!(folder, home.path().join("lucy"));
}

fn hand_off_to(lucyd: &Harness, folder: &str) -> (bool, String) {
    let (_events, called) = hand_off(lucyd, json!({"request": REQUEST, "folder": folder}));
    called
}

fn git_repo(lucyd: &Harness, relative: &str) -> std::path::PathBuf {
    lucyd.home_folder(&format!("{relative}/.git"));
    lucyd.home().join(relative)
}

#[test]
fn a_folder_named_once_under_the_home_folder_is_where_claude_runs() {
    let lucyd = Harness::start();
    let lucy = lucyd.home_folder("develops/lucy");
    lucyd.home_folder("develops/lucy-prototype");
    lucyd.home_folder("develops/other");

    let (failed, said) = hand_off_to(&lucyd, "Lucy");

    assert!(!failed, "{said}");
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, lucy);
}

#[test]
fn several_folders_with_the_name_are_asked_about_git_repos_first_and_nothing_starts() {
    let lucyd = Harness::start();
    let plain = lucyd.home_folder("archive/lucy");
    let repo = git_repo(&lucyd, "work/src/lucy");

    let (failed, said) = hand_off_to(&lucyd, "lucy");

    assert!(failed);
    assert!(said.contains("Ask the user which"), "{said}");
    let repo_at = said.find(repo.to_str().unwrap()).expect(&said);
    let plain_at = said.find(plain.to_str().unwrap()).expect(&said);
    assert!(repo_at < plain_at, "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

#[test]
fn a_name_with_no_folder_is_said_plainly_and_nothing_starts() {
    let lucyd = Harness::start();
    lucyd.home_file("notes/lucy");
    lucyd.home_folder("a/b/c/lucy");

    let (failed, said) = hand_off_to(&lucyd, "lucy");

    assert!(failed);
    assert!(said.contains("No folder called \"lucy\""), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

#[test]
fn the_folder_search_stops_three_levels_down() {
    let lucyd = Harness::start();
    let found = lucyd.home_folder("a/b/lucy");
    lucyd.home_folder("a/b/c/lucy");

    let (failed, said) = hand_off_to(&lucyd, "lucy");

    assert!(!failed, "{said}");
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, found);
}

#[test]
fn the_folder_search_looks_under_the_configured_roots() {
    let lucyd = Harness::with_config("[handoff]\nroots = [\"~/work\"]\n");
    lucyd.home_folder("archive/lucy");
    let lucy = lucyd.home_folder("work/lucy");

    let (failed, said) = hand_off_to(&lucyd, "lucy");

    assert!(!failed, "{said}");
    assert_eq!(lucyd.thinking.hand_off.hand_off().folder, lucy);
}

#[test]
fn a_folder_that_isnt_there_is_refused() {
    let lucyd = Harness::start();
    let missing = lucyd.scratch().join("no-such-folder");

    let arguments = json!({"request": REQUEST, "folder": missing.to_str().unwrap()});
    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(failed);
    assert!(said.contains("no folder"), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

fn hand_off_with_config(config: &str) -> (bool, String) {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say(&format!("tell Claude to {REQUEST}"));
    lucyd.thinking.next_turn();
    until_state(&mut events, State::Thinking);
    std::fs::write(lucyd.config_path(), config).unwrap();

    let called = hand_to_claude(&lucyd, json!({"request": REQUEST}));

    assert!(lucyd.thinking.hand_off.opened().is_empty());
    called
}

#[test]
fn a_budget_that_isnt_a_positive_number_of_minutes_is_refused_and_nothing_starts() {
    for budget in ["-5", "nan", "0"] {
        let (failed, said) = hand_off_with_config(&format!("[handoff]\nbudget_mins = {budget}\n"));

        assert!(failed, "{budget}: {said}");
        assert!(said.contains("config.toml"), "{budget}: {said}");
    }
}

#[test]
fn a_config_that_cant_be_read_is_said_and_nothing_starts() {
    let (failed, said) = hand_off_with_config("[handoff\n");

    assert!(failed, "{said}");
    assert!(said.contains("config.toml"), "{said}");
}

#[test]
fn the_session_stays_open_in_listening_while_claude_works() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 30\n");
    let _events = handed_off(&lucyd, &lucyd.scratch());

    lucyd.clock.advance(Duration::from_secs(60));

    assert_eq!(lucyd.client().status().unwrap().state, State::Listening);
}

fn hand_to_claude(lucyd: &Harness, arguments: serde_json::Value) -> (bool, String) {
    called(lucyd.thinking.call_tool_with("hand_to_claude", arguments))
}

#[test]
fn a_second_request_offers_to_queue_and_a_third_is_refused_while_one_waits() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    let (failed, offer) = hand_to_claude(&lucyd, json!({"request": "check my email"}));
    assert!(failed, "nothing starts before the user says to queue it");
    assert!(offer.contains(REQUEST), "{offer}");
    assert!(offer.contains("want me to queue this?"), "{offer}");

    let (failed, queued) =
        hand_to_claude(&lucyd, json!({"request": "check my email", "queue": true}));
    assert!(!failed, "{queued}");
    let (failed, refused) =
        hand_to_claude(&lucyd, json!({"request": "tidy my desktop", "queue": true}));
    assert!(failed, "{refused}");
    assert!(refused.contains("check my email"), "{refused}");
    hand_off.assert_no_turn();
    assert_eq!(hand_off.opened().len(), 1);

    hand_off.answer(CONVERSATION, ANSWER);

    assert_eq!(hand_off.next_turn(), "check my email");
    assert_eq!(hand_off.opened().len(), 2);
}

#[test]
fn claude_carries_on_after_the_session_ends_and_its_summary_is_offered_at_the_next_session() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let mut events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    lucyd.client().end_session().unwrap();
    let still = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(still.summary, "Claude is still working");
    until_state(&mut events, State::Idle);

    hand_off.answer(CONVERSATION, ANSWER);
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude has finished");
    assert!(shown.body.starts_with(ANSWER), "{}", shown.body);
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    lucyd.thinking.assert_no_turn();

    lucyd.client().start_session().unwrap();
    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains(ANSWER), "{turn}");
    assert!(turn.contains("one to three"), "{turn}");
}

#[test]
fn the_state_carries_a_working_mark_while_claude_works_even_after_the_session_ends() {
    let lucyd = Harness::start();
    assert!(!lucyd.client().status().unwrap().working);
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    assert!(lucyd.client().status().unwrap().working);

    lucyd.client().end_session().unwrap();
    let ended = next_state(&mut events);
    assert_eq!((ended.state, ended.working), (State::Idle, true));

    hand_off.answer(CONVERSATION, ANSWER);
    let over = next_state(&mut events);
    assert_eq!((over.state, over.working), (State::Idle, false));
}

#[test]
fn a_permission_request_with_no_session_open_is_notified_at_once_and_asked_at_the_next_session() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);

    let call = asks_permission(&lucyd, run_the_tests());
    let kept = loop {
        let shown = lucyd
            .notifier
            .next_notification_of(NotificationKind::HandOff);
        if shown.summary == "Claude is waiting for your permission" {
            break shown;
        }
    };
    assert!(
        kept.body.contains("Bash") && kept.body.contains("cargo test -p lucyd"),
        "{}",
        kept.body
    );
    assert!(kept.body.contains("next Session"), "{}", kept.body);
    assert!(lucyd.tts.sentences().is_empty(), "nothing was asked aloud");

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude wants to run lucyd's tests. Okay?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("yes");

    assert_eq!(decision(call)["behavior"], "allow");
}

#[test]
fn a_question_with_no_session_open_is_notified_at_once_and_asked_at_the_next_session() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);

    let hand_off = lucyd.thinking.hand_off.clone();
    let call = std::thread::spawn(move || {
        hand_off.call_tool_with("ask_user", json!({"question": "Which team, LUC or OPS?"}))
    });
    let kept = loop {
        let shown = lucyd
            .notifier
            .next_notification_of(NotificationKind::HandOff);
        if shown.summary == "Claude has a question" {
            break shown;
        }
    };
    assert!(kept.body.contains("next Session"), "{}", kept.body);

    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Confirming);
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Claude asks: Which team, LUC or OPS?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("LUC");

    let (failed, answer) = called(call.join().unwrap());
    assert!(!failed, "{answer}");
    assert!(answer.contains("LUC"), "{answer}");
}

#[test]
fn nothing_about_the_hand_off_reaches_the_public_socket() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch().join("linear-work");
    std::fs::create_dir(&folder).unwrap();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    let call = asks_permission(
        &lucyd,
        json!({"tool_name": "mcp__linear__list_issues", "input": {"team": "LUC"}}),
    );
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    decision(call);
    hand_off.answer(CONVERSATION, ANSWER);
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("Claude found three tickets.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    lucyd.client().end_session().unwrap();

    lucyd.assert_nothing_private_on_bus();
    lucyd.assert_not_on_bus(&[
        REQUEST,
        "linear-work",
        "mcp__linear__list_issues",
        "LUC-2",
        CONVERSATION,
    ]);
}

#[test]
fn a_failure_kept_for_the_next_session_is_not_notified_as_finished() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    lucyd.say("what time is it");
    lucyd.thinking.next_turn();

    hand_off.fail("Linear's MCP server isn't connected");
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude couldn't finish");
    lucyd.client().end_session().unwrap();

    let kept = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(kept.summary, "Claude couldn't finish");
}

#[test]
fn claude_finishing_without_a_final_message_says_so() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    hand_off.conversation(CONVERSATION);
    hand_off.turn_end();

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert!(
        shown.body.starts_with("Claude left no final message"),
        "{}",
        shown.body
    );
    let turn = lucyd.thinking.next_turn();
    assert!(turn.contains("left no final message"), "{turn}");
    assert!(!turn.contains("Its final message"), "{turn}");
}

#[test]
fn at_its_budget_claude_is_stopped_on_the_clock_with_the_resume_command_and_folder_notified() {
    let lucyd = Harness::with_config("[handoff]\nbudget_mins = 30\n");
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);

    lucyd.clock.advance(Duration::from_secs(30 * 60 - 1));
    lucyd.barrier();
    let shown = lucyd.notifier.shown();
    assert!(
        shown
            .iter()
            .all(|shown| shown.kind != NotificationKind::HandOff),
        "{shown:?}"
    );
    lucyd.clock.advance(Duration::from_secs(1));

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude ran out of time");
    assert!(shown.body.contains("30 minutes"), "{}", shown.body);
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    let report = lucyd.thinking.next_turn();
    assert!(report.contains("ran out of time"), "{report}");
}

#[test]
fn the_budget_runs_on_with_no_session_open() {
    let lucyd = Harness::with_config("[handoff]\nbudget_mins = 30\n");
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);
    lucyd.client().end_session().unwrap();
    let still = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(still.summary, "Claude is still working");

    lucyd.clock.advance(Duration::from_secs(30 * 60));

    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Claude ran out of time");
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    wait_until("Claude to stop working", || {
        (!lucyd.client().status().unwrap().working).then_some(())
    });
}

const HAND_OFF_STOPPED: &str = "Okay, I've stopped Claude.";

#[test]
fn cancel_it_stops_claude_with_the_resume_command_notified() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);

    lucyd.say("Cancel it.");

    assert_eq!(lucyd.tts.next_sentence(), HAND_OFF_STOPPED);
    let shown = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(shown.summary, "Lucy stopped Claude");
    let resume = format!("claude --resume {CONVERSATION}");
    assert!(shown.body.contains(&resume), "{}", shown.body);
    assert!(
        shown.body.contains(folder.to_str().unwrap()),
        "{}",
        shown.body
    );
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    lucyd.barrier();
    lucyd.thinking.assert_no_turn();
}

fn request_a_build(lucyd: &Harness) {
    let (failed, said) = called(lucyd.thinking.call_tool_with(
        "request_plugin",
        json!({
            "name": "photos",
            "goal": "find the user's photos",
            "clarifications": "",
            "request": "find my photos of Rome",
        }),
    ));
    assert!(!failed, "{said}");
    assert_eq!(
        lucyd.tts.next_sentence(),
        "This is going to take a few minutes."
    );
    lucyd.tts.finish_playing();
}

#[test]
fn a_hand_off_and_a_build_run_together_and_cancel_it_asks_which_one() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    assert_eq!(hand_off.next_turn(), REQUEST);
    lucyd.say("make a Plugin to find my photos");
    lucyd.thinking.next_turn();
    request_a_build(&lucyd);
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    let build = &lucyd.thinking.delegate;
    assert!(build.next_turn().contains("photos"));

    lucyd.say("cancel it");

    assert_eq!(
        lucyd.tts.next_sentence(),
        "Do you mean the Plugin build, or Claude?"
    );
    lucyd.tts.finish_playing();
    lucyd.say("Claude");
    assert_eq!(lucyd.tts.next_sentence(), HAND_OFF_STOPPED);
    assert!(hand_off.call_tool_with("permission", json!({})).is_err());
    assert!(build
        .call_tool_with("report_progress", json!({"text": "still going"}))
        .is_ok());
    assert!(lucyd.client().status().unwrap().building);
    lucyd.thinking.assert_no_turn();
}

const QUESTION: &str = "Which team, LUC or OPS?";

fn asked_while_away(
    lucyd: &Harness,
    tool: &'static str,
    arguments: serde_json::Value,
    kept: &str,
) -> (Client, JoinHandle<Result<CallToolResult, String>>) {
    let mut events = handed_off(lucyd, &lucyd.scratch());
    let hand_off = lucyd.thinking.hand_off.clone();
    hand_off.next_turn();
    hand_off.conversation(CONVERSATION);
    lucyd.client().end_session().unwrap();
    until_state(&mut events, State::Idle);
    let call = std::thread::spawn(move || hand_off.call_tool_with(tool, arguments));
    while lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff)
        .summary
        != kept
    {}
    (events, call)
}

fn ask_user() -> serde_json::Value {
    json!({"question": QUESTION})
}

fn dropped(call: &JoinHandle<Result<CallToolResult, String>>) {
    wait_until("the stopped Hand-off's call to end", || {
        call.is_finished().then_some(())
    });
}

fn cancelled_with_its_ask_waiting(tool: &'static str, arguments: serde_json::Value, kept: &str) {
    let lucyd = Harness::start();
    let (mut events, call) = asked_while_away(&lucyd, tool, arguments, kept);
    lucyd.start_speaking();
    lucyd.client().start_session().unwrap();
    until_state(&mut events, State::Listening);

    lucyd.say("cancel it");

    assert_eq!(lucyd.tts.next_sentence(), HAND_OFF_STOPPED);
    dropped(&call);
    lucyd.tts.finish_playing();
    lucyd.barrier();
    assert_eq!(lucyd.tts.sentences(), [HAND_OFF_STOPPED]);
}

#[test]
fn cancel_it_drops_claudes_waiting_question() {
    cancelled_with_its_ask_waiting("ask_user", ask_user(), "Claude has a question");
}

#[test]
fn cancel_it_drops_claudes_waiting_permission_request() {
    cancelled_with_its_ask_waiting(
        "permission",
        run_the_tests(),
        "Claude is waiting for your permission",
    );
}

fn out_of_time_with_its_ask_kept(tool: &'static str, arguments: serde_json::Value, kept: &str) {
    let lucyd = Harness::with_config("[handoff]\nbudget_mins = 30\n");
    let (_events, call) = asked_while_away(&lucyd, tool, arguments, kept);

    lucyd.clock.advance(Duration::from_secs(30 * 60));

    dropped(&call);
    lucyd.client().start_session().unwrap();
    let report = lucyd.thinking.next_turn();
    assert!(report.contains("ran out of time"), "{report}");
    lucyd.barrier();
    assert!(
        lucyd.tts.sentences().is_empty(),
        "{:?}",
        lucyd.tts.sentences()
    );
}

#[test]
fn a_hand_off_out_of_time_drops_its_question_kept_for_the_next_session() {
    out_of_time_with_its_ask_kept("ask_user", ask_user(), "Claude has a question");
}

#[test]
fn a_hand_off_out_of_time_drops_its_permission_request_kept_for_the_next_session() {
    out_of_time_with_its_ask_kept(
        "permission",
        run_the_tests(),
        "Claude is waiting for your permission",
    );
}

const QUEUED: &str = "check my email";

fn queue_another(lucyd: &Harness) {
    let (failed, queued) = hand_to_claude(lucyd, json!({"request": QUEUED, "queue": true}));
    assert!(!failed, "{queued}");
}

#[test]
fn cancel_it_says_the_queued_request_is_dropped_too() {
    let lucyd = Harness::start();
    let _events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    queue_another(&lucyd);
    lucyd.barrier();

    lucyd.say("cancel it");

    let said = lucyd.tts.next_sentence();
    assert!(said.starts_with(HAND_OFF_STOPPED), "{said}");
    assert!(said.contains("queued"), "{said}");
    lucyd.barrier();
    hand_off.assert_no_turn();
    assert_eq!(hand_off.opened().len(), 1);
}

#[test]
fn which_one_answered_after_claude_finished_leaves_the_queued_hand_off_running() {
    let lucyd = Harness::start();
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    let hand_off = &lucyd.thinking.hand_off;
    assert_eq!(hand_off.next_turn(), REQUEST);
    queue_another(&lucyd);
    lucyd.say("make a Plugin to find my photos");
    lucyd.thinking.next_turn();
    request_a_build(&lucyd);
    lucyd.thinking.turn_end();
    until_state(&mut events, State::Listening);
    lucyd.thinking.delegate.next_turn();
    lucyd.say("cancel it");
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Do you mean the Plugin build, or Claude?"
    );

    hand_off.answer(CONVERSATION, ANSWER);
    assert_eq!(hand_off.next_turn(), QUEUED);
    lucyd.tts.finish_playing();
    lucyd.say("Claude");

    let said = lucyd.tts.next_sentence();
    assert!(said.contains("already finished"), "{said}");
    assert!(hand_off
        .call_tool_with("report_progress", json!({"text": "still going"}))
        .is_ok());
    assert!(lucyd.client().status().unwrap().working);
}

fn ends_with_no_session_open(config: &str, end: impl FnOnce(&Harness)) -> (Harness, String) {
    let lucyd = Harness::with_config(config);
    let mut events = handed_off(&lucyd, &lucyd.scratch());
    lucyd.thinking.hand_off.next_turn();
    lucyd.client().end_session().unwrap();
    let still = lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    assert_eq!(still.summary, "Claude is still working");
    until_state(&mut events, State::Idle);

    end(&lucyd);

    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    lucyd.client().start_session().unwrap();
    let told = lucyd.thinking.next_turn();
    let shown: Vec<_> = lucyd
        .notifier
        .shown()
        .into_iter()
        .filter(|shown| shown.kind == NotificationKind::HandOff)
        .collect();
    assert!(shown.is_empty(), "a second notification: {shown:?}");
    (lucyd, told)
}

#[test]
fn claude_finishing_with_no_session_open_sends_one_notification() {
    let (_lucyd, told) = ends_with_no_session_open("", |lucyd| {
        lucyd.thinking.hand_off.answer(CONVERSATION, ANSWER);
    });
    assert!(told.contains(ANSWER), "{told}");
}

#[test]
fn claude_failing_with_no_session_open_sends_one_notification() {
    let (_lucyd, told) = ends_with_no_session_open("", |lucyd| {
        lucyd
            .thinking
            .hand_off
            .fail("Linear's MCP server isn't connected");
    });
    assert!(told.contains("couldn't finish"), "{told}");
}

#[test]
fn claude_running_out_of_time_with_no_session_open_sends_one_notification() {
    let (_lucyd, told) = ends_with_no_session_open("[handoff]\nbudget_mins = 30\n", |lucyd| {
        lucyd.clock.advance(Duration::from_secs(30 * 60));
    });
    assert!(told.contains("ran out of time"), "{told}");
}

const REPLY: &str = "yes, go ahead";

fn finished(lucyd: &Harness, folder: &Path) -> Client {
    let events = handed_off(lucyd, folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.answer(CONVERSATION, ANSWER);
    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    lucyd.thinking.next_turn();
    lucyd.thinking.turn_end();
    events
}

fn continued(lucyd: &Harness) -> lucyd::adapters::HandOff {
    let hand_off = &lucyd.thinking.hand_off;
    wait_until("the Hand-off to be continued", || {
        Some(hand_off.hand_off()).filter(|opened| opened.resume.is_some())
    })
}

#[test]
fn continue_resumes_the_last_hand_offs_conversation_in_its_folder() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = finished(&lucyd, &folder);

    lucyd.say("tell Claude yes, go ahead");
    lucyd.thinking.next_turn();
    let (failed, said) = hand_to_claude(&lucyd, json!({"request": REPLY, "continue": true}));

    assert!(!failed, "{said}");
    let opened = continued(&lucyd);
    assert_eq!(opened.resume.as_deref(), Some(CONVERSATION));
    assert_eq!(opened.folder, folder);
    assert_eq!(lucyd.thinking.hand_off.next_turn(), REPLY);
}

#[test]
fn with_no_earlier_hand_off_continue_says_theres_nothing_to_continue_and_nothing_starts() {
    let lucyd = Harness::start();
    let arguments = json!({"request": REPLY, "continue": true});

    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(failed, "{said}");
    assert!(said.contains("nothing to continue"), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}

#[test]
fn while_one_runs_continue_gets_the_queue_offer_and_carries_on_the_one_that_ran() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch();
    let _events = handed_off(&lucyd, &folder);
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();

    let (failed, offer) = hand_to_claude(&lucyd, json!({"request": REPLY, "continue": true}));
    assert!(failed, "nothing starts before the user says to queue it");
    assert!(offer.contains("want me to queue this?"), "{offer}");
    let (failed, queued) = hand_to_claude(
        &lucyd,
        json!({"request": REPLY, "continue": true, "queue": true}),
    );
    assert!(!failed, "{queued}");
    hand_off.answer(CONVERSATION, ANSWER);

    let opened = continued(&lucyd);
    assert_eq!(opened.resume.as_deref(), Some(CONVERSATION));
    assert_eq!(opened.folder, folder);
    assert_eq!(hand_off.next_turn(), REPLY);
}

#[test]
fn nothing_about_the_continued_conversation_reaches_the_public_socket() {
    let lucyd = Harness::start();
    let folder = lucyd.scratch().join("linear-work");
    std::fs::create_dir(&folder).unwrap();
    let _events = finished(&lucyd, &folder);
    lucyd.say("tell Claude yes, go ahead");
    lucyd.thinking.next_turn();
    let (failed, said) = hand_to_claude(&lucyd, json!({"request": REPLY, "continue": true}));
    assert!(!failed, "{said}");
    continued(&lucyd);
    lucyd.thinking.hand_off.next_turn();
    lucyd.thinking.turn_end();
    lucyd.client().end_session().unwrap();

    lucyd.assert_nothing_private_on_bus();
    lucyd.assert_not_on_bus(&[CONVERSATION, "linear-work", REPLY]);
}

#[test]
fn after_a_newer_hand_off_ends_without_a_conversation_theres_nothing_to_continue() {
    let lucyd = Harness::start();
    let _events = finished(&lucyd, &lucyd.scratch());
    lucyd.say(&format!("tell Claude to {REQUEST}"));
    lucyd.thinking.next_turn();
    let arguments = json!({"request": REQUEST, "folder": lucyd.scratch().to_str().unwrap()});
    let (failed, said) = hand_to_claude(&lucyd, arguments);
    assert!(!failed, "{said}");
    let hand_off = &lucyd.thinking.hand_off;
    hand_off.next_turn();
    hand_off.fail("Claude couldn't start");
    lucyd
        .notifier
        .next_notification_of(NotificationKind::HandOff);
    lucyd.thinking.turn_end();
    lucyd.say("tell Claude yes, go ahead");
    lucyd.thinking.next_turn();

    let (failed, said) = hand_to_claude(&lucyd, json!({"request": REPLY, "continue": true}));

    assert!(failed, "{said}");
    assert!(said.contains("nothing to continue"), "{said}");
}

#[test]
fn an_unreadable_record_of_the_last_hand_off_says_lucy_couldnt_read_it() {
    let lucyd = Harness::start();
    std::fs::create_dir_all(lucyd.state_dir()).unwrap();
    std::fs::write(lucyd.state_dir().join("hand-off.toml"), "conversation = [").unwrap();
    let arguments = json!({"request": REPLY, "continue": true});

    let (_events, (failed, said)) = hand_off(&lucyd, arguments);

    assert!(failed, "{said}");
    assert!(said.contains("couldn't read"), "{said}");
    assert!(!said.contains("nothing to continue"), "{said}");
    assert!(lucyd.thinking.hand_off.opened().is_empty());
}
