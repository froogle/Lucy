use lucy_config::TtsKind;
use ratatui::backend::Backend;

use super::{Context, Page};
use crate::settings::{Applies, Change, Key};
use crate::ui::{Stop, Ui};
use crate::{kokoro, piper};

struct Voice {
    name: &'static str,
    about: &'static str,
    commercial: bool,
}

const PIPER_VOICES: &[Voice] = &[
    Voice {
        name: "en_GB-cori-medium",
        about: "CC BY 4.0",
        commercial: true,
    },
    Voice {
        name: "en_GB-cori-high",
        about: "CC BY 4.0; slower to start speaking",
        commercial: true,
    },
    Voice {
        name: "en_US-lessac-medium",
        about: "non-commercial: its training data allows research and exploration only",
        commercial: false,
    },
    Voice {
        name: "en_US-ryan-high",
        about: "CC BY-NC-SA 4.0, non-commercial",
        commercial: false,
    },
];

const KOKORO_VOICES: &[Voice] = &[
    kokoro_voice("bf_emma", "British woman"),
    kokoro_voice("bf_isabella", "British woman"),
    kokoro_voice("bf_alice", "British woman"),
    kokoro_voice("bm_george", "British man"),
    kokoro_voice("bm_lewis", "British man"),
    kokoro_voice("bm_daniel", "British man"),
    kokoro_voice("af_heart", "American woman"),
    kokoro_voice("af_bella", "American woman"),
    kokoro_voice("am_michael", "American man"),
    kokoro_voice("am_fenrir", "American man"),
];

const fn kokoro_voice(name: &'static str, about: &'static str) -> Voice {
    Voice {
        name,
        about,
        commercial: true,
    }
}

const ENGINES: [(TtsKind, &str); 2] = [
    (
        TtsKind::Piper,
        "Piper: starts speaking at once (recommended)",
    ),
    (
        TtsKind::Kokoro,
        "Kokoro: sounds more natural, but up to about half a second slower to start each reply",
    ),
];

const VOICE_SAMPLE: &str = "Hello, I'm Lucy. This is how I'll sound.";

pub(super) fn page(context: &Context) -> Page {
    let voice = context.config().voice();
    match voice.kind {
        TtsKind::Piper if !piper::installed(&context.env.path) => Page::new(vec![
            "Install Piper, for Lucy's voice. Without it she shows her replies as text.".to_owned(),
        ]),
        TtsKind::Kokoro if !kokoro::installed(&context.env.kokoro) => Page::new(vec![
            "Kokoro isn't installed, so Lucy shows her replies as text. Pick it again to install it."
                .to_owned(),
        ]),
        TtsKind::Piper => Page::new(vec![format!("Voice: {} (Piper)", voice.name)]).needing_lucyd(),
        TtsKind::Kokoro => {
            Page::new(vec![format!("Voice: {} (Kokoro)", voice.name)]).needing_lucyd()
        }
    }
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let labels: Vec<String> = ENGINES
        .iter()
        .map(|(_, label)| (*label).to_owned())
        .collect();
    match ENGINES[ui.choose("How Lucy speaks:", &labels)?].0 {
        TtsKind::Piper => match piper::installed(&context.env.path) {
            true => voice(ui, context, TtsKind::Piper),
            false => install(ui, context),
        },
        TtsKind::Kokoro => {
            match kokoro::installed(&context.env.kokoro) || install_kokoro(ui, context)? {
                true => voice(ui, context, TtsKind::Kokoro),
                false => Ok(()),
            }
        }
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

fn install_kokoro<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<bool, Stop> {
    let env = context.env;
    if !ui.yes(
        "Install Kokoro now? It downloads about 360 MB into Lucy's own folder: \
         about 160 MB of Python packages now, and its 200 MB model the first time she speaks in it.",
        false,
    )? {
        ui.say("Lucy keeps speaking as she does now.")?;
        return Ok(false);
    }
    let plan = kokoro::install_plan(&env.kokoro, |program| {
        piper::on_path(program, &env.path).is_some()
    });
    for commands in plan {
        let ran = run_all(ui, context, &commands)?;
        if ran && kokoro::installed(&env.kokoro) {
            ui.say("Kokoro is installed.")?;
            return Ok(true);
        }
        ui.say("That didn't install Kokoro.")?;
    }
    ui.say(
        "Lucy couldn't install Kokoro. Install uv (docs.astral.sh/uv), then pick Kokoro again. \
         Until then, Lucy speaks as she does now.",
    )?;
    Ok(false)
}

fn run_all<B: Backend>(
    ui: &mut Ui<B>,
    context: &Context,
    commands: &[Vec<String>],
) -> Result<bool, Stop> {
    for command in commands {
        let running = format!("Running: {}", command.join(" "));
        ui.say(&running)?;
        let ran = ui.outside(|| {
            println!("{running}");
            std::process::Command::new(&command[0])
                .args(&command[1..])
                .env("PATH", &context.env.path)
                .env_remove("PYTHONPATH")
                .status()
        })?;
        if !ran.is_ok_and(|status| status.success()) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn voice<B: Backend>(ui: &mut Ui<B>, context: &Context, kind: TtsKind) -> Result<(), Stop> {
    let (voices, key, fetching) = match kind {
        TtsKind::Piper => (
            PIPER_VOICES,
            Key::PiperVoice,
            "Fetching the voice if need be, then Lucy speaks in it...",
        ),
        TtsKind::Kokoro => (
            KOKORO_VOICES,
            Key::KokoroVoice,
            "Fetching Kokoro's model if need be (the 200 MB part, once), then Lucy speaks in it...",
        ),
    };
    let labels: Vec<String> = voices
        .iter()
        .enumerate()
        .map(|(i, voice)| match i {
            0 => format!("{}: {} (recommended)", voice.name, voice.about),
            _ => format!("{}: {}", voice.name, voice.about),
        })
        .collect();
    loop {
        let voice = &voices[ui.choose("Lucy's voice:", &labels)?];
        if !voice.commercial {
            ui.say(format!(
                "{} is non-commercial. Its licence: {}. Its MODEL_CARD has the full terms.",
                voice.name, voice.about
            ))?;
            if !ui.yes("Use it anyway?", false)? {
                continue;
            }
        }
        let chosen = [
            Change::Set(Key::Tts, kind.config_name()),
            Change::Set(key.clone(), voice.name.to_owned()),
        ];
        let Some(mut client) = context.client() else {
            ui.say("lucyd isn't running, so Lucy can't speak in it for you to hear first.")?;
            return context.save(ui, &chosen);
        };
        ui.say(fetching)?;
        if let Err(e) = client.voice_preview_in(VOICE_SAMPLE, ipc_kind(kind), voice.name) {
            ui.say(format!("Lucy couldn't speak in it: {e}"))?;
        }
        if ui.yes("Keep this voice?", true)? {
            return context.save(ui, &chosen);
        }
    }
}

fn ipc_kind(kind: TtsKind) -> lucy_ipc::TtsKind {
    match kind {
        TtsKind::Piper => lucy_ipc::TtsKind::Piper,
        TtsKind::Kokoro => lucy_ipc::TtsKind::Kokoro,
    }
}
