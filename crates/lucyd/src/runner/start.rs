use std::path::PathBuf;
use std::sync::Arc;

use lucy_config::{Config, Provider, Tier};
use tokio::sync::mpsc;

use super::Runner;
use crate::adapters::{InertSessionModel, Relay, SessionModel, ThinkingEvent};
use crate::audio::Devices;
use crate::desktop::Desktop;
use crate::mcp::{Caller, Token};
use crate::notifications;
use crate::persona::{Loaded, Persona};
use crate::plugins::SessionTools;

/// The user's files read at each Session start.
pub struct SessionFiles {
    pub persona: Persona,
    pub config_path: PathBuf,
}

pub struct Started {
    pub model: Box<dyn SessionModel>,
    pub token: Token,
    pub settings: Config,
    pub first_run: bool,
    pub opened: Option<Opened>,
    pub devices: Devices,
}

pub struct Opened {
    pub provider: Provider,
    pub relay: Relay,
    pub instructions: String,
}

impl Runner {
    pub(super) fn start(
        &mut self,
        plugin_tools: Arc<SessionTools>,
        events: mpsc::UnboundedSender<ThinkingEvent>,
    ) -> Started {
        let persona = self.files.persona.load().unwrap_or_else(|e| {
            let path = self.files.persona.path().display();
            eprintln!("lucyd: can't load {path}: {e}");
            Loaded::fallback()
        });
        let config = lucy_config::load(&self.files.config_path);
        let settings = config.as_ref().cloned().unwrap_or_default();
        let devices = match &mut self.adapters.hearing {
            Some(hearing) => {
                hearing.configure(&settings.audio, &settings.endpointing, &settings.barge_in)
            }
            None => Devices::as_configured(&settings.audio, &settings.barge_in),
        };
        if !devices.missing.is_empty() {
            self.notify(notifications::devices_missing(&devices.missing));
        }
        let desktop = Desktop {
            launcher: self.adapters.launcher.clone(),
            applications: self.adapters.applications.clone(),
            launcher_command: settings.desktop.launcher.clone(),
        };
        let mut instructions = persona.text;
        if let Some(report) = plugin_tools.scan_report() {
            instructions = format!("{instructions}\n\n{report}");
        }
        let (relay, token) = self.relays.issue(Caller::SessionModel {
            plugins: plugin_tools,
            desktop,
        });
        let provider = match &config {
            Ok(config) => config.provider(Tier::Fast),
            Err(e) => Err(format!(
                "I {}. Fix the file and wake me again.",
                e.to_string().lines().next().unwrap_or_default()
            )),
        };
        let (model, opened) = match provider {
            Ok(provider) => {
                let opened = Opened {
                    provider: provider.clone(),
                    relay,
                    instructions,
                };
                (self.open_model(&opened, events), Some(opened))
            }
            Err(reason) => {
                eprintln!("lucyd: {reason}");
                let _ = events.send(ThinkingEvent::Unavailable(reason));
                let model: Box<dyn SessionModel> = Box::new(InertSessionModel::new(events));
                (model, None)
            }
        };
        Started {
            model,
            token,
            settings,
            first_run: persona.first_run,
            opened,
            devices,
        }
    }

    pub(super) fn open_model(
        &self,
        opened: &Opened,
        events: mpsc::UnboundedSender<ThinkingEvent>,
    ) -> Box<dyn SessionModel> {
        self.adapters.thinking.open(
            &opened.provider,
            events,
            opened.relay.clone(),
            opened.instructions.clone(),
        )
    }
}
