//! `lucy setup`: gets a new user running in one sitting. It writes only the
//! settings it asks about into `config.toml`, keeping its comments; every
//! other setting stays file-only. The tests of the microphone, voice and
//! Wake word run inside lucyd. It never approves a Plugin: lucyd approves
//! shipped ones itself, and other Approvals are granted only by voice.

use std::time::Duration;

use lucy_config::{Kind, SttKind, Tier, RECOMMENDATIONS};
use lucy_ipc::Client;
use ratatui::backend::Backend;

use crate::piper;
use crate::ui::{Stop, Ui};
use crate::Env;

/// A Piper voice setup offers, with what its licence allows.
struct Voice {
    name: &'static str,
    licence: &'static str,
    /// False if its licence, or that of a voice it was trained from, rules
    /// out commercial use.
    commercial: bool,
}

/// Recommended voices have a permissive training chain only; the others are
/// offered with their terms shown.
const VOICES: &[Voice] = &[
    Voice {
        name: "en_GB-cori-medium",
        licence: "CC BY 4.0",
        commercial: true,
    },
    Voice {
        name: "en_GB-cori-high",
        licence: "CC BY 4.0; slower to start speaking",
        commercial: true,
    },
    Voice {
        name: "en_US-lessac-medium",
        licence: "non-commercial: its training data allows research and exploration only",
        commercial: false,
    },
    Voice {
        name: "en_US-ryan-high",
        licence: "CC BY-NC-SA 4.0, non-commercial",
        commercial: false,
    },
];

const VOICE_SAMPLE: &str = "Hello, I'm Lucy. This is how I'll sound.";

/// How long the level meter runs.
const METER: Duration = Duration::from_secs(3);
const METER_EVERY: Duration = Duration::from_millis(100);

/// How long the wake-word test listens.
const WAKE_TEST_SECS: f64 = 10.0;

impl From<Stop> for String {
    fn from(stop: Stop) -> String {
        stop.to_string()
    }
}

pub fn run<B: Backend>(ui: &mut Ui<B>, env: &Env) -> Result<(), String> {
    let config = Config(env.config.clone());
    ui.say("Setting up Lucy. Enter takes the suggestion.")?;
    ui.say("")?;
    let has = |program: &str| piper::on_path(program, &env.path).is_some();
    for (program, what) in [
        ("claude", "Claude, for thinking"),
        ("codex", "Codex, for thinking"),
        ("superwhisper", "Superwhisper, for speech-to-text"),
    ] {
        let found = if has(program) { "found" } else { "not found" };
        ui.say(format!("{what}: {program} {found}"))?;
    }
    let piper = piper_step(ui, env)?;
    tiers_step(ui, &config, &has)?;
    stt_step(ui, &config, env)?;

    let mut enabled = false;
    let mut client = match env.connect() {
        Ok(client) => Some(client),
        Err(_) => {
            ui.say("")?;
            ui.say("lucyd isn't running, and the microphone, voice and Wake word tests need it.")?;
            if ui.yes("Start Lucy now, and whenever you log in?", true)? {
                enable(ui, env)?;
                enabled = true;
                wait_for_lucyd(env)
            } else {
                None
            }
        }
    };
    match &mut client {
        Some(client) => {
            devices_step(ui, &config, env, client)?;
            if piper {
                voice_step(ui, &config, client)?;
            }
            wake_word_step(ui, client)?;
        }
        None => ui.say("Skipping the microphone, voice and Wake word tests.")?,
    }
    if !enabled {
        ui.say("")?;
        if ui.yes("Start Lucy whenever you log in (lucyd.service)?", true)? {
            enable(ui, env)?;
        }
    }
    ui.say("")?;
    ui.say(format!(
        "Setup is done. Every other setting is in {}.",
        config.0.display()
    ))?;
    Ok(())
}

/// `config.toml`, edited one key at a time, comments kept.
struct Config(std::path::PathBuf);

impl Config {
    fn set(&self, key: &[&str], value: impl Into<toml_edit::Value>) -> Result<(), String> {
        lucy_config::set(&self.0, key, value).map_err(|e| e.to_string())
    }
}

/// Whether Piper is there, once the user has had the chance to install it.
fn piper_step<B: Backend>(ui: &mut Ui<B>, env: &Env) -> Result<bool, Stop> {
    ui.say("")?;
    if piper::installed(&env.path) {
        ui.say("Piper, for Lucy's voice: installed")?;
        return Ok(true);
    }
    ui.say("Piper, for Lucy's voice: not installed")?;
    if !ui.yes("Install Piper now?", false)? {
        ui.say("Without Piper, Lucy shows her replies as text in notifications.")?;
        return Ok(false);
    }
    let os_release = std::fs::read_to_string(&env.os_release).unwrap_or_default();
    let plan = piper::install_plan(&os_release, |program| {
        piper::on_path(program, &env.path).is_some()
    });
    for command in plan {
        let running = format!("Running: {}", command.join(" "));
        ui.say(&running)?;
        // Outside the full screen: an installer may ask for a password.
        let ran = ui.outside(|| {
            println!("{running}");
            std::process::Command::new(&command[0])
                .args(&command[1..])
                .env("PATH", &env.path)
                .status()
        })?;
        if ran.is_ok_and(|status| status.success()) && piper::installed(&env.path) {
            ui.say("Piper is installed.")?;
            return Ok(true);
        }
        ui.say("That didn't install Piper.")?;
    }
    ui.say(
        "Lucy couldn't install Piper. Install it with: uv tool install 'piper-tts[http]'. \
         Until then, Lucy shows her replies as text.",
    )?;
    Ok(false)
}

fn tiers_step<B: Backend>(
    ui: &mut Ui<B>,
    config: &Config,
    has: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    for tier in [Tier::Fast, Tier::Advanced] {
        let options: Vec<_> = RECOMMENDATIONS
            .iter()
            .filter(|r| r.tier == tier && has(cli(r.kind)))
            .collect();
        ui.say("")?;
        if options.is_empty() {
            ui.say(
                "Neither claude nor codex is installed. Install one, then run lucy setup again.",
            )?;
            return Ok(());
        }
        let labels: Vec<String> = options
            .iter()
            .map(|r| format!("{}, {}: {}", r.kind.name(), r.model, r.why))
            .collect();
        let chosen = options[ui.choose(&format!("The {tier} tier thinks with:"), &labels)?];
        let model = ui.ask("Model, or any the CLI accepts:", chosen.model)?;
        let name = format!("{}-{tier}", cli(chosen.kind));
        config.set(&["providers", &name, "kind"], cli(chosen.kind))?;
        config.set(&["providers", &name, "model"], model)?;
        config.set(&["tiers", &tier.to_string()], name)?;
    }
    Ok(())
}

fn cli(kind: Kind) -> &'static str {
    match kind {
        Kind::Claude => "claude",
        Kind::Codex => "codex",
    }
}

fn stt_step<B: Backend>(ui: &mut Ui<B>, config: &Config, env: &Env) -> Result<(), String> {
    let options = SttKind::options(Some(&env.path));
    ui.say("")?;
    let mut kind = SttKind::Whisper;
    if options.len() > 1 {
        let labels = options
            .iter()
            .map(|kind| match kind {
                SttKind::Whisper => "Whisper, on this computer (recommended)",
                SttKind::Superwhisper => "Superwhisper",
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        kind = options[ui.choose("Speech-to-text:", &labels)?];
    }
    if !kind.warnings().is_empty() {
        for warning in kind.warnings() {
            ui.say(format!("  - {warning}"))?;
        }
        if !ui.yes("Use Superwhisper anyway?", false)? {
            kind = SttKind::Whisper;
        }
    }
    let name = match kind {
        SttKind::Whisper => "whisper",
        SttKind::Superwhisper => "superwhisper",
    };
    config.set(&["stt", "kind"], name)?;
    if kind == SttKind::Whisper {
        let models = ["small.en", "base.en"];
        let labels = [
            "Sized to this computer: small.en on a GPU, base.en on the CPU".to_owned(),
            "small.en: more accurate, best on a GPU".to_owned(),
            "base.en: faster on the CPU".to_owned(),
        ];
        let chosen = ui.choose("Whisper's model:", &labels)?;
        if chosen > 0 {
            config.set(&["whisper", "model"], models[chosen - 1])?;
        }
    }
    Ok(())
}

/// The node names `pactl` lists of `kind` (`sources` or `sinks`), leaving
/// out monitors of speakers.
fn devices(env: &Env, kind: &str) -> Vec<String> {
    let Ok(output) = std::process::Command::new("pactl")
        .args(["list", "short", kind])
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

fn pick_device<B: Backend>(
    ui: &mut Ui<B>,
    env: &Env,
    title: &str,
    kind: &str,
) -> Result<String, Stop> {
    let mut names = vec!["default".to_owned()];
    names.extend(devices(env, kind));
    let mut labels = names.clone();
    labels[0] = "default: whatever the system uses".to_owned();
    Ok(names.swap_remove(ui.choose(title, &labels)?))
}

fn devices_step<B: Backend>(
    ui: &mut Ui<B>,
    config: &Config,
    env: &Env,
    client: &mut Client,
) -> Result<(), String> {
    loop {
        ui.say("")?;
        let mic = pick_device(ui, env, "Microphone:", "sources")?;
        config.set(&["audio", "mic"], mic)?;
        meter(ui, client)?;
        if ui.yes("Keep this microphone?", true)? {
            break;
        }
    }
    loop {
        ui.say("")?;
        let speaker = pick_device(ui, env, "Speaker:", "sinks")?;
        config.set(&["audio", "speaker"], speaker)?;
        ui.say("Lucy is speaking on it...")?;
        match client.voice_preview("This is Lucy, on this speaker.") {
            Ok(()) => {
                if ui.yes("Did you hear Lucy?", true)? {
                    break;
                }
            }
            Err(e) => {
                ui.say(format!("Lucy couldn't speak: {e}"))?;
                break;
            }
        }
    }
    Ok(())
}

/// Shows how loud the microphone is for a few seconds.
fn meter<B: Backend>(ui: &mut Ui<B>, client: &mut Client) -> Result<(), Stop> {
    // The first asks lucyd to switch to it.
    if let Err(e) = client.mic_level() {
        return ui.say(format!("Can't test it: {e}"));
    }
    ui.say("Say something: the meter should move.")?;
    ui.meter(Some(0.0))?;
    let started = std::time::Instant::now();
    while started.elapsed() < METER {
        std::thread::sleep(METER_EVERY);
        let Ok(level) = client.mic_level() else {
            break;
        };
        // Speech is around 0.05 to 0.3 RMS; show it on a log scale.
        let db = 20.0 * level.rms.max(1e-5).log10();
        ui.meter(Some(f64::from((db + 60.0) / 60.0)))?;
    }
    ui.meter(None)
}

fn voice_step<B: Backend>(
    ui: &mut Ui<B>,
    config: &Config,
    client: &mut Client,
) -> Result<(), String> {
    let labels: Vec<String> = VOICES
        .iter()
        .enumerate()
        .map(|(i, voice)| match i {
            0 => format!("{}: {} (recommended)", voice.name, voice.licence),
            _ => format!("{}: {}", voice.name, voice.licence),
        })
        .collect();
    loop {
        ui.say("")?;
        let voice = &VOICES[ui.choose("Lucy's voice:", &labels)?];
        if !voice.commercial {
            ui.say(format!(
                "{} is non-commercial. Its licence: {}. Its MODEL_CARD has the full terms.",
                voice.name, voice.licence
            ))?;
            if !ui.yes("Use it anyway?", false)? {
                continue;
            }
        }
        ui.say("Fetching the voice if need be, then Lucy speaks in it...")?;
        if let Err(e) = client.voice_preview_in(VOICE_SAMPLE, Some(voice.name)) {
            ui.say(format!("Lucy couldn't speak in it: {e}"))?;
        }
        if ui.yes("Keep this voice?", true)? {
            config.set(&["piper", "voice"], voice.name)?;
            ui.say("Lucy speaks in it from lucyd's next start.")?;
            return Ok(());
        }
    }
}

fn wake_word_step<B: Backend>(ui: &mut Ui<B>, client: &mut Client) -> Result<(), Stop> {
    ui.say("")?;
    if !ui.yes("Test the Wake word now?", true)? {
        return Ok(());
    }
    loop {
        ui.say(format!(
            "Say the Wake word in the next {WAKE_TEST_SECS} seconds..."
        ))?;
        match client.test_wake_word(WAKE_TEST_SECS) {
            Ok(result) if result.heard => {
                ui.say(format!("Heard it, with a score of {:.2}.", result.score))?
            }
            Ok(result) => ui.say(format!(
                "Lucy didn't hear it. Its best score was {:.2}; [wake_word] threshold \
                 in config.toml sets the score it needs.",
                result.score
            ))?,
            Err(e) => return ui.say(format!("Can't test it: {e}")),
        }
        if !ui.yes("Try again?", false)? {
            return Ok(());
        }
    }
}

fn enable<B: Backend>(ui: &mut Ui<B>, env: &Env) -> Result<(), Stop> {
    ui.say("Enabling lucyd.service...")?;
    match crate::systemd::enable(env) {
        Ok(()) => ui.say("Lucy starts whenever you log in."),
        Err(e) => ui.say(format!("Couldn't enable lucyd.service: {e}")),
    }
}

/// lucyd, once it is listening after being started.
fn wait_for_lucyd(env: &Env) -> Option<Client> {
    for _ in 0..50 {
        if let Ok(client) = env.connect() {
            return Some(client);
        }
        std::thread::sleep(METER_EVERY);
    }
    None
}
