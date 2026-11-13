mod harness;

use harness::Harness;
use serde_json::json;

/// A Manifest for the fixture Plugin offering `tools`, with `access` as the
/// body of its `[access]` table.
fn manifest(name: &str, tools: &[&str], access: &str) -> String {
    let mut manifest = format!(
        "name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         author = \"human\"\n\
         summary = \"A fixture Plugin\"\n\
         command = [\"./fixture-plugin\", {}]\n",
        tools
            .iter()
            .map(|tool| format!("\"{tool}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for tool in tools {
        manifest.push_str(&format!(
            "\n[[tools]]\n\
             name = \"{tool}\"\n\
             handles = \"{tool} things\"\n\
             examples = [\"please {tool}\", \"{tool} it\"]\n"
        ));
    }
    manifest.push_str(&format!("\n[access]\n{access}\n"));
    manifest
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

#[test]
fn an_approved_plugin_tool_is_listed_with_its_manifest_description_and_approved_schema() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    lucyd.client().start_session().unwrap();

    let tools = lucyd.thinking.tools();

    let tool = tools
        .iter()
        .find(|tool| tool.name == "fixture__echo")
        .expect("fixture__echo is listed");
    let description = tool.description.as_deref().unwrap();
    assert!(description.contains("echo things"), "{description}");
    assert!(description.contains("please echo"), "{description}");
    assert!(description.contains("echo it"), "{description}");
    assert_eq!(tool.input_schema["description"], "approved echo");
}

#[test]
fn a_call_to_a_plugin_tool_is_forwarded_to_the_plugin() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    lucyd.client().start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("fixture__echo", json!({"text": "hello there"}))
        .unwrap();

    assert_eq!(result.is_error, Some(false));
    assert_eq!(text(&result), "hello there");
}
