//! The shipped news Plugin, run by lucyd.
//! It is approved when lucyd scans it, offered to the model, and keeps its
//! preferences across Sessions.
//! Nothing here reaches the network; the Plugin's own tests cover briefings
//! against recorded feeds.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::path::{Path, PathBuf};

use harness::{next_state, Harness};
use lucy_ipc::State;
use serde_json::{json, Value};

fn news_binary() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let binary = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("lucy-news"))
        .find(|path| path.is_file());
    binary.expect("lucy-news not found; run `cargo build -p lucy-news` or `cargo test --workspace`")
}

fn install_news(lucyd: &Harness) -> PathBuf {
    let manifest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lucy-news/manifest.toml"),
    )
    .unwrap();
    let folder = lucyd.install_shipped_plugin("news", &manifest);
    let copied = std::process::Command::new("cp")
        .arg(news_binary())
        .arg(folder.join("lucy-news"))
        .status()
        .unwrap();
    assert!(copied.success());
    folder
}

fn reply(lucyd: &Harness, arguments: Value) -> Value {
    let result = lucyd
        .thinking
        .call_tool_with("news__set_preference", arguments)
        .unwrap();
    let text: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect();
    assert_eq!(result.is_error, Some(false), "{text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_shipped_news_plugin_is_offered_without_asking_and_keeps_a_swap_into_the_next_session() {
    let lucyd = Harness::start();
    install_news(&lucyd);
    let mut events = lucyd.state_client();

    events.start_session().unwrap();

    assert_eq!(next_state(&mut events).state, State::Listening);
    let tools = lucyd.thinking.tool_names();
    assert!(tools.contains(&"news__briefing".into()), "{tools:?}");
    assert_eq!(lucyd.tts.sentences(), Vec::<String>::new());

    let swapped = reply(&lucyd, json!({"drop": "Sky"}));
    assert_eq!(
        swapped["changed"],
        json!(["Sky News is out; you won't hear from it again."]),
        "no Confirmation: the call answers straight away"
    );
    events.end_session().unwrap();

    events.start_session().unwrap();
    let next = reply(&lucyd, json!({}));
    assert_eq!(next["rotation"], json!(["CNN", "NPR"]));
    assert!(lucyd
        .state_dir()
        .join("plugins/news/preferences.toml")
        .exists());
}
