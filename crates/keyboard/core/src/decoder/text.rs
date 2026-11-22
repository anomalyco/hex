//! Swift's `String` semantics the decoder relies on. Swift compares and hashes strings by
//! canonical equivalence, so they are compared here composed (NFC). Swift counts and walks
//! `Character`s; a word the decoder can read is made of the model's letters, each one
//! composed character, so here a word's characters are its composed `char`s (a string with
//! anything else fails to encode either way).

use unicode_normalization::UnicodeNormalization;

/// The string's characters, composed.
pub(crate) fn characters(s: &str) -> impl Iterator<Item = char> + '_ {
    s.nfc()
}

/// `s.count`.
pub(crate) fn count(s: &str) -> usize {
    characters(s).count()
}

/// The form two canonically equivalent strings share, so `==` and hashing match Swift's.
pub(crate) fn canonical(s: &str) -> String {
    s.nfc().collect()
}

/// `s.lowercased()`: each scalar's full lowercase mapping, without context (Swift lowers a
/// final capital sigma to σ, as here).
pub(crate) fn lowercased(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// The text with acute, grave, circumflex and diaeresis marks removed (é -> e, ü -> u),
/// keeping ñ (`TypingDecoder.withoutAccents`): decomposed, U+0300–U+0308 but U+0303 dropped,
/// composed again.
pub fn without_accents(text: &str) -> String {
    text.nfd()
        .filter(|&c| !(('\u{300}'..='\u{308}').contains(&c) && c != '\u{303}'))
        .nfc()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_go_but_the_tilde_stays() {
        assert_eq!(without_accents("está"), "esta");
        assert_eq!(without_accents("über"), "uber");
        assert_eq!(without_accents("año"), "año");
        assert_eq!(without_accents("an\u{303}o"), "año");
        assert_eq!(without_accents("Crème brûlée"), "Creme brulee");
    }

    #[test]
    fn swift_string_semantics() {
        assert_eq!(count("e\u{301}"), 1);
        assert_eq!(canonical("e\u{301}"), "é");
        assert_eq!(lowercased("ΟΔΟΣ"), "οδοσ");
        assert_eq!(lowercased("İ").chars().count(), 2);
    }
}
