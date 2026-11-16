use std::collections::BTreeMap;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;

use lucy_ipc::{error_code, Client, Event, Field, FieldKind, InputOpen, Role};
use ratatui::backend::Backend;

use crate::ui::{Stop, Ui};
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
    let typed = fields(ui, &open.fields);
    let mut client = watch.stop();
    let values = match typed {
        Ok(values) => values,
        Err(Stop::Closed) => return ui.say("Lucy closed the box."),
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
    let label = field.description.as_deref().unwrap_or(match field.kind {
        FieldKind::Text => "Your answer:",
        FieldKind::Url => "URL:",
        FieldKind::Number => "Number:",
        FieldKind::Choice => "Pick one:",
    });
    if field.kind == FieldKind::Choice {
        let picked = ui.choose(label, &field.choices)?;
        return Ok(field.choices[picked].clone());
    }
    loop {
        let typed = ui.ask(label, "")?;
        let typed = typed.trim();
        if typed.is_empty() {
            ui.say("Type an answer, or Esc to close the box.")?;
        } else if field.kind == FieldKind::Number && typed.parse::<f64>().is_err() {
            ui.say(format!("{typed} isn't a number."))?;
        } else {
            return Ok(typed.to_owned());
        }
    }
}
