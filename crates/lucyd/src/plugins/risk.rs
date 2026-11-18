use rmcp::model::JsonObject;
use serde_json::Value;

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
    let Some(describe) = argument("describe")
        .map(str::trim)
        .filter(|d| !d.is_empty())
    else {
        return Risk::Undescribed(undescribed);
    };
    if matches_any(describe, risky) {
        Risk::Risky {
            prompt: format!("{asks} {}?", describe.trim_end_matches(['.', '?', '!'])),
        }
    } else {
        Risk::Plain
    }
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
