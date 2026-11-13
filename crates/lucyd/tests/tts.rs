mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

#[test]
fn when_tts_fails_the_reply_becomes_notification_text_and_the_session_continues() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.tts.fail();

    lucyd.say("what time is it");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("It's three o'clock.");
    lucyd.thinking.sentence_end();
    lucyd.thinking.delta(" Time for tea");
    lucyd.thinking.turn_end();

    assert_eq!(lucyd.notifier.next_notification(), "It's three o'clock.");
    assert_eq!(lucyd.notifier.next_notification(), "Time for tea");
    while next_state(&mut events).state != State::Listening {}

    // The Session carries on with the next Turn.
    lucyd.say("thanks");
    assert_eq!(lucyd.thinking.next_turn(), "thanks");
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}
