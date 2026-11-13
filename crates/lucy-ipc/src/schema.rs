//! The JSON Schema of the public IPC, generated from the types.

use schemars::generate::SchemaSettings;
use serde_json::{json, Value};

use crate::{
    Hello, HelloResult, ProviderCheck, ProviderCheckResult, ProvidersStatus, SessionStarted,
    StateEvent, Status, Subscribe, PROTOCOL,
};

/// The schema committed as `schema/lucy-ipc.schema.json`. `x-methods` maps
/// each request to its params and result; `x-events` maps each event to its
/// params. Every message is JSON-RPC 2.0 on one NDJSON line.
pub fn schema() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let hello = generator.subschema_for::<Hello>();
    let hello_result = generator.subschema_for::<HelloResult>();
    let status = generator.subschema_for::<Status>();
    let subscribe = generator.subschema_for::<Subscribe>();
    let state_event = generator.subschema_for::<StateEvent>();
    let session_started = generator.subschema_for::<SessionStarted>();
    let providers_status = generator.subschema_for::<ProvidersStatus>();
    let provider_check = generator.subschema_for::<ProviderCheck>();
    let provider_check_result = generator.subschema_for::<ProviderCheckResult>();

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": format!("lucyd public IPC, protocol {PROTOCOL}"),
        "description": "Newline-delimited JSON-RPC 2.0 on $XDG_RUNTIME_DIR/lucy/lucyd.sock. \
            Send hello first. Within a major protocol version, fields are only ever added.",
        "x-methods": {
            "hello": {"params": hello, "result": hello_result},
            "status": {"result": status},
            "subscribe": {"params": subscribe, "result": {"type": "object"}},
            "session.start": {"result": session_started},
            "session.end": {"result": {"type": "object"}},
            "providers.status": {"result": providers_status},
            "provider.check": {"params": provider_check, "result": provider_check_result},
        },
        "x-events": {
            "state": {"params": state_event},
        },
        "$defs": generator.take_definitions(true),
    })
}
