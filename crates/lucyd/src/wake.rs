//! The wake phrase inside a Session: ordinary speech, with a leading wake
//! phrase taken off what the model hears.

use std::path::Path;

use crate::audio::BUNDLED_WAKE_MODEL;

/// The phrase the configured model listens for, from its file name:
/// `hey_lucy.onnx` listens for "hey lucy".
pub fn phrase(config: &lucy_config::WakeWord) -> String {
    let model = config
        .model
        .as_deref()
        .unwrap_or(Path::new(BUNDLED_WAKE_MODEL));
    let stem = model.file_stem().unwrap_or_default().to_string_lossy();
    stem.replace(['_', '-'], " ")
}

/// `transcript` without a leading `phrase`, however STT punctuated or
/// spaced it ("Hey, Lucy." or "hey lucy"), and without the punctuation
/// after it.
pub fn strip<'a>(transcript: &'a str, phrase: &str) -> &'a str {
    let mut wanted = phrase
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .peekable();
    if wanted.peek().is_none() {
        return transcript;
    }
    for (i, c) in transcript.char_indices() {
        if !c.is_alphanumeric() {
            continue;
        }
        if !c.to_lowercase().all(|c| wanted.next() == Some(c)) {
            return transcript;
        }
        if wanted.peek().is_none() {
            let rest = &transcript[i + c.len_utf8()..];
            // The phrase must end a word: "Lucy's" is not "Lucy".
            if rest.starts_with(|c: char| c.is_alphanumeric() || c == '\'' || c == '’') {
                return transcript;
            }
            return rest.trim_start_matches(|c: char| !c.is_alphanumeric());
        }
    }
    transcript
}
