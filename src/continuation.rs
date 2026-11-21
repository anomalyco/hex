//! The input session a dictation paste continues: back-to-back dictations
//! with no typing, clicking, or focus change between them read as one
//! sentence, joined with a space and sentence-aware capitalization.
//!
//! `InputActivity`'s revision changes on that input, and a paste joins the
//! one before it only while the revision is unchanged.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The user's own input since Hex started: its revision changes when they
/// type, click, or switch windows, which ends a paste continuation.
#[derive(Clone, Default)]
pub struct InputActivity(Arc<AtomicU64>);

impl InputActivity {
    pub fn revision(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    pub fn invalidate(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

/// The last paste a new one may continue, and the input session it
/// belongs to.
#[derive(Default)]
pub struct Continuation {
    previous: Option<Inserted>,
}

struct Inserted {
    revision: u64,
    text: String,
}

impl Continuation {
    /// `next` joined onto the previous paste when `revision` is still the
    /// session that made it and the paste is `joinable`; `None` stands
    /// alone.
    pub fn join(&self, revision: u64, joinable: bool, next: &str) -> Option<String> {
        let previous = self.previous.as_ref()?;
        (joinable && previous.revision == revision).then(|| join(&previous.text, next))
    }

    /// Records a successful paste as the one the next may continue; a
    /// standalone paste ends the continuation instead.
    pub fn record(&mut self, revision: u64, joinable: bool, inserted: String) {
        self.previous = joinable.then_some(Inserted {
            revision,
            text: inserted,
        });
    }

    /// Ends the continuation without inserting anything.
    pub fn clear(&mut self) {
        self.previous = None;
    }
}

fn join(previous: &str, next: &str) -> String {
    let sentence_start = ends_sentence(previous);
    let mut next = set_initial_case(next, sentence_start);
    let needs_space = previous
        .chars()
        .next_back()
        .is_some_and(|character| !character.is_whitespace() && !is_opening(character))
        && next
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace() && !is_closing(character));
    if needs_space {
        next.insert(0, ' ');
    }
    next
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(['\'', '"', ')', ']', '}'])
        .ends_with(['.', '?', '!'])
}

fn is_opening(character: char) -> bool {
    matches!(character, '(' | '[' | '{')
}

fn is_closing(character: char) -> bool {
    matches!(
        character,
        ',' | '.' | '?' | '!' | ';' | ':' | ')' | ']' | '}'
    )
}

fn set_initial_case(text: &str, uppercase: bool) -> String {
    let Some((index, character)) = text
        .char_indices()
        .find(|(_, character)| character.is_alphabetic())
    else {
        return text.to_string();
    };
    if uppercase && character.is_lowercase() {
        return replace_character(text, index, character.to_uppercase());
    }
    if !uppercase && character.is_uppercase() && sentence_initial_word(text, index) {
        return replace_character(text, index, character.to_lowercase());
    }
    text.to_string()
}

fn sentence_initial_word(text: &str, start: usize) -> bool {
    let word = text[start..]
        .split(|character: char| !character.is_alphabetic() && character != '\'')
        .next()
        .unwrap_or_default();
    matches!(
        word.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "because"
            | "but"
            | "for"
            | "if"
            | "nor"
            | "or"
            | "so"
            | "the"
            | "then"
            | "though"
            | "to"
            | "when"
            | "while"
            | "yet"
    )
}

fn replace_character(text: &str, index: usize, replacement: impl Iterator<Item = char>) -> String {
    let character_length = text[index..].chars().next().unwrap().len_utf8();
    let mut output = String::with_capacity(text.len());
    output.push_str(&text[..index]);
    output.extend(replacement);
    output.push_str(&text[index + character_length..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_activity_revision_invalidates_continuation() {
        let activity = InputActivity::default();
        let revision = activity.revision();

        activity.invalidate();

        assert_ne!(activity.revision(), revision);
    }

    #[test]
    fn a_paste_joins_only_the_session_that_made_it() {
        let activity = InputActivity::default();
        let mut continuation = Continuation::default();
        let revision = activity.revision();

        continuation.record(revision, true, "Because if I don't do that,".into());
        assert_eq!(
            continuation.join(activity.revision(), true, "And if you see it."),
            Some(" and if you see it.".into())
        );

        // Real input between pastes breaks the join.
        activity.invalidate();
        assert_eq!(
            continuation.join(activity.revision(), true, "And if you see it."),
            None
        );
    }

    #[test]
    fn a_standalone_paste_neither_joins_nor_is_continued() {
        let activity = InputActivity::default();
        let mut continuation = Continuation::default();
        let revision = activity.revision();
        continuation.record(revision, true, "earlier dictation".into());

        assert_eq!(continuation.join(revision, false, "standalone text"), None);

        continuation.record(revision, false, "standalone text".into());
        assert_eq!(continuation.join(revision, true, "next"), None);
        assert_eq!(continuation.join(activity.revision(), true, "next"), None);

        continuation.record(revision, true, "earlier dictation".into());
        continuation.clear();
        assert_eq!(continuation.join(revision, true, "next"), None);
    }

    #[test]
    fn joins_contiguous_dictation_with_sentence_aware_spacing() {
        assert_eq!(
            join("Because if I don't do that,", "And if you see it."),
            " and if you see it."
        );
        assert_eq!(
            join("And if you see it.", "well, that works."),
            " Well, that works."
        );
        assert_eq!(join("Already spaced. ", "Next sentence."), "Next sentence.");
        assert_eq!(join("Hello", ", world."), ", world.");
    }

    #[test]
    fn preserves_likely_proper_nouns_mid_sentence() {
        assert_eq!(join("Open", "GitHub next."), " GitHub next.");
        assert_eq!(join("Message", "Slack now."), " Slack now.");
    }
}
