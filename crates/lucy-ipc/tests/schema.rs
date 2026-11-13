//! The committed JSON Schema must match the one generated from the types.
//! Regenerate it with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`.

const COMMITTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/lucy-ipc.schema.json");

#[test]
fn committed_schema_matches_the_types() {
    let generated = serde_json::to_string_pretty(&lucy_ipc::schema()).unwrap() + "\n";

    if std::env::var_os("LUCY_UPDATE_SCHEMA").is_some() {
        std::fs::write(COMMITTED, &generated).unwrap();
        return;
    }

    let committed = std::fs::read_to_string(COMMITTED).unwrap_or_default();
    assert!(
        committed == generated,
        "{COMMITTED} is out of date with the lucy-ipc types; \
         regenerate it with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`"
    );
}

#[test]
fn schema_describes_every_method_and_event() {
    let schema = lucy_ipc::schema();

    for method in [
        "hello",
        "status",
        "subscribe",
        "session.start",
        "session.end",
        "providers.status",
        "provider.check",
    ] {
        assert!(
            schema["x-methods"][method].is_object(),
            "missing method {method}"
        );
    }
    assert!(schema["x-events"]["state"].is_object());
}
