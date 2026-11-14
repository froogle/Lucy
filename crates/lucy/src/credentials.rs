//! `lucy credentials <plugin>`: the user types each credential the Plugin's
//! Manifest declares, hidden. lucyd says which names it declares; the values
//! go straight to the Secret Service, never to lucyd, the screen or the
//! transcript. lucyd hands them to the Plugin when it next starts it.

use lucy_config::credentials::CredentialStore;
use ratatui::backend::Backend;

use crate::ui::Ui;
use crate::Env;

pub fn run<B: Backend>(
    ui: &mut Ui<B>,
    env: &Env,
    plugin: &str,
    store: &dyn CredentialStore,
) -> Result<(), String> {
    let listed = env.connect().map_err(|e| e.to_string())?.plugins();
    let names = listed
        .map_err(|e| e.to_string())?
        .plugins
        .into_iter()
        .find(|p| p.name == plugin)
        .ok_or_else(|| format!("there's no Plugin called {plugin}"))?
        .credentials;
    if names.is_empty() {
        ui.say(format!("The {plugin} Plugin declares no credentials."))?;
        return Ok(());
    }
    ui.say(format!(
        "The {plugin} Plugin needs {}. Each goes into your keyring; Lucy never sees it.",
        names.join(" and ")
    ))?;
    for name in &names {
        let entered = store.get(plugin, name)?.is_some();
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
        store.set(plugin, name, &value)?;
        ui.say(format!("{name} saved in your keyring."))?;
    }
    Ok(())
}
