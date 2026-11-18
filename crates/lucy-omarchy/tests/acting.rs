//! The Omarchy Plugin acting on the screen through MCP, with a fake `Screen`:
//! clicks, scrolls, keys and typing, at points in the last look's image.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use lucy_omarchy::{Button, Combo, Direction, Modifier, Point, Region};
use serde_json::json;
use support::{Act, Monitor, Plugin};

const FIREFOX: &str = "0xf1";

const DESKTOP: Region = Region {
    x: 0,
    y: 0,
    width: 4880,
    height: 1440,
};

fn two_monitors() -> Plugin {
    let omarchy = Plugin::start();
    omarchy.hyprland.monitors(&[
        Monitor {
            name: "DP-1",
            at: (0, 0),
            mode: (3440, 1440),
            scale: 1.0,
            transform: 0,
            focused: false,
        },
        Monitor {
            name: "eDP-1",
            at: (3440, 0),
            mode: (2880, 1800),
            scale: 2.0,
            transform: 0,
            focused: true,
        },
    ]);
    omarchy.hyprland.windows(
        &[(FIREFOX, "firefox", "YouTube - Mozilla Firefox", "1")],
        Some(FIREFOX),
    );
    omarchy.hyprland.set(FIREFOX, "at", json!([3500, 40]));
    omarchy.hyprland.set(FIREFOX, "size", json!([600, 400]));
    omarchy.hyprland.set(FIREFOX, "monitor", json!(1));
    omarchy
}

fn looked_at_firefox() -> Plugin {
    let omarchy = two_monitors();
    omarchy.call("screenshot_for_model", json!({"window": "firefox"}));
    omarchy
}

fn at(x: f64, y: f64) -> Point {
    Point {
        x,
        y,
        desktop: DESKTOP,
    }
}

#[test]
fn a_click_lands_where_the_image_shows_it_on_the_real_screen() {
    let omarchy = looked_at_firefox();

    omarchy.call("click", json!({"x": 200, "y": 100}));

    assert_eq!(
        omarchy.screen.acts(),
        [Act::Click(at(3600.0, 90.0), Button::Left)]
    );
}

#[test]
fn a_click_on_a_scaled_down_monitor_look_maps_back_to_logical_pixels() {
    let omarchy = Plugin::start();
    omarchy.hyprland.monitors(&[Monitor {
        name: "DP-1",
        at: (0, 0),
        mode: (1920, 1080),
        scale: 1.0,
        transform: 0,
        focused: true,
    }]);
    let look = omarchy.call("screenshot_for_model", json!({}));
    let (width, height) = (
        look["width"].as_f64().unwrap(),
        look["height"].as_f64().unwrap(),
    );

    omarchy.call("click", json!({"x": width / 2.0, "y": height / 2.0}));

    let [Act::Click(point, Button::Left)] = omarchy.screen.acts()[..] else {
        panic!("{:?}", omarchy.screen.acts());
    };
    assert!((point.x - 960.0).abs() < 1.0, "{point:?}");
    assert!((point.y - 540.0).abs() < 1.0, "{point:?}");
}

#[test]
fn a_right_click_uses_the_right_button() {
    let omarchy = looked_at_firefox();

    omarchy.call("click", json!({"x": 0, "y": 0, "button": "right"}));

    assert_eq!(
        omarchy.screen.acts(),
        [Act::Click(at(3500.0, 40.0), Button::Right)]
    );
}

#[test]
fn a_click_before_any_look_is_refused() {
    let omarchy = two_monitors();

    let refused = omarchy.refused("click", json!({"x": 1, "y": 1}));

    assert!(refused.contains("look first"), "{refused}");
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn a_point_outside_the_last_image_is_refused() {
    let omarchy = looked_at_firefox();

    let refused = omarchy.refused("click", json!({"x": 1201, "y": 1}));

    assert!(refused.contains("1200x800"), "{refused}");
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn scroll_turns_the_wheel_at_the_point() {
    let omarchy = looked_at_firefox();

    omarchy.call(
        "scroll",
        json!({"x": 600, "y": 400, "direction": "down", "amount": 3}),
    );

    assert_eq!(
        omarchy.screen.acts(),
        [Act::Scroll(at(3800.0, 240.0), Direction::Down, 3)]
    );
}

#[test]
fn a_scroll_of_no_notches_or_too_many_is_refused() {
    let omarchy = looked_at_firefox();

    for amount in [0, 26] {
        let refused = omarchy.refused(
            "scroll",
            json!({"x": 1, "y": 1, "direction": "up", "amount": amount}),
        );
        assert!(refused.contains("amount"), "{refused}");
    }
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn key_presses_a_combination_named_the_usual_ways() {
    let omarchy = two_monitors();

    for combo in ["Return", "ctrl+l", "Ctrl+Shift+T", "alt+Left", "ctrl++"] {
        omarchy.call("key", json!({"combo": combo}));
    }

    let combo = |modifiers: &[Modifier], keysym: &str| {
        Act::Key(Combo {
            modifiers: modifiers.to_vec(),
            keysym: keysym.into(),
        })
    };
    assert_eq!(
        omarchy.screen.acts(),
        [
            combo(&[], "Return"),
            combo(&[Modifier::Ctrl], "U006C"),
            combo(&[Modifier::Ctrl, Modifier::Shift], "U0074"),
            combo(&[Modifier::Alt], "Left"),
            combo(&[Modifier::Ctrl], "U002B"),
        ]
    );
}

#[test]
fn an_unknown_key_is_refused_saying_which_keys_there_are() {
    let omarchy = two_monitors();

    for combo in ["hyper+x", "Launch5", ""] {
        let refused = omarchy.refused("key", json!({"combo": combo}));
        assert!(refused.contains("Page_Down"), "{refused}");
    }
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn type_types_the_text_with_its_newline() {
    let omarchy = two_monitors();

    omarchy.call("type", json!({"text": "Ünïcode café\n"}));

    assert_eq!(omarchy.screen.acts(), [Act::Type("Ünïcode café\n".into())]);
}

#[test]
fn text_holding_a_control_character_is_refused_untyped() {
    let omarchy = two_monitors();

    let refused = omarchy.refused("type", json!({"text": "a\u{1b}b"}));

    assert!(refused.contains("key tool"), "{refused}");
    assert!(omarchy.screen.acts().is_empty());
}

#[test]
fn a_failed_action_says_why() {
    let omarchy = looked_at_firefox();
    omarchy
        .screen
        .fail("the compositor offers no virtual pointer");

    let refused = omarchy.refused("click", json!({"x": 1, "y": 1}));

    assert!(refused.contains("no virtual pointer"), "{refused}");
}

#[test]
fn the_manifest_marks_each_acting_tool_and_none_asks_for_a_describe() {
    let manifest: toml::Table = toml::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/manifest.toml")).unwrap(),
    )
    .unwrap();
    let marked: Vec<(&str, &str)> = manifest["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| Some((tool["name"].as_str()?, tool.get("screen")?.as_str()?)))
        .collect();
    assert_eq!(
        marked,
        [
            ("screenshot_for_model", "look"),
            ("click", "click"),
            ("type", "type"),
            ("key", "key"),
            ("scroll", "scroll"),
        ]
    );
    let omarchy = Plugin::start();
    assert_eq!(omarchy.required("click"), ["x", "y"]);
    assert_eq!(omarchy.required("key"), ["combo"]);
    assert_eq!(omarchy.required("type"), ["text"]);
}
