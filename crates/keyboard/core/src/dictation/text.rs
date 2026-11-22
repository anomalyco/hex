//! Swift's `String` and `Character` semantics, which the rules were written against: a
//! `Character` is an extended grapheme cluster, equality is canonical equivalence, case
//! mappings go scalar by scalar (no final sigma), and Foundation's trims work on scalars.

use std::borrow::Cow;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use super::unicode;

/// A string's Characters.
pub(super) fn chars(text: &str) -> Vec<&str> {
    text.graphemes(true).collect()
}

/// A string's Characters, owned, for the rules that edit them in place.
pub(super) fn owned_chars(text: &str) -> Vec<String> {
    text.graphemes(true).map(str::to_owned).collect()
}

/// `String(chars)`.
pub(super) fn join<S: AsRef<str>>(chars: &[S]) -> String {
    let mut out = String::new();
    for c in chars {
        out.push_str(c.as_ref());
    }
    out
}

fn first_scalar(c: &str) -> char {
    c.chars().next().unwrap_or('\0')
}

fn is_single_scalar(c: &str) -> bool {
    c.chars().nth(1).is_none()
}

/// `Character.isLetter`: the first scalar is Alphabetic.
pub(super) fn is_letter(c: &str) -> bool {
    unicode::is_letter(first_scalar(c))
}

/// `Character.isNumber`: the first scalar has a numeric type.
pub(super) fn is_number(c: &str) -> bool {
    unicode::is_number(first_scalar(c))
}

pub(super) fn is_letter_or_number(c: &str) -> bool {
    let scalar = first_scalar(c);
    unicode::is_letter(scalar) || unicode::is_number(scalar)
}

/// `Character.isWhitespace`: the first scalar is White_Space.
pub(super) fn is_whitespace(c: &str) -> bool {
    unicode::is_white_space(first_scalar(c))
}

/// `Character.isNewline`.
pub(super) fn is_newline(c: &str) -> bool {
    unicode::is_newline(first_scalar(c))
}

/// `Character.isASCII`: one ASCII scalar, or CR-LF.
pub(super) fn is_ascii(c: &str) -> bool {
    c == "\r\n" || (c.len() == 1 && c.as_bytes()[0] < 0x80)
}

/// `Character.isUppercase`.
pub(super) fn is_uppercase(c: &str) -> bool {
    if is_single_scalar(c) && unicode::is_uppercase(first_scalar(c)) {
        return true;
    }
    same(c, &uppercased(c)) && is_cased(c)
}

/// `Character.isLowercase`.
pub(super) fn is_lowercase(c: &str) -> bool {
    if is_single_scalar(c) && unicode::is_lowercase(first_scalar(c)) {
        return true;
    }
    same(c, &lowercased(c)) && is_cased(c)
}

/// `Character.isCased`.
pub(super) fn is_cased(c: &str) -> bool {
    if is_single_scalar(c) && unicode::is_cased(first_scalar(c)) {
        return true;
    }
    !same(c, &uppercased(c)) || !same(c, &lowercased(c))
}

/// `String.lowercased()`: each scalar's full lowercase mapping.
pub(super) fn lowercased(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.chars().flat_map(char::to_lowercase).collect()
}

/// `String.uppercased()`: each scalar's full uppercase mapping.
pub(super) fn uppercased(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_uppercase();
    }
    text.chars().flat_map(char::to_uppercase).collect()
}

/// The form Swift compares strings in (NFC).
pub(super) fn canon(text: &str) -> Cow<'_, str> {
    if text.is_ascii() || unicode_normalization::is_nfc(text) {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.nfc().collect())
    }
}

/// Swift's `==` on strings and Characters: canonical equivalence.
pub(super) fn same(a: &str, b: &str) -> bool {
    a == b || (!(a.is_ascii() && b.is_ascii()) && canon(a) == canon(b))
}

/// `set.contains(character)` for a string of one-scalar Characters.
pub(super) fn is_one_of(c: &str, set: &str) -> bool {
    if c.len() == 1 {
        return set.as_bytes().contains(&c.as_bytes()[0]) && c.is_ascii();
    }
    if is_single_scalar(c) && set.contains(first_scalar(c)) {
        return true;
    }
    let key = canon(c);
    let mut keys = key.chars();
    match (keys.next(), keys.next()) {
        (Some(only), None) => set.contains(only),
        _ => false,
    }
}

/// Whether a Character is `mark`.
pub(super) fn is(c: &str, mark: char) -> bool {
    let mut buffer = [0; 4];
    same(c, mark.encode_utf8(&mut buffer))
}

/// `text.hasPrefix(prefix)`, Character by Character.
pub(super) fn has_prefix(text: &str, prefix: &str) -> bool {
    if text.is_ascii() && prefix.is_ascii() {
        return text.starts_with(prefix) && !splits_crlf(text, prefix.len());
    }
    let mut own = text.graphemes(true);
    for p in prefix.graphemes(true) {
        match own.next() {
            Some(c) if same(c, p) => {}
            _ => return false,
        }
    }
    true
}

/// `text.hasSuffix(suffix)`, Character by Character.
pub(super) fn has_suffix(text: &str, suffix: &str) -> bool {
    if text.is_ascii() && suffix.is_ascii() {
        return text.ends_with(suffix) && !splits_crlf(text, text.len() - suffix.len());
    }
    let mut own = text.graphemes(true).rev();
    for s in suffix.graphemes(true).rev() {
        match own.next() {
            Some(c) if same(c, s) => {}
            _ => return false,
        }
    }
    true
}

/// Whether an offset in ASCII text falls inside a "\r\n", which is one Character.
fn splits_crlf(text: &str, at: usize) -> bool {
    at > 0 && at < text.len() && text.as_bytes()[at - 1] == b'\r' && text.as_bytes()[at] == b'\n'
}

/// `text.count`.
pub(super) fn count(text: &str) -> usize {
    if text.is_ascii() {
        return text.len() - text.matches("\r\n").count();
    }
    text.graphemes(true).count()
}

/// `text.dropFirst(n)`.
pub(super) fn drop_first(text: &str, n: usize) -> &str {
    match text.grapheme_indices(true).nth(n) {
        Some((index, _)) => &text[index..],
        None => "",
    }
}

/// `text.prefix(n)`.
pub(super) fn prefix(text: &str, n: usize) -> &str {
    match text.grapheme_indices(true).nth(n) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

/// `text.first`.
pub(super) fn first(text: &str) -> Option<&str> {
    text.graphemes(true).next()
}

/// `text.last`.
pub(super) fn last(text: &str) -> Option<&str> {
    text.graphemes(true).next_back()
}

/// `text.removeLast()`.
pub(super) fn remove_last(text: &mut String) {
    if let Some(c) = last(text) {
        let len = text.len() - c.len();
        text.truncate(len);
    }
}

/// `text.dropLast()`.
pub(super) fn drop_last(text: &str) -> &str {
    match last(text) {
        Some(c) => &text[..text.len() - c.len()],
        None => text,
    }
}

/// `text.trimmingCharacters(in: .whitespaces)`, scalar by scalar.
pub(super) fn trim_whitespaces(text: &str) -> &str {
    text.trim_matches(unicode::is_horizontal_whitespace)
}

/// `text.trimmingCharacters(in: .whitespacesAndNewlines)`.
pub(super) fn trim_whitespaces_and_newlines(text: &str) -> &str {
    text.trim_matches(unicode::is_whitespace_or_newline)
}

/// `text.components(separatedBy: "\n")`: literal, so "\r\n" splits too.
pub(super) fn lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// Where `needle` first occurs in `text`, Character by Character (Foundation's
/// `range(of:)` and `components(separatedBy:)` without options), as byte offsets.
pub(super) fn find(text: &str, needle: &str) -> Option<(usize, usize)> {
    find_from(text, needle, 0)
}

fn find_from(text: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    if needle.is_empty() || from >= text.len() {
        return None;
    }
    let wanted: Vec<&str> = needle.graphemes(true).collect();
    let own: Vec<(usize, &str)> = text[from..].grapheme_indices(true).collect();
    if own.len() < wanted.len() {
        return None;
    }
    'start: for start in 0..=own.len() - wanted.len() {
        for (offset, w) in wanted.iter().enumerate() {
            if !same(own[start + offset].1, w) {
                continue 'start;
            }
        }
        let (begin, _) = own[start];
        let (last, c) = own[start + wanted.len() - 1];
        return Some((from + begin, from + last + c.len()));
    }
    None
}

/// `text.contains(needle)` for a string needle.
pub(super) fn contains(text: &str, needle: &str) -> bool {
    if text.is_ascii() && needle.is_ascii() {
        return text.match_indices(needle).any(|(start, _)| {
            !splits_crlf(text, start) && !splits_crlf(text, start + needle.len())
        });
    }
    find(text, needle).is_some()
}

/// `text.components(separatedBy: separator)` for a separator other than "\n".
pub(super) fn components<'a>(text: &'a str, separator: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0;
    while start < text.len() {
        match find_from(text, separator, start) {
            Some((lo, hi)) if hi > lo => {
                parts.push(&text[start..lo]);
                start = hi;
            }
            _ => break,
        }
    }
    parts.push(&text[start..]);
    parts
}

/// `text.split(separator: c)` (empty pieces dropped) or, with `keep_empty`, with them.
pub(super) fn split_on(text: &str, separator: char, keep_empty: bool) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (index, c) in text.grapheme_indices(true) {
        if is(c, separator) {
            if keep_empty || index > start {
                parts.push(&text[start..index]);
            }
            start = index + c.len();
        }
    }
    if keep_empty || start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

/// `"(prefix(1)).uppercased() + dropFirst()"`.
pub(super) fn capitalized_first(text: &str) -> String {
    let first = prefix(text, 1);
    uppercased(first) + &text[first.len()..]
}

/// `prefix(1).lowercased() + dropFirst()`.
pub(super) fn lowercased_first(text: &str) -> String {
    let first = prefix(text, 1);
    lowercased(first) + &text[first.len()..]
}
