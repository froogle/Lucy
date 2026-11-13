//! The desktop Built-in tools: `open_url`, `launch_app` and `system_status`.

mod harness;

use harness::Harness;
use serde_json::json;

#[test]
fn open_url_opens_an_http_or_https_url() {
    let lucyd = Harness::start();
    lucyd.client().start_session().unwrap();

    for url in ["https://github.com", "http://example.org/a?b=c"] {
        let result = lucyd
            .thinking
            .call_tool_with("open_url", json!({ "url": url }))
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
    }

    assert_eq!(
        lucyd.launcher.opened(),
        ["https://github.com", "http://example.org/a?b=c"]
    );
}

#[test]
fn open_url_refuses_anything_but_http_and_https() {
    let lucyd = Harness::start();
    lucyd.client().start_session().unwrap();

    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ftp://example.org",
        "github.com",
        "httpx://example.org",
        "https://",
        "",
    ] {
        let result = lucyd
            .thinking
            .call_tool_with("open_url", json!({ "url": url }))
            .unwrap();
        assert_eq!(result.is_error, Some(true), "{url}: {result:?}");
    }

    assert!(lucyd.launcher.opened().is_empty());
}

/// Calls `launch_app` with `name` and returns its structured result.
fn launch_app(lucyd: &Harness, name: &str) -> serde_json::Value {
    let result = lucyd
        .thinking
        .call_tool_with("launch_app", json!({ "name": name }))
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    result.structured_content.expect("a structured result")
}

#[test]
fn launch_app_launches_the_one_app_whose_name_matches() {
    let lucyd = Harness::start();
    let firefox = lucyd.desktop_entry("firefox.desktop", "Name=Firefox");
    lucyd.desktop_entry("gimp.desktop", "Name=GIMP");
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "firefox");

    assert_eq!(result, json!({ "result": "launched", "app": "Firefox" }));
    assert_eq!(lucyd.launcher.launched(), [firefox]);
}

#[test]
fn launch_app_matches_generic_names_and_keywords() {
    let lucyd = Harness::start();
    let kitty = lucyd.desktop_entry(
        "kitty.desktop",
        "Name=kitty\nGenericName=Terminal emulator\nKeywords=shell;prompt;",
    );
    lucyd.desktop_entry("gimp.desktop", "Name=GIMP\nKeywords=paint;");
    lucyd.client().start_session().unwrap();

    for spoken in ["Terminal Emulator", "shell", "prompt"] {
        let result = launch_app(&lucyd, spoken);
        assert_eq!(result, json!({ "result": "launched", "app": "kitty" }));
    }

    assert_eq!(lucyd.launcher.launched(), vec![kitty; 3]);
}

#[test]
fn an_apps_own_name_beats_another_apps_keyword() {
    let lucyd = Harness::start();
    let firefox = lucyd.desktop_entry("firefox.desktop", "Name=Firefox");
    lucyd.desktop_entry(
        "profiles.desktop",
        "Name=Profile Manager\nKeywords=firefox;",
    );
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "Firefox");

    assert_eq!(result, json!({ "result": "launched", "app": "Firefox" }));
    assert_eq!(lucyd.launcher.launched(), [firefox]);
}

#[test]
fn launch_app_offers_candidates_when_several_apps_match_and_launches_none() {
    let lucyd = Harness::start();
    lucyd.desktop_entry("firefox.desktop", "Name=Firefox\nGenericName=Web Browser");
    lucyd.desktop_entry("chromium.desktop", "Name=Chromium\nGenericName=Web Browser");
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "web browser");

    assert_eq!(
        result,
        json!({ "result": "candidates", "candidates": ["Chromium", "Firefox"] })
    );
    assert!(lucyd.launcher.launched().is_empty());
}

#[test]
fn a_users_desktop_entry_replaces_the_system_one_with_the_same_id() {
    let lucyd = Harness::start();
    lucyd.system_desktop_entry("firefox.desktop", "Name=Firefox");
    let mine = lucyd.desktop_entry("firefox.desktop", "Name=Firefox");
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "firefox");

    assert_eq!(result, json!({ "result": "launched", "app": "Firefox" }));
    assert_eq!(lucyd.launcher.launched(), [mine]);
}

#[test]
fn a_user_can_hide_a_system_app() {
    let lucyd = Harness::start();
    lucyd.system_desktop_entry("firefox.desktop", "Name=Firefox");
    lucyd.desktop_entry("firefox.desktop", "Name=Firefox\nHidden=true");
    lucyd.client().start_session().unwrap();

    assert_eq!(
        launch_app(&lucyd, "firefox"),
        json!({ "result": "not_found" })
    );
}

#[test]
fn launch_app_says_not_found_when_nothing_matches() {
    let lucyd = Harness::start();
    lucyd.desktop_entry("firefox.desktop", "Name=Firefox");
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "photoshop");

    assert_eq!(result, json!({ "result": "not_found" }));
    assert!(lucyd.launcher.launched().is_empty());
}

#[test]
fn launch_app_ignores_hidden_entries_and_entries_that_are_not_apps() {
    let lucyd = Harness::start();
    lucyd.desktop_entry("a.desktop", "Name=Notes\nHidden=true");
    lucyd.desktop_entry("b.desktop", "Name=Notes\nNoDisplay=true");
    lucyd.desktop_entry("c.desktop", "Name=Notes\nType=Link");
    lucyd.client().start_session().unwrap();

    let result = launch_app(&lucyd, "notes");

    assert_eq!(result, json!({ "result": "not_found" }));
    assert!(lucyd.launcher.launched().is_empty());
}
