//! `lucy credentials <plugin>`: the Plugins section, opened entering the
//! credentials that Plugin declares, each typed hidden. The values go to
//! the Secret Service (here a fake), never to lucyd, the screen or the
//! transcript.

mod support;

use lucy::menu::Open;
use lucy_config::credentials::CredentialStore;
use ratatui::crossterm::event::KeyCode;
use support::{FakeStore, Home, Script, Tui};

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

fn configured_home() -> Home {
    let home = Home::with_lucyd();
    std::fs::write(home.config_path(), "# Mine.\n").unwrap();
    home
}

fn enter(home: &Home, plugin: &str, store: &FakeStore, keys: Script) -> Tui {
    home.tui_with(
        &home.env(),
        store,
        Open::Credentials(plugin.to_owned()),
        keys,
    )
}

#[test]
fn it_opens_the_plugins_section_entering_that_plugins_credentials() {
    let home = configured_home();
    install(&home, "weather", &["WEATHER_KEY", "CITY_ID"]);
    let store = FakeStore::default();

    let keys = Script::new()
        .typed("w-secret-1")
        .enter()
        .typed("c-secret-2")
        .enter()
        .press(KeyCode::Esc) // back to the list
        .typed("q");
    let tui = enter(&home, "weather", &store, keys);

    assert_eq!(tui.result, Ok(()));
    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "w-secret-1");
    assert_eq!(store.value("weather", "CITY_ID").unwrap(), "c-secret-2");
    assert!(
        tui.shown.contains(
            "weather: awaiting Approval; credentials: WEATHER_KEY (entered), CITY_ID (entered)"
        ),
        "{}",
        tui.shown
    );
    for value in ["w-secret", "c-secret"] {
        assert!(!tui.shown.contains(value), "{}", tui.shown);
        assert!(!tui.screen.contains(value), "{}", tui.screen);
    }
    assert!(
        tui.screen.contains("Approvals"),
        "back on the list:\n{}",
        tui.screen
    );
}

#[test]
fn the_secret_is_hidden_on_screen_while_it_is_typed() {
    let home = configured_home();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();
    // Ctrl-C mid-entry: the screen still shows what was being typed.
    let keys = Script::new().typed("w-secret-1").ctrl_c();

    let tui = enter(&home, "weather", &store, keys);

    assert!(!tui.screen.contains("w-secret"), "{}", tui.screen);
    assert_eq!(store.value("weather", "WEATHER_KEY"), None);
}

#[test]
fn q_is_typed_into_a_credential_not_taken_as_quit() {
    let home = configured_home();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();

    let keys = Script::new().typed("qq").enter().typed("q");
    enter(&home, "weather", &store, keys);

    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "qq");
}

#[test]
fn enter_alone_keeps_a_credential_already_entered() {
    let home = configured_home();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();
    store.set("weather", "WEATHER_KEY", "old").unwrap();

    let tui = enter(&home, "weather", &store, Script::new().enter().typed("q"));

    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "old");
    assert!(tui.shown.contains("already"), "{}", tui.shown);
}

#[test]
fn an_unknown_plugin_or_one_declaring_none_is_said_plainly() {
    let home = configured_home();
    install(&home, "clock", &[]);
    let store = FakeStore::default();

    let unknown = enter(&home, "weather", &store, Script::new().typed("q"));
    let none = enter(&home, "clock", &store, Script::new().typed("q"));

    assert!(
        unknown.shown.contains("There's no Plugin called weather."),
        "{}",
        unknown.shown
    );
    assert!(
        none.shown.contains("declares no credentials"),
        "{}",
        none.shown
    );
}

#[test]
fn any_plugins_credentials_can_be_entered_from_its_row() {
    let home = configured_home();
    install(&home, "weather", &["WEATHER_KEY"]);
    let store = FakeStore::default();

    // Plugins, then the weather Plugin's row.
    let keys = Script::new()
        .down(5)
        .enter()
        .enter()
        .typed("k")
        .enter()
        .typed("q");
    home.tui_with(&home.env(), &store, Open::Lucy, keys);

    assert_eq!(store.value("weather", "WEATHER_KEY").unwrap(), "k");
}
