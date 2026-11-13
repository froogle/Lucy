//! What `lucy` shows with no arguments: each Provider's health, with a real
//! check on request, and the Approvals Lucy holds, with revoke. Approvals
//! are granted only by voice, so nothing here grants one.

use lucy_ipc::{Client, ProviderKind, Tier};

use crate::ui::Ui;

pub fn run(client: &mut Client, ui: &mut Ui) -> Result<(), lucy_ipc::Error> {
    loop {
        let providers = client.providers_status()?.providers;
        let approvals = client.approvals()?.approvals;
        ui.say("");
        ui.say("Providers");
        for (i, provider) in providers.iter().enumerate() {
            let kind = match provider.kind {
                ProviderKind::Claude => "Claude",
                ProviderKind::Codex => "Codex",
            };
            let model = provider.model.as_deref().unwrap_or("its default model");
            let tiers: Vec<&str> = provider.tiers.iter().map(|t| tier_name(*t)).collect();
            let tiers = match tiers.is_empty() {
                true => String::new(),
                false => format!(", {} tier", tiers.join(" and ")),
            };
            let health = provider.problem.as_deref().unwrap_or("OK");
            ui.say(format!(
                "  {}. {}: {kind}, {model}{tiers}. {health}",
                i + 1,
                provider.name
            ));
        }
        ui.say("");
        ui.say("Approvals (granted only by voice, in a Session)");
        if approvals.is_empty() {
            ui.say("  none");
        }
        for (i, approval) in approvals.iter().enumerate() {
            let hash: String = approval.hash.chars().take(12).collect();
            ui.say(format!("  {}. {} ({hash})", i + 1, approval.plugin));
        }
        ui.say("");
        let Some(answer) =
            ui.ask("c<N> runs a real check of Provider N, r<N> revokes Approval N, q quits:")
        else {
            return Ok(());
        };
        let mut chars = answer.chars();
        let action = chars.next();
        let pick = chars.as_str().trim().parse::<usize>().ok();
        let pick = pick.and_then(|n| n.checked_sub(1));
        match (action, pick) {
            (Some('q'), _) => return Ok(()),
            (Some('c'), Some(i)) if i < providers.len() => {
                let name = &providers[i].name;
                ui.say(format!("Real check of {name}: asking it to reply OK..."));
                match client.check_provider(name)?.problem {
                    None => ui.say(format!("{name} works.")),
                    Some(problem) => ui.say(problem),
                }
            }
            (Some('r'), Some(i)) if i < approvals.len() => {
                let plugin = &approvals[i].plugin;
                let prompt = format!(
                    "Revoke the {plugin} Plugin's Approval? Its tools go at once, even mid-Session."
                );
                if ui.yes(&prompt, false) {
                    client.revoke_approval(plugin)?;
                    ui.say(format!(
                        "Revoked. Lucy will ask again before using {plugin}."
                    ));
                }
            }
            _ => ui.say("Type c and a Provider's number, r and an Approval's number, or q."),
        }
    }
}

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Fast => "fast",
        Tier::Advanced => "advanced",
    }
}
