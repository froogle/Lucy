//! Approvals: the ones Lucy holds, each revoked after a yes. They are
//! granted only by voice, so nothing here grants one.

use ratatui::backend::Backend;

use lucy_ipc::Client;

use super::{Context, Page, REVOKE};
use crate::ui::{Stop, Ui};

fn approvals(context: &Context) -> Vec<lucy_ipc::ApprovalInfo> {
    context
        .query(Client::approvals)
        .map(|list| list.approvals)
        .unwrap_or_default()
}

pub(super) fn page(context: &Context) -> Page {
    let approvals = approvals(context);
    let mut note = vec!["Approvals are granted only by voice, in a Session.".to_owned()];
    if approvals.is_empty() && context.client().is_some() {
        note.push("Lucy holds none.".to_owned());
    }
    let rows = approvals
        .iter()
        .map(|approval| {
            let hash: String = approval.hash.chars().take(12).collect();
            format!("{} ({hash})", approval.plugin)
        })
        .collect();
    Page {
        note,
        actions: vec![REVOKE],
        ..Page::new(rows)
    }
    .needing_lucyd()
}

pub(super) fn revoke<B: Backend>(
    ui: &mut Ui<B>,
    context: &Context,
    row: usize,
) -> Result<(), Stop> {
    let Some(approval) = approvals(context).into_iter().nth(row) else {
        return Ok(());
    };
    let plugin = approval.plugin;
    let question =
        format!("Revoke the {plugin} Plugin's Approval? Its tools go at once, even mid-Session.");
    if !ui.yes(&question, false)? {
        return ui.say("Not revoked.");
    }
    let revoked = context
        .client()
        .ok_or_else(|| "lucyd isn't running".to_owned())
        .and_then(|mut client| client.revoke_approval(&plugin).map_err(|e| e.to_string()));
    match revoked {
        Ok(()) => ui.say(format!(
            "Revoked. Lucy will ask again before using {plugin}."
        )),
        Err(e) => ui.say(format!("Not revoked: {e}")),
    }
}
