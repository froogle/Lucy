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

#[test]
fn lucy_is_told_at_session_start_which_plugin_failed_its_scan_and_why() {
    let lucyd = Harness::start();
    let mismatched = manifest("fixture", &["echo", "pid"], "").replace(
        "[\"./fixture-plugin\", \"echo\", \"pid\"]",
        "[\"./fixture-plugin\", \"echo\"]",
    );
    lucyd.install_plugin("fixture", &mismatched);
    lucyd.approve("fixture");

    lucyd.client().start_session().unwrap();

    let instructions = lucyd.thinking.instructions();
    assert!(
        instructions.contains(
            "The fixture Plugin is broken: its Manifest lists the tools echo, pid but it \
             offers echo."
        ),
        "{instructions}"
    );
}

#[test]
fn a_plugin_that_fails_a_call_is_broken_says_so_and_is_left_out() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["echo", "crash"], ""));
    lucyd.approve("fixture");
    let mut client = lucyd.client();
    client.start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("fixture__crash", json!({}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    let said = text(&result);
    assert!(
        said.contains("fixture") && said.contains("broken"),
        "{said}"
    );
    assert!(!lucyd
        .thinking
        .tool_names()
        .contains(&"fixture__echo".to_owned()));
    client.end_session().unwrap();
    assert!(!offered(&lucyd, "fixture__echo"));
}

#[test]
fn a_plugin_that_rejects_a_call_is_not_broken() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["echo", "reject"], ""));
    lucyd.approve("fixture");
    lucyd.client().start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("fixture__reject", json!({}))
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("rejected"), "{}", text(&result));
    assert!(lucyd
        .thinking
        .tool_names()
        .contains(&"fixture__echo".to_owned()));
}

#[test]
fn landlock_confines_a_plugin_to_its_declared_paths() {
    let lucyd = Harness::start();
    let scratch = lucyd.scratch();
    let (readable, writable, private) = (
        scratch.join("readable"),
        scratch.join("writable"),
        scratch.join("private"),
    );
    for dir in [&readable, &writable, &private] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("note"), "contents").unwrap();
    }
    let access = format!(
        "read = [{:?}]\nwrite = [{:?}]",
        readable.display().to_string(),
        writable.display().to_string()
    );
    lucyd.install_plugin(
        "fixture",
        &manifest("fixture", &["read_file", "write_file"], &access),
    );
    lucyd.approve("fixture");
    lucyd.client().start_session().unwrap();
    let read = |path: &std::path::Path| {
        lucyd
            .thinking
            .call_tool_with("fixture__read_file", json!({"path": path}))
            .unwrap()
    };
    let write = |path: &std::path::Path| {
        lucyd
            .thinking
            .call_tool_with("fixture__write_file", json!({"path": path, "text": "x"}))
            .unwrap()
    };

    assert_eq!(text(&read(&readable.join("note"))), "contents");
    assert_eq!(read(&private.join("note")).is_error, Some(true));
    assert_eq!(
        read(&lucyd.state_dir().join("approvals.toml")).is_error,
        Some(true)
    );

    assert_eq!(write(&writable.join("new")).is_error, Some(false));
    assert_eq!(write(&readable.join("new")).is_error, Some(true));
    assert_eq!(write(&private.join("new")).is_error, Some(true));
    assert_eq!(
        write(&lucyd.state_dir().join("approvals.toml")).is_error,
        Some(true)
    );
    assert!(!readable.join("new").exists() && !private.join("new").exists());
}

#[test]
fn each_plugin_gets_a_private_writable_state_dir() {
    use std::os::unix::fs::PermissionsExt;
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["save_state"], ""));
    lucyd.approve("fixture");
    lucyd.client().start_session().unwrap();

    let result = lucyd
        .thinking
        .call_tool_with("fixture__save_state", json!({"text": "my preference"}))
        .unwrap();

    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let dir = lucyd.state_dir().join("plugins/fixture");
    assert_eq!(
        std::fs::read_to_string(dir.join("saved")).unwrap(),
        "my preference"
    );
    let mode = std::fs::metadata(dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn plugins_are_stopped_when_the_session_ends() {
    let lucyd = Harness::start();
    lucyd.install_plugin("fixture", &manifest("fixture", &["pid"], ""));
    lucyd.approve("fixture");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    let pid = text(
        &lucyd
            .thinking
            .call_tool_with("fixture__pid", json!({}))
            .unwrap(),
    );
    assert!(running(&pid), "the Plugin runs during the Session");

    client.end_session().unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while running(&pid) {
        assert!(
            std::time::Instant::now() < deadline,
            "the Plugin still runs"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Whether process `pid` exists and isn't a zombie.
fn running(pid: &str) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // The state follows the parenthesised command name.
        Ok(stat) => !stat
            .rsplit(')')
            .next()
            .unwrap()
            .trim_start()
            .starts_with('Z'),
        Err(_) => false,
    }
}
