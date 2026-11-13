//! A Session's start: the Persona and `config.toml` are read afresh, so
//! edits apply at the next Session, and the model is opened with them.

use std::path::PathBuf;
use std::sync::Arc;

use lucy_config::{Config, Tier};
use tokio::sync::mpsc;

use super::Runner;
use crate::adapters::{InertSessionModel, SessionModel, ThinkingEvent};
use crate::desktop::Desktop;
use crate::mcp::{Caller, Token};
use crate::persona::{Loaded, Persona};
use crate::plugins::SessionTools;

/// The user's files read at each Session start.
pub struct SessionFiles {
    pub persona: Persona,
    /// `config.toml`. It need not exist.
    pub config_path: PathBuf,
}

/// A Session's model and settings, ready to run.
pub struct Started {
    pub model: Box<dyn SessionModel>,
    /// Keeps the model's relay working until the Session ends.
    pub token: Token,
    /// The config, or the defaults if it is invalid.
    pub settings: Config,
    /// The user hasn't been introduced to Lucy yet.
    pub first_run: bool,
}

impl Runner {
    /// Reads the Persona and config, applies the settings read at each
    /// Session start, and opens the model with the fast tier's Provider. If
    /// there is no usable Provider, the model says why on `events`.
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
        if let Some(hearing) = &mut self.adapters.hearing {
            hearing.configure(&settings.audio, &settings.endpointing);
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
        let (relay, token) = self
            .relays
            .issue(Caller::SessionModel, plugin_tools, desktop);
        let provider = match &config {
            Ok(config) => config.provider(Tier::Fast),
            Err(e) => Err(format!(
                "I {}. Fix the file and wake me again.",
                e.to_string().lines().next().unwrap_or_default()
            )),
        };
        let model = match provider {
            Ok(provider) => self
                .adapters
                .thinking
                .open(provider, events, relay, instructions),
            Err(reason) => {
                eprintln!("lucyd: {reason}");
                let _ = events.send(ThinkingEvent::Unavailable(reason));
                Box::new(InertSessionModel::new(events))
            }
        };
        Started {
            model,
            token,
            settings,
            first_run: persona.first_run,
        }
    }
}
