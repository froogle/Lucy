mod harness;

use harness::Harness;

#[test]
fn a_missing_persona_is_created_with_the_default_and_given_to_the_model() {
    let lucyd = Harness::first_run();

    lucyd.client().start_session().unwrap();

    let persona = std::fs::read_to_string(lucyd.persona_path()).expect("persona.md created");
    let instructions = lucyd.thinking.instructions();
    assert!(instructions.contains(persona.trim()), "{instructions}");
    // The spoken-style rules.
    assert!(persona.contains("one to three short"), "{persona}");
    assert!(persona.contains("markdown"), "{persona}");
    assert!(persona.contains("Let me check"), "{persona}");
    assert!(persona.contains("serious news"), "{persona}");
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
    // An opening Turn would have been sent before anything the user said.
    assert_eq!(lucyd.thinking.next_turn(), "What's the time?");
}
