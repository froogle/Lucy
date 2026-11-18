//! The Omarchy Plugin's password check on `type`, through MCP with a fake
//! `Screen`: it refuses to type into a password field, and types when it
//! can't tell what has the focus, logging that the check couldn't run.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use lucy_omarchy::{FocusedWindow, Pid, Role};
use serde_json::json;
use support::{Act, Plugin};

const FIREFOX: &str = "0xf1";

const TYPED: &str = "hunter2-typed-text";

const UNKNOWN_FOCUS_TEST: &str = "unknown_focus_types_anyway";

fn firefox_focused() -> Plugin {
    let omarchy = Plugin::start();
    omarchy.hyprland.windows(
        &[(FIREFOX, "firefox", "Sign in - Mozilla Firefox", "1")],
        Some(FIREFOX),
    );
    omarchy.hyprland.set(FIREFOX, "pid", json!(5150));
    omarchy
}

#[test]
fn typing_into_a_password_field_is_refused_with_the_way_to_ask_for_it() {
    let omarchy = firefox_focused();
    omarchy.screen.focus_on(Role::PasswordText);

    let refused = omarchy.refused("type", json!({"text": TYPED}));

    assert!(refused.contains("password field"), "{refused}");
    assert!(refused.contains("ask_typed"), "{refused}");
    assert!(refused.contains("credential"), "{refused}");
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn the_focus_is_looked_for_in_the_focused_window() {
    let omarchy = firefox_focused();
    omarchy.screen.focus_on(Role::Entry);

    omarchy.call("type", json!({"text": TYPED}));

    assert_eq!(
        omarchy.screen.focus_asked(),
        [FocusedWindow {
            pid: Pid(5150),
            title: "Sign in - Mozilla Firefox".into(),
        }]
    );
}

#[test]
fn a_plain_field_is_typed_into() {
    let omarchy = firefox_focused();
    omarchy.screen.focus_on(Role::Entry);

    omarchy.call("type", json!({"text": TYPED}));

    assert_eq!(omarchy.screen.acts(), [Act::Type(TYPED.into())]);
}

#[test]
fn unknown_focus_types_anyway() {
    let omarchy = firefox_focused();

    omarchy.call("type", json!({"text": TYPED}));

    assert_eq!(omarchy.screen.acts(), [Act::Type(TYPED.into())]);
}

#[test]
fn no_focused_window_or_an_xwayland_one_types_unasked() {
    let omarchy = firefox_focused();
    omarchy.hyprland.windows(&[], None);
    omarchy.call("type", json!({"text": TYPED}));

    let xwayland = firefox_focused();
    xwayland.hyprland.set(FIREFOX, "xwayland", json!(true));
    xwayland.call("type", json!({"text": TYPED}));

    for omarchy in [omarchy, xwayland] {
        assert_eq!(omarchy.screen.acts(), [Act::Type(TYPED.into())]);
        assert!(omarchy.screen.focus_asked().is_empty());
    }
}

#[test]
fn unknown_focus_is_logged_without_the_text() {
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([UNKNOWN_FOCUS_TEST, "--exact", "--nocapture"])
        .output()
        .unwrap();

    let log =
        String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
    assert!(log.contains("password check couldn't run"), "{log}");
    assert!(!log.contains(TYPED), "{log}");
}
