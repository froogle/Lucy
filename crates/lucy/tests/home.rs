//! `lucy` with no arguments: the Provider status page and the Approval list.

mod support;

use support::{stderr, stdout, Home};

fn client(home: &Home) -> lucy_ipc::Client {
    let socket = home.runtime().join("lucy").join("lucyd.sock");
    lucy_ipc::Client::connect(&socket, "test", None).unwrap()
}

fn approve(home: &Home, plugin: &str) {
    let path = home.state().join("approvals.toml");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(&format!(
        "[plugins.{plugin}]\nhash = \"{plugin}-hash\"\nschemas = {{}}\n"
    ));
    std::fs::write(path, text).unwrap();
}

#[test]
fn it_shows_each_provider_with_its_health_and_runs_a_real_check_on_request() {
    let home = Home::with_lucyd();

    let output = home.lucy(&[], &["c1", "q"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let page = stdout(&output);
    for shown in [
        "claude-advanced",
        "opus",
        "claude-fast",
        "haiku",
        "fast",
        "advanced",
    ] {
        assert!(page.contains(shown), "{shown} missing:\n{page}");
    }
    // The inert lucyd has no Thinking Provider, for the status and the check.
    let problem = "This lucyd has no Thinking Provider.";
    assert!(page.matches(problem).count() >= 3, "{page}");
    assert!(page.contains("Real check of claude-advanced"), "{page}");
}

#[test]
fn it_lists_approvals_and_revokes_one_after_a_yes() {
    let home = Home::with_lucyd();
    approve(&home, "news");
    approve(&home, "notes");

    let output = home.lucy(&[], &["r2", "y", "r1", "n", "q"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let page = stdout(&output);
    assert!(page.contains("news") && page.contains("notes"), "{page}");
    let left: Vec<String> = client(&home)
        .approvals()
        .unwrap()
        .approvals
        .into_iter()
        .map(|a| a.plugin)
        .collect();
    assert_eq!(left, ["news"]);
}

#[test]
fn approvals_are_never_granted_here_only_by_voice() {
    let home = Home::with_lucyd();

    let output = home.lucy(&[], &["a1", "q"]);

    let page = stdout(&output).to_lowercase();
    assert!(page.contains("granted only by voice"), "{page}");
    assert!(!page.contains("approve"), "{page}");
}

#[test]
fn it_says_plainly_when_lucyd_is_not_running() {
    let home = Home::without_lucyd();

    let output = home.lucy(&[], &[]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("lucyd isn't running"),
        "{}",
        stderr(&output)
    );
}
