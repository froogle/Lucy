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

/// Whether the model of a fresh Session is offered `tool`.
fn offered(lucyd: &Harness, tool: &str) -> bool {
    let mut client = lucyd.client();
    client.start_session().unwrap();
    let offered = lucyd.thinking.tool_names().iter().any(|name| name == tool);
    client.end_session().unwrap();
    offered
}

#[test]
fn a_plugin_without_an_approval_is_not_listed() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));

    assert!(!offered(&lucyd, "fixture__echo"));
}

#[test]
fn changing_the_manifest_after_approval_makes_the_plugin_unusable_until_reapproved() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    assert!(offered(&lucyd, "fixture__echo"));

    let changed = manifest("fixture", &["echo"], "read = [\"/home\"]");
    std::fs::write(folder.join("manifest.toml"), changed).unwrap();

    assert!(!offered(&lucyd, "fixture__echo"));
    lucyd.approve("fixture");
    assert!(offered(&lucyd, "fixture__echo"));
}

#[test]
fn changing_the_executable_after_approval_makes_the_plugin_unusable() {
    use std::io::Write;
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    assert!(offered(&lucyd, "fixture__echo"));

    let mut executable = std::fs::OpenOptions::new()
        .append(true)
        .open(folder.join("fixture-plugin"))
        .unwrap();
    executable.write_all(b"\0").unwrap();
    drop(executable);

    assert!(!offered(&lucyd, "fixture__echo"));
}

#[test]
fn an_approval_inside_the_plugin_folder_counts_for_nothing() {
    let lucyd = Harness::start();
    let folder = lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    let approvals = lucyd.state_dir().join("approvals.toml");
    std::fs::rename(&approvals, folder.join("approvals.toml")).unwrap();

    assert!(!offered(&lucyd, "fixture__echo"));
}

#[test]
fn a_user_plugin_overrides_a_shipped_one_of_the_same_name() {
    let lucyd = Harness::start();
    lucyd.install_shipped_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    assert!(offered(&lucyd, "fixture__echo"));

    lucyd.install_plugin("fixture", &manifest("fixture", &["pid"], ""));
    // The user copy needs its own Approval; the shipped one no longer counts.
    assert!(!offered(&lucyd, "fixture__echo"));
    lucyd.approve("fixture");

    assert!(offered(&lucyd, "fixture__pid"));
    assert!(!offered(&lucyd, "fixture__echo"));
}

#[test]
fn a_manifest_with_more_than_three_examples_or_a_multiline_summary_is_rejected() {
    let lucyd = Harness::start();
    let examples = manifest("fixture", &["echo"], "").replace(
        "examples = [\"please echo\", \"echo it\"]",
        "examples = [\"a\", \"b\", \"c\", \"d\"]",
    );
    lucyd.install_plugin("fixture", &examples);
    lucyd.approve("fixture");
    assert!(!offered(&lucyd, "fixture__echo"));

    let summary = manifest("fixture", &["echo"], "").replace(
        "summary = \"A fixture Plugin\"",
        "summary = \"\"\"A fixture\nPlugin\"\"\"",
    );
    lucyd.install_plugin("fixture", &summary);
    lucyd.approve("fixture");
    assert!(!offered(&lucyd, "fixture__echo"));

    lucyd.install_plugin("fixture", &manifest("fixture", &["echo"], ""));
    lucyd.approve("fixture");
    assert!(offered(&lucyd, "fixture__echo"));
}

#[test]
fn a_plugin_whose_manifest_does_not_match_its_live_tools_is_left_out() {
    let lucyd = Harness::start();
    let mismatched = manifest("fixture", &["echo", "pid"], "").replace(
        "[\"./fixture-plugin\", \"echo\", \"pid\"]",
        "[\"./fixture-plugin\", \"echo\"]",
    );
    lucyd.install_plugin("fixture", &mismatched);
    lucyd.approve("fixture");

    assert!(!offered(&lucyd, "fixture__echo"));
    assert!(!offered(&lucyd, "fixture__pid"));
}
