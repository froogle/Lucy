use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

use lucy_ipc::{
    error_code, Client, Event, Field, FieldKind, InputAnswer, InputCancel, InputOpen, Role,
};
use ratatui::backend::Backend;
use serde::Deserialize;
use serde_json::Value;

use crate::ui::{Details, Stop, Ui};
use crate::Env;

pub(crate) fn page<B: Backend>(ui: &mut Ui<B>, env: &Env) -> Result<(), Stop> {
    let mut client = match env.connect_as(Some(Role::Input)) {
        Ok(client) => client,
        Err(lucy_ipc::Error::Rpc {
            code: error_code::ROLE_TAKEN,
            ..
        }) => return ui.say("Something else is showing Lucy's box already."),
        Err(e) => return ui.say(format!("Can't reach lucyd: {e}")),
    };
    let open = match next_open(&mut client) {
        Ok(open) => open,
        Err(e) => return ui.say(format!("Lucy didn't ask anything: {e}")),
    };
    ui.say(open.message.clone())?;
    let (closes, closed) = mpsc::channel();
    let watch = Watch::start(client, open.id.clone(), closes);
    ui.close_on(closed);
    let shown = show(ui, &open);
    let mut client = watch.stop();
    let values = match shown {
        Ok(Shown::Typed(values)) => values,
        Ok(Shown::Read) => return Ok(()),
        Err(Stop::Closed) => return ui.say("Lucy closed the box."),
        Err(stop) if open.fields.is_empty() => return Err(stop),
        Err(stop) => {
            let _ = client.cancel_input(&open.id);
            return Err(stop);
        }
    };
    match client.answer_input(&open.id, values) {
        Ok(()) => Ok(()),
        Err(e) => ui.say(format!("Lucy didn't take the answer: {e}")),
    }
}

enum Shown {
    Read,
    Typed(BTreeMap<String, String>),
}

fn show<B: Backend>(ui: &mut Ui<B>, open: &InputOpen) -> Result<Shown, Stop> {
    if open.fields.is_empty() {
        return ui.details(open.items.clone()).map(|()| Shown::Read);
    }
    ui.screen.details = (!open.items.is_empty()).then(|| Details::new(open.items.clone()));
    let typed = fields(ui, &open.fields);
    ui.screen.details = None;
    typed.map(Shown::Typed)
}

struct Watch {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<Client>,
}

impl Watch {
    fn start(mut client: Client, id: String, closes: mpsc::Sender<()>) -> Watch {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            while let Err(TryRecvError::Empty) = stopped.try_recv() {
                let closed = match client.next_event_within(WATCH_EVERY) {
                    Ok(Some(Event::InputClose(close))) => close.id == id,
                    Ok(_) => false,
                    Err(_) => true,
                };
                if closed {
                    let _ = closes.send(());
                    break;
                }
            }
            client
        });
        Watch { stop, thread }
    }

    fn stop(self) -> Client {
        drop(self.stop);
        self.thread.join().expect("the watch doesn't panic")
    }
}

const WATCH_EVERY: Duration = Duration::from_millis(50);

/// Holds the `input` role for a client that draws the box.
///
/// It prints each `input.open` and `input.close` lucyd sends to `out` as it
/// came, one JSON line each, and sends each `input.answer` or `input.cancel`
/// line read from `replies`. While lucyd is away or another client holds the
/// role, it tries again.
///
/// # Errors
///
/// When `out` can't be written. It returns `Ok` once `replies` ends.
pub fn hold(
    env: &Env,
    replies: impl BufRead + Send + 'static,
    out: &mut impl Write,
) -> io::Result<()> {
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in replies.lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                return;
            }
        }
    });
    loop {
        if let Ok(mut client) = env.connect_as(Some(Role::Input)) {
            if let Held::RepliesEnded = serve(&mut client, &lines, out)? {
                return Ok(());
            }
        }
        match lines.recv_timeout(HOLD_AGAIN) {
            Ok(_) => eprintln!("lucy: no box is open, so that reply went nowhere"),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

const HOLD_AGAIN: Duration = Duration::from_secs(1);

enum Held {
    LucydGone,
    RepliesEnded,
}

#[derive(Deserialize)]
#[serde(tag = "method", content = "params")]
enum Reply {
    #[serde(rename = "input.answer")]
    Answer(InputAnswer),
    #[serde(rename = "input.cancel")]
    Cancel(InputCancel),
}

fn serve(client: &mut Client, lines: &Receiver<String>, out: &mut impl Write) -> io::Result<Held> {
    loop {
        match client.next_event_line_within(WATCH_EVERY) {
            Ok(Some(line)) if is_input_event(&line) => {
                writeln!(out, "{line}")?;
                out.flush()?;
            }
            Ok(_) => {}
            Err(_) => return Ok(Held::LucydGone),
        }
        loop {
            match lines.try_recv() {
                Ok(line) => {
                    if let Err(lucy_ipc::Error::Io(_) | lucy_ipc::Error::Closed) =
                        reply(client, &line)
                    {
                        return Ok(Held::LucydGone);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(Held::RepliesEnded),
            }
        }
    }
}

fn is_input_event(line: &str) -> bool {
    let event: Value = serde_json::from_str(line).unwrap_or_default();
    matches!(event["method"].as_str(), Some("input.open" | "input.close"))
}

fn reply(client: &mut Client, line: &str) -> Result<(), lucy_ipc::Error> {
    let sent = match serde_json::from_str(line) {
        Ok(Reply::Answer(answer)) => client.answer_input(&answer.id, answer.values),
        Ok(Reply::Cancel(cancel)) => client.cancel_input(&cancel.id),
        Err(_) => {
            eprintln!("lucy: expected an input.answer or input.cancel line");
            return Ok(());
        }
    };
    if let Err(e) = &sent {
        eprintln!("lucy: lucyd didn't take that reply: {e}");
    }
    sent
}

fn next_open(client: &mut Client) -> Result<InputOpen, lucy_ipc::Error> {
    loop {
        if let Event::InputOpen(open) = client.next_event()? {
            return Ok(open);
        }
    }
}

fn fields<B: Backend>(ui: &mut Ui<B>, fields: &[Field]) -> Result<BTreeMap<String, String>, Stop> {
    fields
        .iter()
        .map(|field| Ok((field.name.clone(), value(ui, field)?)))
        .collect()
}

fn value<B: Backend>(ui: &mut Ui<B>, field: &Field) -> Result<String, Stop> {
    let label = field.description.as_deref().unwrap_or(match &field.kind {
        FieldKind::Text => "Your answer:",
        FieldKind::Url => "URL:",
        FieldKind::Number => "Number:",
        FieldKind::Choice { .. } => "Pick one:",
        FieldKind::Secret => "Secret:",
    });
    if let FieldKind::Choice { choices } = &field.kind {
        let choices = choices.as_slice();
        let picked = ui.choose(label, choices)?;
        let picked = choices
            .get(picked)
            .expect("ui.choose picks one of its options");
        return Ok(picked.clone());
    }
    loop {
        let typed = match field.kind {
            FieldKind::Secret => ui.secret(label)?,
            _ => ui.ask(label, "")?,
        };
        let typed = typed.trim();
        if typed.is_empty() {
            ui.say("Type an answer, or Esc to close the box.")?;
        } else if matches!(field.kind, FieldKind::Number) && typed.parse::<f64>().is_err() {
            ui.say(format!("{typed} isn't a number."))?;
        } else {
            return Ok(typed.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Script;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyCode;
    use ratatui::Terminal;

    fn open(fields: Vec<Field>) -> InputOpen {
        InputOpen {
            id: "1".to_owned(),
            message: "Trash these emails?".to_owned(),
            fields,
            items: vec![
                "**Ann** — Rent for December".to_owned(),
                "**Bob** — Lunch on Friday?".to_owned(),
            ],
        }
    }

    fn reason() -> Field {
        Field {
            name: "reason".to_owned(),
            description: Some("Why?".to_owned()),
            kind: FieldKind::Text,
        }
    }

    fn shown_with(keys: Script, open: &InputOpen) -> (Result<Shown, Stop>, String) {
        let mut keys = keys;
        let mut ui = Ui::new(
            Terminal::new(TestBackend::new(50, 16)).unwrap(),
            &mut keys,
            "Lucy",
        );
        ui.say(open.message.clone()).unwrap();
        let shown = show(&mut ui, open);
        let screen = ui.terminal().backend().to_string();
        drop(ui);
        assert!(keys.is_done(), "keys left over:\n{screen}");
        (shown, screen)
    }

    #[test]
    fn a_list_only_page_shows_the_items_and_esc_or_q_answers_nothing() {
        let (esc, screen) = shown_with(Script::new().press(KeyCode::Esc), &open(vec![]));
        let (q, _) = shown_with(Script::new().typed("q"), &open(vec![]));

        assert!(screen.contains("Ann — Rent for December"), "{screen}");
        assert!(screen.contains("Bob — Lunch on Friday?"), "{screen}");
        assert!(matches!(esc, Ok(Shown::Read)));
        assert!(matches!(q, Ok(Shown::Read)));
    }

    #[test]
    fn items_show_above_the_fields_while_q_j_and_k_are_typed() {
        let open = open(vec![reason()]);
        let keys = Script::new().typed("quick").press(KeyCode::Esc);

        let (stopped, screen) = shown_with(keys, &open);
        let rows: Vec<&str> = screen.lines().collect();
        let item = rows.iter().position(|row| row.contains("Bob")).unwrap();
        let field = rows.iter().position(|row| row.contains("Why?")).unwrap();

        assert!(item < field, "{screen}");
        assert!(screen.contains("quick"), "{screen}");
        assert!(matches!(stopped, Err(Stop::Back)));
    }

    #[test]
    fn the_fields_answer_with_items_above_them() {
        let keys = Script::new()
            .press(KeyCode::PageDown)
            .press(KeyCode::Down)
            .typed("junk")
            .enter();

        let (shown, _) = shown_with(keys, &open(vec![reason()]));

        let Ok(Shown::Typed(values)) = shown else {
            panic!("no answer")
        };
        assert_eq!(values["reason"], "junk");
    }
}
