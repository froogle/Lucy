//! With no client holding the `indicator` role, lucyd shows Session start,
//! Confirming and end as desktop notifications instead.

mod harness;

use harness::{next_state, Harness};
use lucy_ipc::State;
use lucyd::adapters::NotificationAction;

#[test]
fn session_start_without_an_indicator_shows_a_notification_with_an_end_action() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();

    client.start_session().unwrap();

    let shown = lucyd.notifier.next_notification();
    assert_eq!(shown.summary, "Lucy is listening");
    assert_eq!(shown.action, Some(NotificationAction::EndSession));
}

#[test]
fn session_end_without_an_indicator_shows_a_notification() {
    let lucyd = Harness::start();
    let mut client = lucyd.client();
    client.start_session().unwrap();
    lucyd.notifier.next_notification();

    client.end_session().unwrap();

    let shown = lucyd.notifier.next_notification();
    assert_eq!(shown.summary, "Lucy has stopped listening");
    assert_eq!(shown.action, None);
}

#[test]
fn the_end_action_ends_the_session() {
    let lucyd = Harness::start();
    let mut client = lucyd.state_client();
    client.start_session().unwrap();
    assert_eq!(next_state(&mut client).state, State::Listening);
    lucyd.notifier.next_notification();

    lucyd.notifier.pick_action();

    assert_eq!(next_state(&mut client).state, State::Idle);
}

#[test]
fn a_third_party_client_can_claim_the_indicator_role() {
    let lucyd = Harness::start();
    let indicator = lucyd.indicator("third-party-indicator");
    let mut client = lucyd.client();
    client.start_session().unwrap();
    client.end_session().unwrap();

    // Once the Indicator goes, the next Session is notified. Had the first
    // Session been notified too, its notifications would arrive first.
    drop(indicator);
    client.start_session().unwrap();

    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy is listening"
    );
    client.end_session().unwrap();
    assert_eq!(
        lucyd.notifier.next_notification().summary,
        "Lucy has stopped listening"
    );
}

#[test]
fn an_indicator_crash_mid_session_falls_back_to_a_notification() {
    let lucyd = Harness::start();
    let indicator = lucyd.indicator("lucy-indicator");
    let mut client = lucyd.client();
    client.start_session().unwrap();

    drop(indicator);

    let shown = lucyd.notifier.next_notification();
    assert_eq!(shown.summary, "Lucy is listening");
    assert_eq!(shown.action, Some(NotificationAction::EndSession));
}
