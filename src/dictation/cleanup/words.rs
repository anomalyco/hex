//! The transcript as it was heard, split into words once.

use std::ops::Range;

/// The words the recogniser heard. The text is kept exactly as it came,
/// spacing and punctuation included, and is never changed: every edit names
/// words in it by position, and the cleaned text is rendered from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Words {
    text: String,
    /// Where each whitespace-separated word sits in `text`, in bytes.
    bounds: Vec<Range<usize>>,
}

impl Words {
    pub(crate) fn new(transcript: impl Into<String>) -> Self {
        let text = transcript.into();
        let start = text.as_ptr() as usize;
        let bounds = text
            .split_whitespace()
            .map(|word| {
                let offset = word.as_ptr() as usize - start;
                offset..offset + word.len()
            })
            .collect();
        Self { text, bounds }
    }

    /// The transcript exactly as it was heard.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn len(&self) -> usize {
        self.bounds.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }

    /// Each word with the punctuation attached to it, in order.
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        self.bounds.iter().map(|bounds| &self.text[bounds.clone()])
    }

    /// The words at `range`, or nothing when the range is empty or reaches
    /// past the last word. This is the only way to make a [`Span`].
    pub(crate) fn span(&self, range: Range<usize>) -> Option<Span> {
        (range.start < range.end && range.end <= self.len()).then_some(Span {
            start: range.start,
            end: range.end,
        })
    }

    /// Where a span sits in [`Words::text`], in bytes, from the first letter
    /// of its first word to the end of its last. A span made by other words
    /// that does not fit these covers nothing.
    pub(crate) fn bytes(&self, span: Span) -> Range<usize> {
        match (self.bounds.get(span.start), self.bounds.get(span.end - 1)) {
            (Some(first), Some(last)) => first.start..last.end,
            _ => 0..0,
        }
    }
}

/// One or more neighbouring words of a transcript, by position.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Span {
    start: usize,
    end: usize,
}

impl Span {
    /// The positions of the span's words: never empty.
    pub(crate) fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_the_text_and_find_each_word_in_it() {
        let words = Words::new("  Hey  Tim,\tum,\nare you\u{a0}free? ");
        assert_eq!(words.text(), "  Hey  Tim,\tum,\nare you\u{a0}free? ");
        assert_eq!(
            words.iter().collect::<Vec<_>>(),
            ["Hey", "Tim,", "um,", "are", "you", "free?"]
        );
        assert_eq!(words.len(), 6);
        assert!(!words.is_empty());

        let span = words.span(1..3).unwrap();
        assert_eq!(span.range(), 1..3);
        assert_eq!(&words.text()[words.bytes(span)], "Tim,\tum,");
    }

    #[test]
    fn a_span_always_names_words_that_exist() {
        let words = Words::new("one two three");
        assert!(words.span(0..3).is_some());
        assert_eq!(words.span(0..0), None);
        assert_eq!(words.span(2..4), None);
        let backwards = std::ops::Range { start: 2, end: 1 };
        assert_eq!(words.span(backwards), None);

        let nothing = Words::new(" \n ");
        assert!(nothing.is_empty());
        assert_eq!(nothing.span(0..1), None);
        assert_eq!(nothing.iter().count(), 0);
    }

    #[test]
    fn a_span_from_longer_words_covers_nothing_here() {
        let long = Words::new("one two three four");
        let short = Words::new("one two");
        assert_eq!(short.bytes(long.span(2..4).unwrap()), 0..0);
        assert_eq!(short.bytes(long.span(1..3).unwrap()), 0..0);
    }
}
