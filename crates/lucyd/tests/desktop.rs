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
