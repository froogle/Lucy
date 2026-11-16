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

fn asked_for(lucyd: &Harness, holder: &mut Client, events: &mut Client, what: &str) -> InputOpen {
    assert_eq!(next_state(events).state, State::Typing);
    assert_eq!(
        line_said(lucyd),
        format!("I've put a box on screen for your {what}.")
    );
    let open = next_open(holder);
    assert_eq!(open.fields.len(), 1);
    assert_eq!(open.fields[0].kind, FieldKind::Secret);
    open
}

const ASKED_TEST: &str =
    "a_missing_credential_is_asked_for_after_the_confirmation_and_the_call_carries_on";

#[test]
fn a_missing_credential_is_asked_for_after_the_confirmation_and_the_call_carries_on() {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = env_logger::try_init();
    }
    let lucyd = Harness::start();
    let declared = ["LINEAR_API_KEY", "CITY", "UNITS"];
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
    let first = asked_for(&lucyd, &mut holder, &mut events, "Linear API key");
    assert_eq!(first.fields[0].name, "LINEAR_API_KEY");
    lucyd.answer_fields(&mut holder, &first.id, &[("LINEAR_API_KEY", "w-typed-1")]);
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let second = asked_for(&lucyd, &mut holder, &mut events, "Units");
    assert_eq!(second.fields[0].name, "UNITS");
    lucyd.answer_fields(&mut holder, &second.id, &[("UNITS", "u-typed-2")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let env = plugin_env(&lucyd, "weather");
    assert_eq!(env["LINEAR_API_KEY"], "w-typed-1");
    assert_eq!(env["CITY"], "Leeds");
    assert_eq!(env["UNITS"], "u-typed-2");
    let mut kept = lucyd.credentials.values();
    kept.sort();
    assert_eq!(kept, ["Leeds", "u-typed-2", "w-typed-1"]);
    events.end_session().unwrap();
    lucyd.assert_nothing_private_on_bus();
}

#[test]
fn a_typed_credential_never_reaches_lucyds_log_or_a_plugins_stderr() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([ASKED_TEST, "--exact", "--nocapture"])
        .env("RUST_LOG", "trace")
        .output()
        .unwrap();

    let log =
        String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
    assert!(!log.contains("w-typed-1"), "{log}");
    assert!(!log.contains("u-typed-2"), "{log}");
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

fn assert_unentered(result: &CallToolResult, name: &str, reason: &str) {
    assert_eq!(result.is_error, Some(true));
    let said = text(result);
    assert!(said.contains(name), "{said}");
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

    let open = asked_for(&lucyd, &mut holder, &mut events, "Weather key");
    holder.cancel_input(&open.id).unwrap();

    assert_unentered(&call.join().unwrap().unwrap(), "WEATHER_KEY", "cancelled");
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
    asked_for(&lucyd, &mut holder, &mut events, "Weather key");

    lucyd.clock.advance_once_set(Duration::from_secs(120));

    assert_unentered(&call.join().unwrap().unwrap(), "WEATHER_KEY", "timed out");
    assert!(lucyd.credentials.values().is_empty());
}

#[test]
fn a_credential_typed_before_a_cancelled_one_is_kept_and_only_the_other_is_asked_again() {
    let lucyd = Harness::start();
    let declared = ["GITHUB_OAUTH_TOKEN", "CLIENT_ID"];
    lucyd.install_plugin("weather", &manifest("weather", &declared));
    lucyd.approve("weather");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let call = model_calls(&lucyd);

    let first = asked_for(&lucyd, &mut holder, &mut events, "Github OAuth token");
    lucyd.answer_fields(
        &mut holder,
        &first.id,
        &[("GITHUB_OAUTH_TOKEN", "g-typed-1")],
    );
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let second = asked_for(&lucyd, &mut holder, &mut events, "Client ID");
    holder.cancel_input(&second.id).unwrap();

    assert_unentered(&call.join().unwrap().unwrap(), "CLIENT_ID", "cancelled");
    assert_eq!(lucyd.credentials.values(), ["g-typed-1"]);
    assert_eq!(next_state(&mut events).state, State::Thinking);

    let call = model_calls(&lucyd);
    let again = asked_for(&lucyd, &mut holder, &mut events, "Client ID");
    assert_eq!(again.fields[0].name, "CLIENT_ID");
    lucyd.answer_fields(&mut holder, &again.id, &[("CLIENT_ID", "c-typed-2")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let env = plugin_env(&lucyd, "weather");
    assert_eq!(env["GITHUB_OAUTH_TOKEN"], "g-typed-1");
    assert_eq!(env["CLIENT_ID"], "c-typed-2");
}

#[test]
fn a_plugin_stopped_while_confirming_asks_for_a_credential_gone_meanwhile() {
    let lucyd = Harness::start();
    lucyd.install_plugin(
        "weather",
        &confirming(&manifest("weather", &["WEATHER_KEY"])),
    );
    lucyd.approve("weather");
    lucyd.credentials.enter("weather", "WEATHER_KEY", "w-old");
    let mut holder = lucyd.input("a widget");
    let mut events = session(&lucyd);
    let confirm = |events: &mut Client| {
        assert_eq!(next_state(events).state, State::Confirming);
        line_said(&lucyd);
    };
    let call = model_calls(&lucyd);
    confirm(&mut events);
    lucyd.say("Yes.");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));

    let call = model_calls(&lucyd);
    confirm(&mut events);
    lucyd.credentials.forget("weather", "WEATHER_KEY");
    lucyd.client().set_plugin_enabled("weather", true).unwrap();
    lucyd.say("Yes.");
    assert_eq!(next_state(&mut events).state, State::Thinking);
    let open = asked_for(&lucyd, &mut holder, &mut events, "Weather key");
    lucyd.answer_fields(&mut holder, &open.id, &[("WEATHER_KEY", "w-new")]);

    let result = call.join().unwrap().unwrap();
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    assert_eq!(plugin_env(&lucyd, "weather")["WEATHER_KEY"], "w-new");
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
