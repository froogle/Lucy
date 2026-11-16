use std::time::Duration;

use lucy_ipc::Client;
use ratatui::backend::Backend;

use super::{Context, Page};
use crate::devices::{self, Kind};
use crate::settings::{Change, Key};
use crate::ui::{Stop, Ui};

const METER: Duration = Duration::from_secs(3);
const METER_EVERY: Duration = Duration::from_millis(100);

pub(super) fn page(context: &Context) -> Page {
    let audio = context.config().audio;
    let label = |kind, pinned: &str| {
        devices::list(context.env, kind)
            .choices(pinned)
            .into_iter()
            .find(|device| device.name == pinned)
            .expect("the choices hold the pinned device")
            .label
    };
    Page::new(vec![
        format!("Microphone: {}", label(Kind::Mic, &audio.mic)),
        format!("Speaker: {}", label(Kind::Speaker, &audio.speaker)),
    ])
    .needing_lucyd()
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
    match row {
        0 => mic(ui, context),
        _ => speaker(ui, context),
    }
}

fn pick_device<B: Backend>(
    ui: &mut Ui<B>,
    context: &Context,
    title: &str,
    kind: Kind,
    pinned: &str,
) -> Result<String, Stop> {
    let mut choices = devices::list(context.env, kind).choices(pinned);
    let labels: Vec<String> = choices.iter().map(|device| device.label.clone()).collect();
    Ok(choices.swap_remove(ui.choose(title, &labels)?).name)
}

fn mic<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    loop {
        let mic = pick_device(
            ui,
            context,
            "Microphone:",
            Kind::Mic,
            &context.config().audio.mic,
        )?;
        context.save(ui, &[Change::Set(Key::Mic, mic)])?;
        let Some(mut client) = context.lucyd(ui, "and the level meter needs it")? else {
            return Ok(());
        };
        let quiet = meter(ui, &mut client)?;
        if quiet {
            ui.say(
                "Lucy can barely hear you on this mic, so talking over her may not work. \
                 Move it closer, turn it up in your sound settings, or pick another mic.",
            )?;
        }
        if ui.yes("Keep this microphone?", !quiet)? {
            return Ok(());
        }
    }
}

fn meter<B: Backend>(ui: &mut Ui<B>, client: &mut Client) -> Result<bool, Stop> {
    let switched_to_it = client.mic_level();
    if let Err(e) = switched_to_it {
        ui.say(format!("Can't test it: {e}"))?;
        return Ok(false);
    }
    ui.say("Say something: the meter should move.")?;
    ui.meter(Some(0.0))?;
    let started = std::time::Instant::now();
    let (mut readings, mut all_quiet) = (0, true);
    while started.elapsed() < METER {
        std::thread::sleep(METER_EVERY);
        let Ok(level) = client.mic_level() else {
            break;
        };
        readings += 1;
        all_quiet &= level.quiet;
        ui.meter(Some(on_log_scale(level.rms)))?;
    }
    ui.meter(None)?;
    Ok(readings > 0 && all_quiet)
}

const METER_FLOOR_DB: f32 = -60.0;

fn on_log_scale(rms: f32) -> f64 {
    let db = 20.0 * rms.max(1e-5).log10();
    f64::from((db - METER_FLOOR_DB) / -METER_FLOOR_DB)
}

fn speaker<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    loop {
        let speaker = pick_device(
            ui,
            context,
            "Speaker:",
            Kind::Speaker,
            &context.config().audio.speaker,
        )?;
        context.save(ui, &[Change::Set(Key::Speaker, speaker)])?;
        let Some(mut client) = context.lucyd(ui, "so Lucy can't speak on it")? else {
            return Ok(());
        };
        ui.say("Lucy is speaking on it...")?;
        match client.voice_preview("This is Lucy, on this speaker.") {
            Ok(()) => {
                if ui.yes("Did you hear Lucy?", true)? {
                    return Ok(());
                }
            }
            Err(e) => return ui.say(format!("Lucy couldn't speak: {e}")),
        }
    }
}
