//! The microphones and speakers PipeWire has, by node name, as `pactl`
//! lists them, each labelled with its description.

use std::collections::HashMap;
use std::process::Command;

use serde::Deserialize;

use crate::Env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Mic,
    Speaker,
}

#[derive(Debug, Clone)]
pub(crate) struct Device {
    pub(crate) name: String,
    pub(crate) label: String,
}

#[derive(Debug)]
pub(crate) struct Listed {
    devices: Vec<Device>,
    default: Option<String>,
}

#[derive(Deserialize)]
struct Node {
    name: String,
    #[serde(default)]
    description: String,
}

const DEFAULT: &str = "default";

pub(crate) fn list(env: &Env, kind: Kind) -> Listed {
    let (listed, default) = match kind {
        Kind::Mic => ("sources", "get-default-source"),
        Kind::Speaker => ("sinks", "get-default-sink"),
    };
    let nodes: Vec<Node> = pactl(env, &["-f", "json", "list", listed])
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let nodes: Vec<Node> = nodes
        .into_iter()
        .filter(|node| !node.name.ends_with(".monitor"))
        .collect();
    Listed {
        devices: labelled(nodes),
        default: pactl(env, &[default]).map(|name| name.trim().to_owned()),
    }
}

fn pactl(env: &Env, args: &[&str]) -> Option<String> {
    let output = Command::new("pactl")
        .args(args)
        .env("PATH", &env.path)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn labelled(nodes: Vec<Node>) -> Vec<Device> {
    let described = |node: &Node| {
        if node.description.is_empty() {
            node.name.clone()
        } else {
            node.description.clone()
        }
    };
    let mut sharing: HashMap<String, Vec<&str>> = HashMap::new();
    for node in &nodes {
        sharing.entry(described(node)).or_default().push(&node.name);
    }
    for names in sharing.values_mut() {
        names.sort_unstable();
    }
    nodes
        .iter()
        .map(|node| {
            let description = described(node);
            let names = &sharing[&description];
            let label = match names.iter().position(|name| *name == node.name) {
                Some(at) if names.len() > 1 => format!("{description} ({})", at + 1),
                _ => description,
            };
            Device {
                name: node.name.clone(),
                label,
            }
        })
        .collect()
}

impl Listed {
    pub(crate) fn choices(&self, pinned: &str) -> Vec<Device> {
        let default = self
            .default
            .as_deref()
            .and_then(|name| self.devices.iter().find(|device| device.name == name))
            .map_or_else(
                || "Default".to_owned(),
                |device| format!("Default ({})", device.label),
            );
        let mut choices = vec![Device {
            name: DEFAULT.to_owned(),
            label: default,
        }];
        choices.extend(self.devices.iter().cloned());
        if !choices.iter().any(|device| device.name == pinned) {
            choices.push(Device {
                name: pinned.to_owned(),
                label: format!("{pinned} (not connected)"),
            });
        }
        choices
    }
}
