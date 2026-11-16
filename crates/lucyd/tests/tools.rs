#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]
mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

#[test]
fn end_session_through_the_relay_ends_the_session_after_speech() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("that's all, thanks");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    lucyd.thinking.delta("Bye for now.");
    lucyd.thinking.sentence_end();
    lucyd.tts.next_sentence();
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.thinking.call_tool("end_session").unwrap();
    lucyd.thinking.turn_end();

    // Still speaking the goodbye: the Session waits for it.
    assert_eq!(lucyd.client().status().unwrap().state, State::Speaking);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Idle);
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn the_session_model_is_offered_end_session() {
    let lucyd = Harness::start();
    lucyd.client().start_session().unwrap();

    assert!(lucyd
        .thinking
        .tool_names()
        .contains(&"end_session".to_owned()));
}

#[test]
fn a_relay_with_an_unknown_token_is_refused() {
    let lucyd = Harness::start();

    let output = lucyd.relay(&["not-a-token"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refused"), "{stderr}");
}

#[test]
fn a_relay_with_no_token_is_refused() {
    let lucyd = Harness::start();

    assert!(!lucyd.relay(&[]).status.success());
}

#[test]
fn a_token_stops_working_when_its_session_ends() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();
    client.end_session().unwrap();

    let refused = lucyd.thinking.call_tool("end_session");

    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn the_mcp_socket_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let lucyd = Harness::start();

    let mcp = lucyd.mcp_socket_path();

    assert_ne!(mcp, lucyd.socket_path());
    let mode = std::fs::metadata(mcp).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
