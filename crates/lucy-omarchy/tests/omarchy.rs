//! The Omarchy Plugin, driven through MCP against a fake Hyprland and Omarchy.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use serde_json::json;
use support::Plugin;

const FIREFOX: &str = "0xf1";
const GHOSTTY: &str = "0xa2";
const SLACK: &str = "0xb3";

fn desktop() -> Plugin {
    let omarchy = Plugin::start();
    omarchy.hyprland.windows(
        &[
            (FIREFOX, "firefox", "Lucy - Mozilla Firefox", "1"),
            (GHOSTTY, "com.mitchellh.ghostty", "Ghostty", "2"),
            (SLACK, "Slack", "general - Slack", "3"),
        ],
        Some(GHOSTTY),
    );
    omarchy
}

fn window(address: &str) -> String {
    format!("\"address:{address}\"")
}

#[test]
fn the_manifest_lists_exactly_the_tools_it_offers_and_only_kill_window_confirms() {
    let omarchy = Plugin::start();
    let manifest = manifest();
    let tools = manifest["tools"].as_array().unwrap();
    let mut declared: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let confirmed: Vec<&str> = tools
        .iter()
        .filter(|t| t.get("confirm").and_then(|c| c.as_bool()) == Some(true))
        .map(|t| t["name"].as_str().unwrap())
        .collect();

    let mut offered = omarchy.tool_names();

    declared.sort();
    offered.sort();
    assert_eq!(
        offered,
        [
            "bar",
            "click",
            "idle_lock",
            "key",
            "kill_window",
            "layout",
            "list_windows",
            "night_light",
            "screenshot",
            "screenshot_for_model",
            "scroll",
            "theme",
            "type",
            "window",
            "workspace"
        ]
    );
    assert_eq!(declared, offered);
    assert_eq!(confirmed, ["kill_window"]);
}

#[test]
fn the_manifest_declares_exactly_the_commands_its_tools_run() {
    let omarchy = Plugin::start();
    omarchy.hyprland.windows(
        &[(FIREFOX, "firefox", "Lucy - Mozilla Firefox", "1")],
        Some(FIREFOX),
    );
    omarchy.hyprland.monitors(&[support::Monitor {
        name: "DP-1",
        at: (0, 0),
        mode: (1920, 1080),
        scale: 1.0,
        transform: 0,
        focused: true,
    }]);
    let calls = [
        ("layout", json!({"mode": "scrolling"})),
        ("theme", json!({"action": "list"})),
        ("theme", json!({"action": "set", "name": "Rose Pine"})),
        ("bar", json!({"action": "toggle"})),
        ("night_light", json!({"action": "toggle"})),
        ("screenshot", json!({"action": "screen"})),
        ("idle_lock", json!({"action": "lock"})),
        ("idle_lock", json!({"action": "stay_awake"})),
        ("screenshot_for_model", json!({})),
    ];
    for (tool, arguments) in calls {
        let result = omarchy.call_result(tool, arguments);
        assert_eq!(result.is_error, Some(false), "{}", support::text(&result));
    }

    let execs = omarchy.hyprland.dispatched();
    let started = execs.iter().filter_map(|dispatch| {
        dispatch
            .strip_prefix("hl.dsp.exec_cmd(\"")?
            .strip_suffix("\")")
    });
    let mut ran: Vec<String> = omarchy
        .ran()
        .iter()
        .map(String::as_str)
        .chain(started)
        .map(|ran| ran.split(' ').next().unwrap().to_owned())
        .collect();
    ran.sort();
    ran.dedup();
    let manifest = manifest();
    let mut declared: Vec<&str> = manifest["access"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command.as_str().unwrap())
        .collect();
    declared.sort();
    assert_eq!(declared, ran);
}

fn manifest() -> toml::Table {
    toml::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.toml")).unwrap(),
    )
    .unwrap()
}

#[test]
fn list_windows_gives_each_windows_address_class_title_and_workspace() {
    let omarchy = desktop();

    let listed = omarchy.call("list_windows", json!({}));

    let windows = listed["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0]["address"], FIREFOX);
    assert_eq!(windows[0]["class"], "firefox");
    assert_eq!(windows[0]["title"], "Lucy - Mozilla Firefox");
    assert_eq!(windows[0]["workspace"], "1");
    assert_eq!(windows[0]["focused"], false);
    assert_eq!(windows[1]["focused"], true);
    assert!(listed["untrusted"].as_str().unwrap().contains("untrusted"));
    assert_eq!(omarchy.hyprland.dispatched(), Vec::<String>::new());
}

#[test]
fn a_window_named_by_its_app_moves_to_a_workspace_without_following_it() {
    let omarchy = desktop();

    omarchy.call(
        "window",
        json!({"action": "move_to_workspace", "target": "Firefox", "workspace": 3}),
    );

    assert_eq!(
        omarchy.hyprland.dispatched(),
        [format!(
            "hl.dsp.window.move({{ workspace = \"3\", follow = false, window = {} }})",
            window(FIREFOX)
        )]
    );
}

#[test]
fn close_with_no_target_politely_closes_the_focused_window() {
    let omarchy = desktop();

    let done = omarchy.call("window", json!({"action": "close"}));

    assert_eq!(
        omarchy.hyprland.dispatched(),
        [format!(
            "hl.dsp.window.close({{ window = {} }})",
            window(GHOSTTY)
        )]
    );
    assert!(done["done"].as_str().unwrap().contains("unsaved"), "{done}");
}

#[test]
fn focus_float_and_pin_dispatch_to_the_window_by_address() {
    let omarchy = desktop();

    omarchy.call("window", json!({"action": "focus", "target": SLACK}));
    let floated = omarchy.call("window", json!({"action": "float", "target": SLACK}));
    omarchy.hyprland.set(SLACK, "floating", json!(true));
    let pinned = omarchy.call("window", json!({"action": "pin", "target": SLACK}));

    let at = window(SLACK);
    assert_eq!(
        omarchy.hyprland.dispatched(),
        [
            format!("hl.dsp.focus({{ window = {at} }})"),
            format!("hl.dsp.window.float({{ action = \"toggle\", window = {at} }})"),
            format!("hl.dsp.window.pin({{ window = {at} }})"),
        ]
    );
    assert_eq!(floated["done"], "now floating");
    assert_eq!(pinned["done"], "now pinned");
}

#[test]
fn fullscreen_focuses_another_window_first_and_says_when_it_turns_off() {
    let omarchy = desktop();

    let on = omarchy.call("window", json!({"action": "fullscreen", "target": "slack"}));
    omarchy.hyprland.set(GHOSTTY, "fullscreen", json!(2));
    let off = omarchy.call("window", json!({"action": "fullscreen"}));

    let fullscreen = "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })";
    assert_eq!(
        omarchy.hyprland.dispatched(),
        [
            format!("hl.dsp.focus({{ window = {} }})", window(SLACK)),
            fullscreen.to_owned(),
            fullscreen.to_owned(),
        ]
    );
    assert_eq!(on["done"], "now full screen");
    assert_eq!(off["done"], "no longer full screen");
}

#[test]
fn a_name_matching_two_windows_is_refused_naming_both_and_nothing_is_dispatched() {
    let omarchy = desktop();
    omarchy
        .hyprland
        .set(FIREFOX, "title", json!("Slack pricing - Mozilla Firefox"));

    let refused = omarchy.refused("window", json!({"action": "close", "target": "slack"}));

    assert!(
        refused.contains(FIREFOX) && refused.contains(SLACK),
        "{refused}"
    );
    assert_eq!(omarchy.hyprland.dispatched(), Vec::<String>::new());
}

#[test]
fn a_name_matching_no_window_is_refused() {
    let omarchy = desktop();

    let refused = omarchy.refused("window", json!({"action": "focus", "target": "spotify"}));

    assert!(refused.contains("spotify"), "{refused}");
    assert_eq!(omarchy.hyprland.dispatched(), Vec::<String>::new());
}

#[test]
fn kill_window_force_quits_the_named_window() {
    let omarchy = desktop();

    omarchy.call("kill_window", json!({"target": "slack"}));

    assert_eq!(
        omarchy.hyprland.dispatched(),
        [format!(
            "hl.dsp.window.kill({{ window = {} }})",
            window(SLACK)
        )]
    );
}

#[test]
fn workspace_switches_and_renames_with_the_name_quoted_safely() {
    let omarchy = desktop();

    omarchy.call("workspace", json!({"action": "switch", "workspace": 2}));
    omarchy.call(
        "workspace",
        json!({"action": "rename", "workspace": 3, "name": "mail\" }) os.exit() --"}),
    );

    assert_eq!(
        omarchy.hyprland.dispatched(),
        [
            "hl.dsp.focus({ workspace = \"2\" })",
            "hl.dsp.workspace.rename({ workspace = \"3\", name = \"mail\\\" }) os.exit() --\" })",
        ]
    );
}

#[test]
fn a_workspace_name_with_a_newline_is_refused() {
    let omarchy = desktop();

    omarchy.refused(
        "workspace",
        json!({"action": "rename", "workspace": 3, "name": "mail\nos.exit()"}),
    );

    assert_eq!(omarchy.hyprland.dispatched(), Vec::<String>::new());
}

#[test]
fn hyprland_refusing_a_dispatch_fails_the_call_with_its_reason() {
    let omarchy = desktop();
    omarchy
        .hyprland
        .answer_dispatches_with("pin: window is not floating");

    let refused = omarchy.refused("window", json!({"action": "pin"}));

    assert!(refused.contains("window is not floating"), "{refused}");
}

#[test]
fn layout_runs_omarchys_toggle_the_short_way_round_to_the_mode_asked_for() {
    let omarchy = desktop();

    omarchy.call("layout", json!({"mode": "scrolling"}));
    assert_eq!(omarchy.layout(), "scrolling");
    omarchy.call("layout", json!({"mode": "tiling"}));
    assert_eq!(omarchy.layout(), "dwindle");

    assert_eq!(omarchy.toggles(), ["toggle ", "toggle back"]);
}

#[test]
fn layout_already_in_the_mode_asked_for_changes_nothing() {
    let omarchy = desktop();
    omarchy.set_layout("floating");

    let done = omarchy.call("layout", json!({"mode": "floating"}));

    assert_eq!(done["done"], "already so");
    assert_eq!(omarchy.toggles(), Vec::<String>::new());
}

#[test]
fn bar_shows_hides_or_toggles_it_through_omarchys_bar_toggle() {
    let omarchy = desktop();

    omarchy.call("bar", json!({"action": "hide"}));
    omarchy.call("bar", json!({"action": "show"}));
    omarchy.call("bar", json!({"action": "toggle"}));

    assert_eq!(
        omarchy.ran(),
        [
            "omarchy-toggle-bar on",
            "omarchy-toggle-bar off",
            "omarchy-toggle-bar toggle"
        ]
    );
}

#[test]
fn night_light_checks_its_status_in_the_sandbox_and_starts_the_toggle_through_hyprland_only_when_it_must(
) {
    let omarchy = desktop();

    let on = omarchy.call("night_light", json!({"action": "on"}));
    omarchy.set_night_light(true);
    let again = omarchy.call("night_light", json!({"action": "on"}));
    let off = omarchy.call("night_light", json!({"action": "off"}));

    assert!(on["done"].as_str().unwrap().contains("on"), "{on}");
    assert_eq!(again["done"], "already so");
    assert!(off["done"].as_str().unwrap().contains("off"), "{off}");
    assert_eq!(omarchy.ran(), ["omarchy-toggle-nightlight --status"; 3]);
    assert_eq!(
        omarchy.hyprland.dispatched(),
        ["hl.dsp.exec_cmd(\"omarchy-toggle-nightlight\")"; 2]
    );
}

#[test]
fn night_light_toggle_says_which_way_it_is_turning() {
    let omarchy = desktop();

    let first = omarchy.call("night_light", json!({"action": "toggle"}));
    omarchy.set_night_light(true);
    let second = omarchy.call("night_light", json!({"action": "toggle"}));

    assert_eq!(first["done"], "turning on");
    assert_eq!(second["done"], "turning off");
}

#[test]
fn screenshot_starts_omarchys_capture_through_hyprland_for_the_screen_a_region_or_a_window() {
    let omarchy = desktop();

    let started = omarchy.call("screenshot", json!({"action": "screen"}));
    omarchy.call("screenshot", json!({"action": "region"}));
    omarchy.call("screenshot", json!({"action": "window"}));

    assert_eq!(
        omarchy.hyprland.dispatched(),
        [
            "hl.dsp.exec_cmd(\"omarchy-capture-screenshot fullscreen save\")",
            "hl.dsp.exec_cmd(\"omarchy-capture-screenshot region save\")",
            "hl.dsp.exec_cmd(\"omarchy-capture-screenshot windows save\")",
        ]
    );
    assert_eq!(omarchy.ran(), Vec::<String>::new());
    assert!(
        started["done"].as_str().unwrap().starts_with("started"),
        "{started}"
    );
}

#[test]
fn theme_list_gives_the_installed_themes_and_the_current_one() {
    let omarchy = desktop();

    let listed = omarchy.call("theme", json!({"action": "list"}));

    assert_eq!(
        listed,
        json!({"themes": ["Catppuccin", "Rose Pine", "Tokyo Night"], "current": "Catppuccin"})
    );
}

#[test]
fn theme_set_switches_to_the_installed_theme_named_in_any_case() {
    let omarchy = desktop();

    let done = omarchy.call("theme", json!({"action": "set", "name": "tokyo night"}));

    assert_eq!(omarchy.theme(), "Tokyo Night");
    assert_eq!(done["theme"], "Tokyo Night");
}

#[test]
fn theme_set_to_a_theme_not_installed_is_refused_naming_the_installed_ones() {
    let omarchy = desktop();

    let refused = omarchy.refused("theme", json!({"action": "set", "name": "Dracula"}));

    assert!(
        refused.contains("Dracula") && refused.contains("Rose Pine"),
        "{refused}"
    );
    assert_eq!(omarchy.theme(), "Catppuccin");
}

#[test]
fn idle_lock_locks_the_screen_or_keeps_it_awake() {
    let omarchy = desktop();

    omarchy.call("idle_lock", json!({"action": "lock"}));
    omarchy.call("idle_lock", json!({"action": "stay_awake"}));
    omarchy.call("idle_lock", json!({"action": "allow_idle"}));

    assert_eq!(
        omarchy.ran(),
        [
            "omarchy-system-lock ",
            "omarchy-toggle-idle stay-awake",
            "omarchy-toggle-idle allow-idle",
        ]
    );
}
