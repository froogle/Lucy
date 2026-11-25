use std::collections::BTreeMap;

use lucy_ipc::{Choices, Field, FieldKind};
use rmcp::model::{
    ElicitResult, ElicitationAction, ElicitationSchema, MetaObject, RequestMetaObject,
};
use serde_json::{json, Value};

use crate::input::Filled;

const SECRET: &str = "lucy/secret";

const ITEMS: &str = "lucy/items";

const RESULT: &str = "lucy/result";

const PROVIDED: &str = "provided";

pub(super) enum Question {
    Spoken { field: String },
    Typed(Fields),
}

pub(super) struct Fields {
    fields: Vec<(Field, Reply)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reply {
    Text,
    Number,
    Boolean,
    Secret,
}

pub(super) fn read(
    schema: ElicitationSchema,
    meta: Option<&RequestMetaObject>,
    credentials: &[String],
) -> Option<Question> {
    let secrets = secrets(meta)?;
    let asked_for = |name: &String| schema.properties.contains_key(name);
    if !secrets
        .iter()
        .all(|name| credentials.contains(name) && asked_for(name))
    {
        return None;
    }
    let order = schema
        .property_order
        .unwrap_or_else(|| schema.properties.keys().cloned().collect());
    let properties = order
        .into_iter()
        .map(|name| {
            let property = serde_json::to_value(schema.properties.get(&name)?).ok()?;
            Some((name, property))
        })
        .collect::<Option<Vec<_>>>()?;
    match &*properties {
        [] => None,
        [(name, property)] if is_plain(property) && !secrets.contains(name) => {
            Some(Question::Spoken {
                field: name.clone(),
            })
        }
        _ => {
            let fields = properties
                .into_iter()
                .map(|(name, property)| {
                    let secret = secrets.contains(&name);
                    field(name, &property, secret)
                })
                .collect::<Option<_>>()?;
            Some(Question::Typed(Fields { fields }))
        }
    }
}

pub(super) fn items(meta: Option<&RequestMetaObject>) -> Vec<String> {
    meta.and_then(|meta| meta.get(ITEMS))
        .and_then(items_in)
        .unwrap_or_default()
}

pub(super) fn items_in(listed: &Value) -> Option<Vec<String>> {
    listed
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn secrets(meta: Option<&RequestMetaObject>) -> Option<Vec<String>> {
    let Some(named) = meta.and_then(|meta| meta.get(SECRET)) else {
        return Some(Vec::new());
    };
    named
        .as_array()?
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect()
}

fn is_plain(property: &Value) -> bool {
    property["type"] == "string" && property.get("format").is_none() && !is_enum(property)
}

fn is_enum(property: &Value) -> bool {
    property.get("enum").is_some() || property.get("oneOf").is_some()
}

fn field(name: String, property: &Value, secret: bool) -> Option<(Field, Reply)> {
    let (kind, reply) = match property["type"].as_str()? {
        "string" if secret => (FieldKind::Secret, Reply::Secret),
        _ if secret => return None,
        "string" if is_enum(property) => (choice(enumerated(property)?)?, Reply::Text),
        "string" if property["format"] == "uri" => (FieldKind::Url, Reply::Text),
        "string" => (FieldKind::Text, Reply::Text),
        "number" | "integer" => (FieldKind::Number, Reply::Number),
        "boolean" => (choice(vec!["yes".into(), "no".into()])?, Reply::Boolean),
        _ => return None,
    };
    let description = ["title", "description"]
        .iter()
        .find_map(|key| property[key].as_str().map(str::to_owned));
    let field = Field {
        name,
        description,
        kind,
    };
    Some((field, reply))
}

fn enumerated(property: &Value) -> Option<Vec<String>> {
    if let Some(values) = property.get("enum") {
        return values
            .as_array()?
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect();
    }
    property["oneOf"]
        .as_array()?
        .iter()
        .map(|option| option["const"].as_str().map(str::to_owned))
        .collect()
}

fn choice(choices: Vec<String>) -> Option<FieldKind> {
    Some(FieldKind::Choice {
        choices: Choices::try_from(choices).ok()?,
    })
}

impl Fields {
    pub(super) fn split(self) -> (Vec<Field>, Replies) {
        let replies = self
            .fields
            .iter()
            .map(|(field, reply)| (field.name.clone(), *reply))
            .collect();
        let fields = self.fields.into_iter().map(|(field, _)| field).collect();
        (fields, Replies(replies))
    }
}

pub(super) struct Replies(BTreeMap<String, Reply>);

impl Replies {
    pub(super) fn secrets<'a>(
        &'a self,
        values: &'a BTreeMap<String, String>,
    ) -> impl Iterator<Item = (&'a str, &'a str)> {
        values
            .iter()
            .filter(|(name, _)| self.0.get(*name) == Some(&Reply::Secret))
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub(super) fn answer(&self, filled: Filled) -> ElicitResult {
        let values = match filled {
            Filled::Values(values) => values,
            Filled::Cancelled => return unanswered("cancelled"),
            Filled::TimedOut => return unanswered("timed_out"),
        };
        let content: serde_json::Map<String, Value> = values
            .into_iter()
            .map(|(name, typed)| {
                let value = match self.0.get(&name) {
                    Some(Reply::Secret) => json!(PROVIDED),
                    Some(Reply::Number) => number(typed),
                    Some(Reply::Boolean) => json!(typed == "yes"),
                    _ => json!(typed),
                };
                (name, value)
            })
            .collect();
        ElicitResult::new(ElicitationAction::Accept).with_content(Value::Object(content))
    }
}

fn number(typed: String) -> Value {
    if let Ok(integer) = typed.parse::<i64>() {
        return json!(integer);
    }
    match typed.parse::<f64>() {
        Ok(number) => json!(number),
        Err(_) => json!(typed),
    }
}

fn unanswered(why: &str) -> ElicitResult {
    let mut meta = serde_json::Map::new();
    meta.insert(RESULT.to_owned(), json!(why));
    ElicitResult::new(ElicitationAction::Cancel).with_meta(MetaObject(meta))
}
