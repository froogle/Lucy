use crate::adapters::ThinkingEvent;

#[derive(Default)]
pub struct Sentences {
    open: bool,
    after_terminator: bool,
}

impl Sentences {
    pub fn push(&mut self, text: &str) -> Vec<ThinkingEvent> {
        let mut events = vec![];
        let mut start = 0;
        for (i, c) in text.char_indices() {
            let ends = c == '\n' || (self.after_terminator && c.is_whitespace());
            if ends && (self.open || !text[start..i].trim().is_empty()) {
                if i > start {
                    events.push(ThinkingEvent::Delta(text[start..i].to_owned()));
                }
                events.push(ThinkingEvent::SentenceEnd);
                self.open = false;
                start = i;
            }
            self.after_terminator = matches!(c, '.' | '!' | '?');
        }
        if start < text.len() {
            events.push(ThinkingEvent::Delta(text[start..].to_owned()));
            self.open |= !text[start..].trim().is_empty();
        }
        events
    }

    pub fn finish(&mut self) -> Option<ThinkingEvent> {
        self.after_terminator = false;
        std::mem::take(&mut self.open).then_some(ThinkingEvent::SentenceEnd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ThinkingEvent::{Delta, SentenceEnd};

    fn delta(text: &str) -> ThinkingEvent {
        Delta(text.to_owned())
    }

    #[test]
    fn a_sentence_ends_as_soon_as_whitespace_follows_its_terminator() {
        let mut s = Sentences::default();
        assert_eq!(s.push("Hello"), [delta("Hello")]);
        assert_eq!(s.push(" there."), [delta(" there.")]);
        assert_eq!(s.push(" How"), [SentenceEnd, delta(" How")]);
    }

    #[test]
    fn one_delta_can_end_several_sentences() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("Yes! Sure? Fine.\nOK"),
            [
                delta("Yes!"),
                SentenceEnd,
                delta(" Sure?"),
                SentenceEnd,
                delta(" Fine."),
                SentenceEnd,
                delta("\nOK"),
            ]
        );
    }

    #[test]
    fn a_newline_ends_a_sentence() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("Hi\nThere"),
            [delta("Hi"), SentenceEnd, delta("\nThere")]
        );
        assert_eq!(
            s.push("\n"),
            [SentenceEnd, delta("\n")],
            "across deltas too"
        );
        assert_eq!(
            s.push("\nOK"),
            [delta("\nOK")],
            "a blank line doesn't end an empty sentence"
        );
    }

    #[test]
    fn a_terminator_inside_a_word_or_number_does_not_end_a_sentence() {
        let mut s = Sentences::default();
        assert_eq!(
            s.push("It costs 3.50 at example.com today"),
            [delta("It costs 3.50 at example.com today")]
        );
    }

    #[test]
    fn finishing_ends_the_open_sentence_once() {
        let mut s = Sentences::default();
        s.push("Let me check.");
        assert_eq!(s.finish(), Some(SentenceEnd));
        assert_eq!(s.finish(), None);
        assert_eq!(
            s.push(" Done"),
            [delta(" Done")],
            "the next block's first space doesn't end an empty sentence"
        );
    }
}
