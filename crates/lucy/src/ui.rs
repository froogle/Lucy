//! The full-screen terminal: where key presses come from, and the setup
//! screen. A [`Screen`] is the model: what has been said, and the question
//! being asked, which answers key presses. [`draw`] renders it. [`Ui`] ties
//! the two to a terminal and a [`Keys`] source, so setup reads as a series
//! of questions. Tests drive it with a [`Script`] and ratatui's
//! `TestBackend`.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListState, Paragraph};
use ratatui::{Frame, Terminal};

/// Why the questions stopped before setup finished.
#[derive(Debug)]
pub enum Stop {
    /// The user pressed Ctrl-C.
    Quit,
    Io(io::Error),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Quit => write!(f, "cancelled"),
            Stop::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Stop {
        Stop::Io(e)
    }
}

/// Where key presses come from.
pub trait Keys {
    /// The next key press, waiting for it; None when the screen needs
    /// drawing again (the terminal was resized).
    fn next(&mut self) -> io::Result<Option<KeyEvent>>;

    /// Whether Ctrl-C was pressed, without waiting. Other keys pressed
    /// meanwhile are dropped.
    fn interrupted(&mut self) -> io::Result<bool>;

    /// Runs `run` with the terminal back to normal, for a program that
    /// talks to the user itself (an installer asking for a password).
    fn outside(&mut self, run: &mut dyn FnMut()) -> io::Result<()> {
        run();
        Ok(())
    }
}

/// Whether `key` is Ctrl-C, which raw mode delivers as a key press.
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
pub struct Script(VecDeque<KeyEvent>);

impl Script {
    pub fn new() -> Script {
        Script::default()
    }

    pub fn press(mut self, code: KeyCode) -> Script {
        self.0.push_back(KeyEvent::new(code, KeyModifiers::NONE));
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

    pub fn ctrl_c(mut self) -> Script {
        self.0
            .push_back(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        self
    }

    /// Whether every key has been pressed.
    pub fn is_done(&self) -> bool {
        self.0.is_empty()
    }
}

impl Keys for Script {
    fn next(&mut self) -> io::Result<Option<KeyEvent>> {
        match self.0.pop_front() {
            Some(key) => Ok(Some(key)),
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the script ran out of key presses",
            )),
        }
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
}

/// What a question was answered with.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Index(usize),
    Yes(bool),
    Text(String),
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

    /// Takes a key press; the answer once there is one.
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
        }
    }

    /// The question, and the answer as the transcript keeps it.
    fn answered(&self, answer: &Answer) -> String {
        match (self, answer) {
            (Prompt::Choose { title, options, .. }, Answer::Index(i)) => {
                format!("{title} {}", options[*i])
            }
            (Prompt::Yes { question, .. }, Answer::Yes(yes)) => {
                format!("{question} {}", if *yes { "yes" } else { "no" })
            }
            (Prompt::Ask { question, .. }, Answer::Text(text)) => format!("{question} {text}"),
            _ => unreachable!("an answer of another kind"),
        }
    }

    /// Every piece of text the question puts on the screen.
    fn text(&self) -> Vec<String> {
        match self {
            Prompt::Choose { title, options, .. } => std::iter::once(title.clone())
                .chain(options.clone())
                .collect(),
            Prompt::Yes { question, .. } => vec![question.clone()],
            Prompt::Ask {
                question, default, ..
            } => vec![question.clone(), default.clone()],
        }
    }
}

/// The setup screen: a transcript of what was said and answered, and the
/// question or meter at the bottom.
#[derive(Debug, Default)]
pub struct Screen {
    pub title: String,
    pub lines: Vec<String>,
    pub prompt: Option<Prompt>,
    /// The microphone's level, 0 to 1, while the meter runs.
    pub meter: Option<f64>,
    /// Everything ever shown, for tests.
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

    /// Passes `key` to the question; once answered, the question leaves the
    /// screen and the transcript keeps it with its answer.
    pub fn key(&mut self, key: KeyEvent) -> Option<Answer> {
        let prompt = self.prompt.as_mut()?;
        let answer = prompt.key(key)?;
        let line = prompt.answered(&answer);
        self.prompt = None;
        self.lines.push(format!("> {line}"));
        Some(answer)
    }

    /// Everything shown so far, said or asked, one piece a line.
    pub fn shown(&self) -> String {
        self.shown.join("\n")
    }
}

/// Renders `screen` across the whole frame.
pub fn draw(frame: &mut Frame, screen: &Screen) {
    let bottom = match (&screen.prompt, screen.meter) {
        (Some(Prompt::Choose { options, .. }), _) => options.len() as u16 + 2,
        (Some(_), _) | (None, Some(_)) => 3,
        (None, None) => 0,
    };
    let [title, transcript, below, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(bottom),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(screen.title.as_str()).style(Style::new().add_modifier(Modifier::BOLD)),
        title,
    );
    draw_transcript(frame, transcript, &screen.lines);
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
                "Say something: the bar should move."
            }
            None => "Working...",
        },
    };
    frame.render_widget(
        Paragraph::new(format!("Ctrl-C quits   {keys}"))
            .style(Style::new().add_modifier(Modifier::DIM)),
        hint,
    );
}

/// The transcript, its last lines kept in view.
fn draw_transcript(frame: &mut Frame, area: Rect, lines: &[String]) {
    let rows: Vec<Line> = lines
        .iter()
        .flat_map(|line| wrap(line, area.width.max(1) as usize))
        .map(Line::raw)
        .collect();
    let first = rows.len().saturating_sub(area.height as usize);
    frame.render_widget(Paragraph::new(rows[first..].to_vec()), area);
}

/// `line` broken into rows of at most `width` characters, between words
/// where it can be.
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

/// Draws the question; the keys that answer it.
fn draw_prompt(frame: &mut Frame, prompt: &Prompt, area: Rect) -> &'static str {
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
            "Up/Down picks, Enter chooses (the first is suggested)."
        }
        Prompt::Yes { question, default } => {
            let hint = if *default { "[Y/n]" } else { "[y/N]" };
            frame.render_widget(
                Paragraph::new(format!("{question} {hint}")).block(Block::bordered()),
                area,
            );
            "y or n; Enter takes the capital."
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
            "Type, then Enter; empty takes the suggestion."
        }
    }
}

/// Questions asked full-screen, one at a time.
pub struct Ui<'k, B: Backend> {
    terminal: Terminal<B>,
    keys: &'k mut dyn Keys,
    pub screen: Screen,
}

impl<'k, B: Backend> Ui<'k, B> {
    pub fn new(terminal: Terminal<B>, keys: &'k mut dyn Keys, title: &str) -> Ui<'k, B> {
        Ui {
            terminal,
            keys,
            screen: Screen::new(title),
        }
    }

    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    fn draw(&mut self) -> Result<(), Stop> {
        let screen = &self.screen;
        self.terminal
            .draw(|frame| draw(frame, screen))
            .map_err(|e| Stop::Io(io::Error::other(e.to_string())))?;
        Ok(())
    }

    /// Adds a line to the transcript and shows it at once.
    pub fn say(&mut self, text: impl Into<String>) -> Result<(), Stop> {
        self.screen.say(text);
        self.draw()
    }

    fn answer(&mut self, prompt: Prompt) -> Result<Answer, Stop> {
        self.screen.ask(prompt);
        loop {
            self.draw()?;
            let Some(key) = self.keys.next()? else {
                continue;
            };
            if is_ctrl_c(&key) {
                return Err(Stop::Quit);
            }
            if let Some(answer) = self.screen.key(key) {
                self.draw()?;
                return Ok(answer);
            }
        }
    }

    /// The index of one of `options`, listed under `title`.
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

    /// Text typed in answer to `question`, `default` if none.
    pub fn ask(&mut self, question: &str, default: &str) -> Result<String, Stop> {
        match self.answer(Prompt::ask(question, default))? {
            Answer::Text(text) => Ok(text),
            _ => unreachable!(),
        }
    }

    /// Shows the meter at `level` (0 to 1), or takes it away.
    pub fn meter(&mut self, level: Option<f64>) -> Result<(), Stop> {
        self.screen.meter = level;
        self.draw()?;
        match self.keys.interrupted()? {
            true => Err(Stop::Quit),
            false => Ok(()),
        }
    }

    /// Runs `run` outside the full screen, then comes back to it.
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
        assert!(shown.contains("Ctrl-C quits"), "{shown}");
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
