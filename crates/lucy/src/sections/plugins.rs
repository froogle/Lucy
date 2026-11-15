//! Plugins: each one's state, and entering the credentials it declares.

use lucy_ipc::{Client, PluginInfo, PluginStatus};
use ratatui::backend::Backend;

use super::{Context, Page};
use crate::credentials;
use crate::ui::{Stop, Ui};

fn plugins(context: &Context) -> Vec<PluginInfo> {
    context
        .query(Client::plugins)
        .map(|list| list.plugins)
        .unwrap_or_default()
}

pub fn row(context: &Context, name: &str) -> Option<usize> {
    plugins(context).iter().position(|p| p.name == name)
}

pub(super) fn page(context: &Context) -> Page {
    let rows = plugins(context)
        .iter()
        .map(|plugin| {
            let state = match (plugin.enabled, plugin.status) {
                (false, _) => "disabled".to_owned(),
                (true, PluginStatus::Approved) => "approved".to_owned(),
                (true, PluginStatus::AwaitingApproval) => "awaiting Approval".to_owned(),
                (true, PluginStatus::Broken) => match &plugin.problem {
                    Some(problem) => format!("broken: {problem}"),
                    None => "broken".to_owned(),
                },
            };
            let credentials = match plugin.credentials.is_empty() {
                true => "no credentials".to_owned(),
                false => format!("credentials: {}", entered(context, plugin)),
            };
            format!("{}: {state}; {credentials}", plugin.name)
        })
        .collect();
    Page::new(rows).needing_lucyd()
}

fn entered(context: &Context, plugin: &PluginInfo) -> String {
    plugin
        .credentials
        .iter()
        .map(|name| match context.store.get(&plugin.name, name) {
            Ok(Some(_)) => format!("{name} (entered)"),
            _ => format!("{name} (not entered)"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn pick<B: Backend>(ui: &mut Ui<B>, context: &Context, row: usize) -> Result<(), Stop> {
    let Some(plugin) = plugins(context).into_iter().nth(row) else {
        return Ok(());
    };
    credentials::enter(ui, context.store, &plugin.name, &plugin.credentials)
}
