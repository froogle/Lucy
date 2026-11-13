//! Facts about this machine for `system_status`, read from `/proc`, `/sys`
//! and a few standard commands. Blocking.

use std::path::Path;
use std::process::Command;

pub const TOPICS: [&str; 7] = [
    "battery",
    "disk",
    "memory_cpu",
    "network",
    "volume",
    "time",
    "uptime",
];

/// Facts about `topic`, or `None` if it isn't one of `TOPICS`. A fact that
/// can't be read is reported as such rather than failing.
pub fn facts(topic: &str) -> Option<String> {
    let facts = match topic {
        "battery" => battery(),
        "disk" => disk(),
        "memory_cpu" => memory_cpu(),
        "network" => network(),
        "volume" => volume(),
        "time" => time(),
        "uptime" => uptime(),
        _ => return None,
    };
    Some(facts.unwrap_or_else(|| format!("The {topic} isn't available on this machine.")))
}

fn battery() -> Option<String> {
    let mut lines = Vec::new();
    for supply in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let dir = supply.path();
        let is_system_battery = read(&dir.join("type")).as_deref() == Some("Battery")
            && read(&dir.join("scope")).as_deref() != Some("Device");
        if !is_system_battery {
            continue;
        }
        let capacity = read(&dir.join("capacity")).unwrap_or_else(|| "unknown".to_owned());
        let status = read(&dir.join("status")).unwrap_or_else(|| "Unknown".to_owned());
        lines.push(format!("Battery: {capacity}%, {}.", status.to_lowercase()));
    }
    if lines.is_empty() {
        return Some("This machine has no battery; it runs on mains power.".to_owned());
    }
    Some(lines.join("\n"))
}

/// Free space on `/` and on the home directory's filesystem.
fn disk() -> Option<String> {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
    let output = Command::new("df")
        .arg("-Pk")
        .arg("/")
        .arg(&home)
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    // `/` and home are often the same filesystem.
    let mut filesystems: Vec<&str> = Vec::new();
    let mut lines = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [filesystem, size, _used, available, used_percent, mount] = fields[..] else {
            continue;
        };
        if filesystems.contains(&filesystem) {
            continue;
        }
        filesystems.push(filesystem);
        let kib = |n: &str| n.parse::<u64>().ok();
        lines.push(format!(
            "{mount}: {} free of {} ({used_percent} used).",
            gigabytes(kib(available)? * 1024),
            gigabytes(kib(size)? * 1024),
        ));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn memory_cpu() -> Option<String> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kib = |key: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    let total = kib("MemTotal")? * 1024;
    let used = total - kib("MemAvailable")? * 1024;
    let loadavg = std::fs::read_to_string("/proc/loadavg").ok()?;
    let load: Vec<&str> = loadavg.split_whitespace().take(3).collect();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    Some(format!(
        "Memory: {} used of {} ({}%).\nCPU load average over 1, 5 and 15 minutes: {} on {cores} cores.",
        gigabytes(used),
        gigabytes(total),
        used * 100 / total,
        load.join(", "),
    ))
}

/// The physical network interfaces that are up.
fn network() -> Option<String> {
    let mut up = Vec::new();
    for interface in std::fs::read_dir("/sys/class/net").ok()?.flatten() {
        let dir = interface.path();
        let name = interface.file_name().to_string_lossy().into_owned();
        let physical = dir.join("device").exists();
        if !physical || read(&dir.join("operstate")).as_deref() != Some("up") {
            continue;
        }
        let kind = if dir.join("wireless").exists() {
            "Wi-Fi"
        } else {
            "wired"
        };
        up.push(format!("{name} ({kind})"));
    }
    if up.is_empty() {
        return Some("Not connected to any network.".to_owned());
    }
    Some(format!("Connected through {}.", up.join(", ")))
}

/// The default output's volume, through PipeWire.
fn volume() -> Option<String> {
    let output = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()?;
    // "Volume: 0.40" or "Volume: 0.40 [MUTED]"
    let text = String::from_utf8(output.stdout).ok()?;
    let level: f64 = text
        .strip_prefix("Volume:")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let percent = (level * 100.0).round();
    if text.contains("[MUTED]") {
        Some(format!("Volume: {percent}%, muted."))
    } else {
        Some(format!("Volume: {percent}%."))
    }
}

fn time() -> Option<String> {
    let output = Command::new("date")
        .arg("+%A %-d %B %Y, %H:%M %Z")
        .output()
        .ok()?;
    let now = String::from_utf8(output.stdout).ok()?;
    Some(format!("It is {}.", now.trim()))
}

fn uptime() -> Option<String> {
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    let seconds = uptime.split_whitespace().next()?.parse::<f64>().ok()? as u64;
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    let mut parts = Vec::new();
    for (n, unit) in [(days, "day"), (hours, "hour"), (minutes, "minute")] {
        match n {
            0 => {}
            1 => parts.push(format!("1 {unit}")),
            n => parts.push(format!("{n} {unit}s")),
        }
    }
    if parts.is_empty() {
        return Some("Up for less than a minute.".to_owned());
    }
    Some(format!("Up for {}.", parts.join(", ")))
}

fn read(path: &Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_owned())
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}
