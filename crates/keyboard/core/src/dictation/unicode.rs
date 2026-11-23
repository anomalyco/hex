//! What the rules read from Unicode, per scalar, as iOS has it. The tables come from iOS's own
//! data (`unicode_generated.rs`); case mappings and White_Space are Rust's, which match iOS's
//! (Unicode 17) for every scalar. `tests/dictation_parity.rs` checks all of it against a dump
//! of the iOS simulator's data.

use super::unicode_generated as tables;

fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let value = c as u32;
    ranges
        .binary_search_by(|&(lo, hi)| {
            if hi < value {
                std::cmp::Ordering::Less
            } else if lo > value {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Swift's `Unicode.Scalar.Properties.isAlphabetic` (`Character.isLetter`).
pub(super) fn is_letter(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    in_ranges(tables::LETTER, c)
}

/// Swift's `numericType != nil` (`Character.isNumber`).
pub(super) fn is_number(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    in_ranges(tables::NUMBER, c)
}

/// White_Space (`Character.isWhitespace`, ICU's `\s`).
pub(super) fn is_white_space(c: char) -> bool {
    c.is_whitespace()
}

/// `Character.isNewline`'s scalars.
pub(super) fn is_newline(c: char) -> bool {
    matches!(c, '\n'..='\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// The Uppercase, Lowercase and Cased properties of a scalar.
pub(super) fn is_uppercase(c: char) -> bool {
    c.is_uppercase()
}

pub(super) fn is_lowercase(c: char) -> bool {
    c.is_lowercase()
}

/// Cased: Uppercase, Lowercase or a titlecase letter (Lt).
pub(super) fn is_cased(c: char) -> bool {
    c.is_uppercase() || c.is_lowercase() || is_titlecase(c)
}

fn is_titlecase(c: char) -> bool {
    matches!(
        c,
        '\u{1C5}'
            | '\u{1C8}'
            | '\u{1CB}'
            | '\u{1F2}'
            | '\u{1F88}'..='\u{1F8F}'
            | '\u{1F98}'..='\u{1F9F}'
            | '\u{1FA8}'..='\u{1FAF}'
            | '\u{1FBC}'
            | '\u{1FCC}'
            | '\u{1FFC}'
    )
}

/// Foundation's `CharacterSet.whitespaces`: Zs, the tab and (Apple's) U+200B.
pub(super) fn is_horizontal_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// `CharacterSet.whitespacesAndNewlines`: `whitespaces`, LF to CR, NEL, LS and PS.
pub(super) fn is_whitespace_or_newline(c: char) -> bool {
    is_horizontal_whitespace(c) || is_newline(c)
}

/// ICU's `\p{L}`.
pub(super) fn icu_letter(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    in_ranges(tables::ICU_LETTER, c)
}

/// ICU's `\p{N}`.
pub(super) fn icu_number(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    in_ranges(tables::ICU_NUMBER, c)
}

/// The set ICU's `\b` calls word characters.
pub(super) fn icu_word(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '_';
    }
    in_ranges(tables::ICU_WORD, c)
}

/// Grapheme_Extend or Cf, which ICU's `\b` looks through.
pub(super) fn icu_extend(c: char) -> bool {
    !c.is_ascii() && in_ranges(tables::ICU_EXTEND, c)
}

/// CoreFoundation's Grapheme_Extend set.
pub(super) fn cf_extend(c: char) -> bool {
    !c.is_ascii() && in_ranges(tables::CF_EXTEND, c)
}

/// CoreFoundation's case- and diacritic-insensitive folding of one scalar, when it changes it.
pub(super) fn fold_scalar(c: char) -> Option<&'static str> {
    let value = c as u32;
    tables::FOLD
        .binary_search_by_key(&value, |&(key, _)| key)
        .ok()
        .map(|index| tables::FOLD[index].1)
}
