//! Swift's `String` and `Character` as the swipe rules use them, so the rules read text as
//! iOS's did: a character is an extended grapheme cluster, two strings are equal when they're
//! canonically equivalent, case mapping is the full mapping without context (Σ is always σ),
//! and a character's properties are its first scalar's (or, for its case, the whole
//! character's mapping). The Unicode data is Rust's, which agrees with Swift's (checked scalar
//! by scalar on macOS 26 for Alphabetic, Uppercase, Lowercase, Cased, White_Space and the case
//! mappings) but for the two tables below.

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// The characters (extended grapheme clusters) of `text`.
pub(crate) fn characters(text: &str) -> Vec<&str> {
    text.graphemes(true).collect()
}

/// Swift's `==` on strings and characters: canonical equivalence.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a == b || a.nfc().eq(b.nfc())
}

/// A key for sets and maps that compare as Swift's do.
pub(crate) fn key(text: &str) -> String {
    text.nfc().collect()
}

/// `uppercased()`.
pub(crate) fn uppercased(text: &str) -> String {
    text.chars().flat_map(char::to_uppercase).collect()
}

/// `lowercased()`: each scalar's own mapping, with no final-sigma rule.
pub(crate) fn lowercased(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn first_scalar(character: &str) -> Option<char> {
    character.chars().next()
}

fn single_scalar(character: &str) -> Option<char> {
    let mut scalars = character.chars();
    let first = scalars.next()?;
    scalars.next().is_none().then_some(first)
}

/// Unicode's Cased property: lowercase, uppercase or titlecase (ǅ), which Rust doesn't expose.
fn scalar_is_cased(c: char) -> bool {
    c.is_lowercase() || c.is_uppercase() || (!c.to_lowercase().eq([c]) && !c.to_uppercase().eq([c]))
}

fn is_cased(character: &str) -> bool {
    if single_scalar(character).is_some_and(scalar_is_cased) {
        return true;
    }
    !same(&uppercased(character), character) || !same(&lowercased(character), character)
}

/// `Character.isUppercase`.
pub(crate) fn is_uppercase(character: &str) -> bool {
    if single_scalar(character).is_some_and(char::is_uppercase) {
        return true;
    }
    same(&uppercased(character), character) && is_cased(character)
}

/// `Character.isLowercase`.
pub(crate) fn is_lowercase(character: &str) -> bool {
    if single_scalar(character).is_some_and(char::is_lowercase) {
        return true;
    }
    same(&lowercased(character), character) && is_cased(character)
}

/// Apple's private-use characters that Swift counts as letters (Alphabetic) and Rust doesn't.
const APPLE_LETTERS: [u32; 35] = [
    0xF882, 0xF89A, 0xF89B, 0xF89C, 0xF89D, 0xF89E, 0xF8A2, 0xF8A3, 0xF8A4, 0xF8A5, 0xF8A6, 0xF8A7,
    0xF8B8, 0xF8C1, 0xF8C2, 0xF8C3, 0xF8C4, 0xF8C5, 0xF8C6, 0xF8C7, 0xF8C8, 0xF8C9, 0xF8CA, 0xF8CB,
    0xF8CC, 0xF8CD, 0xF8CE, 0xF8CF, 0xF8D0, 0xF8D1, 0xF8D2, 0xF8D3, 0xF8D4, 0xF8D5, 0xF8D6,
];

/// `Character.isLetter`: its first scalar is Alphabetic.
pub(crate) fn is_letter(character: &str) -> bool {
    first_scalar(character)
        .is_some_and(|c| c.is_alphabetic() || APPLE_LETTERS.binary_search(&(c as u32)).is_ok())
}

/// Scalars with a numeric value that aren't numbers by their category (Han numerals and the
/// like): Swift's `isNumber` counts them, Rust's `is_numeric` doesn't.
const OTHER_NUMBERS: [u32; 99] = [
    0x3405, 0x3483, 0x382A, 0x3B4D, 0x4E00, 0x4E03, 0x4E07, 0x4E09, 0x4E24, 0x4E5D, 0x4E8C, 0x4E94,
    0x4E96, 0x4EAC, 0x4EBF, 0x4EC0, 0x4EDF, 0x4EE8, 0x4F0D, 0x4F70, 0x4FE9, 0x5006, 0x5104, 0x5146,
    0x5169, 0x516B, 0x516D, 0x5341, 0x5343, 0x5344, 0x5345, 0x534C, 0x53C1, 0x53C2, 0x53C3, 0x53C4,
    0x56DB, 0x58F1, 0x58F9, 0x5E7A, 0x5EFE, 0x5EFF, 0x5F0C, 0x5F0D, 0x5F0E, 0x5F10, 0x62D0, 0x62FE,
    0x634C, 0x67D2, 0x6D1E, 0x6F06, 0x7396, 0x767E, 0x7695, 0x79ED, 0x8086, 0x842C, 0x8CAE, 0x8CB3,
    0x8D30, 0x920E, 0x94A9, 0x9621, 0x9646, 0x964C, 0x9678, 0x96F6, 0xF96B, 0xF973, 0xF978, 0xF9B2,
    0xF9D1, 0xF9D3, 0xF9FD, 0x12038, 0x12039, 0x12079, 0x12226, 0x1222B, 0x1230B, 0x1230D, 0x12399,
    0x20001, 0x20064, 0x200E2, 0x20121, 0x2092A, 0x20983, 0x2098C, 0x2099C, 0x20AEA, 0x20AFD,
    0x20B19, 0x22390, 0x22998, 0x23B1B, 0x2626D, 0x2F890,
];

/// `Character.isNumber`: its first scalar has a numeric type.
pub(crate) fn is_number(character: &str) -> bool {
    first_scalar(character)
        .is_some_and(|c| c.is_numeric() || OTHER_NUMBERS.binary_search(&(c as u32)).is_ok())
}

/// `Character.isWhitespace`: its first scalar is White_Space.
pub(crate) fn is_whitespace(character: &str) -> bool {
    first_scalar(character).is_some_and(char::is_whitespace)
}

/// `Character.isNewline`.
pub(crate) fn is_newline(character: &str) -> bool {
    first_scalar(character)
        .is_some_and(|c| matches!(c, '\u{A}'..='\u{D}' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_are_swifts() {
        assert_eq!(characters("e\u{301}t\u{e9}"), ["e\u{301}", "t", "\u{e9}"]);
        assert!(same("e\u{301}", "\u{e9}"));
        assert_eq!(lowercased("ΑΣ"), "ασ", "no final sigma, as Swift's");
        assert!(
            !is_uppercase("ǅ") && !is_lowercase("ǅ"),
            "titlecase is neither"
        );
        assert!(is_number("四") && is_number("7") && !is_number("a"));
        assert!(is_letter("\u{F8D6}") && is_letter("ñ") && !is_letter("'"));
        assert!(is_newline("\r\n") && !is_newline(" "));
    }
}
