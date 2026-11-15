//! Entering a Plugin's credentials: the user types each one its Manifest
//! declares, hidden. lucyd says which names it declares; the values go
//! straight to the Secret Service, never to lucyd, the screen or the
//! transcript. lucyd hands them to the Plugin when it next starts it.

use lucy_config::credentials::CredentialStore;
use ratatui::backend::Backend;

use crate::ui::{Stop, Ui};

/// Asks for each of `names`, the credentials `plugin` declares, and keeps
/// what is typed in `store`.
pub(crate) fn enter<B: Backend>(
    ui: &mut Ui<B>,
    store: &dyn CredentialStore,
    plugin: &str,
    names: &[String],
) -> Result<(), Stop> {
    if names.is_empty() {
        return ui.say(format!("The {plugin} Plugin declares no credentials."));
    }
    ui.say(format!(
        "The {plugin} Plugin needs {}. Each goes into your keyring; Lucy never sees it.",
        names.join(" and ")
    ))?;
    for name in names {
        let entered = match store.get(plugin, name) {
            Ok(value) => value.is_some(),
            Err(e) => return ui.say(format!("Can't read your keyring: {e}")),
        };
        let question = match entered {
            true => format!("{name} (entered already; Enter alone keeps it):"),
            false => format!("{name}:"),
        };
        let value = ui.secret(&question)?;
        if value.is_empty() {
            ui.say(match entered {
                true => format!("{name} kept as it was."),
                false => format!("{name} left out; the Plugin can't be used without it."),
            })?;
            continue;
        }
        match store.set(plugin, name, &value) {
            Ok(()) => ui.say(format!("{name} saved in your keyring."))?,
            Err(e) => ui.say(format!("Can't save {name} in your keyring: {e}"))?,
        }
    }
    Ok(())
}
