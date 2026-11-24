use pulldown_cmark::{Event, HeadingLevel, LinkType, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) fn lines(items: &[String], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            rows.push(Line::styled("─".repeat(width), dim()));
        }
        let mut writer = Writer::new(width);
        for event in Parser::new(item) {
            writer.event(event);
        }
        writer.flush();
        rows.extend(writer.rows);
    }
    rows
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn code() -> Style {
    Style::new().fg(Color::Cyan)
}

#[derive(Default)]
struct Writer {
    width: usize,
    rows: Vec<Line<'static>>,
    line: Vec<Span<'static>>,
    styles: Vec<Style>,
    quotes: usize,
    lists: Vec<Option<u64>>,
    indents: Vec<usize>,
    marker: Option<String>,
    links: Vec<Option<String>>,
    gap: bool,
    in_code: bool,
}

impl Writer {
    fn new(width: usize) -> Writer {
        Writer {
            width,
            ..Writer::default()
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) if self.in_code => self.verbatim(&text, code()),
            Event::Html(text) => self.verbatim(&text, Style::new()),
            Event::Text(text) | Event::InlineHtml(text) => self.push(&text, self.style()),
            Event::Code(text) => self.push(&text, self.style().patch(code())),
            Event::SoftBreak => self.push(" ", self.style()),
            Event::HardBreak => self.row(),
            Event::Rule => {
                self.block();
                let room = self.width.saturating_sub(self.prefix_width()).max(1);
                self.push(&"─".repeat(room), dim());
                self.row();
                self.gap = true;
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph | Tag::HtmlBlock => self.block(),
            Tag::Heading { level, .. } => {
                self.block();
                let bold = Style::new().add_modifier(Modifier::BOLD);
                self.styles.push(match level {
                    HeadingLevel::H1 => bold.add_modifier(Modifier::UNDERLINED),
                    _ => bold,
                });
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.quotes += 1;
                self.styles
                    .push(Style::new().add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(_) => {
                self.block();
                self.in_code = true;
            }
            Tag::List(first) => {
                self.flush();
                if self.lists.is_empty() {
                    self.block();
                }
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                self.gap = false;
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "• ".to_owned(),
                };
                self.indents.push(marker.chars().count());
                self.marker = Some(marker);
            }
            Tag::Emphasis => self
                .styles
                .push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let shown_as_text = matches!(link_type, LinkType::Autolink | LinkType::Email);
                self.links
                    .push((!shown_as_text).then(|| dest_url.into_string()));
                self.styles
                    .push(Style::new().add_modifier(Modifier::UNDERLINED));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.end_block(),
            TagEnd::Heading(_) => {
                self.styles.pop();
                self.end_block();
            }
            TagEnd::BlockQuote(_) => {
                self.end_block();
                self.styles.pop();
                self.quotes -= 1;
            }
            TagEnd::CodeBlock => {
                self.in_code = false;
                self.end_block();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                self.gap = self.lists.is_empty();
            }
            TagEnd::Item => {
                match self.marker.is_some() && self.line.is_empty() {
                    true => self.row(),
                    false => self.flush(),
                }
                self.indents.pop();
                self.marker = None;
            }
            TagEnd::Emphasis | TagEnd::Strong => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                let url = self.links.pop().expect("a link ends after it starts");
                if let Some(url) = url {
                    self.push(&format!(" [{url}]"), dim());
                }
            }
            _ => {}
        }
    }

    fn style(&self) -> Style {
        self.styles
            .iter()
            .fold(Style::new(), |style, patch| style.patch(*patch))
    }

    fn push(&mut self, text: &str, style: Style) {
        self.line.push(Span::styled(text.to_owned(), style));
    }

    fn verbatim(&mut self, text: &str, style: Style) {
        for part in text.split_inclusive('\n') {
            match part.strip_suffix('\n') {
                Some(line) => {
                    self.push(line, style);
                    self.row();
                }
                None => self.push(part, style),
            }
        }
    }

    fn block(&mut self) {
        self.flush();
        if self.gap && !self.rows.is_empty() {
            self.row();
        }
        self.gap = false;
    }

    fn end_block(&mut self) {
        self.flush();
        self.gap = true;
    }

    fn flush(&mut self) {
        if !self.line.is_empty() {
            self.row();
        }
    }

    fn row(&mut self) {
        let hang = self.hang();
        let first = match self.marker.take() {
            Some(marker) => {
                let outer = self.indents.iter().rev().skip(1).sum::<usize>();
                let mut first = self.quote_bars();
                first.push(Span::raw(" ".repeat(outer)));
                first.push(Span::raw(marker));
                first.extend(self.code_indent());
                first
            }
            None => hang.clone(),
        };
        let spans = std::mem::take(&mut self.line);
        self.rows.extend(wrap(first, hang, &spans, self.width));
    }

    fn hang(&self) -> Vec<Span<'static>> {
        let mut hang = self.quote_bars();
        hang.push(Span::raw(" ".repeat(self.indents.iter().sum())));
        hang.extend(self.code_indent());
        hang
    }

    fn quote_bars(&self) -> Vec<Span<'static>> {
        vec![Span::styled("│ ".repeat(self.quotes), dim())]
    }

    fn code_indent(&self) -> Option<Span<'static>> {
        self.in_code.then(|| Span::raw("  "))
    }

    fn prefix_width(&self) -> usize {
        width_of(&self.hang())
    }
}

fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn wrap(
    first: Vec<Span<'static>>,
    hang: Vec<Span<'static>>,
    spans: &[Span<'static>],
    width: usize,
) -> Vec<Line<'static>> {
    let cells: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
        .collect();
    let mut rows = Vec::new();
    let mut prefix = first;
    let mut start = 0;
    loop {
        let room = width.saturating_sub(width_of(&prefix)).max(1);
        if cells.len() - start <= room {
            rows.push(row(prefix, &cells[start..]));
            return rows;
        }
        let end = start + room;
        match (start + 1..=end).rev().find(|&i| cells[i].0 == ' ') {
            Some(space) => {
                rows.push(row(prefix, &cells[start..space]));
                start = space + 1;
            }
            None => {
                rows.push(row(prefix, &cells[start..end]));
                start = end;
            }
        }
        prefix = hang.clone();
    }
}

fn row(prefix: Vec<Span<'static>>, cells: &[(char, Style)]) -> Line<'static> {
    let mut text: Vec<Span<'static>> = Vec::new();
    for (c, style) in cells {
        match text.last_mut() {
            Some(span) if span.style == *style => span.content.to_mut().push(*c),
            _ => text.push(Span::styled(c.to_string(), *style)),
        }
    }
    Line::from([prefix, text].concat())
}
