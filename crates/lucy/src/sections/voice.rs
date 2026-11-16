use ratatui::backend::Backend;

use super::{Context, Page};
use crate::piper;
use crate::settings::{Applies, Change, Key};
use crate::ui::{Stop, Ui};

struct Voice {
    name: &'static str,
    licence: &'static str,
    commercial: bool,
}

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

pub(super) fn page(context: &Context) -> Page {
    if !piper::installed(&context.env.path) {
        return Page::new(vec![
            "Install Piper, for Lucy's voice. Without it she shows her replies as text.".to_owned(),
        ]);
    }
    let voice = context.config().piper.voice;
    Page::new(vec![format!("Voice: {voice}")]).needing_lucyd()
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    match piper::installed(&context.env.path) {
        true => voice(ui, context),
        false => install(ui, context),
    }
}

fn install<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let env = context.env;
    if !ui.yes("Install Piper now?", false)? {
        return ui.say("Without Piper, Lucy shows her replies as text in notifications.");
    }
    let os_release = std::fs::read_to_string(&env.os_release).unwrap_or_default();
    let plan = piper::install_plan(&os_release, |program| {
        piper::on_path(program, &env.path).is_some()
    });
    for command in plan {
        let running = format!("Running: {}", command.join(" "));
        ui.say(&running)?;
        let ran = ui.outside(|| {
            println!("{running}");
            std::process::Command::new(&command[0])
                .args(&command[1..])
                .env("PATH", &env.path)
                .status()
        })?;
        if ran.is_ok_and(|status| status.success()) && piper::installed(&env.path) {
            ui.say("Piper is installed.")?;
            return context.say_when(ui, Applies::Restart);
        }
        ui.say("That didn't install Piper.")?;
    }
    ui.say(
        "Lucy couldn't install Piper. Install it with: uv tool install 'piper-tts[http]'. \
         Until then, Lucy shows her replies as text.",
    )
}

fn voice<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let labels: Vec<String> = VOICES
        .iter()
        .enumerate()
        .map(|(i, voice)| match i {
            0 => format!("{}: {} (recommended)", voice.name, voice.licence),
            _ => format!("{}: {}", voice.name, voice.licence),
        })
        .collect();
    loop {
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
        let Some(mut client) = context.client() else {
            ui.say("lucyd isn't running, so Lucy can't speak in it for you to hear first.")?;
            return context.save(ui, &[Change::Set(Key::Voice, voice.name.to_owned())]);
        };
        ui.say("Fetching the voice if need be, then Lucy speaks in it...")?;
        if let Err(e) = client.voice_preview_in(VOICE_SAMPLE, Some(voice.name)) {
            ui.say(format!("Lucy couldn't speak in it: {e}"))?;
        }
        if ui.yes("Keep this voice?", true)? {
            return context.save(ui, &[Change::Set(Key::Voice, voice.name.to_owned())]);
        }
    }
}
