pub fn strip<'a>(transcript: &'a str, word: &str) -> &'a str {
    let mut wanted = word
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
            let inside_a_word =
                rest.starts_with(|c: char| c.is_alphanumeric() || c == '\'' || c == '’');
            if inside_a_word {
                return transcript;
            }
            return rest.trim_start_matches(|c: char| !c.is_alphanumeric());
        }
    }
    transcript
}
