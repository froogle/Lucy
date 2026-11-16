//! An Input dialog field on the wire: its kind, and a choice's options.

use lucy_ipc::{Field, FieldKind};
use serde_json::json;

#[test]
fn a_choice_field_carries_its_options_beside_its_kind() {
    let wire = json!({"name": "answer", "kind": "choice", "choices": ["red", "blue"]});

    let field: Field = serde_json::from_value(wire.clone()).unwrap();

    let FieldKind::Choice { choices } = &field.kind else {
        panic!("not a choice: {field:?}");
    };
    assert_eq!(choices.as_slice(), ["red", "blue"]);
    assert_eq!(serde_json::to_value(&field).unwrap(), wire);
}

#[test]
fn a_text_field_is_its_kind_alone() {
    let wire = json!({"name": "answer", "kind": "url"});

    let field: Field = serde_json::from_value(wire.clone()).unwrap();

    assert_eq!(field.kind, FieldKind::Url);
    assert_eq!(serde_json::to_value(&field).unwrap(), wire);
}

#[test]
fn a_choice_with_nothing_to_pick_is_refused() {
    let wire = json!({"name": "answer", "kind": "choice", "choices": []});

    assert!(serde_json::from_value::<Field>(wire).is_err());
}
