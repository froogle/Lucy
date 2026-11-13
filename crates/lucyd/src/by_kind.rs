//! The Thinking adapter for each Provider kind.

use std::sync::Arc;

use lucy_config::{Kind, Provider};
use tokio::sync::mpsc;

use crate::adapters::{Relay, SessionModel, Thinking, ThinkingEvent};

/// Hands each call to the adapter for the Provider's kind.
pub struct ByKind {
    pub claude: Arc<dyn Thinking>,
    pub codex: Arc<dyn Thinking>,
}

impl ByKind {
    fn adapter(&self, provider: &Provider) -> &dyn Thinking {
        match provider.kind {
            Kind::Claude => &*self.claude,
            Kind::Codex => &*self.codex,
        }
    }
}

impl Thinking for ByKind {
    fn open(
        &self,
        provider: &Provider,
        events: mpsc::UnboundedSender<ThinkingEvent>,
        relay: Relay,
        instructions: String,
    ) -> Box<dyn SessionModel> {
        self.adapter(provider)
            .open(provider, events, relay, instructions)
    }

    fn health(&self, provider: &Provider) -> Result<(), String> {
        self.adapter(provider).health(provider)
    }

    fn check(&self, provider: &Provider) -> Result<(), String> {
        self.adapter(provider).check(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::InertSessionModel;

    /// An adapter whose every check fails with its name.
    struct Named(&'static str);

    impl Thinking for Named {
        fn open(
            &self,
            _provider: &Provider,
            events: mpsc::UnboundedSender<ThinkingEvent>,
            _relay: Relay,
            _instructions: String,
        ) -> Box<dyn SessionModel> {
            let _ = events.send(ThinkingEvent::Unavailable(self.0.to_owned()));
            Box::new(InertSessionModel::new(events))
        }

        fn health(&self, _provider: &Provider) -> Result<(), String> {
            Err(self.0.to_owned())
        }

        fn check(&self, _provider: &Provider) -> Result<(), String> {
            Err(self.0.to_owned())
        }
    }

    fn provider(kind: Kind) -> Provider {
        Provider {
            kind,
            model: None,
            allow_api_key: false,
        }
    }

    #[test]
    fn each_provider_goes_to_the_adapter_for_its_kind() {
        let by_kind = ByKind {
            claude: Arc::new(Named("claude")),
            codex: Arc::new(Named("codex")),
        };

        for (kind, name) in [(Kind::Claude, "claude"), (Kind::Codex, "codex")] {
            assert_eq!(by_kind.health(&provider(kind)), Err(name.to_owned()));
            assert_eq!(by_kind.check(&provider(kind)), Err(name.to_owned()));
            let (tx, mut events) = mpsc::unbounded_channel();
            let relay = Relay {
                program: "lucy".into(),
                args: vec![],
            };
            let _model = by_kind.open(&provider(kind), tx, relay, String::new());
            assert_eq!(
                events.try_recv(),
                Ok(ThinkingEvent::Unavailable(name.to_owned()))
            );
        }
    }
}
