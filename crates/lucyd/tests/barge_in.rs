//! Barge-in: the user cuts Lucy off, by the Wake word or by talking. She
//! stops speaking, stops the model if it is still going, listens, and tells
//! the model what the user heard.

mod harness;

use std::time::Duration;

use harness::{next_state, wait_until, Harness, SILENCE, SPEECH, WAKE};
use lucy_ipc::{Client, State};

/// A Session in which Lucy is saying "One." in reply to "count", the Turn
/// still going.
fn lucy_speaking(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say("count");
    assert_eq!(lucyd.thinking.next_turn(), "count");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    lucyd.thinking.delta("One.");
    lucyd.thinking.sentence_end();
    assert_eq!(lucyd.tts.next_sentence(), "One.");
    assert_eq!(next_state(&mut events).state, State::Speaking);
    events
}

#[test]
fn the_wake_word_while_lucy_speaks_cuts_her_off_and_stops_the_model() {
    let lucyd = Harness::start();
    let mut events = lucy_speaking(&lucyd);

    lucyd.wake();

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
    assert_eq!(lucyd.thinking.interrupts(), 1);
}

#[test]
fn the_next_utterance_tells_the_model_what_the_user_heard() {
    let lucyd = Harness::start();
    let mut events = lucy_speaking(&lucyd);
    lucyd.wake();
    assert_eq!(next_state(&mut events).state, State::Listening);

    lucyd.say("stop counting");

    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted; the user heard up to: \"One.\"]\nstop counting"
    );
}

#[test]
fn nothing_said_after_a_barge_in_returns_silently_to_listening() {
    let lucyd = Harness::with_config("[session]\nsilence_timeout_secs = 30\n");
    let mut events = lucy_speaking(&lucyd);
    lucyd.wake();
    assert_eq!(next_state(&mut events).state, State::Listening);

    lucyd.mumble_handled();

    // Nothing more is said, and the Session goes on to its silence timeout.
    lucyd.clock.advance(ms(30_000));
    assert_eq!(next_state(&mut events).state, State::Idle);
    assert_eq!(lucyd.tts.sentences(), ["One."]);
    lucyd.thinking.assert_no_turn();
}

#[test]
fn speech_while_lucy_thinks_is_a_barge_in() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    lucyd.start_speaking();

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.thinking.interrupts(), 1);
    lucyd.say("in Paris I mean");
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted]\nin Paris I mean"
    );
    assert_eq!(lucyd.tts.stops(), 0, "nothing was playing");
}

#[test]
fn with_barge_in_by_wake_word_only_speech_while_thinking_waits() {
    let lucyd = Harness::with_config("[barge_in]\nmode = \"wake_word\"\n");
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    lucyd.start_speaking();
    lucyd.say("in Paris I mean");
    lucyd.thinking.delta("Sunny.");
    lucyd.thinking.turn_end();

    assert_eq!(next_state(&mut events).state, State::Speaking);
    assert_eq!(lucyd.thinking.interrupts(), 0);
    lucyd.thinking.assert_no_turn();
}

#[test]
fn with_barge_in_by_wake_word_only_the_wake_word_still_cuts_in() {
    let lucyd = Harness::with_config("[barge_in]\nmode = \"wake_word\"\n");
    let mut events = lucy_speaking(&lucyd);

    lucyd.wake();

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
}

#[test]
fn the_user_heard_every_sentence_played_and_the_one_cut_off() {
    let lucyd = Harness::start();
    let mut events = lucy_speaking(&lucyd);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    lucyd.thinking.delta("Two.");
    lucyd.thinking.sentence_end();
    assert_eq!(next_state(&mut events).state, State::Speaking);

    lucyd.wake();

    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.say("thanks");
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted; the user heard up to: \"One. Two.\"]\nthanks"
    );
}

#[test]
fn once_the_reply_is_all_written_the_model_is_not_interrupted() {
    let lucyd = Harness::start();
    let mut events = lucy_speaking(&lucyd);
    lucyd.thinking.turn_end();
    lucyd.thinking.assert_no_turn();

    lucyd.wake();

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
    assert_eq!(lucyd.thinking.interrupts(), 0);
}

// Through the microphone: the gate, the echo canceller and the Wake word.

/// A level the fake VAD hears as likely speech, but not likely enough while
/// the echo canceller is unconverged.
const PROBABLY_SPEECH: f32 = 0.8;

/// The echo canceller has converged from the start, if its ERLE says so.
const NO_WARM_UP: &str = "[barge_in]\nwarm_up_ms = 0\n";

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Lucy, hearing through the microphone, says "One." in reply to "count",
/// the Turn still going.
fn lucy_speaking_heard(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    assert_eq!(next_state(&mut events).state, State::Listening);
    lucyd.stt.script("count");
    lucyd.mic.feed(SPEECH, ms(256));
    lucyd.mic.feed(SILENCE, ms(1024));
    // None of that is heard while Lucy speaks.
    lucyd.mic.wait_until_heard();
    assert_eq!(lucyd.thinking.next_turn(), "count");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    lucyd.thinking.delta("One.");
    lucyd.thinking.sentence_end();
    assert_eq!(lucyd.tts.next_sentence(), "One.");
    assert_eq!(next_state(&mut events).state, State::Speaking);
    events
}

/// Waits until playback has been ducked and unducked as `expected` says.
fn wait_for_ducks(lucyd: &Harness, expected: &[bool]) {
    wait_until(&format!("ducks never reached {expected:?}"), || {
        (lucyd.tts.ducks() == expected).then_some(())
    });
}

#[test]
fn talking_over_lucy_ducks_her_at_once_then_cuts_her_off_after_300_ms() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(SPEECH, ms(320));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.ducks(), [true]);
    assert_eq!(lucyd.tts.stops(), 1);
    assert_eq!(lucyd.thinking.interrupts(), 1);
    // What they go on to say is the next Turn.
    lucyd.stt.script("stop counting");
    lucyd.mic.feed(SPEECH, ms(320));
    lucyd.mic.feed(SILENCE, ms(1024));
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted; the user heard up to: \"One.\"]\nstop counting"
    );
}

#[test]
fn a_short_sound_over_lucy_ducks_her_then_lets_her_go_on() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(SPEECH, ms(288));
    lucyd.mic.feed(SILENCE, ms(1024));

    wait_for_ducks(&lucyd, &[true, false]);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    assert_eq!(lucyd.tts.stops(), 0);
    assert_eq!(lucyd.thinking.interrupts(), 0);
    // Nor was the sound an utterance.
    assert_eq!(lucyd.stt.segments().len(), 1);
}

#[test]
fn while_the_echo_canceller_is_unconverged_it_takes_085_for_500_ms() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    lucyd.echo.set_erle(Some(5.0));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(PROBABLY_SPEECH, ms(640));
    lucyd.mic.feed(SILENCE, ms(64));
    lucyd.mic.feed(SPEECH, ms(320));
    lucyd.mic.feed(SILENCE, ms(64));
    wait_for_ducks(&lucyd, &[true, false]);
    assert_eq!(lucyd.tts.stops(), 0);

    lucyd.mic.feed(SPEECH, ms(512));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.ducks(), [true, false, true]);
    assert_eq!(lucyd.tts.stops(), 1);
}

#[test]
fn for_the_first_2_s_of_lucys_speech_the_echo_canceller_counts_as_unconverged() {
    let lucyd = Harness::hearing("");
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(SPEECH, ms(320));
    lucyd.mic.feed(SILENCE, ms(64));
    wait_for_ducks(&lucyd, &[true, false]);
    lucyd.mic.feed(SILENCE, ms(2048));
    lucyd.mic.feed(SPEECH, ms(320));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
}

#[test]
fn with_no_echo_at_the_microphone_the_canceller_counts_as_converged_after_the_warm_up() {
    let lucyd = Harness::hearing("");
    // On headphones the ERLE never gets anywhere: there is no echo to remove.
    lucyd.echo.set_erle(Some(0.2));
    lucyd.echo.set_echo_heard(Some(false));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(SPEECH, ms(320));
    lucyd.mic.feed(SILENCE, ms(64));
    wait_for_ducks(&lucyd, &[true, false]);
    lucyd.mic.feed(SILENCE, ms(2048));
    lucyd.mic.feed(SPEECH, ms(320));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
}

#[test]
fn with_echo_at_the_microphone_or_no_answer_yet_an_unconverged_canceller_keeps_the_stricter_bar() {
    for heard in [Some(true), None] {
        let lucyd = Harness::hearing(NO_WARM_UP);
        lucyd.echo.set_erle(Some(5.0));
        lucyd.echo.set_echo_heard(heard);
        let mut events = lucy_speaking_heard(&lucyd);

        lucyd.mic.feed(SPEECH, ms(320));
        lucyd.mic.feed(SILENCE, ms(1024));

        wait_for_ducks(&lucyd, &[true, false]);
        lucyd.tts.finish_playing();
        assert_eq!(next_state(&mut events).state, State::Thinking, "{heard:?}");
        assert_eq!(lucyd.tts.stops(), 0, "{heard:?}");
    }
}

#[test]
fn lucys_echo_just_after_she_speaks_is_not_heard_as_the_user() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(&lucyd);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    // Within the 300 ms tail: enough to start an utterance, not to pass the
    // gate.
    lucyd.mic.feed(SPEECH, ms(160));
    lucyd.mic.feed(SILENCE, ms(1024));
    wait_for_ducks(&lucyd, &[true, false]);

    lucyd.thinking.delta("Two.");
    lucyd.thinking.turn_end();
    assert_eq!(lucyd.tts.next_sentence(), "Two.");
    assert_eq!(next_state(&mut events).state, State::Speaking);
    assert_eq!(lucyd.thinking.interrupts(), 0);
    assert_eq!(lucyd.stt.segments().len(), 1);
}

#[test]
fn speech_after_the_tail_while_lucy_thinks_is_a_barge_in() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    let mut events = lucy_speaking_heard(&lucyd);
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    lucyd.mic.feed(SILENCE, ms(512));

    lucyd.mic.feed(SPEECH, ms(160));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.thinking.interrupts(), 1);
}

#[test]
fn the_wake_word_over_lucy_cuts_her_off_and_what_follows_is_heard() {
    let lucyd = Harness::hearing(
        "[barge_in]\nmode = \"wake_word\"\n[wake_word]\nmodel = \"/models/hey_lucy.onnx\"\n",
    );
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(WAKE, ms(160));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
    lucyd.stt.script("hey lucy stop");
    lucyd.mic.feed(SPEECH, ms(320));
    lucyd.mic.feed(SILENCE, ms(1024));
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted; the user heard up to: \"One.\"]\nstop"
    );
}

/// Talking over Lucy for long enough to cut her off by speech, then letting
/// her finish: she isn't cut off.
fn assert_talking_over_lucy_does_nothing(lucyd: &Harness) {
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(lucyd);

    lucyd.mic.feed(SPEECH, ms(640));
    lucyd.mic.feed(SILENCE, ms(1024));
    lucyd.tts.finish_playing();

    assert_eq!(next_state(&mut events).state, State::Thinking);
    assert_eq!(lucyd.tts.ducks(), Vec::<bool>::new());
    assert_eq!(lucyd.tts.stops(), 0);
}

#[test]
fn with_barge_in_by_wake_word_only_talking_over_lucy_does_nothing() {
    let lucyd = Harness::hearing("[barge_in]\nmode = \"wake_word\"\nwarm_up_ms = 0\n");
    assert_talking_over_lucy_does_nothing(&lucyd);
}

#[test]
fn without_pipewire_talking_over_lucy_does_nothing() {
    let lucyd = Harness::hearing(NO_WARM_UP);
    lucyd.mic.use_pulseaudio();
    assert_talking_over_lucy_does_nothing(&lucyd);
}

#[test]
fn without_pipewire_barge_in_by_speech_can_be_forced_on() {
    let lucyd = Harness::hearing("[barge_in]\nwarm_up_ms = 0\nforce = true\n");
    lucyd.mic.use_pulseaudio();
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucy_speaking_heard(&lucyd);

    lucyd.mic.feed(SPEECH, ms(320));
    // Completes the last 10 ms frame and VAD chunk.
    lucyd.mic.feed(SILENCE, ms(64));

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.tts.stops(), 1);
}

#[test]
fn lucys_echo_canceller_cleans_the_microphone_before_anything_hears_it() {
    let lucyd = Harness::hearing("");
    lucyd.echo.cancel_everything();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);

    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));

    wait_until("the echo canceller saw the speech", || {
        (lucyd.echo.frames() >= 150).then_some(())
    });
    assert_eq!(lucyd.stt.segments().len(), 0);
}

#[test]
fn with_pipewires_echo_canceller_lucys_own_is_off() {
    let lucyd = Harness::hearing("[audio]\necho_cancellation = \"pipewire\"\n");
    lucyd.echo.cancel_everything();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.stt.script("hello");

    lucyd.mic.feed(SPEECH, ms(512));
    lucyd.mic.feed(SILENCE, ms(1024));

    assert_eq!(lucyd.thinking.next_turn(), "hello");
    assert_eq!(lucyd.echo.frames(), 0);
}

// Tool calls in flight.

/// lucyd with the fixture Plugin `slow` approved, offering `wait`, which
/// needs Confirmation if `confirm`.
fn with_slow_plugin(confirm: bool) -> Harness {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "slow",
        &format!(
            "name = \"slow\"\n\
             version = \"0.1.0\"\n\
             author = \"human\"\n\
             summary = \"Waits\"\n\
             command = [\"./fixture-plugin\", \"wait\"]\n\
             \n\
             [[tools]]\n\
             name = \"wait\"\n\
             handles = \"wait for something\"\n\
             confirm = {confirm}\n\
             confirm_prompt = \"Shall I wait?\"\n\
             \n\
             [access]\n\
             write = [{:?}]\n",
            "/"
        ),
    );
    lucyd.approve("slow");
    lucyd
}

/// `path` with `suffix` added to its file name.
fn beside(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn wait_for_file(path: &std::path::Path) {
    wait_until(&format!("{} never appeared", path.display()), || {
        path.exists().then_some(())
    });
}

/// The model, in a Turn, calls `slow__wait` on a file in the scratch dir.
fn model_waits(
    lucyd: &Harness,
    events: &mut Client,
) -> (
    std::path::PathBuf,
    std::thread::JoinHandle<Result<rmcp::model::CallToolResult, String>>,
) {
    events.start_session().unwrap();
    next_state(events);
    lucyd.say("wait for it");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(events).state, State::Thinking);
    let file = lucyd.scratch().join("go");
    let thinking = lucyd.thinking.clone();
    let path = file.clone();
    let call = std::thread::spawn(move || {
        thinking.call_tool_with("slow__wait", serde_json::json!({"path": path}))
    });
    (file, call)
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

#[test]
fn a_barge_in_cancels_an_unconfirmed_call_in_flight() {
    let lucyd = with_slow_plugin(false);
    let mut events = lucyd.state_client();
    let (file, call) = model_waits(&lucyd, &mut events);
    wait_for_file(&beside(&file, ".waiting"));

    lucyd.start_speaking();

    assert_eq!(next_state(&mut events).state, State::Listening);
    wait_for_file(&beside(&file, ".cancelled"));
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(true), "{}", text(&result));
}

#[test]
fn a_confirmed_call_runs_on_after_a_barge_in_and_the_next_turn_reports_it() {
    let lucyd = with_slow_plugin(true);
    let mut events = lucyd.state_client();
    let (file, call) = model_waits(&lucyd, &mut events);
    assert_eq!(next_state(&mut events).state, State::Confirming);
    assert_eq!(lucyd.tts.next_sentence(), "Shall I wait?");
    lucyd.tts.finish_playing();
    lucyd.say("yes");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    wait_for_file(&beside(&file, ".waiting"));

    lucyd.start_speaking();
    assert_eq!(next_state(&mut events).state, State::Listening);
    std::fs::write(&file, "").unwrap();
    let result = call.join().unwrap().unwrap();
    assert_eq!(text(&result), "done");
    assert!(!beside(&file, ".cancelled").exists());

    lucyd.say("did it work");
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[A confirmed call finished after the user cut in: slow__wait returned: done]\n\
         [You were interrupted; the user heard up to: \"Shall I wait?\"]\n\
         did it work"
    );
}

// With an Authoring build running (ticket 22).

#[test]
fn a_delegate_question_parked_while_lucy_speaks_waits_out_a_barge_in_and_its_turn() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("find my Mount Rainier photo");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let requested = lucyd.thinking.call_tool_with(
        "request_plugin",
        serde_json::json!({
            "name": "photos",
            "goal": "find the user's photos by place or date",
            "clarifications": "",
            "request": "find my Mount Rainier photo",
        }),
    );
    assert_eq!(requested.unwrap().is_error, Some(false));
    let a_few_minutes = lucyd.tts.next_sentence();
    lucyd.thinking.turn_end();
    lucyd.tts.finish_playing();
    until_state(&mut events, State::Listening);
    let delegate = &lucyd.thinking.delegate;
    delegate.next_turn();
    // Lucy is answering something else, and still speaking, when the
    // Delegate asks.
    lucyd.say("what's the weather?");
    lucyd.thinking.next_turn();
    lucyd.thinking.delta("It's sunny.");
    lucyd.thinking.sentence_end();
    assert_eq!(lucyd.tts.next_sentence(), "It's sunny.");
    until_state(&mut events, State::Speaking);
    let asked = delegate.call_tool_with(
        "ask_user",
        serde_json::json!({"question": "Which folder are your photos in?"}),
    );
    assert_eq!(asked.unwrap().is_error, Some(false));

    // The user cuts in, and is still talking once Lucy has stopped.
    lucyd.start_speaking();
    lucyd.wake();
    // The building mark may go off first: the build waits on the user.
    until_state(&mut events, State::Listening);
    lucyd.say("never mind");
    assert_eq!(
        lucyd.thinking.next_turn(),
        "[You were interrupted; the user heard up to: \"It's sunny.\"]\nnever mind"
    );
    lucyd.thinking.delta("Okay.");
    lucyd.thinking.turn_end();
    assert_eq!(lucyd.tts.next_sentence(), "Okay.");
    lucyd.tts.finish_playing();

    // Only now is the Delegate's question asked.
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Which folder are your photos in?"
    );
    assert_eq!(
        lucyd.tts.sentences(),
        [
            a_few_minutes.as_str(),
            "It's sunny.",
            "Okay.",
            "Which folder are your photos in?"
        ]
    );
    assert_eq!(lucyd.tts.stops(), 1);
    assert_eq!(lucyd.thinking.interrupts(), 1);
}

/// Reads state events until `state`.
fn until_state(events: &mut Client, state: State) {
    while next_state(events).state != state {}
}

#[test]
fn talking_over_a_voice_preview_leaves_it_be_and_the_wake_word_opens_a_session_unducked() {
    let lucyd = Harness::hearing(
        "[barge_in]\nwarm_up_ms = 0\n[wake_word]\nmodel = \"/models/hey_lucy.onnx\"\n",
    );
    lucyd.echo.set_erle(Some(20.0));
    let mut events = lucyd.state_client();
    let mut client = lucyd.client();
    let preview = std::thread::spawn(move || client.voice_preview("hello"));
    assert_eq!(lucyd.tts.next_sentence(), "hello");

    // Long enough to cut Lucy off by speech in a Session: outside one,
    // nothing ducks or stops the preview.
    lucyd.mic.feed(SPEECH, ms(640));
    lucyd.mic.feed(SILENCE, ms(960));
    lucyd.mic.wait_until_heard();
    assert_eq!(lucyd.tts.ducks(), Vec::<bool>::new());
    assert_eq!(lucyd.tts.stops(), 0);

    lucyd.stt.script("Hey Lucy, what's the time?");
    lucyd.mic.feed(WAKE, ms(512));
    lucyd.mic.feed(SPEECH, ms(1024));
    lucyd.mic.feed(SILENCE, ms(1024));

    match preview.join().unwrap() {
        Err(lucy_ipc::Error::Rpc { code, .. }) => {
            assert_eq!(code, lucy_ipc::error_code::INTERRUPTED);
        }
        other => panic!("expected the preview interrupted, got {other:?}"),
    }
    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.thinking.next_turn(), "what's the time?");
    assert_eq!(lucyd.tts.ducks(), Vec::<bool>::new());
    assert_eq!(lucyd.thinking.interrupts(), 0);
}
