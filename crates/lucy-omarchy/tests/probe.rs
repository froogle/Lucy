//! The Omarchy Plugin's `text_at` probe through MCP, with a fake `Screen`: it
//! reads the element under a point in the last look's image, in the
//! coordinates of the window drawn there.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use lucy_omarchy::{Element, InWindow};
use serde_json::json;
use support::{Monitor, Plugin};

const FIREFOX: &str = "0xf1";

const TERMINAL: &str = "0xa2";

fn desktop() -> Plugin {
    let omarchy = Plugin::start();
    omarchy.hyprland.monitors(&[Monitor {
        name: "DP-1",
        at: (0, 0),
        mode: (1280, 800),
        scale: 1.0,
        transform: 0,
        focused: true,
    }]);
    omarchy.hyprland.windows(
        &[
            (FIREFOX, "firefox", "Inbox - Mozilla Firefox", "1"),
            (TERMINAL, "ghostty", "~", "1"),
        ],
        Some(FIREFOX),
    );
    omarchy.hyprland.set(FIREFOX, "at", json!([100, 40]));
    omarchy.hyprland.set(FIREFOX, "size", json!([800, 600]));
    omarchy.hyprland.set(FIREFOX, "pid", json!(5150));
    omarchy.hyprland.set(TERMINAL, "at", json!([1000, 40]));
    omarchy.hyprland.set(TERMINAL, "size", json!([800, 600]));
    omarchy.call("screenshot_for_model", json!({}));
    omarchy
}

fn send_button() -> Element {
    Element {
        name: "Send".into(),
        text: "Send".into(),
        role: "push button".into(),
    }
}

#[test]
fn the_probe_reads_the_element_at_the_point_in_its_windows_own_coordinates() {
    let omarchy = desktop();
    omarchy.screen.show(send_button());

    let read = omarchy.call("text_at", json!({"x": 300.5, "y": 240}));

    assert_eq!(
        read["found"],
        json!({"name": "Send", "text": "Send", "role": "push button"})
    );
    assert_eq!(
        omarchy.screen.probed(),
        [InWindow {
            pid: 5150,
            title: "Inbox - Mozilla Firefox".into(),
            x: 200,
            y: 200,
        }]
    );
}

#[test]
fn nothing_readable_at_the_point_is_found_null() {
    let omarchy = desktop();

    let read = omarchy.call("text_at", json!({"x": 300, "y": 240}));

    assert_eq!(read["found"], json!(null));
}

#[test]
fn a_point_on_no_window_or_on_an_xwayland_one_is_found_null_unprobed() {
    let omarchy = desktop();
    omarchy.screen.show(send_button());
    omarchy.hyprland.set(TERMINAL, "xwayland", json!(true));

    for (x, y) in [(50, 20), (1200, 300)] {
        let read = omarchy.call("text_at", json!({"x": x, "y": y}));
        assert_eq!(read["found"], json!(null));
    }
    assert!(omarchy.screen.probed().is_empty());
}

#[test]
fn a_floating_window_over_a_tiled_one_is_the_one_probed() {
    let omarchy = desktop();
    omarchy.hyprland.set(TERMINAL, "at", json!([200, 100]));
    omarchy.hyprland.set(TERMINAL, "size", json!([300, 300]));
    omarchy.hyprland.set(TERMINAL, "floating", json!(true));
    omarchy.hyprland.set(TERMINAL, "pid", json!(77));

    omarchy.call("text_at", json!({"x": 300, "y": 240}));

    let [probed] = &omarchy.screen.probed()[..] else {
        panic!("{:?}", omarchy.screen.probed());
    };
    assert_eq!((probed.pid, probed.x, probed.y), (77, 100, 140));
}

#[test]
fn a_window_on_a_hidden_workspace_is_never_probed() {
    let omarchy = desktop();
    omarchy.hyprland.set(FIREFOX, "visible", json!(false));

    let read = omarchy.call("text_at", json!({"x": 300, "y": 240}));

    assert_eq!(read["found"], json!(null));
    assert!(omarchy.screen.probed().is_empty());
}

#[test]
fn the_probe_before_any_look_is_refused() {
    let omarchy = Plugin::start();

    let refused = omarchy.refused("text_at", json!({"x": 1, "y": 1}));

    assert!(refused.contains("look first"), "{refused}");
}
