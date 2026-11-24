//! `lucy input`: the Input dialog as a page, opened by lucyd in a terminal.
//! lucyd opens it while no other client holds the `input` role. It claims the role for one
//! dialog, shows the question and sends back what is typed. `lucy input --json` holds the role
//! for a client that draws the box, such as the bar panel, as JSON lines on stdin and stdout.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use std::io::{BufRead, BufReader, Lines, Write};
use std::process::{Child, ChildStdout};
use std::time::{Duration, Instant};

use lucy::menu::Open;
use lucyd::{Choices, FieldKind, TypedAnswer};
use ratatui::crossterm::event::KeyCode;
use serde_json::{json, Value};
use support::{Home, ParentStdin, Script};

#[test]
fn the_page_shows_the_question_and_sends_back_what_is_typed() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);

    let tui = home.tui(
        Open::Input,
        Script::new().typed("https://example.com/a").enter(),
    );

    assert_eq!(tui.result, Ok(()));
    assert!(
        tui.shown.contains("Which page shall I open?"),
        "{}",
        tui.shown
    );
    assert_eq!(
        asked.recv().unwrap(),
        TypedAnswer::Answer("https://example.com/a".to_owned())
    );
}

#[test]
fn the_page_shows_the_items_lucyd_sends_above_the_box() {
    let home = Home::with_lucyd();
    let items = vec!["**Ann** sent *Rent for December*".to_owned()];
    let asked = home.ask_typed_about("Why trash it?", FieldKind::Text, items);

    let esc_mid_entry = Script::new().typed("spam").press(KeyCode::Esc);
    let tui = home.tui(Open::Input, esc_mid_entry);

    let rows: Vec<&str> = tui.screen.lines().collect();
    let item = rows
        .iter()
        .position(|row| row.contains("Ann sent Rent for December"));
    let field = rows.iter().position(|row| row.contains("spam"));
    assert!(item.is_some() && item < field, "{}", tui.screen);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn esc_closes_the_box_unanswered() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);

    home.tui(Open::Input, Script::new().press(KeyCode::Esc));

    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn the_page_goes_when_lucy_closes_the_box() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);
    let env = home.env();
    let click_the_indicator = move || env.connect().unwrap().end_session().unwrap();

    let tui = home.tui(Open::Input, Script::new().then(click_the_indicator).idle());

    assert_eq!(tui.result, Ok(()));
    assert!(tui.shown.contains("Lucy closed the box"), "{}", tui.shown);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn a_choice_is_picked_from_its_options() {
    let home = Home::with_lucyd();
    let choices = vec!["red".to_owned(), "blue".to_owned()];
    let choices = FieldKind::Choice {
        choices: Choices::try_from(choices).unwrap(),
    };
    let asked = home.ask_typed("Which colour?", choices);

    let tui = home.tui(Open::Input, Script::new().down(1).enter());

    assert!(tui.shown.contains("red"), "{}", tui.shown);
    assert_eq!(
        asked.recv().unwrap(),
        TypedAnswer::Answer("blue".to_owned())
    );
}

#[test]
fn a_number_is_asked_again_until_it_is_one() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("How many?", FieldKind::Number);

    let tui = home.tui(
        Open::Input,
        Script::new().typed("lots").enter().typed("12").enter(),
    );

    assert!(tui.shown.contains("isn't a number"), "{}", tui.shown);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Answer("12".to_owned()));
}

#[test]
fn a_secret_is_hidden_as_it_is_typed() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Your LINEAR_KEY?", FieldKind::Secret);

    let esc_mid_entry = Script::new().typed("lin-s3cret").press(KeyCode::Esc);
    let tui = home.tui(Open::Input, esc_mid_entry);

    assert!(tui.screen.contains("Your LINEAR_KEY?"), "{}", tui.screen);
    assert!(!tui.screen.contains("lin-s3cret"), "{}", tui.screen);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn a_secret_typed_is_sent_back() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Your LINEAR_KEY?", FieldKind::Secret);

    home.tui(Open::Input, Script::new().typed("lin-s3cret").enter());

    assert_eq!(
        asked.recv().unwrap(),
        TypedAnswer::Answer("lin-s3cret".to_owned())
    );
}

#[test]
fn with_another_holder_the_page_says_so() {
    let home = Home::with_lucyd();
    let _holder = home.env().connect_as(Some(lucy_ipc::Role::Input)).unwrap();

    let tui = home.tui(Open::Input, Script::new());

    assert!(tui.shown.contains("already"), "{}", tui.shown);
}

struct Holder {
    child: Child,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Holder {
    fn start(home: &Home) -> Holder {
        let mut child = home.command(&["input", "--json"]).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let lines = BufReader::new(stdout).lines();
        Holder { child, lines }
    }

    fn next(&mut self) -> Value {
        let line = self.lines.next().unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn send(&mut self, line: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
    }

    fn cancel(&mut self, open: &Value) {
        self.send(json!({
            "method": "input.cancel",
            "params": { "id": open["params"]["id"] },
        }));
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_holder_gets_the_question_as_json_and_answers_on_stdin() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);
    let mut holder = Holder::start(&home);

    let open = holder.next();
    assert_eq!(open["method"], "input.open", "{open}");
    assert_eq!(open["params"]["message"], "Which page shall I open?");
    assert_eq!(open["params"]["fields"][0]["kind"], "url");
    let field = open["params"]["fields"][0]["name"].as_str().unwrap();
    holder.send(json!({
        "method": "input.answer",
        "params": { "id": open["params"]["id"], "values": { field: "https://example.com/a" } },
    }));

    assert_eq!(
        asked.recv().unwrap(),
        TypedAnswer::Answer("https://example.com/a".to_owned())
    );
}

#[test]
fn a_holder_gets_a_secret_field_and_sends_its_value_back() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Your LINEAR_KEY?", FieldKind::Secret);
    let mut holder = Holder::start(&home);

    let open = holder.next();
    assert_eq!(open["params"]["fields"][0]["kind"], "secret", "{open}");
    let field = open["params"]["fields"][0]["name"].as_str().unwrap();
    holder.send(json!({
        "method": "input.answer",
        "params": { "id": open["params"]["id"], "values": { field: "lin-s3cret" } },
    }));

    assert_eq!(
        asked.recv().unwrap(),
        TypedAnswer::Answer("lin-s3cret".to_owned())
    );
}

#[test]
fn a_holder_cancels_on_stdin() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);
    let mut holder = Holder::start(&home);

    let open = holder.next();
    holder.cancel(&open);

    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn a_holder_hears_when_lucy_closes_the_box() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url);
    let mut holder = Holder::start(&home);

    let open = holder.next();
    home.env().connect().unwrap().end_session().unwrap();

    let close = holder.next();
    assert_eq!(close["method"], "input.close", "{close}");
    assert_eq!(close["params"]["id"], open["params"]["id"]);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn a_holder_keeps_the_role_for_the_next_question() {
    let home = Home::without_lucyd();
    let lucyd = home.spawn_lucyd();
    home.env().connect().unwrap().start_session().unwrap();
    let first = lucyd.ask_typed("Which page shall I open?", FieldKind::Url);
    let mut holder = Holder::start(&home);
    let open = holder.next();
    holder.cancel(&open);
    assert_eq!(first.blocking_recv().unwrap(), TypedAnswer::Cancelled);

    let _second = lucyd.ask_typed("How many?", FieldKind::Number);

    let open = holder.next();
    assert_eq!(open["params"]["message"], "How many?", "{open}");
}

#[test]
fn a_holder_waits_for_lucyd() {
    let home = Home::without_lucyd();
    let mut holder = Holder::start(&home);
    let lucyd = home.spawn_lucyd();

    home.env().connect().unwrap().start_session().unwrap();
    let _asked = lucyd.ask_typed("Which page shall I open?", FieldKind::Url);

    let open = holder.next();
    assert_eq!(
        open["params"]["message"], "Which page shall I open?",
        "{open}"
    );
}

#[test]
fn a_holder_lets_the_role_go_when_the_bar_that_ran_it_goes() {
    let home = Home::with_lucyd();
    let _asked = home.ask_typed("Which page shall I open?", FieldKind::Url);
    let mut bar = home.parented(&["input", "--json"], ParentStdin::Held);
    assert_eq!(bar.next()["method"], "input.open");

    bar.kill_parent();

    assert!(bar.exits_soon());
    assert!(role_claimed_soon(&home));
}

fn role_claimed_soon(home: &Home) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if home.env().connect_as(Some(lucy_ipc::Role::Input)).is_ok() {
            return true;
        }
        // sleep-ok: a poll loop with a deadline, while lucyd sees the holder's socket close
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}
