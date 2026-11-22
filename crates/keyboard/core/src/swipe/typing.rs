//! What a swipe types: the space before the word, the word cased as the sentence and your
//! words need, the strip's other readings, the rare names a reading ranks lower, and whether a
//! swipe was a tap that slid. Ported from iOS's `FullKeyboardEngine+SwipeTyping.swift` and
//! `SwipeTyping.swift`, so both platforms type a swipe alike.
//!
//! The host keeps what only it can know and hands it in: whether the dictionary knows a word
//! (`is_word_valid`, iOS's `UITextChecker`), your text replacements and words
//! ([`SwipeWords`]), and the field, shift and auto-capitalisation state with the text before
//! the cursor ([`SwipeContext`]). Which readings may be offered at all (offensive words) is the
//! host's filter between [`SwipeDecoder::ranked`] and [`typed`].

use super::{Candidate, SwipeDecoder};

/// Your text replacements and words, as autocorrect has them: made once, and again whenever
/// one of them changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwipeWords {
    _private: (),
}

impl SwipeWords {
    /// `language` is the layout's ("en", "de"…): German keeps a capital a word has, and in
    /// English "I" and its contractions keep theirs. `has_dictionary` is whether the host has
    /// one for it (else `is_word_valid` is never asked). `replacements` are (shortcut,
    /// replacement) with the shortcut lowercased; `own` your other own spellings (lexicon
    /// terms, words you kept, fixes Hex learned); `vocabulary` your vocabulary's words as
    /// you added them.
    pub fn new(
        language: &str,
        has_dictionary: bool,
        replacements: Vec<(String, String)>,
        own: Vec<String>,
        vocabulary: Vec<String>,
    ) -> Self {
        let _ = (language, has_dictionary, replacements, own, vocabulary);
        todo!("SwipeWords::new")
    }
}

/// Shift as the keys show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Off,
    /// One-shot: the next letter is a capital.
    On,
    /// Caps lock.
    Locked,
}

/// The field's auto-capitalisation (`UITextAutocapitalizationType`), `None` also when
/// Settings › Auto-capitalization is off or the language never capitalises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCapitalization {
    None,
    Words,
    Sentences,
    AllCharacters,
}

/// The state a swipe is typed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeContext<'a> {
    /// The text before the cursor, as Hex expects the host to report it; `None` when the host
    /// reports none.
    pub before: Option<&'a str>,
    pub shift: Shift,
    /// Whether shift is held down.
    pub shift_held: bool,
    pub auto_capitalization: AutoCapitalization,
    /// Settings › Auto-capitalization.
    pub auto_capitalizes: bool,
    /// Whether the field allows autocorrection: text replacements show only then.
    pub autocorrection: bool,
}

/// What a finished swipe types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    /// Whether a space goes in before the word.
    pub space: bool,
    pub word: String,
    /// The strip's other readings, cased as the word is, best first (at most three).
    pub alternatives: Vec<String>,
}

/// How much more (nats) a reading costs that the dictionary knows only capitalised and the
/// model counts as rare (`SwipeTyping.nameCost`).
pub const NAME_COST: f64 = 4.0;
/// The model's cost past which such a reading is rare, about its 2,500th word
/// (`SwipeTyping.commonNameCost`).
pub const COMMON_NAME_COST: f64 = 10.0;
/// How far (key pitches) a tap that slid may go from where it went down
/// (`SwipeTyping.slidTapReach`).
pub const SLID_TAP_REACH: f64 = 1.6;

impl SwipeDecoder {
    /// The order of `candidates` (a reading's, best first) once a rare name or abbreviation
    /// costs [`NAME_COST`] more: indices into `candidates`.
    pub fn ranked(
        &self,
        candidates: &[Candidate],
        words: &SwipeWords,
        is_word_valid: &mut dyn FnMut(&str) -> bool,
    ) -> Vec<usize> {
        let _ = (candidates, words, is_word_valid);
        todo!("SwipeDecoder::ranked")
    }
}

/// What a swipe whose readings are `readings` (best first, those that may be offered) types in
/// `context`; `None` when there are none.
pub fn typed(
    readings: &[&str],
    context: &SwipeContext,
    words: &SwipeWords,
    is_word_valid: &mut dyn FnMut(&str) -> bool,
) -> Option<Typed> {
    let _ = (readings, context, words, is_word_valid);
    todo!("typed")
}

/// Whether a swiped word needs a space before it after `before`, as Apple's keyboard adds one.
pub fn needs_space(before: Option<&str>) -> bool {
    let _ = before;
    todo!("needs_space")
}

/// Whether a swipe through `points` (the touch's, in points) was a tap that slid: it never went
/// more than [`SLID_TAP_REACH`] × `pitch` from where it went down, it lifted on the key it
/// went down on (`key`, that key's character; `None` when it lifted elsewhere), and its best
/// reading (`first_reading`) doesn't start and end on that key ("lol").
pub fn is_slid_tap(
    points: &[(f64, f64)],
    pitch: f64,
    key: Option<&str>,
    first_reading: Option<&str>,
) -> bool {
    let _ = (points, pitch, key, first_reading);
    todo!("is_slid_tap")
}
