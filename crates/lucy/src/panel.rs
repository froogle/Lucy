//! What the bar panel shows: a snapshot of whether lucyd runs, whether a
//! Session is open, and each choice the panel offers, with its options.

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use lucy_config::SttKind;
use lucy_ipc::{Event, Status, Topic};
use serde::Serialize;

use crate::devices::{self, Kind};
use crate::settings::{self, Key, WHISPER_MODELS};
use crate::systemd::{self, Activity};
use crate::Env;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Stopped,
    Starting,
    Waiting,
    Session,
    Failed,
}

/// One snapshot, printed as a line of JSON. A choice's None option unsets
/// its key.
#[derive(Debug, PartialEq, Serialize)]
pub struct Snapshot {
    state: State,
    wake_word: String,
    fast: Choice<String>,
    advanced: Choice<String>,
    stt: Choice<Option<String>>,
    mic: Choice<String>,
    speaker: Choice<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Choice<T> {
    key: String,
    current: T,
    options: Vec<T>,
}

impl<T> Choice<T> {
    fn new(key: impl ToString, current: T, options: Vec<T>) -> Choice<T> {
        Choice {
            key: key.to_string(),
            current,
            options,
        }
    }
}

/// The microphones and speakers PipeWire has.
pub struct Devices {
    mics: Vec<String>,
    speakers: Vec<String>,
}

impl Devices {
    /// The devices `pactl` lists now.
    pub fn list(env: &Env) -> Devices {
        Devices {
            mics: devices::names(env, Kind::Mic),
            speakers: devices::names(env, Kind::Speaker),
        }
    }
}

impl Snapshot {
    /// The snapshot now, given lucyd's `status`, None if it isn't running.
    pub fn of(env: &Env, status: Option<&Status>, devices: &Devices) -> Snapshot {
        let config = lucy_config::load(&env.config).unwrap_or_default();
        let providers: Vec<String> = config.providers.keys().cloned().collect();
        let device = |key, current: String, names: &[String]| {
            let mut options = vec!["default".to_owned()];
            options.extend_from_slice(names);
            Choice::new(key, current, options)
        };
        Snapshot {
            state: state(env, status),
            wake_word: titled(&config.wake_word.word()),
            fast: Choice::new(
                Key::Tier(lucy_config::Tier::Fast),
                config.tiers.fast.clone(),
                providers.clone(),
            ),
            advanced: Choice::new(
                Key::Tier(lucy_config::Tier::Advanced),
                config.tiers.advanced.clone(),
                providers,
            ),
            stt: stt(env, &config),
            mic: device(Key::Mic, config.audio.mic.clone(), &devices.mics),
            speaker: device(
                Key::Speaker,
                config.audio.speaker.clone(),
                &devices.speakers,
            ),
        }
    }

    /// One line of JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a snapshot is plain data")
    }
}

fn titled(phrase: &str) -> String {
    let word = |word: &str| {
        let mut letters = word.chars();
        letters
            .next()
            .map(|first| first.to_uppercase().chain(letters).collect::<String>())
            .unwrap_or_default()
    };
    phrase
        .split_whitespace()
        .map(word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn stt(env: &Env, config: &lucy_config::Config) -> Choice<Option<String>> {
    let superwhisper = SttKind::Superwhisper.config_name();
    let mut options: Vec<Option<String>> = WHISPER_MODELS
        .iter()
        .map(|(name, _)| name.map(str::to_owned))
        .collect();
    if SttKind::options(Some(&env.path)).contains(&SttKind::Superwhisper) {
        options.push(Some(superwhisper.clone()));
    }
    let current = match config.stt.kind {
        SttKind::Whisper => config.whisper.model.clone(),
        SttKind::Superwhisper => Some(superwhisper),
    };
    Choice::new(settings::SPEECH, current, options)
}

const POLL: Duration = Duration::from_secs(1);

enum Change {
    Lucyd(Option<Status>),
    Devices,
}

/// Writes a snapshot to `out` as a line of JSON, then another each time it
/// changes, until `out` can't be written to.
///
/// # Errors
///
/// When writing to `out` fails, such as when its reader has gone.
pub fn follow(env: &Env, out: &mut impl Write) -> io::Result<()> {
    let (send, changes) = mpsc::channel();
    let (lucyd, watched) = (send.clone(), env.clone());
    std::thread::spawn(move || watch_lucyd(&watched, &lucyd));
    let watched = env.clone();
    std::thread::spawn(move || watch_devices(&watched, &send));
    let mut status = env.connect().and_then(|mut client| client.status()).ok();
    let mut devices = Devices::list(env);
    let mut shown = None;
    loop {
        let snapshot = Snapshot::of(env, status.as_ref(), &devices);
        if shown.as_ref() != Some(&snapshot) {
            writeln!(out, "{}", snapshot.to_json())?;
            out.flush()?;
            shown = Some(snapshot);
        }
        match changes.recv_timeout(POLL) {
            Ok(Change::Lucyd(now)) => status = now,
            Ok(Change::Devices) => devices = Devices::list(env),
            Err(_) => {}
        }
    }
}

fn watch_lucyd(env: &Env, changes: &Sender<Change>) {
    loop {
        if let Ok(mut client) = env.connect() {
            if client.subscribe(&[Topic::State]).is_ok() {
                while let Ok(Event::State(status)) = client.next_event() {
                    if changes.send(Change::Lucyd(Some(status))).is_err() {
                        return;
                    }
                }
            }
            if changes.send(Change::Lucyd(None)).is_err() {
                return;
            }
        }
        std::thread::sleep(POLL);
    }
}

fn watch_devices(env: &Env, changes: &Sender<Change>) {
    let Ok(mut pactl) = Command::new("pactl")
        .arg("subscribe")
        .env("PATH", &env.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let events = BufReader::new(pactl.stdout.take().expect("stdout is piped"));
    for event in events.lines().map_while(Result::ok) {
        let gained_or_lost = event.contains("'new'") || event.contains("'remove'");
        let device = event.contains(" source ") || event.contains(" sink ");
        if gained_or_lost && device && changes.send(Change::Devices).is_err() {
            return;
        }
    }
}

const PLUGIN: &str = "omarchy-plugin";

pub(crate) fn link(env: &Env) -> Result<(), String> {
    if !env.omarchy.is_dir() {
        return Ok(());
    }
    let shipped = env.bin.join(PLUGIN);
    let checkout = env.bin.join("../..").join(PLUGIN);
    let panel = [shipped, checkout]
        .into_iter()
        .find(|dir| dir.join("manifest.json").is_file())
        .ok_or_else(|| {
            format!(
                "there's no {PLUGIN} beside {} or in the checkout it was built in",
                env.bin.display()
            )
        })?;
    let panel = std::fs::canonicalize(&panel)
        .map_err(|e| format!("can't follow {}: {e}", panel.display()))?;
    let plugins = env.omarchy.join("plugins");
    std::fs::create_dir_all(&plugins)
        .map_err(|e| format!("can't create {}: {e}", plugins.display()))?;
    let link = plugins.join("lucy-panel");
    if link.is_symlink() {
        std::fs::remove_file(&link)
            .map_err(|e| format!("can't replace {}: {e}", link.display()))?;
    }
    std::os::unix::fs::symlink(&panel, &link)
        .map_err(|e| format!("can't link {}: {e}", link.display()))
}

const STARTUP: Duration = Duration::from_secs(60);

fn state(env: &Env, status: Option<&Status>) -> State {
    match status {
        Some(status) if status.session_id.is_some() => State::Session,
        Some(_) => State::Waiting,
        None => match systemd::activity(env) {
            Activity::Stopped => State::Stopped,
            Activity::Up(up) if up < STARTUP => State::Starting,
            Activity::Up(_) | Activity::Restarting | Activity::Failed => State::Failed,
        },
    }
}
