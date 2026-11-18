use rmcp::model::{CallToolResult, JsonObject};
use serde_json::Value;

use crate::mcp;

use super::manifest::ScreenUse;

const UNDESCRIBED: &str = "refused: describe is required, in a few plain words naming what \
                           this acts on and what it does (\"the Send button\", \"Enter, to send \
                           the message\"), so nothing was done.";

const UNDESCRIBED_ENTER: &str = "refused: a newline in the text presses Enter, so describe \
                                 is required, in a few plain words saying what that Enter does \
                                 (\"Enter, to send the message\"), so nothing was done.";

#[derive(Debug, PartialEq, Eq)]
pub enum Risk {
    Plain,
    Risky { prompt: String },
    Undescribed(&'static str),
}

pub fn check(screen: Option<ScreenUse>, arguments: Option<&JsonObject>, risky: &[String]) -> Risk {
    confirmation(screen, arguments, |describe| matches_any(describe, risky))
}

pub fn under_click(arguments: Option<&JsonObject>, under: Option<&str>, risky: &[String]) -> Risk {
    let readably_plain = under.is_some_and(|under| !matches_any(under, risky));
    confirmation(Some(ScreenUse::Click), arguments, |_| !readably_plain)
}

fn confirmation(
    screen: Option<ScreenUse>,
    arguments: Option<&JsonObject>,
    risky: impl FnOnce(&str) -> bool,
) -> Risk {
    let argument = |name: &str| {
        arguments
            .and_then(|args| args.get(name))
            .and_then(Value::as_str)
    };
    let (asks, undescribed) = match screen {
        Some(ScreenUse::Click) => ("Shall I click", UNDESCRIBED),
        Some(ScreenUse::Key) => ("Shall I press", UNDESCRIBED),
        Some(ScreenUse::Type) if argument("text").is_some_and(|text| text.contains('\n')) => {
            ("Shall I type that and press", UNDESCRIBED_ENTER)
        }
        _ => return Risk::Plain,
    };
    let Some(describe) = described(arguments) else {
        return Risk::Undescribed(undescribed);
    };
    if risky(describe) {
        Risk::Risky {
            prompt: format!("{asks} {}?", describe.trim_end_matches(['.', '?', '!'])),
        }
    } else {
        Risk::Plain
    }
}

pub fn probed(result: &CallToolResult) -> Result<Option<String>, String> {
    let answer = mcp::result_text(result);
    if result.is_error == Some(true) {
        return Err(format!("it answered with an error: {answer}"));
    }
    let read: Value = serde_json::from_str(&answer)
        .map_err(|e| format!("its answer isn't the JSON a probe returns: {e}"))?;
    let field = |name: &str| {
        read["found"][name]
            .as_str()
            .map_or("", str::trim)
            .to_owned()
    };
    let (name, text) = (field("name"), field("text"));
    let under = match (name.is_empty(), text.is_empty() || text == name) {
        (true, _) => text,
        (false, true) => name,
        (false, false) => format!("{name}, {text}"),
    };
    Ok((!under.is_empty()).then_some(under))
}

fn described(arguments: Option<&JsonObject>) -> Option<&str> {
    arguments
        .and_then(|args| args.get("describe"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|d| !d.is_empty())
}

fn matches_any(describe: &str, risky: &[String]) -> bool {
    let described = words(describe);
    risky
        .iter()
        .map(|word| words(word))
        .filter(|word| !word.is_empty())
        .any(|word| described.contains(&word))
}

fn words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| format!(" {word}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn risky(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    fn click(describe: &str, words: &[&str]) -> Risk {
        let arguments = serde_json::json!({"describe": describe});
        check(Some(ScreenUse::Click), arguments.as_object(), &risky(words))
    }

    #[test]
    fn a_risky_phrase_matches_its_words_in_order_from_a_word_start() {
        assert_ne!(click("Buy it now", &["buy now", "buy it"]), Risk::Plain);
        assert_eq!(click("Buy, then go now", &["buy now"]), Risk::Plain);
    }

    #[test]
    fn a_blank_risky_word_matches_nothing() {
        assert_eq!(click("the Play button", &["", " - "]), Risk::Plain);
    }
}
