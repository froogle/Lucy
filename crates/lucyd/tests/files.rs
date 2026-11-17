//! The file Built-in tools: `find_files`, `show_files` and `open_file`.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod harness;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use harness::Harness;
use serde_json::{json, Value};

fn find(lucyd: &Harness, arguments: Value) -> Value {
    let result = lucyd
        .thinking
        .call_tool_with("find_files", arguments)
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    result.structured_content.expect("a structured result")
}

fn found(lucyd: &Harness, arguments: Value) -> Vec<String> {
    let result = find(lucyd, arguments);
    assert_eq!(result["result"], "found", "{result}");
    result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap().to_owned())
        .collect()
}

fn paths(paths: &[&Path]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn days_old(path: &Path, days: u64) {
    let when = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    std::fs::File::open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn call(lucyd: &Harness, name: &str, arguments: Value) -> Result<String, String> {
    let result = lucyd.thinking.call_tool_with(name, arguments).unwrap();
    let text = rmcp_text(&result);
    match result.is_error {
        Some(true) => Err(text),
        _ => Ok(text),
    }
}

fn rmcp_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect()
}

#[test]
fn words_match_names_in_any_order_and_case_and_a_matching_folder_hides_its_children() {
    let lucyd = Harness::start();
    let folder = lucyd.home_folder("Documents/1921 Naval Conference");
    lucyd.home_file("Documents/1921 Naval Conference/naval conference agenda.pdf");
    let file = lucyd.home_file("Work/CONFERENCE-naval.txt");
    lucyd.home_file("Work/naval.txt");
    lucyd.client().start_session().unwrap();

    let result = find(&lucyd, json!({ "words": ["naval", "Conference"] }));

    assert_eq!(
        result,
        json!({
            "result": "found",
            "total": 2,
            "items": [
                { "path": folder.display().to_string(), "kind": "folder" },
                { "path": file.display().to_string(), "kind": "file" },
            ],
        })
    );
}

#[test]
fn folders_rank_before_files_then_newest_first() {
    let lucyd = Harness::start();
    let old_file = lucyd.home_file("a/naval old.txt");
    let new_file = lucyd.home_file("b/naval new.txt");
    let old_folder = lucyd.home_folder("c/naval old");
    let new_folder = lucyd.home_folder("d/naval new");
    days_old(&old_file, 30);
    days_old(&new_file, 1);
    days_old(&old_folder, 40);
    days_old(&new_folder, 20);
    lucyd.client().start_session().unwrap();

    assert_eq!(
        found(&lucyd, json!({ "words": ["naval"] })),
        paths(&[&new_folder, &old_folder, &new_file, &old_file])
    );
}

#[test]
fn near_ranks_matches_inside_a_matching_folder_first() {
    let lucyd = Harness::start();
    let downloaded = lucyd.home_folder("Downloads/naval conference");
    let documented = lucyd.home_file("Documents/old/naval conference.pdf");
    days_old(&documented, 300);
    lucyd.client().start_session().unwrap();

    assert_eq!(
        found(
            &lucyd,
            json!({ "words": ["naval", "conference"], "near": "documents" })
        ),
        paths(&[&documented, &downloaded])
    );
    assert_eq!(
        found(&lucyd, json!({ "words": ["naval", "conference"] })),
        paths(&[&downloaded, &documented])
    );
}

#[test]
fn hidden_and_skipped_folders_are_left_out() {
    let lucyd = Harness::start();
    for left_out in [
        ".secret/naval.txt",
        ".naval.txt",
        "code/node_modules/naval.js",
        "code/target/naval",
        ".cache/naval",
        "code/venv/naval.py",
        "code/__pycache__/naval.pyc",
    ] {
        lucyd.home_file(left_out);
    }
    lucyd.client().start_session().unwrap();

    assert_eq!(
        find(&lucyd, json!({ "words": ["naval"] })),
        json!({ "result": "not_found" })
    );
}

#[test]
fn at_most_20_results_are_returned_with_the_total() {
    let lucyd = Harness::start();
    for n in 0..25 {
        lucyd.home_file(&format!("reports/naval {n}.txt"));
    }
    lucyd.client().start_session().unwrap();

    let result = find(&lucyd, json!({ "words": ["naval"] }));

    assert_eq!(result["total"], 25);
    assert_eq!(result["items"].as_array().unwrap().len(), 20);
}

#[test]
fn custom_roots_and_skip_replace_the_defaults() {
    let lucyd = Harness::with_config("[files]\nroots = [\"~/Work\"]\nskip = [\"archive\"]\n");
    let kept = lucyd.home_file("Work/naval plan.txt");
    let unskipped = lucyd.home_file("Work/node_modules/naval.js");
    lucyd.home_file("Work/archive/naval old.txt");
    lucyd.home_file("Elsewhere/naval.txt");
    days_old(&unskipped, 2);
    lucyd.client().start_session().unwrap();

    assert_eq!(
        found(&lucyd, json!({ "words": ["naval"] })),
        paths(&[&kept, &unskipped])
    );
}

#[test]
fn an_absolute_root_is_searched() {
    let lucyd = Harness::start();
    let elsewhere = lucyd.scratch().join("naval.txt");
    std::fs::write(&elsewhere, "").unwrap();
    std::fs::write(
        lucyd.config_path(),
        format!("[files]\nroots = [{:?}]\n", lucyd.scratch()),
    )
    .unwrap();
    lucyd.client().start_session().unwrap();

    assert_eq!(
        found(&lucyd, json!({ "words": ["naval"] })),
        paths(&[&elsewhere])
    );
}

#[test]
fn show_files_shows_the_items_in_the_file_manager() {
    let lucyd = Harness::start();
    let folder = lucyd.home_folder("Documents/1921 Naval Conference");
    let file = lucyd.home_file("Documents/naval.txt");
    lucyd.client().start_session().unwrap();

    call(
        &lucyd,
        "show_files",
        json!({ "paths": paths(&[&folder, &file]) }),
    )
    .unwrap();

    assert_eq!(lucyd.launcher.shown(), [vec![folder, file]]);
}

#[test]
fn open_file_opens_a_file_under_the_roots() {
    let lucyd = Harness::start();
    let file = lucyd.home_file("Documents/naval.pdf");
    lucyd.client().start_session().unwrap();

    call(&lucyd, "open_file", json!({ "path": file })).unwrap();

    assert_eq!(lucyd.launcher.opened_files(), [file]);
}

#[test]
fn open_file_refuses_executables_desktop_entries_and_paths_outside_the_roots() {
    let lucyd = Harness::start();
    let program = lucyd.home_file("bin/naval");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let entry = lucyd.home_file("naval.desktop");
    let outside = lucyd.scratch().join("naval.pdf");
    std::fs::write(&outside, "").unwrap();
    let climbing = lucyd.home().join("..").join("scratch").join("naval.pdf");
    let link = lucyd.home().join("link.pdf");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    lucyd.client().start_session().unwrap();

    for refused in [
        program,
        entry,
        outside,
        climbing,
        link,
        PathBuf::from("naval.pdf"),
    ] {
        let result = call(&lucyd, "open_file", json!({ "path": refused }));
        assert!(result.is_err(), "{refused:?}: {result:?}");
    }

    assert!(lucyd.launcher.opened_files().is_empty());
}

#[test]
fn the_file_tools_are_offered_and_run_without_confirmation() {
    let lucyd = Harness::start();
    let file = lucyd.home_file("Documents/1921 Naval Conference/minutes.pdf");
    lucyd.client().start_session().unwrap();
    lucyd.say("show me my naval conference files in Documents");
    lucyd.thinking.next_turn();

    let offered = lucyd.thinking.tool_names();
    for name in ["find_files", "show_files", "open_file"] {
        assert!(offered.contains(&name.to_owned()), "{name} not offered");
    }
    let shown = found(
        &lucyd,
        json!({ "words": ["naval", "conference"], "near": "documents" }),
    );
    call(&lucyd, "show_files", json!({ "paths": shown })).unwrap();
    call(&lucyd, "open_file", json!({ "path": file })).unwrap();

    assert_eq!(lucyd.launcher.shown().len(), 1);
    assert_eq!(lucyd.launcher.opened_files().len(), 1);
    assert!(lucyd.tts.sentences().is_empty(), "nothing was confirmed");
}

#[test]
fn no_path_reaches_the_bus() {
    let lucyd = Harness::start();
    lucyd.home_folder("Documents/1921 Naval Conference");
    let file = lucyd.home_file("Documents/naval minutes.pdf");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.say("show me my naval files");
    lucyd.thinking.next_turn();

    let shown = found(&lucyd, json!({ "words": ["naval"] }));
    call(&lucyd, "show_files", json!({ "paths": shown })).unwrap();
    call(&lucyd, "open_file", json!({ "path": file })).unwrap();
    client.end_session().unwrap();

    lucyd.assert_nothing_private_on_bus();
}
