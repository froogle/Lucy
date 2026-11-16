//! `lucy input`: the Input dialog as a page, opened by lucyd in a terminal.
//! lucyd opens it while no other client holds the `input` role. It claims the role for one
//! dialog, shows the question and sends back what is typed.
#![expect(clippy::unwrap_used, reason = "test helpers unwrap")]

mod support;

use lucy::menu::Open;
use lucyd::{FieldKind, TypedAnswer};
use ratatui::crossterm::event::KeyCode;
use support::{Home, Script};

#[test]
fn the_page_shows_the_question_and_sends_back_what_is_typed() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url, vec![]);

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
fn esc_closes_the_box_unanswered() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url, vec![]);

    home.tui(Open::Input, Script::new().press(KeyCode::Esc));

    assert_eq!(asked.recv().unwrap(), TypedAnswer::Cancelled);
}

#[test]
fn the_page_goes_when_lucy_closes_the_box() {
    let home = Home::with_lucyd();
    let asked = home.ask_typed("Which page shall I open?", FieldKind::Url, vec![]);
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
    let asked = home.ask_typed("Which colour?", FieldKind::Choice, choices);

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
    let asked = home.ask_typed("How many?", FieldKind::Number, vec![]);

    let tui = home.tui(
        Open::Input,
        Script::new().typed("lots").enter().typed("12").enter(),
    );

    assert!(tui.shown.contains("isn't a number"), "{}", tui.shown);
    assert_eq!(asked.recv().unwrap(), TypedAnswer::Answer("12".to_owned()));
}

#[test]
fn with_another_holder_the_page_says_so() {
    let home = Home::with_lucyd();
    let _holder = home.env().connect_as(Some(lucy_ipc::Role::Input)).unwrap();

    let tui = home.tui(Open::Input, Script::new());

    assert!(tui.shown.contains("already"), "{}", tui.shown);
}
