//! Thinking: the Provider each Model tier thinks with, its health, and a
//! real check of it on request.

use lucy_config::{Tier, RECOMMENDATIONS};
use ratatui::backend::Backend;

use super::{Context, Page};
use crate::piper::on_path;
use crate::settings::Key;
use crate::ui::{Stop, Ui};

const TIERS: [Tier; 2] = [Tier::Fast, Tier::Advanced];

pub(super) fn page(context: &Context) -> Page {
    let config = context.config();
    let statuses = context
        .client()
        .and_then(|mut client| client.providers_status().ok())
        .map(|status| status.providers)
        .unwrap_or_default();
    let rows = TIERS
        .iter()
        .map(|&tier| {
            let name = config.tiers.name(tier);
            let provider = match config.provider(tier) {
                Ok(provider) => provider,
                Err(problem) => return format!("{} tier: {problem}", capital(tier)),
            };
            let model = provider.model.as_deref().unwrap_or("its default model");
            let health = statuses
                .iter()
                .find(|status| status.name == name)
                .map(|status| format!(". {}", status.problem.as_deref().unwrap_or("OK")))
                .unwrap_or_default();
            format!(
                "{} tier: {name}, {}, {model}{health}",
                capital(tier),
                provider.kind.name()
            )
        })
        .collect();
    Page {
        actions: vec![('c', "runs a real check")],
        ..Page::new(rows)
    }
    .needing_lucyd()
}

fn capital(tier: Tier) -> &'static str {
    match tier {
        Tier::Fast => "Fast",
        Tier::Advanced => "Advanced",
    }
}

/// Picks the Provider and model the tier on `row` thinks with.
pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
    let tier = TIERS[row];
    let (options, missing): (Vec<_>, Vec<_>) = RECOMMENDATIONS
        .iter()
        .filter(|r| r.tier == tier)
        .partition(|r| on_path(&r.kind.config_name(), &context.env.path).is_some());
    if options.is_empty() {
        let clis: Vec<String> = missing.iter().map(|r| r.kind.config_name()).collect();
        return ui.say(format!(
            "No thinking CLI for the {tier} tier is installed: install {}, then pick it \
             here. Until then the {tier} tier stays as config.toml has it.",
            clis.join(" or ")
        ));
    }
    let labels: Vec<String> = options
        .iter()
        .map(|r| format!("{}, {}: {}", r.kind.name(), r.model, r.why))
        .collect();
    let chosen = options[ui.choose(&format!("The {tier} tier thinks with:"), &labels)?];
    let model = ui.ask("Model, or any the CLI accepts:", chosen.model)?;
    let kind = chosen.kind.config_name();
    let name = format!("{kind}-{tier}");
    context.save(
        ui,
        &[
            (Key::ProviderKind(name.clone()), Some(&kind)),
            (Key::ProviderModel(name.clone()), Some(&model)),
            (Key::Tier(tier), Some(&name)),
        ],
    )
}

/// A real check of the Provider the tier on `row` thinks with.
pub(super) fn check<B: Backend>(ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
    let Some(mut client) = context.client() else {
        return ui.say("lucyd isn't running, and the real check needs it. s starts it.");
    };
    let name = context.config().tiers.name(TIERS[row]).to_owned();
    ui.say(format!("Real check of {name}: asking it to reply OK..."))?;
    let outcome = match client.check_provider(&name) {
        Ok(result) => result.problem.unwrap_or_else(|| "it works.".to_owned()),
        Err(e) => e.to_string(),
    };
    ui.say(format!("Real check of {name}: {outcome}"))
}
