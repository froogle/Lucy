//! Speech-to-text: the STT Provider, warning about Superwhisper when it is
//! chosen, and Whisper's model.

use lucy_config::SttKind;
use ratatui::backend::Backend;

use super::{Context, Page};
use crate::settings::{Change, Key, WHISPER_MODELS as MODELS};
use crate::ui::{Stop, Ui};

fn label(kind: SttKind) -> &'static str {
    match kind {
        SttKind::Whisper => "Whisper, on this computer (recommended)",
        SttKind::Superwhisper => "Superwhisper",
    }
}

pub(super) fn page(context: &Context) -> Page {
    let config = context.config();
    let mut rows = vec![format!("Speech-to-text: {}", label(config.stt.kind))];
    if config.stt.kind == SttKind::Whisper {
        let model = config.whisper.model.as_deref();
        let (_, label) = MODELS
            .iter()
            .find(|(name, _)| *name == model)
            .copied()
            .unwrap_or((None, model.unwrap_or_default()));
        rows.push(format!("Whisper's model: {label}"));
    }
    Page::new(rows)
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
    match row {
        0 => kind(ui, context),
        _ => model(ui, context),
    }
}

fn kind<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let options = SttKind::options(Some(&context.env.path));
    let mut kind = SttKind::Whisper;
    if options.len() > 1 {
        let labels: Vec<String> = options.iter().map(|k| label(*k).to_owned()).collect();
        kind = options[ui.choose("Speech-to-text:", &labels)?];
    } else {
        ui.say("Superwhisper isn't installed, so Lucy uses Whisper.")?;
    }
    if !kind.warnings().is_empty() {
        for warning in kind.warnings() {
            ui.say(format!("  - {warning}"))?;
        }
        if !ui.yes("Use Superwhisper anyway?", false)? {
            kind = SttKind::Whisper;
        }
    }
    context.save(ui, &[Change::Set(Key::Stt, kind.config_name())])
}

fn model<B: Backend>(ui: &mut Ui<B>, context: &Context) -> Result<(), Stop> {
    let labels = MODELS.map(|(_, label)| label.to_owned());
    let change = match MODELS[ui.choose("Whisper's model:", &labels)?] {
        (Some(model), _) => Change::Set(Key::WhisperModel, model.to_owned()),
        (None, _) => Change::Unset(Key::WhisperModel),
    };
    context.save(ui, &[change])
}
