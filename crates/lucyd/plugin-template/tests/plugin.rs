//! The Plugin's tests, through its MCP interface and recorded web
//! responses (`tests/support`). Write one for each thing the user will ask.

mod support;

use serde_json::json;
use support::Plugin;

#[test]
fn it_lists_the_tools_its_manifest_lists() {
    let plugin = Plugin::start();
    assert_eq!(plugin.tool_names(), ["example"]);
}

#[test]
fn example_echoes_its_text() {
    let plugin = Plugin::start();
    assert_eq!(plugin.call("example", json!({"text": "hello"})), "hello");
    assert!(plugin.web.requested().is_empty());
}
