//! The systemd user units Lucy ships in `dist/systemd`.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

use std::path::Path;

/// The unit's `key=value` lines, in order, as (section, key, value).
fn unit(name: &str) -> Vec<(String, String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../dist/systemd")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut section = String::new();
    let mut entries = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_owned();
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        entries.push((section.clone(), key.to_owned(), value.to_owned()));
    }
    entries
}

fn values(unit: &[(String, String, String)], section: &str, key: &str) -> Vec<String> {
    unit.iter()
        .filter(|(s, k, _)| s == section && k == key)
        .flat_map(|(_, _, v)| v.split_whitespace().map(str::to_owned))
        .collect()
}

#[test]
fn both_units_are_part_of_and_wanted_by_the_graphical_session() {
    for name in ["lucyd.service", "lucy-indicator.service"] {
        let unit = unit(name);
        assert_eq!(
            values(&unit, "Unit", "PartOf"),
            ["graphical-session.target"],
            "{name}"
        );
        assert!(
            values(&unit, "Unit", "After").contains(&"graphical-session.target".to_owned()),
            "{name}"
        );
        assert_eq!(
            values(&unit, "Install", "WantedBy"),
            ["graphical-session.target"],
            "{name}"
        );
        assert_eq!(
            values(&unit, "Service", "Restart"),
            ["on-failure"],
            "{name}"
        );
    }
}

#[test]
fn lucyd_starts_after_pipewire_and_its_children_die_with_it() {
    let unit = unit("lucyd.service");

    let after = values(&unit, "Unit", "After");
    assert!(after.contains(&"pipewire.service".to_owned()), "{after:?}");
    // Everything in lucyd's control group goes when it stops or dies.
    assert_eq!(values(&unit, "Service", "KillMode"), ["control-group"]);
    assert_eq!(values(&unit, "Service", "Type"), ["exec"]);
    assert!(values(&unit, "Service", "ExecStart")[0].ends_with("/lucyd"));
    // No socket activation: lucyd owns its socket.
    assert!(values(&unit, "Unit", "Requires").is_empty());
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../dist/systemd/lucyd.socket")
        .exists());
}

#[test]
fn the_indicator_follows_lucyd() {
    let unit = unit("lucy-indicator.service");

    assert!(values(&unit, "Unit", "After").contains(&"lucyd.service".to_owned()));
    assert!(values(&unit, "Service", "ExecStart")[0].ends_with("/lucy-indicator"));
}
