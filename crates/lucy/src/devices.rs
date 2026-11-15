//! The microphones and speakers PipeWire has, by node name, as `pactl`
//! lists them.

use crate::Env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Mic,
    Speaker,
}

pub(crate) fn names(env: &Env, kind: Kind) -> Vec<String> {
    let listed = match kind {
        Kind::Mic => "sources",
        Kind::Speaker => "sinks",
    };
    let Ok(output) = std::process::Command::new("pactl")
        .args(["list", "short", listed])
        .env("PATH", &env.path)
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .filter(|name| !name.ends_with(".monitor"))
        .map(str::to_owned)
        .collect()
}
