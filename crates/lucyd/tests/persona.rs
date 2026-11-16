#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]
mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

fn introduce(lucyd: &Harness) {
    let mut events = lucyd.state_client();
    lucyd.client().start_session().unwrap();
    lucyd.thinking.next_turn();
    while next_state(&mut events).state != State::Thinking {}
    lucyd.thinking.turn_end();
    assert_eq!(next_state(&mut events).state, State::Listening);
}

#[test]
fn a_missing_persona_is_created_with_the_default_and_given_to_the_model() {
    let lucyd = Harness::first_run();

    introduce(&lucyd);

    let persona = std::fs::read_to_string(lucyd.persona_path()).expect("persona.md created");
    let instructions = lucyd.thinking.instructions();
    assert!(instructions.contains(persona.trim()), "{instructions}");
    assert!(persona.contains("one to three short"), "{persona}");
    assert!(persona.contains("markdown"), "{persona}");
    assert!(persona.contains("Let me check"), "{persona}");
    assert!(persona.contains("serious news"), "{persona}");
}

#[test]
fn the_default_persona_keeps_greetings_and_goodbyes_to_a_word_or_two() {
    let lucyd = Harness::first_run();

    introduce(&lucyd);

    let instructions = lucyd.thinking.instructions();
    let rule = instructions
        .lines()
        .find(|line| line.contains("only greets you"))
        .unwrap_or_else(|| panic!("no greeting rule in {instructions}"));
    assert!(rule.contains("word or two"), "{rule}");
    assert!(rule.contains("goodbye"), "{rule}");
    assert!(rule.contains("end_session"), "{rule}");
}

#[test]
fn the_default_persona_names_no_one_until_told() {
    let lucyd = Harness::first_run();

    introduce(&lucyd);

    let persona = std::fs::read_to_string(lucyd.persona_path()).unwrap();
    assert!(
        persona.contains("You don't know what to call the user yet."),
        "{persona}"
    );
    for var in ["USER", "LOGNAME"] {
        if let Some(name) = std::env::var(var).ok().filter(|n| n.len() > 2) {
            assert!(!persona.contains(&name), "{var} {name:?} is in {persona}");
        }
    }
}

#[test]
fn hand_edits_apply_at_the_next_session() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();
    client.end_session().unwrap();

    std::fs::write(lucyd.persona_path(), "You are Lucy. Speak like a pirate.").unwrap();
    client.start_session().unwrap();

    assert!(lucyd
        .thinking
        .instructions()
        .contains("Speak like a pirate."));
}

#[test]
fn the_first_session_opens_with_an_introduction_and_only_the_first() {
    let lucyd = Harness::first_run();
    let mut client = lucyd.client();

    client.start_session().unwrap();
    let opening = lucyd.thinking.next_turn();
    assert!(opening.contains("Introduce yourself"), "{opening}");
    assert!(opening.contains("what to call them"), "{opening}");
    lucyd
        .thinking
        .delta("Hi, I'm Lucy! What should I call you?");
    lucyd.thinking.sentence_end();
    lucyd.thinking.turn_end();
    assert_eq!(
        lucyd.tts.next_sentence(),
        "Hi, I'm Lucy! What should I call you?"
    );
    client.end_session().unwrap();

    client.start_session().unwrap();
    lucyd.say("What's the time?");
    assert_eq!(
        lucyd.thinking.next_turn(),
        "What's the time?",
        "an opening Turn would have been sent before anything the user said"
    );
}

#[test]
fn the_name_given_in_the_first_session_is_stored_in_the_persona() {
    let lucyd = Harness::first_run();
    let mut events = lucyd.state_client();
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.thinking.next_turn();
    lucyd
        .thinking
        .delta("Hi, I'm Lucy! What should I call you?");
    lucyd.thinking.turn_end();
    lucyd.tts.next_sentence();
    while next_state(&mut events).state != State::Speaking {}
    lucyd.tts.finish_playing();
    assert_eq!(next_state(&mut events).state, State::Listening);

    lucyd.say("Call me Sam.");
    assert_eq!(lucyd.thinking.next_turn(), "Call me Sam.");
    let persona = format!("{}\nCall the user Sam.\n", lucyd.thinking.instructions());
    let result = lucyd
        .thinking
        .call_tool_with("update_persona", serde_json::json!({"persona": persona}))
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    client.end_session().unwrap();

    assert_eq!(
        std::fs::read_to_string(lucyd.persona_path()).unwrap(),
        persona
    );
    client.start_session().unwrap();
    assert!(lucyd.thinking.instructions().contains("Call the user Sam."));
}

#[test]
fn a_persona_change_by_voice_is_saved_and_read_back_without_approval() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    lucyd.client().start_session().unwrap();
    next_state(&mut events);
    lucyd.say("Be a bit more sarcastic.");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);

    let persona = "You are Lucy. Be properly sarcastic.";
    let result = lucyd
        .thinking
        .call_tool_with("update_persona", serde_json::json!({"persona": persona}))
        .unwrap();

    assert_ne!(
        result.is_error,
        Some(true),
        "saved at once: no Approval, so no Confirming: {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(lucyd.persona_path()).unwrap(),
        persona
    );
    assert_eq!(lucyd.client().status().unwrap().state, State::Thinking);
    let text = format!("{:?}", result.content);
    assert!(text.contains(persona), "{text}");
    assert!(text.contains("Tell the user what changed"), "{text}");
}

#[test]
fn an_invalid_config_on_the_first_run_does_not_use_up_the_introduction() {
    let lucyd = Harness::first_run();
    let mut events = lucyd.state_client();
    let mut client = lucyd.client();
    std::fs::write(lucyd.config_path(), "[tiers]\nfast = \"missing\"\n").unwrap();
    client.start_session().unwrap();
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    while next_state(&mut events).state != State::Idle {}

    std::fs::remove_file(lucyd.config_path()).unwrap();
    client.start_session().unwrap();

    let opening = lucyd.thinking.next_turn();
    assert!(opening.contains("Introduce yourself"), "{opening}");
}

#[test]
fn a_refused_provider_on_the_first_run_does_not_use_up_the_introduction() {
    let lucyd = Harness::first_run();
    let mut events = lucyd.state_client();
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.thinking.next_turn();
    lucyd.thinking.unavailable("I can't reach Claude.");
    lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    while next_state(&mut events).state != State::Idle {}

    client.start_session().unwrap();

    let opening = lucyd.thinking.next_turn();
    assert!(opening.contains("Introduce yourself"), "{opening}");
}

#[test]
fn update_persona_needs_the_new_text() {
    let lucyd = Harness::start();
    lucyd.client().start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("update_persona", serde_json::json!({"persona": "  "}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        std::fs::read_to_string(lucyd.persona_path()).unwrap(),
        "You are Lucy."
    );
}
