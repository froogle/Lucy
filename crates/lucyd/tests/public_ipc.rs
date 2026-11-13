mod harness;

use harness::Harness;
use lucy_ipc::State;

#[test]
fn fresh_lucyd_reports_idle_status_after_handshake() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    let status = client.status().unwrap();

    assert_eq!(status.state, State::Idle);
    assert_eq!(status.session_id, None);
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
        r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":2,"client":"future-widget","role":null}}"#,
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
