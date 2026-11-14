mod harness;

use std::time::{Duration, Instant};

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

fn with_silence_timeout(secs: f64) -> Harness {
    Harness::with_config(&format!("[session]\nsilence_timeout_secs = {secs}\n"))
}

#[test]
fn silence_in_listening_ends_the_session_quietly() {
    let lucyd = with_silence_timeout(0.3);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);

    assert_eq!(
        next_state(&mut events),
        StateEvent {
            state: State::Idle,
            session_id: None,
            building: false
        }
    );
    assert_eq!(lucyd.tts.sentences(), Vec::<String>::new());
}

#[test]
fn silence_timer_pauses_while_thinking_and_speaking() {
    let lucyd = with_silence_timeout(0.5);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    // sleep-ok: timer test; lucyd has no clock seam yet
    std::thread::sleep(Duration::from_millis(300));
    lucyd.say("tell me a long story");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    // sleep-ok: timer test; lucyd has no clock seam yet
    std::thread::sleep(Duration::from_secs(1));
    lucyd.thinking.delta("Once upon a time.");
    lucyd.thinking.turn_end();
    assert_eq!(next_state(&mut events).state, State::Speaking);
    // sleep-ok: timer test; lucyd has no clock seam yet
    std::thread::sleep(Duration::from_secs(1));
    lucyd.tts.finish_playing();

    // Still open after two timeouts' worth outside Listening. Back in
    // Listening the timer starts over, rather than using the 200 ms left.
    assert_eq!(next_state(&mut events).state, State::Listening);
    let listening = Instant::now();
    assert_eq!(next_state(&mut events).state, State::Idle);
    assert!(listening.elapsed() >= Duration::from_millis(400));
}

#[test]
fn no_speech_recognised_keeps_listening_without_resetting_the_timer() {
    let lucyd = with_silence_timeout(1.5);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let opened = Instant::now();

    // sleep-ok: timer test; lucyd has no clock seam yet
    std::thread::sleep(Duration::from_secs(1));
    lucyd.mumble();

    // Straight from Listening to Idle, at the original deadline.
    assert_eq!(next_state(&mut events).state, State::Idle);
    assert!(
        opened.elapsed() < Duration::from_millis(2200),
        "{:?}",
        opened.elapsed()
    );
    lucyd.thinking.assert_no_turn();
}

#[test]
fn silence_timer_keeps_running_while_an_utterance_is_transcribed() {
    let lucyd = with_silence_timeout(0.5);
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    let opened = Instant::now();

    lucyd.stt.take(Duration::from_secs(2));
    lucyd.say("hello");

    // Transcribing is still Listening, so the timer fires mid-transcription.
    assert_eq!(next_state(&mut events).state, State::Idle);
    assert!(
        opened.elapsed() < Duration::from_millis(1500),
        "{:?}",
        opened.elapsed()
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
            session_id: Some(session_id.clone()),
            building: false
        }
    );

    lucyd.say("what's the weather like");
    assert_eq!(lucyd.thinking.next_turn(), "what's the weather like");
    assert_eq!(
        next_state(&mut events),
        StateEvent {
            state: State::Thinking,
            session_id: Some(session_id.clone()),
            building: false
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
fn every_state_change_reaches_subscribers_even_in_a_quick_burst() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();

    // Drive a whole Turn without reading any state event in between.
    lucyd.say("hi");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("Hello.");
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();

    let states: Vec<State> = (0..4).map(|_| next_state(&mut events).state).collect();
    assert_eq!(
        states,
        [
            State::Listening,
            State::Thinking,
            State::Speaking,
            State::Listening
        ]
    );
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
