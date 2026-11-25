//! The spec's "Cleaning text": every string is composed and its spaces
//! settled before it is checked, compared, or kept, the same way on every
//! platform, and each kind of text has its first broken rule.

use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization as _;

use super::{MAX_HEARD_CHARS, MAX_WORDS, MAX_WRITE_CHARS, Reason};

/// Unicode general category L (letters) or N (numbers).
static LETTER_OR_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]").expect("a fixed pattern"));

/// A term, spelling, heard phrase, or `madeBy` value: composed (NFC), every
/// run of whitespace one space, and no space at either end.
pub(super) fn words(text: &str) -> String {
    let composed: String = text.nfc().collect();
    let mut cleaned = String::with_capacity(composed.len());
    for piece in composed.split(is_space).filter(|piece| !piece.is_empty()) {
        if !cleaned.is_empty() {
            cleaned.push(' ');
        }
        cleaned.push_str(piece);
    }
    cleaned
}

/// A replacement's output: composed (NFC), with CR LF and a lone CR as LF.
/// Its spaces are kept exactly.
pub(super) fn output(text: &str) -> String {
    let composed: String = text.nfc().collect();
    composed.replace("\r\n", "\n").replace('\r', "\n")
}

/// The spec's whitespace: exactly the Unicode White_Space set, spelled out
/// rather than left to a platform's own test.
fn is_space(character: char) -> bool {
    matches!(
        character,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Whether cleaned `text` still holds a character that hides what it types:
/// a control character (an output may keep LF and TAB), a soft hyphen, an
/// invisible mark or space, or a direction control. Zero-width joiners
/// stay: scripts and emoji need them.
pub(super) fn hidden(text: &str, output: bool) -> bool {
    text.chars().any(|character| match character {
        '\n' | '\t' => !output,
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => true,
        '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}' | '\u{200e}' | '\u{200f}' | '\u{feff}' => {
            true
        }
        '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' => true,
        '\u{fff9}'..='\u{fffb}' => true,
        _ => false,
    })
}

/// Characters as the spec counts them: Unicode code points.
pub(super) fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Words in cleaned text: the pieces between its single spaces.
fn words_in(text: &str) -> usize {
    text.split(' ').count()
}

/// The key two cleaned strings compare by: lowercased with the full,
/// locale-independent mapping, then every final sigma as a sigma, where
/// platforms disagree.
pub(super) fn key(cleaned: &str) -> String {
    cleaned.to_lowercase().replace('ς', "σ")
}

/// The first rule a cleaned term or spelling breaks: hidden characters, no
/// letter or digit (or nothing at all), more than `max_chars` characters,
/// then more than [`MAX_WORDS`] words when `words_limited`.
pub(super) fn phrase_fault(cleaned: &str, max_chars: usize, words_limited: bool) -> Option<Reason> {
    if hidden(cleaned, false) {
        Some(Reason::HiddenCharacters)
    } else if !LETTER_OR_DIGIT.is_match(cleaned) {
        Some(Reason::NoLetters)
    } else if chars(cleaned) > max_chars {
        Some(Reason::TooLong)
    } else if words_limited && words_in(cleaned) > MAX_WORDS {
        Some(Reason::TooManyWords)
    } else {
        None
    }
}

/// The first rule a cleaned replacement breaks: hidden characters in either
/// part, no letter or digit in the heard words, either part too long, then
/// too many heard words. An empty output deletes the words and is fine.
pub(super) fn replacement_fault(heard: &str, write: &str) -> Option<Reason> {
    if hidden(heard, false) || hidden(write, true) {
        Some(Reason::HiddenCharacters)
    } else if !LETTER_OR_DIGIT.is_match(heard) {
        Some(Reason::NoLetters)
    } else if chars(heard) > MAX_HEARD_CHARS || chars(write) > MAX_WRITE_CHARS {
        Some(Reason::TooLong)
    } else if words_in(heard) > MAX_WORDS {
        Some(Reason::TooManyWords)
    } else {
        None
    }
}
