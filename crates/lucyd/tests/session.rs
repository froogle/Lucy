mod harness;

use harness::Harness;
use lucy_ipc::{error_code, Error, State};

#[test]
fn session_start_opens_a_session_in_listening() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    let session_id = client.start_session().unwrap();

    let status = client.status().unwrap();
    assert_eq!(status.state, State::Listening);
    assert_eq!(status.session_id, Some(session_id));
}

#[test]
fn only_one_session_is_open_at_a_time() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();

    let second = client.start_session();

    assert!(
        matches!(second, Err(Error::Rpc { code, .. }) if code == error_code::SESSION_OPEN),
        "{second:?}"
    );
}

#[test]
fn session_end_returns_to_idle() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();

    client.end_session().unwrap();

    let status = client.status().unwrap();
    assert_eq!(status.state, State::Idle);
    assert_eq!(status.session_id, None);
}

#[test]
fn session_end_with_no_session_is_refused() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    let ended = client.end_session();

    assert!(
        matches!(ended, Err(Error::Rpc { code, .. }) if code == error_code::NO_SESSION),
        "{ended:?}"
    );
}
