//! What `lucy` shows with no arguments: each Provider's health, with a real
//! check on request, and the Approvals Lucy holds, with revoke. Approvals
//! are granted only by voice, so nothing here grants one.
//!
//! [`Page`] is the model, answering key presses with [`Action`]s; [`draw`]
//! renders it; [`run`] carries the actions out against lucyd.

use lucy_ipc::{ApprovalInfo as Approval, Client, ProviderKind, ProviderStatus, Tier};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, List, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::ui::{is_ctrl_c, Keys};

/// Which list the arrows move in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pane {
    Providers,
    Approvals,
}

/// What a key press asks lucyd for.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// A real check of the named Provider.
    Check(String),
    /// Revoke the named Plugin's Approval; the user has said yes.
    Revoke(String),
    Quit,
}

pub struct Page {
    pub providers: Vec<ProviderStatus>,
    pub approvals: Vec<Approval>,
    pub focus: Pane,
    provider: usize,
    approval: usize,
    /// The Approval waiting on a yes to be revoked.
    confirming: Option<usize>,
    /// The outcome of the last action.
    pub message: Option<String>,
}

impl Page {
    pub fn new(providers: Vec<ProviderStatus>, approvals: Vec<Approval>) -> Page {
        Page {
            providers,
            approvals,
            focus: Pane::Providers,
            provider: 0,
            approval: 0,
            confirming: None,
            message: None,
        }
    }

    /// Replaces the lists, keeping the selection in range.
    pub fn refresh(&mut self, providers: Vec<ProviderStatus>, approvals: Vec<Approval>) {
        self.provider = self.provider.min(providers.len().saturating_sub(1));
        self.approval = self.approval.min(approvals.len().saturating_sub(1));
        self.providers = providers;
        self.approvals = approvals;
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<Action> {
        if is_ctrl_c(&key) {
            return Some(Action::Quit);
        }
        if let Some(i) = self.confirming {
            self.confirming = None;
            return match key.code {
                KeyCode::Char('y' | 'Y') => Some(Action::Revoke(self.approvals[i].plugin.clone())),
                _ => {
                    self.message = Some("Not revoked.".to_owned());
                    None
                }
            };
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                self.focus = match self.focus {
                    Pane::Providers => Pane::Approvals,
                    Pane::Approvals => Pane::Providers,
                };
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let selected = self.selected_mut();
                *selected = selected.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = match self.focus {
                    Pane::Providers => self.providers.len(),
                    Pane::Approvals => self.approvals.len(),
                };
                let selected = self.selected_mut();
                *selected = (*selected + 1).min(len.saturating_sub(1));
                None
            }
            KeyCode::Char('c') => self.check(),
            KeyCode::Char('r') => self.ask_revoke(),
            KeyCode::Enter => match self.focus {
                Pane::Providers => self.check(),
                Pane::Approvals => self.ask_revoke(),
            },
            _ => None,
        }
    }

    fn selected_mut(&mut self) -> &mut usize {
        match self.focus {
            Pane::Providers => &mut self.provider,
            Pane::Approvals => &mut self.approval,
        }
    }

    fn check(&mut self) -> Option<Action> {
        self.focus = Pane::Providers;
        let provider = self.providers.get(self.provider)?;
        Some(Action::Check(provider.name.clone()))
    }

    fn ask_revoke(&mut self) -> Option<Action> {
        self.focus = Pane::Approvals;
        if self.approval < self.approvals.len() {
            self.confirming = Some(self.approval);
        }
        None
    }
}

/// Renders `page` across the whole frame.
pub fn draw(frame: &mut Frame, page: &Page) {
    let providers_height = page.providers.len() as u16 + 2;
    let [title, providers, approvals, message, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(providers_height),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new("Lucy").style(Style::new().add_modifier(Modifier::BOLD)),
        title,
    );

    let focused = |pane| match page.focus == pane {
        true => Style::new().add_modifier(Modifier::BOLD),
        false => Style::new().add_modifier(Modifier::DIM),
    };
    let rows: Vec<String> = page.providers.iter().map(provider_row).collect();
    let list = List::new(rows)
        .block(
            Block::bordered()
                .title("Providers")
                .border_style(focused(Pane::Providers)),
        )
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(
        (page.focus == Pane::Providers && !page.providers.is_empty()).then_some(page.provider),
    );
    frame.render_stateful_widget(list, providers, &mut state);

    let rows: Vec<String> = match page.approvals.is_empty() {
        true => vec!["none".to_owned()],
        false => page
            .approvals
            .iter()
            .map(|approval| {
                let hash: String = approval.hash.chars().take(12).collect();
                format!("{} ({hash})", approval.plugin)
            })
            .collect(),
    };
    let list = List::new(rows)
        .block(
            Block::bordered()
                .title("Approvals (granted only by voice, in a Session)")
                .border_style(focused(Pane::Approvals)),
        )
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(
        (page.focus == Pane::Approvals && !page.approvals.is_empty()).then_some(page.approval),
    );
    frame.render_stateful_widget(list, approvals, &mut state);

    let text = match page.confirming {
        Some(i) => format!(
            "Revoke the {} Plugin's Approval? Its tools go at once, even mid-Session. [y/N]",
            page.approvals[i].plugin
        ),
        None => page.message.clone().unwrap_or_default(),
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(Block::bordered()),
        message,
    );
    frame.render_widget(
        Paragraph::new("Tab switches list   Up/Down picks   c real check   r revoke   q quits")
            .style(Style::new().add_modifier(Modifier::DIM)),
        hint,
    );
}

fn provider_row(provider: &ProviderStatus) -> String {
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
    format!("{}: {kind}, {model}{tiers}. {health}", provider.name)
}

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Fast => "fast",
        Tier::Advanced => "advanced",
    }
}

/// Shows the page until the user quits, carrying out what they ask.
pub fn run<B: Backend>(
    client: &mut Client,
    terminal: &mut Terminal<B>,
    keys: &mut dyn Keys,
) -> Result<(), lucy_ipc::Error> {
    let mut page = Page::new(
        client.providers_status()?.providers,
        client.approvals()?.approvals,
    );
    let draw = |terminal: &mut Terminal<B>, page: &Page| {
        terminal
            .draw(|frame| draw(frame, page))
            .map(drop)
            .map_err(|e| lucy_ipc::Error::Io(std::io::Error::other(e.to_string())))
    };
    loop {
        draw(terminal, &page)?;
        let Some(key) = keys.next().map_err(lucy_ipc::Error::Io)? else {
            continue;
        };
        match page.key(key) {
            None => {}
            Some(Action::Quit) => return Ok(()),
            Some(Action::Check(name)) => {
                page.message = Some(format!("Real check of {name}: asking it to reply OK..."));
                draw(terminal, &page)?;
                let outcome = match client.check_provider(&name)?.problem {
                    None => "it works.".to_owned(),
                    Some(problem) => problem,
                };
                page.message = Some(format!("Real check of {name}: {outcome}"));
            }
            Some(Action::Revoke(plugin)) => {
                client.revoke_approval(&plugin)?;
                page.message = Some(format!(
                    "Revoked. Lucy will ask again before using {plugin}."
                ));
            }
        }
        page.refresh(
            client.providers_status()?.providers,
            client.approvals()?.approvals,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn page() -> Page {
        let provider = |name: &str, tier| ProviderStatus {
            name: name.to_owned(),
            kind: ProviderKind::Claude,
            model: None,
            tiers: vec![tier],
            problem: None,
        };
        let approval = |plugin: &str| Approval {
            plugin: plugin.to_owned(),
            hash: format!("{plugin}-hash"),
            previous_hash: None,
        };
        Page::new(
            vec![
                provider("claude-advanced", Tier::Advanced),
                provider("claude-fast", Tier::Fast),
            ],
            vec![approval("news"), approval("notes")],
        )
    }

    fn rendered(page: &Page) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|frame| draw(frame, page)).unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn c_checks_the_picked_provider() {
        let mut page = page();

        assert_eq!(page.key(key(KeyCode::Down)), None);
        assert_eq!(
            page.key(key(KeyCode::Char('c'))),
            Some(Action::Check("claude-fast".into()))
        );
        assert_eq!(
            page.key(key(KeyCode::Enter)),
            Some(Action::Check("claude-fast".into()))
        );
    }

    #[test]
    fn revoking_needs_a_yes() {
        let mut page = page();
        page.key(key(KeyCode::Tab));
        page.key(key(KeyCode::Down));

        assert_eq!(page.key(key(KeyCode::Char('r'))), None);
        assert!(rendered(&page).contains("Revoke the notes Plugin's Approval?"));
        assert_eq!(page.key(key(KeyCode::Char('n'))), None);
        assert!(rendered(&page).contains("Not revoked."));
        assert_eq!(page.key(key(KeyCode::Enter)), None);
        assert_eq!(
            page.key(key(KeyCode::Char('y'))),
            Some(Action::Revoke("notes".into()))
        );
    }

    #[test]
    fn no_key_grants_an_approval() {
        let mut page = page();
        page.key(key(KeyCode::Tab));

        for c in ['a', 'g', 'y', 'A'] {
            assert_eq!(page.key(key(KeyCode::Char(c))), None);
        }
        let shown = rendered(&page).to_lowercase();
        assert!(shown.contains("granted only by voice"), "{shown}");
        assert!(!shown.contains("approve"), "{shown}");
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        for quit in [
            key(KeyCode::Char('q')),
            key(KeyCode::Esc),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            assert_eq!(page().key(quit), Some(Action::Quit));
        }
    }

    #[test]
    fn with_no_approvals_the_list_says_none_and_r_does_nothing() {
        let mut page = page();
        page.refresh(page.providers.clone(), vec![]);

        assert_eq!(page.key(key(KeyCode::Char('r'))), None);
        assert_eq!(page.key(key(KeyCode::Char('y'))), None);
        assert!(rendered(&page).contains("none"));
    }
}
