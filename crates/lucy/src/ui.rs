//! The full-screen terminal: where key presses come from, and the screen
//! `lucy` draws. A [`Screen`] is the model: lucyd's status, what has been
//! said, and the question or list being shown, which answers key presses.
//! [`draw`] renders it. [`Ui`] ties the two to a terminal and a [`Keys`]
//! source, so a section reads as a series of questions. Tests drive it with
//! a [`Script`] and ratatui's `TestBackend`.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

const WATCH_EVERY: Duration = Duration::from_millis(50);

/// Why a question went unanswered.
#[derive(Debug)]
pub enum Stop {
    /// The user pressed Ctrl-C, or `q` where it isn't typed text.
    Quit,
    /// The user pressed Esc.
    Back,
    /// What [`Ui::close_on`] watches took the question away.
    Closed,
    Io(io::Error),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Quit => write!(f, "cancelled"),
            Stop::Back => write!(f, "went back"),
            Stop::Closed => write!(f, "closed"),
            Stop::Io(e) => write!(f, "{e}"),
        }
    }
}

pub(crate) fn back_is_fine(result: Result<(), Stop>) -> Result<(), Stop> {
    match result {
        Err(Stop::Back) => Ok(()),
        result => result,
    }
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Stop {
        Stop::Io(e)
    }
}

/// Where key presses come from.
pub trait Keys {
    /// The next key press, waiting for it.
    /// `None` when the screen needs drawing again (the terminal was resized).
    ///
    /// # Errors
    ///
    /// When the terminal can't be read.
    fn next(&mut self) -> io::Result<Option<KeyEvent>>;

    /// The next key press if one comes within `wait`.
    /// `None` when none did, or the screen needs drawing again.
    ///
    /// # Errors
    ///
    /// When the terminal can't be read.
    fn within(&mut self, wait: Duration) -> io::Result<Option<KeyEvent>>;

    /// Whether Ctrl-C was pressed, without waiting. Other keys pressed
    /// meanwhile are dropped.
    ///
    /// # Errors
    ///
    /// When the terminal can't be read.
    fn interrupted(&mut self) -> io::Result<bool>;

    /// Runs `run` with the terminal back to normal.
    /// It is for a program that talks to the user itself (an installer asking
    /// for a password).
    ///
    /// # Errors
    ///
    /// When the terminal can't be restored or taken back.
    fn outside(&mut self, run: &mut dyn FnMut()) -> io::Result<()> {
        run();
        Ok(())
    }
}

pub fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// The real terminal's keys.
pub struct Console;

impl Keys for Console {
    fn next(&mut self) -> io::Result<Option<KeyEvent>> {
        loop {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => return Ok(Some(key)),
                Event::Resize(..) => return Ok(None),
                _ => {}
            }
        }
    }

    fn within(&mut self, wait: Duration) -> io::Result<Option<KeyEvent>> {
        if !event::poll(wait)? {
            return Ok(None);
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => Ok(Some(key)),
            _ => Ok(None),
        }
    }

    fn interrupted(&mut self) -> io::Result<bool> {
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if is_ctrl_c(&key) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn outside(&mut self, run: &mut dyn FnMut()) -> io::Result<()> {
        ratatui::try_restore()?;
        run();
        ratatui::crossterm::terminal::enable_raw_mode()?;
        ratatui::crossterm::execute!(
            io::stdout(),
            ratatui::crossterm::terminal::EnterAlternateScreen
        )
    }
}

/// Key presses written down in advance, for tests. Running out while a
/// question waits is an error, so a test can't hang.
#[derive(Default)]
pub struct Script(VecDeque<Step>);

enum Step {
    Key(KeyEvent),
    Run(Box<dyn FnOnce()>),
    Idle,
}

impl Script {
    pub fn new() -> Script {
        Script::default()
    }

    pub fn press(mut self, code: KeyCode) -> Script {
        self.0
            .push_back(Step::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        self
    }

    pub fn enter(self) -> Script {
        self.press(KeyCode::Enter)
    }

    /// Enter, `n` times: each question takes its suggestion.
    pub fn enters(self, n: usize) -> Script {
        (0..n).fold(self, |script, _| script.enter())
    }

    pub fn down(self, n: usize) -> Script {
        (0..n).fold(self, |script, _| script.press(KeyCode::Down))
    }

    /// Each character of `text`, typed.
    pub fn typed(self, text: &str) -> Script {
        text.chars()
            .fold(self, |script, c| script.press(KeyCode::Char(c)))
    }

    /// Then the keys of `then`.
    pub fn chain(mut self, then: Script) -> Script {
        self.0.extend(then.0);
        self
    }

    pub fn ctrl_c(mut self) -> Script {
        self.0.push_back(Step::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        self
    }

    /// Runs `run` while a question waits for the next key.
    ///
    /// It stands in for something outside the terminal.
    pub fn then(mut self, run: impl FnOnce() + 'static) -> Script {
        self.0.push_back(Step::Run(Box::new(run)));
        self
    }

    /// Presses nothing more: a question that [`Ui::close_on`] watches waits
    /// until it is closed.
    pub fn idle(mut self) -> Script {
        self.0.push_back(Step::Idle);
        self
    }

    /// Whether every key has been pressed.
    pub fn is_done(&self) -> bool {
        self.0.iter().all(|step| matches!(step, Step::Idle))
    }

    fn step(
        &mut self,
        idle: impl FnOnce() -> io::Result<Option<KeyEvent>>,
    ) -> io::Result<Option<KeyEvent>> {
        match self.0.pop_front() {
            Some(Step::Key(key)) => Ok(Some(key)),
            Some(Step::Run(run)) => {
                run();
                Ok(None)
            }
            Some(Step::Idle) => {
                self.0.push_front(Step::Idle);
                idle()
            }
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the script ran out of key presses",
            )),
        }
    }
}

impl Keys for Script {
    fn next(&mut self) -> io::Result<Option<KeyEvent>> {
        self.step(|| {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the script is idle, and nothing watches for the question to close",
            ))
        })
    }

    fn within(&mut self, wait: Duration) -> io::Result<Option<KeyEvent>> {
        self.step(|| {
            std::thread::sleep(wait);
            Ok(None)
        })
    }

    fn interrupted(&mut self) -> io::Result<bool> {
        Ok(false)
    }
}

/// A question on the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
    /// One of `options`, the first suggested.
    Choose {
        title: String,
        options: Vec<String>,
        selected: usize,
    },
    Yes {
        question: String,
        default: bool,
    },
    /// Free text; empty takes `default`.
    Ask {
        question: String,
        default: String,
        typed: String,
    },
    /// Text typed hidden, never drawn or kept in the transcript.
    Secret {
        question: String,
        typed: String,
    },
    /// A page's list: Enter picks a row, a letter runs an action on it.
    Rows(Rows),
}

/// A page's list: a note, its rows and the letters that act on them.
/// The note is never picked; each letter says what it does, for the footer.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows {
    pub title: String,
    pub note: Vec<String>,
    pub rows: Vec<String>,
    pub actions: Vec<(char, &'static str)>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Index(usize),
    Yes(bool),
    Text(String),
    Pick(Pick),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick {
    Row(usize),
    Action(char, usize),
}

impl Prompt {
    pub fn choose(title: &str, options: &[String]) -> Prompt {
        Prompt::Choose {
            title: title.to_owned(),
            options: options.to_vec(),
            selected: 0,
        }
    }

    pub fn yes(question: &str, default: bool) -> Prompt {
        Prompt::Yes {
            question: question.to_owned(),
            default,
        }
    }

    pub fn ask(question: &str, default: &str) -> Prompt {
        Prompt::Ask {
            question: question.to_owned(),
            default: default.to_owned(),
            typed: String::new(),
        }
    }

    pub fn secret(question: &str) -> Prompt {
        Prompt::Secret {
            question: question.to_owned(),
            typed: String::new(),
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<Answer> {
        match self {
            Prompt::Choose {
                options, selected, ..
            } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(options.len() - 1);
                    None
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let n = c as usize - '1' as usize;
                    if n < options.len() {
                        *selected = n;
                    }
                    None
                }
                KeyCode::Enter => Some(Answer::Index(*selected)),
                _ => None,
            },
            Prompt::Yes { default, .. } => match key.code {
                KeyCode::Char('y' | 'Y') => Some(Answer::Yes(true)),
                KeyCode::Char('n' | 'N') => Some(Answer::Yes(false)),
                KeyCode::Enter => Some(Answer::Yes(*default)),
                _ => None,
            },
            Prompt::Ask { default, typed, .. } => match key.code {
                KeyCode::Char(c) => {
                    typed.push(c);
                    None
                }
                KeyCode::Backspace => {
                    typed.pop();
                    None
                }
                KeyCode::Enter => Some(Answer::Text(match typed.trim() {
                    "" => default.clone(),
                    text => text.to_owned(),
                })),
                _ => None,
            },
            Prompt::Secret { typed, .. } => match key.code {
                KeyCode::Char(c) => {
                    typed.push(c);
                    None
                }
                KeyCode::Backspace => {
                    typed.pop();
                    None
                }
                KeyCode::Enter => Some(Answer::Text(std::mem::take(typed))),
                _ => None,
            },
            Prompt::Rows(list) => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    list.selected = list.selected.saturating_sub(1);
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    list.selected = (list.selected + 1).min(list.rows.len().saturating_sub(1));
                    None
                }
                KeyCode::Enter if !list.rows.is_empty() => {
                    Some(Answer::Pick(Pick::Row(list.selected)))
                }
                KeyCode::Char(c) if list.actions.iter().any(|(letter, _)| *letter == c) => {
                    Some(Answer::Pick(Pick::Action(c, list.selected)))
                }
                _ => None,
            },
        }
    }

    fn is_typed(&self) -> bool {
        matches!(self, Prompt::Ask { .. } | Prompt::Secret { .. })
    }

    fn answered(&self, answer: &Answer) -> Option<String> {
        let line = match (self, answer) {
            (Prompt::Choose { title, options, .. }, Answer::Index(i)) => {
                format!("{title} {}", options[*i])
            }
            (Prompt::Yes { question, .. }, Answer::Yes(yes)) => {
                format!("{question} {}", if *yes { "yes" } else { "no" })
            }
            (Prompt::Ask { question, .. }, Answer::Text(text)) => format!("{question} {text}"),
            (Prompt::Secret { question, .. }, Answer::Text(text)) => match text.is_empty() {
                true => format!("{question} (nothing typed)"),
                false => format!("{question} (typed, hidden)"),
            },
            (Prompt::Rows(_), _) => return None,
            _ => unreachable!("an answer of another kind"),
        };
        Some(line)
    }

    fn text(&self) -> Vec<String> {
        match self {
            Prompt::Choose { title, options, .. } => std::iter::once(title.clone())
                .chain(options.clone())
                .collect(),
            Prompt::Yes { question, .. } => vec![question.clone()],
            Prompt::Ask {
                question, default, ..
            } => vec![question.clone(), default.clone()],
            Prompt::Secret { question, .. } => vec![question.clone()],
            Prompt::Rows(list) => std::iter::once(list.title.clone())
                .chain(list.note.clone())
                .chain(list.rows.clone())
                .collect(),
        }
    }
}

/// The screen: lucyd's status, a transcript, and a question, list or meter.
/// The transcript is what was said and answered.
#[derive(Debug, Default)]
pub struct Screen {
    pub title: String,
    pub status: String,
    pub lines: Vec<String>,
    pub prompt: Option<Prompt>,
    pub meter: Option<f64>,
    shown: Vec<String>,
}

impl Screen {
    pub fn new(title: &str) -> Screen {
        Screen {
            title: title.to_owned(),
            ..Screen::default()
        }
    }

    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.shown.push(text.clone());
        self.lines.push(text);
    }

    pub fn ask(&mut self, prompt: Prompt) {
        self.shown.extend(prompt.text());
        self.prompt = Some(prompt);
    }

    pub fn key(&mut self, key: KeyEvent) -> Option<Answer> {
        let prompt = self.prompt.as_mut()?;
        let answer = prompt.key(key)?;
        let line = prompt.answered(&answer);
        self.prompt = None;
        if let Some(line) = line {
            self.lines.push(format!("> {line}"));
        }
        Some(answer)
    }

    /// Everything shown so far, said or asked, one piece a line.
    pub fn shown(&self) -> String {
        self.shown.join("\n")
    }
}

pub fn draw(frame: &mut Frame, screen: &Screen) {
    let bottom = match (&screen.prompt, screen.meter) {
        (Some(Prompt::Choose { options, .. }), _) => options.len() as u16 + 2,
        (Some(Prompt::Rows(list)), _) => (list.note.len() + list.rows.len()) as u16 + 2,
        (Some(_), _) | (None, Some(_)) => 3,
        (None, None) => 0,
    };
    let [title, status, body, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(screen.title.as_str()).style(Style::new().add_modifier(Modifier::BOLD)),
        title,
    );
    frame.render_widget(Paragraph::new(screen.status.as_str()), status);
    let bottom = bottom.min(body.height);
    let rows: Vec<Line> = screen
        .lines
        .iter()
        .flat_map(|line| wrap(line, body.width.max(1) as usize))
        .map(Line::raw)
        .collect();
    let height = (rows.len() as u16).min(body.height - bottom);
    let (transcript, below) = match screen.prompt {
        Some(Prompt::Rows(_)) => {
            let [list, what_happened_after] = stacked(body, bottom, height);
            (what_happened_after, list)
        }
        _ => {
            let [what_led_to_it, question] = stacked(body, height, bottom);
            (what_led_to_it, question)
        }
    };
    let newest = &rows[rows.len() - height as usize..];
    frame.render_widget(Paragraph::new(newest.to_vec()), transcript);
    let keys = match &screen.prompt {
        Some(prompt) => draw_prompt(frame, prompt, below),
        None => match screen.meter {
            Some(level) => {
                frame.render_widget(
                    Gauge::default()
                        .block(Block::bordered().title("Microphone level"))
                        .ratio(level.clamp(0.0, 1.0))
                        .label(""),
                    below,
                );
                "Say something: the bar should move.".to_owned()
            }
            None => "Working...".to_owned(),
        },
    };
    let quit = match &screen.prompt {
        Some(prompt) if !prompt.is_typed() => "Esc goes back   q quits",
        Some(_) => "Esc goes back   Ctrl-C quits",
        None => "Ctrl-C quits",
    };
    frame.render_widget(
        Paragraph::new(format!("{keys}   {quit}")).style(Style::new().add_modifier(Modifier::DIM)),
        hint,
    );
}

fn stacked(area: Rect, top: u16, next: u16) -> [Rect; 2] {
    let [top, next, _] = Layout::vertical([
        Constraint::Length(top),
        Constraint::Length(next),
        Constraint::Fill(1),
    ])
    .areas(area);
    [top, next]
}

fn wrap(line: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    for word in line.split(' ') {
        let row = rows.last_mut().expect("a row");
        let used = row.chars().count();
        if used > 0 && used + 1 + word.chars().count() > width {
            rows.push(String::new());
        } else if used > 0 {
            row.push(' ');
        }
        for c in word.chars() {
            if rows.last().expect("a row").chars().count() == width {
                rows.push(String::new());
            }
            rows.last_mut().expect("a row").push(c);
        }
    }
    rows
}

fn draw_prompt(frame: &mut Frame, prompt: &Prompt, area: Rect) -> String {
    match prompt {
        Prompt::Choose {
            title,
            options,
            selected,
        } => {
            let list = List::new(options.iter().map(String::as_str))
                .block(Block::bordered().title(title.as_str()))
                .highlight_symbol("> ")
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_stateful_widget(list, area, &mut state);
            "Up/Down picks, Enter chooses".to_owned()
        }
        Prompt::Yes { question, default } => {
            let hint = if *default { "[Y/n]" } else { "[y/N]" };
            frame.render_widget(
                Paragraph::new(format!("{question} {hint}")).block(Block::bordered()),
                area,
            );
            "y or n; Enter takes the capital.".to_owned()
        }
        Prompt::Ask {
            question,
            default,
            typed,
        } => {
            let line = match typed.is_empty() {
                true => Line::from(Span::styled(
                    default.as_str(),
                    Style::new().add_modifier(Modifier::DIM),
                )),
                false => Line::raw(typed.as_str()),
            };
            frame.render_widget(
                Paragraph::new(line).block(Block::bordered().title(question.as_str())),
                area,
            );
            let x = area.x + 1 + typed.chars().count() as u16;
            frame.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y + 1));
            "Type, then Enter; empty takes the suggestion.".to_owned()
        }
        Prompt::Secret { question, typed } => {
            let stars = "*".repeat(typed.chars().count());
            frame.render_widget(
                Paragraph::new(stars).block(Block::bordered().title(question.as_str())),
                area,
            );
            let x = area.x + 1 + typed.chars().count() as u16;
            frame.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y + 1));
            "Type or paste it (hidden), then Enter.".to_owned()
        }
        Prompt::Rows(list) => {
            let dim = Style::new().add_modifier(Modifier::DIM);
            let items = list
                .note
                .iter()
                .map(|line| ListItem::new(line.as_str()).style(dim))
                .chain(list.rows.iter().map(|row| ListItem::new(row.as_str())));
            let widget = List::new(items)
                .block(Block::bordered().title(list.title.as_str()))
                .highlight_symbol("> ")
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
            let selected = (!list.rows.is_empty()).then_some(list.note.len() + list.selected);
            let mut state = ListState::default().with_selected(selected);
            frame.render_stateful_widget(widget, area, &mut state);
            let mut keys = "Up/Down picks   Enter opens".to_owned();
            for (letter, does) in &list.actions {
                keys.push_str(&format!("   {letter} {does}"));
            }
            keys
        }
    }
}

/// Questions asked full-screen, one at a time.
pub struct Ui<'k, B: Backend> {
    terminal: Terminal<B>,
    keys: &'k mut dyn Keys,
    closes: Option<mpsc::Receiver<()>>,
    pub screen: Screen,
}

impl<'k, B: Backend> Ui<'k, B> {
    pub fn new(terminal: Terminal<B>, keys: &'k mut dyn Keys, title: &str) -> Ui<'k, B> {
        Ui {
            terminal,
            keys,
            closes: None,
            screen: Screen::new(title),
        }
    }

    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// From now on, a question stops with [`Stop::Closed`] once `closes` receives.
    pub fn close_on(&mut self, closes: mpsc::Receiver<()>) {
        self.closes = Some(closes);
    }

    fn next_key(&mut self) -> Result<Option<KeyEvent>, Stop> {
        let Some(closes) = &self.closes else {
            return Ok(self.keys.next()?);
        };
        if closes.try_recv().is_ok() {
            return Err(Stop::Closed);
        }
        Ok(self.keys.within(WATCH_EVERY)?)
    }

    fn draw(&mut self) -> Result<(), Stop> {
        let screen = &self.screen;
        self.terminal
            .draw(|frame| draw(frame, screen))
            .map_err(|e| Stop::Io(io::Error::other(e.to_string())))?;
        Ok(())
    }

    pub fn say(&mut self, text: impl Into<String>) -> Result<(), Stop> {
        self.screen.say(text);
        self.draw()
    }

    fn answer(&mut self, prompt: Prompt) -> Result<Answer, Stop> {
        self.screen.ask(prompt);
        loop {
            self.draw()?;
            let Some(key) = self.next_key()? else {
                continue;
            };
            let typed = self.screen.prompt.as_ref().is_some_and(Prompt::is_typed);
            let stop = match key.code {
                _ if is_ctrl_c(&key) => Some(Stop::Quit),
                KeyCode::Char('q') if !typed => Some(Stop::Quit),
                KeyCode::Esc => Some(Stop::Back),
                _ => None,
            };
            if let Some(stop) = stop {
                self.screen.prompt = None;
                return Err(stop);
            }
            if let Some(answer) = self.screen.key(key) {
                self.draw()?;
                return Ok(answer);
            }
        }
    }

    pub fn rows(&mut self, mut rows: Rows) -> Result<Pick, Stop> {
        rows.selected = rows.selected.min(rows.rows.len().saturating_sub(1));
        match self.answer(Prompt::Rows(rows))? {
            Answer::Pick(pick) => Ok(pick),
            _ => unreachable!(),
        }
    }

    pub fn choose(&mut self, title: &str, options: &[String]) -> Result<usize, Stop> {
        match self.answer(Prompt::choose(title, options))? {
            Answer::Index(i) => Ok(i),
            _ => unreachable!(),
        }
    }

    pub fn yes(&mut self, question: &str, default: bool) -> Result<bool, Stop> {
        match self.answer(Prompt::yes(question, default))? {
            Answer::Yes(yes) => Ok(yes),
            _ => unreachable!(),
        }
    }

    pub fn ask(&mut self, question: &str, default: &str) -> Result<String, Stop> {
        match self.answer(Prompt::ask(question, default))? {
            Answer::Text(text) => Ok(text),
            _ => unreachable!(),
        }
    }

    pub fn secret(&mut self, question: &str) -> Result<String, Stop> {
        match self.answer(Prompt::secret(question))? {
            Answer::Text(text) => Ok(text),
            _ => unreachable!(),
        }
    }

    pub fn meter(&mut self, level: Option<f64>) -> Result<(), Stop> {
        self.screen.meter = level;
        self.draw()?;
        match self.keys.interrupted()? {
            true => Err(Stop::Quit),
            false => Ok(()),
        }
    }

    pub fn outside<T>(&mut self, run: impl FnOnce() -> T) -> Result<T, Stop> {
        let mut run = Some(run);
        let mut result = None;
        self.keys
            .outside(&mut || result = run.take().map(|run| run()))?;
        self.terminal
            .clear()
            .map_err(|e| Stop::Io(io::Error::other(e.to_string())))?;
        self.draw()?;
        Ok(result.expect("run once"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn options() -> Vec<String> {
        vec!["claude".into(), "codex".into(), "gemini".into()]
    }

    #[test]
    fn a_choice_moves_with_the_arrows_and_numbers_and_answers_on_enter() {
        let mut prompt = Prompt::choose("Thinks with:", &options());

        assert_eq!(prompt.key(key(KeyCode::Down)), None);
        assert_eq!(prompt.key(key(KeyCode::Down)), None);
        assert_eq!(prompt.key(key(KeyCode::Down)), None);
        assert_eq!(prompt.key(key(KeyCode::Enter)), Some(Answer::Index(2)));
        assert_eq!(prompt.key(key(KeyCode::Char('2'))), None);
        assert_eq!(prompt.key(key(KeyCode::Char('9'))), None);
        assert_eq!(prompt.key(key(KeyCode::Up)), None);
        assert_eq!(prompt.key(key(KeyCode::Up)), None);
        assert_eq!(prompt.key(key(KeyCode::Enter)), Some(Answer::Index(0)));
    }

    #[test]
    fn a_yes_or_no_takes_y_or_n_and_enter_takes_the_default() {
        let mut prompt = Prompt::yes("Install Piper now?", false);

        assert_eq!(prompt.key(key(KeyCode::Char('x'))), None);
        assert_eq!(prompt.key(key(KeyCode::Enter)), Some(Answer::Yes(false)));
        assert_eq!(prompt.key(key(KeyCode::Char('Y'))), Some(Answer::Yes(true)));
        assert_eq!(
            prompt.key(key(KeyCode::Char('n'))),
            Some(Answer::Yes(false))
        );
    }

    #[test]
    fn typed_text_can_be_corrected_and_empty_takes_the_default() {
        let mut prompt = Prompt::ask("Model:", "haiku");
        assert_eq!(
            prompt.key(key(KeyCode::Enter)),
            Some(Answer::Text("haiku".into()))
        );

        let mut prompt = Prompt::ask("Model:", "haiku");
        for c in "sonx".chars() {
            prompt.key(key(KeyCode::Char(c)));
        }
        prompt.key(key(KeyCode::Backspace));
        for c in "net".chars() {
            prompt.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            prompt.key(key(KeyCode::Enter)),
            Some(Answer::Text("sonnet".into()))
        );
    }

    #[test]
    fn the_transcript_keeps_each_answer() {
        let mut screen = Screen::new("Lucy setup");
        screen.ask(Prompt::choose("Speech-to-text:", &options()));

        screen.key(key(KeyCode::Down));
        let answer = screen.key(key(KeyCode::Enter));

        assert_eq!(answer, Some(Answer::Index(1)));
        assert_eq!(screen.prompt, None);
        assert_eq!(screen.lines, ["> Speech-to-text: codex"]);
    }

    fn rendered(screen: &Screen, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, screen)).unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn a_choice_is_drawn_with_every_option_and_the_pick_marked() {
        let mut screen = Screen::new("Lucy setup");
        screen.say("Claude, for thinking: claude found");
        screen.ask(Prompt::choose("The fast tier thinks with:", &options()));
        screen.key(key(KeyCode::Down));

        let shown = rendered(&screen, 60, 12);

        assert!(shown.contains("Lucy setup"), "{shown}");
        assert!(shown.contains("claude found"), "{shown}");
        assert!(shown.contains("The fast tier thinks with:"), "{shown}");
        assert!(shown.contains("> codex"), "{shown}");
        assert!(shown.contains("gemini"), "{shown}");
        assert!(shown.contains("Esc goes back   q quits"), "{shown}");
    }

    #[test]
    fn the_newest_lines_stay_in_view() {
        let mut screen = Screen::new("Lucy setup");
        for i in 0..40 {
            screen.say(format!("line {i} {}", "x".repeat(i % 3 * 30)));
        }
        screen.ask(Prompt::yes("Keep this microphone?", true));

        let shown = rendered(&screen, 40, 12);

        assert!(shown.contains("line 39"), "{shown}");
        assert!(shown.contains("Keep this microphone? [Y/n]"), "{shown}");
    }

    #[test]
    fn long_lines_wrap_between_words() {
        assert_eq!(
            wrap("say the wake word now", 9),
            ["say the", "wake word", "now"]
        );
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4), [""]);
    }

    #[test]
    fn the_meter_is_drawn_at_its_level() {
        let mut screen = Screen::new("Lucy setup");
        screen.meter = Some(0.5);

        let shown = rendered(&screen, 40, 8);

        assert!(shown.contains("Microphone level"), "{shown}");
        let bar = shown.lines().find(|l| l.contains('█')).unwrap_or_default();
        let filled = bar.matches('█').count();
        assert!((17..=20).contains(&filled), "{shown}");
    }

    #[test]
    fn ctrl_c_stops_the_questions() {
        let mut keys = Script::new().ctrl_c();
        let mut ui = Ui::new(
            Terminal::new(TestBackend::new(40, 10)).unwrap(),
            &mut keys,
            "Lucy setup",
        );

        assert!(matches!(
            ui.yes("Install Piper now?", false),
            Err(Stop::Quit)
        ));
    }

    #[test]
    fn running_out_of_scripted_keys_is_an_error_not_a_hang() {
        let mut keys = Script::new();
        let mut ui = Ui::new(
            Terminal::new(TestBackend::new(40, 10)).unwrap(),
            &mut keys,
            "Lucy setup",
        );

        assert!(matches!(
            ui.yes("Install Piper now?", false),
            Err(Stop::Io(_))
        ));
    }
}
