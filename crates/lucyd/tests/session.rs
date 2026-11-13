mod harness;

use harness::{next_state, Harness};
use lucy_ipc::{error_code, Error, State, StateEvent};

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

#[test]
fn a_turn_goes_from_listening_through_thinking_and_speaking_back_to_listening() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    let session_id = events.start_session().unwrap();
    assert_eq!(
        next_state(&mut events),
        StateEvent {
            state: State::Listening,
            session_id: Some(session_id.clone())
        }
    );

    lucyd.say("what's the weather like");
    assert_eq!(lucyd.thinking.next_turn(), "what's the weather like");
    assert_eq!(
        next_state(&mut events),
        StateEvent {
            state: State::Thinking,
            session_id: Some(session_id.clone())
        }
    );

    lucyd.thinking.delta("It's sunny");
    lucyd.thinking.delta(" today.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.delta(" Take a hat");
    lucyd.thinking.turn_end();
    assert_eq!(lucyd.tts.next_sentence(), "It's sunny today.");
    assert_eq!(next_state(&mut events).state, State::Speaking);
    assert_eq!(lucyd.tts.next_sentence(), "Take a hat");

    lucyd.tts.finish_playing();
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Listening);

    events.end_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Idle);
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn one_turn_alternates_between_thinking_and_speaking() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's on my calendar");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    lucyd.thinking.delta("Let me check.");
    lucyd.thinking.sentence_end();
    lucyd.tts.next_sentence();
    assert_eq!(next_state(&mut events).state, State::Speaking);

    // The audio finishes while the model is still working (say, a tool call).
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    lucyd.thinking.delta("You have a dentist at three.");
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();
    assert_eq!(next_state(&mut events).state, State::Speaking);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Listening);

    events.end_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Idle);
    lucyd.assert_nothing_private_on_bus();
}
