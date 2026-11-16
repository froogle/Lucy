//! Plugin credentials: declared by name in the Manifest.
//! They are kept in the Secret Service, and handed to the approved Plugin
//! as env vars when it starts.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::thread::JoinHandle;
use std::time::Duration;

use harness::{next_state, Harness};
use lucy_ipc::{Client, Event, FieldKind, InputOpen, PluginStatus, State};
use rmcp::model::CallToolResult;
use serde_json::json;

type Call = JoinHandle<Result<CallToolResult, String>>;

fn manifest(name: &str, credentials: &[&str]) -> String {
    let credentials: Vec<String> = credentials.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        "name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         author = \"human\"\n\
         summary = \"A fixture Plugin\"\n\
         command = [\"./fixture-plugin\", \"env\"]\n\
         \n[[tools]]\n\
         name = \"env\"\n\
         handles = \"env things\"\n\
         \n[access]\n\
         credentials = [{}]\n",
        credentials.join(", ")
    )
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn plugin_env(lucyd: &Harness, name: &str) -> std::collections::BTreeMap<String, String> {
    let path = lucyd.state_dir().join("plugins").join(name).join("env");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

#[test]
fn a_plugin_gets_its_declared_credentials_as_env_vars_and_nothing_more() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY"]));
    lucyd.approve("weather");
    lucyd.credentials.enter("weather", "WEATHER_KEY", "w-123");
    lucyd.credentials.enter("weather", "OTHER_KEY", "o-456");
    lucyd.credentials.enter("mail", "WEATHER_KEY", "m-789");
    lucyd.client().start_session().unwrap();

    let result = lucyd.thinking.call_tool_with("weather__env", json!({}));

    let result = result.unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let env = plugin_env(&lucyd, "weather");
    assert_eq!(env["WEATHER_KEY"], "w-123");
    let names: Vec<&str> = env.keys().map(String::as_str).collect();
    let allowed = [
        "HOME",
        "LANG",
        "LUCY_PLUGIN_STATE_DIR",
        "PATH",
        "WEATHER_KEY",
    ];
    assert!(names.iter().all(|n| allowed.contains(n)), "{names:?}");
}

#[test]
fn plugins_list_names_each_declared_credential_but_never_its_value() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY", "CITY"]));
    lucyd.credentials.enter("weather", "WEATHER_KEY", "w-123");

    let listed = lucyd.client().plugins().unwrap().plugins;

    assert_eq!(listed[0].credentials, ["WEATHER_KEY", "CITY"]);
    assert!(!format!("{listed:?}").contains("w-123"));
}

fn confirming(manifest: &str) -> String {
    manifest.replace(
        "handles = \"env things\"\n",
        "handles = \"env things\"\nconfirm = true\n",
    )
}

fn session(lucyd: &Harness) -> Client {
    let mut events = lucyd.state_client();
    events.start_session().unwrap();
    next_state(&mut events);
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();
    assert_eq!(next_state(&mut events).state, State::Thinking);
    events
}

fn model_calls(lucyd: &Harness) -> Call {
    let thinking = lucyd.thinking.clone();
    std::thread::spawn(move || thinking.call_tool_with("weather__env", json!({})))
}

fn next_open(holder: &mut Client) -> InputOpen {
    loop {
        if let Event::InputOpen(open) = holder.next_event().expect("an input.open event") {
            return open;
        }
    }
}

fn line_said(lucyd: &Harness) -> String {
    let line = lucyd.tts.next_sentence();
    lucyd.tts.finish_playing();
    line
}

fn asked_for(lucyd: &Harness, holder: &mut Client, events: &mut Client) -> InputOpen {
    assert_eq!(next_state(events).state, State::Typing);
    assert_eq!(
        line_said(lucyd),
        "I've put a box on screen for your secret."
    );
    let open = next_open(holder);
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].kind, FieldKind::Secret);
    open
}

#[test]
fn a_missing_credential_is_asked_for_after_the_confirmation_and_the_call_carries_on() {
    let lucyd = Harness::start();
    let declared = ["WEATHER_KEY", "CITY", "UNITS"];
    lucyd.install_plugin("weather", &confirming(&manifest("weather", &declared)));
    lucyd.approve("weather");
    lucyd.credentials.enter("weather", "CITY", "Leeds");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd);

    assert_eq!(next_state(&mut events).state, State::Confirming);
    line_said(&lucyd);
    lucyd.say("Yes.");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let first = asked_for(&lucyd, &mut holder, &mut events);
    assert_eq!(first.fields[0].name, "WEATHER_KEY");
    lucyd.answer_fields(&mut holder, &first.id, &[("WEATHER_KEY", "w-typed-1")]);
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let second = asked_for(&lucyd, &mut holder, &mut events);
    assert_eq!(second.fields[0].name, "UNITS");
    lucyd.answer_fields(&mut holder, &second.id, &[("UNITS", "u-typed-2")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let env = plugin_env(&lucyd, "weather");
    assert_eq!(env["WEATHER_KEY"], "w-typed-1");
    assert_eq!(env["CITY"], "Leeds");
    assert_eq!(env["UNITS"], "u-typed-2");
    let mut kept = lucyd.credentials.values();
    kept.sort();
    assert_eq!(kept, ["Leeds", "u-typed-2", "w-typed-1"]);
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn a_declined_confirmation_asks_for_no_credential() {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "weather",
        &confirming(&manifest("weather", &["WEATHER_KEY"])),
    );
    lucyd.approve("weather");
    let _holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd);

    assert_eq!(next_state(&mut events).state, State::Confirming);
    line_said(&lucyd);
    lucyd.say("No.");

    let result = call.join().unwrap().unwrap();
    assert!(text(&result).starts_with("declined"), "{}", text(&result));
    let said = lucyd.tts.sentences();
    assert!(!said.iter().any(|line| line.contains("box")), "{said:?}");
    assert!(lucyd.credentials.values().is_empty());
    assert!(!lucyd.state_dir().join("plugins/weather/env").exists());
}

fn unentered(result: &CallToolResult, reason: &str) {
    assert_eq!(result.is_error, Some(true));
    let said = text(result);
    assert!(said.contains("WEATHER_KEY"), "{said}");
    assert!(said.contains(reason), "{said}");
    assert!(said.contains("lucy credentials weather"), "{said}");
    assert!(!said.contains("broken"), "{said}");
}

#[test]
fn a_cancelled_credential_box_fails_the_call_with_where_to_enter_it() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY"]));
    lucyd.approve("weather");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd);

    let open = asked_for(&lucyd, &mut holder, &mut events);
    holder.cancel_input(&open.id).unwrap();

    unentered(&call.join().unwrap().unwrap(), "cancelled");
    assert!(lucyd.credentials.values().is_empty());
    assert!(!lucyd.state_dir().join("plugins/weather/env").exists());
}

#[test]
fn a_credential_box_left_untyped_fails_the_call_with_where_to_enter_it() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY"]));
    lucyd.approve("weather");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd);
    asked_for(&lucyd, &mut holder, &mut events);

    lucyd.clock.advance_once_set(Duration::from_secs(120));

    unentered(&call.join().unwrap().unwrap(), "timed out");
    assert!(lucyd.credentials.values().is_empty());
}

#[test]
fn an_unreadable_secret_service_is_reported_and_the_plugin_is_not_started() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY"]));
    lucyd.approve("weather");
    lucyd.credentials.fail("it is locked");
    lucyd.client().start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("weather__env", json!({}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let said = text(&result);
    assert!(said.contains("it is locked"), "{said}");
    let env = lucyd.state_dir().join("plugins/weather/env");
    assert!(!env.exists());
}

#[test]
fn credential_values_never_reach_the_bus_or_the_model() {
    let lucyd = Harness::start();
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY"]));
    lucyd.approve("weather");
    lucyd
        .credentials
        .enter("weather", "WEATHER_KEY", "w-secret-123");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.say("what's the weather");
    lucyd.thinking.next_turn();

    let result = lucyd
        .thinking
        .call_tool_with("weather__env", json!({}))
        .unwrap();
    lucyd.thinking.delta("It's sunny.");
    lucyd.thinking.sentence_end();
    lucyd.tts.next_sentence();
    lucyd.thinking.turn_end();
    lucyd.tts.finish_playing();
    client.end_session().unwrap();

    assert_eq!(plugin_env(&lucyd, "weather")["WEATHER_KEY"], "w-secret-123");
    assert!(!text(&result).contains("w-secret-123"));
    assert!(!lucyd.thinking.instructions().contains("w-secret-123"));
    lucyd.assert_nothing_private_on_bus();
    let config = std::fs::read_to_string(lucyd.config_path()).unwrap_or_default();
    assert!(!config.contains("w-secret-123"));
}

#[test]
fn a_manifest_declaring_a_credential_lucy_sets_herself_or_no_env_var_name_is_broken() {
    let lucyd = Harness::start();
    let refused = [
        "PATH",
        "HOME",
        "LANG",
        "LUCY_PLUGIN_STATE_DIR",
        "LD_PRELOAD",
    ];
    let invalid = ["weather-key", "1KEY", "A=B", ""];
    let declared: Vec<&str> = refused.iter().chain(&invalid).copied().collect();
    for (i, credential) in declared.iter().enumerate() {
        let name = format!("plugin{i}");
        lucyd.install_plugin(&name, &manifest(&name, &[credential]));
    }
    lucyd.install_plugin("weather", &manifest("weather", &["WEATHER_KEY", "_CITY2"]));

    let listed = lucyd.client().plugins().unwrap().plugins;

    for (i, credential) in declared.iter().enumerate() {
        let name = format!("plugin{i}");
        let plugin = listed.iter().find(|p| p.name == name).unwrap();
        assert_eq!(plugin.status, PluginStatus::Broken, "{credential}");
        let problem = plugin.problem.as_deref().unwrap_or_default();
        assert!(problem.contains(&format!("{credential:?}")), "{problem}");
    }
    let weather = listed.iter().find(|p| p.name == "weather").unwrap();
    assert_ne!(weather.status, PluginStatus::Broken, "{weather:?}");
}
