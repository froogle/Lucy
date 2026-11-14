//! `lucy credentials <plugin>`: the user types each credential a Plugin
//! declares, full-screen. The values go to the Secret Service (here a fake),
//! never to lucyd, the screen or the transcript.

mod support;

use std::collections::HashMap;
use std::sync::Mutex;

use lucy::ui::Ui;
use lucy_config::credentials::CredentialStore;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyCode;
use ratatui::Terminal;
use support::{Home, Script};

#[derive(Default)]
struct FakeStore(Mutex<HashMap<(String, String), String>>);

impl FakeStore {
    fn value(&self, plugin: &str, name: &str) -> Option<String> {
        let key = (plugin.to_owned(), name.to_owned());
        self.0.lock().unwrap().get(&key).cloned()
    }
}

impl CredentialStore for FakeStore {
    fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, String> {
        Ok(self.value(plugin, name))
    }

    fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), String> {
        let key = (plugin.to_owned(), name.to_owned());
        self.0.lock().unwrap().insert(key, value.to_owned());
        Ok(())
    }
}

/// Installs a Plugin declaring `credentials`, as lucyd lists it.
fn install(home: &Home, name: &str, credentials: &[&str]) {
    let dir = home.dir.path().join("plugins").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let credentials: Vec<String> = credentials.iter().map(|c| format!("\"{c}\"")).collect();
    let manifest = format!(
        "name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"human\"\n\
         summary = \"Weather where you are\"\ncommand = [\"./plugin\"]\n\
         [access]\ncredentials = [{}]\n",
        credentials.join(", ")
    );
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    std::fs::write(dir.join("plugin"), "not run").unwrap();
}

/// What a run showed: everything said and asked, the transcript, and the
/// last screen drawn.
struct Shown {
    result: Result<(), String>,
    shown: String,
    lines: Vec<String>,
    screen: String,
}

fn enter(home: &Home, plugin: &str, store: &FakeStore, mut keys: Script) -> Shown {
    let terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    let mut ui = Ui::new(terminal, &mut keys, "Lucy credentials");
    let result = lucy::credentials::run(&mut ui, &home.env(), plugin, store);
    let shown = Shown {
        result,
        shown: ui.screen.shown(),
        lines: ui.screen.lines.clone(),
        screen: ui.terminal().backend().to_string(),
    };
    drop(ui);
    assert!(keys.is_done(), "keys left over:\n{}", shown.shown);
    shown
}

#[test]
fn each_declared_credential_is_typed_hidden_and_kept_in_the_secret_service() {
    let home = Home::with_lucyd();
    install(&home, "weather", &["WEATHER_KEY", "CITY_ID"]);
    let store = FakeStore::default();

    let keys = Script::new()
        .typed("w-secret-1")
        .enter()
        .typed("c-secret-2")
        .enter();
    let run = enter(&home, "weather", &store, keys);

    assert_eq!(run.result, Ok(()));
    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "w-secret-1");
    assert_eq!(store.value("weather", "CITY_ID").unwrap(), "c-secret-2");
    assert!(run.shown.contains("WEATHER_KEY"), "{}", run.shown);
    assert!(run.shown.contains("CITY_ID"), "{}", run.shown);
    for value in ["w-secret", "c-secret"] {
        assert!(!run.shown.contains(value), "{}", run.shown);
        assert!(!run.lines.join("\n").contains(value), "{:?}", run.lines);
        assert!(!run.screen.contains(value), "{}", run.screen);
    }
}

#[test]
fn the_secret_is_hidden_on_screen_while_it_is_typed() {
    let home = Home::with_lucyd();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();
    // Ctrl-C mid-entry: the screen still shows what was being typed.
    let keys = Script::new().typed("w-secret-1").ctrl_c();

    let run = enter(&home, "weather", &store, keys);

    assert!(run.result.is_err());
    assert!(!run.screen.contains("w-secret"), "{}", run.screen);
    assert_eq!(store.value("weather", "WEATHER_KEY"), None);
}

#[test]
fn enter_alone_keeps_a_credential_already_entered() {
    let home = Home::with_lucyd();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();
    store.set("weather", "WEATHER_KEY", "old").unwrap();

    let run = enter(
        &home,
        "weather",
        &store,
        Script::new().press(KeyCode::Enter),
    );

    assert_eq!(run.result, Ok(()));
    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "old");
    assert!(run.shown.contains("already"), "{}", run.shown);
}

#[test]
fn an_unknown_plugin_or_one_declaring_none_is_said_plainly() {
    let home = Home::with_lucyd();
    install(&home, "clock", &[]);
    let store = FakeStore::default();

    let unknown = enter(&home, "weather", &store, Script::new());
    let none = enter(&home, "clock", &store, Script::new());

    assert!(unknown
        .result
        .unwrap_err()
        .contains("no Plugin called weather"));
    assert_eq!(none.result, Ok(()));
    assert!(
        none.shown.contains("declares no credentials"),
        "{}",
        none.shown
    );
}
