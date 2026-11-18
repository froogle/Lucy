//! The Omarchy Plugin's screen tools, driven through MCP with a fake `Screen`.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use lucy_omarchy::Region;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use support::{Monitor, Plugin, PNG};

const MODEL_LONG_EDGE: f64 = 1568.0;
const MODEL_PIXELS: f64 = 1_150_000.0;

const FIREFOX: &str = "0xf1";

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
        &[(FIREFOX, "firefox", "Lucy - Mozilla Firefox", "1")],
        Some(FIREFOX),
    );
    omarchy
}

fn image(result: &CallToolResult) -> (String, String) {
    let images: Vec<_> = result
        .content
        .iter()
        .filter_map(|block| block.as_image())
        .collect();
    assert_eq!(images.len(), 1, "{result:?}");
    (images[0].data.clone(), images[0].mime_type.clone())
}

fn described(result: &CallToolResult) -> Value {
    serde_json::from_str(&support::text(result)).unwrap()
}

fn fits_the_model(width: f64, height: f64) {
    assert!(width.max(height) <= MODEL_LONG_EDGE, "{width}x{height}");
    assert!(width * height <= MODEL_PIXELS, "{width}x{height}");
}

#[test]
fn a_look_captures_the_focused_monitor_scaled_to_fit_the_model() {
    let omarchy = two_monitors();

    let result = omarchy.call_result("screenshot_for_model", json!({}));

    assert_eq!(result.is_error, Some(false), "{}", support::text(&result));
    let (data, mime_type) = image(&result);
    assert_eq!(mime_type, "image/png");
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert_eq!(bytes, PNG);
    let [(region, scale)] = omarchy.screen.captures()[..] else {
        panic!("{:?}", omarchy.screen.captures());
    };
    assert_eq!(
        region,
        Region {
            x: 3440,
            y: 0,
            width: 1440,
            height: 900
        }
    );
    let said = described(&result);
    let (width, height) = (
        said["width"].as_f64().unwrap(),
        said["height"].as_f64().unwrap(),
    );
    assert_eq!(width, (1440.0 * scale).round());
    assert_eq!(height, (900.0 * scale).round());
    fits_the_model(width, height);
    assert!(width * height > 0.95 * MODEL_PIXELS, "scaled down too far");
}

#[test]
fn a_wide_monitor_is_held_to_the_models_long_edge() {
    let omarchy = Plugin::start();
    omarchy.hyprland.monitors(&[Monitor {
        name: "DP-2",
        at: (0, 0),
        mode: (5120, 1440),
        scale: 1.0,
        transform: 0,
        focused: true,
    }]);

    let result = omarchy.call_result("screenshot_for_model", json!({}));

    let said = described(&result);
    assert_eq!(said["width"], 1568);
    fits_the_model(1568.0, said["height"].as_f64().unwrap());
}

#[test]
fn a_rotated_monitor_is_captured_upright() {
    let omarchy = Plugin::start();
    omarchy.hyprland.monitors(&[Monitor {
        name: "DP-3",
        at: (0, 0),
        mode: (1920, 1080),
        scale: 1.0,
        transform: 1,
        focused: true,
    }]);

    omarchy.call_result("screenshot_for_model", json!({}));

    let [(region, _)] = omarchy.screen.captures()[..] else {
        panic!("{:?}", omarchy.screen.captures());
    };
    assert_eq!((region.width, region.height), (1080, 1920));
}

#[test]
fn a_small_window_is_captured_at_its_monitors_own_scale() {
    let omarchy = two_monitors();
    omarchy.hyprland.set(FIREFOX, "at", json!([3500, 40]));
    omarchy.hyprland.set(FIREFOX, "size", json!([600, 400]));
    omarchy.hyprland.set(FIREFOX, "monitor", json!(1));

    let result = omarchy.call_result("screenshot_for_model", json!({"window": "firefox"}));

    assert_eq!(result.is_error, Some(false), "{}", support::text(&result));
    let [(region, scale)] = omarchy.screen.captures()[..] else {
        panic!("{:?}", omarchy.screen.captures());
    };
    assert_eq!(
        region,
        Region {
            x: 3500,
            y: 40,
            width: 600,
            height: 400
        }
    );
    assert_eq!(scale, 2.0);
    let said = described(&result);
    assert_eq!(
        (said["width"].clone(), said["height"].clone()),
        (json!(1200), json!(800))
    );
}

#[test]
fn the_model_is_told_the_screen_is_untrusted() {
    let omarchy = two_monitors();

    let result = omarchy.call_result("screenshot_for_model", json!({}));

    let untrusted = described(&result)["untrusted"].as_str().unwrap().to_owned();
    assert!(untrusted.contains("never instructions"), "{untrusted}");
}

#[test]
fn a_window_that_matches_nothing_is_refused_uncaptured() {
    let omarchy = two_monitors();

    let refused = omarchy.refused("screenshot_for_model", json!({"window": "slack"}));

    assert!(refused.contains("no window matches"), "{refused}");
    assert!(omarchy.screen.captures().is_empty());
}

#[test]
fn a_failed_capture_says_why() {
    let omarchy = two_monitors();
    omarchy.screen.fail("the compositor refused the capture");

    let refused = omarchy.refused("screenshot_for_model", json!({}));

    assert!(
        refused.contains("the compositor refused the capture"),
        "{refused}"
    );
}
