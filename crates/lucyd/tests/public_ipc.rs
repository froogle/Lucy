#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]
mod harness;

use harness::Harness;
use lucy_ipc::{Event, State, StateEvent, Topic};

#[test]
fn fresh_lucyd_reports_idle_status_after_handshake() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    let status = client.status().unwrap();

    assert_eq!(status.state, State::Idle);
    assert_eq!(status.session_id, None);
}

#[test]
fn subscribing_to_state_sends_the_current_state() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    client.subscribe(&[Topic::State]).unwrap();

    assert_eq!(
        client.next_event().unwrap(),
        Event::State(StateEvent {
            state: State::Idle,
            session_id: None,
            building: false,
            looking: false,
            working: false,
        })
    );
}

#[test]
fn state_events_are_json_rpc_notifications() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.subscribe(&[Topic::State]).unwrap();

    let line = client.next_event_line().unwrap();

    let event: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        event,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "state",
            "params": {"state": "idle", "session_id": null, "building": false, "looking": false, "working": false},
        })
    );
}

#[test]
fn socket_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let lucyd = Harness::start();

    let mode = std::fs::metadata(lucyd.socket_path())
        .unwrap()
        .permissions()
        .mode();

    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn hello_with_another_major_version_is_refused_with_a_clear_error() {
    let lucyd = Harness::start();

    let reply = lucyd.raw_exchange(
        r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":2,"client":"future-indicator","role":null}}"#,
    );

    assert_eq!(reply["id"], 1);
    assert_eq!(
        reply["error"]["code"],
        lucy_ipc::error_code::PROTOCOL_MISMATCH
    );
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(message.contains("protocol 1"), "{message}");
    assert!(message.contains("protocol 2"), "{message}");
    assert!(message.contains("restart lucyd"), "{message}");
}

#[test]
fn requests_before_hello_are_refused() {
    let lucyd = Harness::start();

    let reply = lucyd.raw_exchange(r#"{"jsonrpc":"2.0","id":7,"method":"status"}"#);

    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], lucy_ipc::error_code::HELLO_REQUIRED);
}
