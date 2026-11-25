#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]
mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;

const HEADING: &str = "## What the user asked you to remember";

const MANIFEST: &str = "\
name = \"fixture\"
version = \"0.1.0\"
author = \"human\"
summary = \"A fixture Plugin\"
command = [\"./fixture-plugin\", \"echo\"]

[[tools]]
name = \"echo\"
handles = \"echo things\"
examples = [\"please echo\", \"echo it\"]

[access]
";

#[test]
fn with_no_memory_the_instructions_are_the_persona_alone() {
    let lucyd = Harness::start();

    lucyd.client().start_session().unwrap();

    assert_eq!(lucyd.thinking.instructions(), "You are Lucy.");
}

#[test]
fn memory_follows_the_persona_in_the_instructions() {
    let lucyd = Harness::start();
    let memory = "- 2026-11-20: The user's sister is called Anna.\n";
    std::fs::write(lucyd.memory_path(), memory).unwrap();

    lucyd.client().start_session().unwrap();

    assert_eq!(
        lucyd.thinking.instructions(),
        format!("You are Lucy.\n\n{HEADING}\n\n{}", memory.trim_end())
    );
}

#[test]
fn an_empty_memory_adds_no_heading() {
    let lucyd = Harness::start();
    std::fs::write(lucyd.memory_path(), "\n  \n").unwrap();

    lucyd.client().start_session().unwrap();

    assert_eq!(lucyd.thinking.instructions(), "You are Lucy.");
}

#[test]
fn memory_comes_before_the_plugin_scan_report() {
    let lucyd = Harness::start();
    std::fs::write(lucyd.memory_path(), "- 2026-11-20: Coffee, black.\n").unwrap();
    lucyd.install_plugin("fixture", MANIFEST);

    lucyd.client().start_session().unwrap();

    let instructions = lucyd.thinking.instructions();
    let fact = instructions.find("Coffee, black.").expect("Memory given");
    let heading = instructions.find(HEADING).unwrap();
    let report = instructions
        .find("awaiting Approval")
        .expect("scan report given");
    assert!(
        instructions.starts_with("You are Lucy.\n\n"),
        "{instructions}"
    );
    assert!(heading < fact && fact < report, "{instructions}");
}

#[test]
fn an_unreadable_memory_leaves_it_out_and_the_session_still_starts() {
    let lucyd = Harness::start();
    let mut events = lucyd.state_client();
    std::fs::create_dir(lucyd.memory_path()).unwrap();

    lucyd.client().start_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Listening);
    assert_eq!(lucyd.thinking.instructions(), "You are Lucy.");
}

fn today() -> String {
    let date = std::process::Command::new("date")
        .arg("+%F")
        .output()
        .unwrap();
    String::from_utf8(date.stdout).unwrap().trim().to_owned()
}

fn in_a_session(lucyd: &Harness) {
    let mut events = lucyd.state_client();
    lucyd.client().start_session().unwrap();
    next_state(&mut events);
    lucyd.say("Remember that.");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    format!("{:?}", result.content)
}

#[test]
fn remember_creates_memory_with_one_bullet_dated_today_without_approval() {
    let lucyd = Harness::start();
    in_a_session(&lucyd);
    std::fs::remove_dir_all(lucyd.memory_path().parent().unwrap()).unwrap();

    let result = lucyd
        .thinking
        .call_tool_with(
            "remember",
            serde_json::json!({"fact": "The user's sister is called Anna."}),
        )
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    let bullet = format!("- {}: The user's sister is called Anna.\n", today());
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        bullet
    );
    assert_eq!(lucyd.client().status().unwrap().state, State::Thinking);
    let text = text(&result);
    assert!(text.contains("The user's sister is called Anna."), "{text}");
    assert!(text.contains("Tell the user what you saved"), "{text}");
}

#[test]
fn remember_appends_and_leaves_the_other_facts_as_they_were() {
    let lucyd = Harness::start();
    let before = "- 2026-11-01: Coffee, black.\n- 2026-11-02:   Spaces  kept  as typed.";
    std::fs::write(lucyd.memory_path(), before).unwrap();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with(
            "remember",
            serde_json::json!({"fact": "Dentist on Friday."}),
        )
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        format!("{before}\n- {}: Dentist on Friday.\n", today())
    );
}

#[test]
fn remember_needs_a_fact() {
    let lucyd = Harness::start();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with("remember", serde_json::json!({"fact": "  "}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(!lucyd.memory_path().exists());
}

#[test]
fn a_fact_remembered_in_one_session_is_in_the_next_sessions_instructions() {
    let lucyd = Harness::start();
    in_a_session(&lucyd);
    lucyd
        .thinking
        .call_tool_with("remember", serde_json::json!({"fact": "Coffee, black."}))
        .unwrap();
    let mut client = lucyd.client();
    client.end_session().unwrap();

    client.start_session().unwrap();

    assert_eq!(
        lucyd.thinking.instructions(),
        format!(
            "You are Lucy.\n\n{HEADING}\n\n- {}: Coffee, black.",
            today()
        )
    );
}

#[test]
fn forget_removes_every_matching_fact_and_leaves_the_rest_without_approval() {
    let lucyd = Harness::start();
    std::fs::write(
        lucyd.memory_path(),
        "- 2026-11-01: Anna is the user's sister.\n\
         - 2026-11-02: Coffee, black.\n\
         - 2026-11-03: Anna is the user's sister.\n\
         - 2026-11-04: Anna is the user's sister, and a nurse.\n",
    )
    .unwrap();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with(
            "forget",
            serde_json::json!({"fact": "Anna is the user's sister."}),
        )
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        "- 2026-11-02: Coffee, black.\n\
         - 2026-11-04: Anna is the user's sister, and a nurse.\n"
    );
    assert_eq!(lucyd.client().status().unwrap().state, State::Thinking);
    let text = text(&result);
    assert!(text.contains("Anna is the user's sister."), "{text}");
    assert!(text.contains("Tell the user what you forgot"), "{text}");
    assert!(text.contains("gone from your Memory now"), "{text}");
}

#[test]
fn forget_with_no_match_is_an_error_listing_the_memory() {
    let lucyd = Harness::start();
    let memory = "- 2026-11-02: Coffee, black.\n";
    std::fs::write(lucyd.memory_path(), memory).unwrap();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with("forget", serde_json::json!({"fact": "Coffee"}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = text(&result);
    assert!(text.contains("- 2026-11-02: Coffee, black."), "{text}");
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        memory
    );
}

#[test]
fn a_fact_spread_over_lines_is_remembered_as_one_bullet() {
    let lucyd = Harness::start();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with(
            "remember",
            serde_json::json!({"fact": "  Dentist on Friday,\n  \r\n at  ten.\n"}),
        )
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        format!("- {}: Dentist on Friday, at  ten.\n", today())
    );
}

#[test]
fn a_fact_spread_over_lines_is_forgotten_with_the_same_text() {
    let lucyd = Harness::start();
    in_a_session(&lucyd);
    let fact = "Dentist on Friday,\n at ten.";
    lucyd
        .thinking
        .call_tool_with("remember", serde_json::json!({"fact": fact}))
        .unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("forget", serde_json::json!({"fact": fact}))
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(std::fs::read_to_string(lucyd.memory_path()).unwrap(), "");
}

#[test]
fn a_hand_written_fact_with_spaces_after_its_date_can_be_forgotten() {
    let lucyd = Harness::start();
    std::fs::write(
        lucyd.memory_path(),
        "- 2026-11-01: Coffee, black.\n- 2026-11-02:   Spaces kept\n",
    )
    .unwrap();
    in_a_session(&lucyd);

    let result = lucyd
        .thinking
        .call_tool_with("forget", serde_json::json!({"fact": "Spaces kept"}))
        .unwrap();

    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(lucyd.memory_path()).unwrap(),
        "- 2026-11-01: Coffee, black.\n"
    );
}
