//! What autocorrect learns from you, as data the host hands over: Learn how I type's typos and
//! rejected words (`TypingLearning`), the words you kept twice (`LearnedWordsStore`'s list,
//! which the host keeps on disk), and the latest autocorrection for the strip's revert
//! (`AppliedCorrection`).

use std::collections::{HashMap, HashSet};

use super::language::Language;
use super::text;

/// `TypingLearning`: typos you fixed the same way twice, and words you turned down twice.
#[derive(Debug, Clone, Default)]
pub struct TypingLearning {
    /// Lowercased typo → the word you fixed it to, in the host's order.
    typos: Vec<(String, String)>,
    by_typo: HashMap<String, usize>,
    rejected: HashSet<String>,
}

impl TypingLearning {
    /// `typos` keyed by lowercased typo (a later entry for the same typo wins); `rejected`
    /// lowercased words.
    pub fn new<S: AsRef<str>>(typos: &[(S, S)], rejected: &[S]) -> Self {
        let mut out = Self::default();
        for (typo, fix) in typos {
            let key = text::nfc(typo.as_ref()).into_owned();
            match out.by_typo.get(&key) {
                Some(&i) => out.typos[i].1 = fix.as_ref().to_owned(),
                None => {
                    out.by_typo.insert(key.clone(), out.typos.len());
                    out.typos.push((key, fix.as_ref().to_owned()));
                }
            }
        }
        out.rejected = rejected
            .iter()
            .map(|r| text::nfc(r.as_ref()).into_owned())
            .collect();
        out
    }

    /// The fix learned for a lowercased typo.
    pub fn typo(&self, lower: &str) -> Option<&str> {
        self.by_typo
            .get(text::nfc(lower).as_ref())
            .map(|&i| self.typos[i].1.as_str())
    }

    pub fn is_rejected(&self, lower: &str) -> bool {
        self.rejected.contains(text::nfc(lower).as_ref())
    }

    /// The learned fixes (`typos.values`), in the order they are kept.
    pub fn fixes(&self) -> impl Iterator<Item = &str> {
        self.typos.iter().map(|(_, fix)| fix.as_str())
    }
}

/// `==` as Swift compares the dictionary and set: by content, strings canonically.
impl PartialEq for TypingLearning {
    fn eq(&self, other: &Self) -> bool {
        self.rejected == other.rejected
            && self.typos.len() == other.typos.len()
            && self
                .typos
                .iter()
                .all(|(typo, fix)| other.typo(typo).is_some_and(|theirs| text::eq(fix, theirs)))
    }
}

/// Which list of kept words an engine reads: the shared one, or a language's own (Learn each
/// language separately). The host keeps each list (`LearnedWordsStore`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LearnedList {
    Shared,
    Language(Language),
}

/// An engine's copy of its kept words (`LearnedWordsStore.words` and `contains`).
#[derive(Debug, Clone, Default)]
pub struct LearnedWords {
    words: Vec<String>,
    set: HashSet<String>,
}

impl LearnedWords {
    pub fn new<S: AsRef<str>>(words: &[S]) -> Self {
        let words: Vec<String> = words.iter().map(|w| w.as_ref().to_owned()).collect();
        let set = words.iter().map(|w| text::nfc(w).into_owned()).collect();
        Self { words, set }
    }

    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Whether the list holds `word` lowercased.
    pub fn contains(&self, word: &str) -> bool {
        self.set
            .contains(text::nfc(&text::lowercased(word)).as_ref())
    }
}

/// One word the revert puts back as typed (`from`), and what autocorrect made it.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub from: String,
    pub to: String,
    /// The language the fix was made in.
    pub language: Option<Language>,
}

/// The latest autocorrection, which the strip offers to revert while the cursor is right
/// after it (`AppliedCorrection`).
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedCorrection {
    /// What you typed.
    pub original: String,
    /// What autocorrect put in its place, without the space or mark after it.
    pub corrected: String,
    pub changes: Vec<Change>,
    /// The text before the cursor right after `corrected`.
    pub anchor: String,
}

impl AppliedCorrection {
    /// How much of the text before the cursor must read as `anchor`.
    pub const ANCHOR_TAIL: usize = 64;

    /// Whether the cursor is right after `corrected`, with the same text before it.
    pub fn is_at_end(&self, before: Option<&str>) -> bool {
        let Some(before) = before else {
            return false;
        };
        if !text::has_suffix(before, &self.corrected) {
            return false;
        }
        if text::count(&self.anchor) < Self::ANCHOR_TAIL || text::count(before) < Self::ANCHOR_TAIL
        {
            return text::eq(before, &self.anchor);
        }
        text::eq(
            text::suffix(before, Self::ANCHOR_TAIL),
            text::suffix(&self.anchor, Self::ANCHOR_TAIL),
        )
    }
}
