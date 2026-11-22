//! Swift's `String` and `Character` semantics and the Foundation calls autocorrect makes,
//! replicated: a `Character` is an extended grapheme cluster, strings and characters compare
//! by canonical equivalence (here: composed, NFC), and `lowercased()`/`uppercased()` map each
//! scalar without context. The parity test checks these against Swift's own answers.

use std::borrow::Cow;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use unicode_segmentation::UnicodeSegmentation;

/// The string's `Character`s.
pub fn chars(s: &str) -> Vec<&str> {
    s.graphemes(true).collect()
}

/// `s.count`.
pub fn count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// The composed form, which canonically equivalent strings share.
pub fn nfc(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfc().collect())
    }
}

/// Swift's `==` on strings and characters: canonical equivalence.
pub fn eq(a: &str, b: &str) -> bool {
    a == b || (!(a.is_ascii() && b.is_ascii()) && nfc(a) == nfc(b))
}

fn first_scalar(c: &str) -> char {
    c.chars().next().unwrap_or('\0')
}

fn single_scalar(c: &str) -> bool {
    let mut it = c.chars();
    it.next().is_some() && it.next().is_none()
}

/// `Character.isLetter`: its first scalar is alphabetic.
pub fn is_letter(c: &str) -> bool {
    first_scalar(c).is_alphabetic()
}

/// Ideographs with a numeric value (Unihan's numeric fields: Numeric_Type=Numeric, general
/// category Lo), which Swift counts as numbers and `char::is_numeric` doesn't. Unicode 16's
/// `DerivedNumericType.txt`.
const NUMERIC_IDEOGRAPHS: [char; 91] = [
    '\u{3405}',
    '\u{3483}',
    '\u{382A}',
    '\u{3B4D}',
    '\u{4E00}',
    '\u{4E03}',
    '\u{4E07}',
    '\u{4E09}',
    '\u{4E24}',
    '\u{4E5D}',
    '\u{4E8C}',
    '\u{4E94}',
    '\u{4E96}',
    '\u{4EAC}',
    '\u{4EBF}',
    '\u{4EC0}',
    '\u{4EDF}',
    '\u{4EE8}',
    '\u{4F0D}',
    '\u{4F70}',
    '\u{4FE9}',
    '\u{5006}',
    '\u{5104}',
    '\u{5146}',
    '\u{5169}',
    '\u{516B}',
    '\u{516D}',
    '\u{5341}',
    '\u{5343}',
    '\u{5344}',
    '\u{5345}',
    '\u{534C}',
    '\u{53C1}',
    '\u{53C2}',
    '\u{53C3}',
    '\u{53C4}',
    '\u{56DB}',
    '\u{58F1}',
    '\u{58F9}',
    '\u{5E7A}',
    '\u{5EFE}',
    '\u{5EFF}',
    '\u{5F0C}',
    '\u{5F0D}',
    '\u{5F0E}',
    '\u{5F10}',
    '\u{62D0}',
    '\u{62FE}',
    '\u{634C}',
    '\u{67D2}',
    '\u{6D1E}',
    '\u{6F06}',
    '\u{7396}',
    '\u{767E}',
    '\u{7695}',
    '\u{79ED}',
    '\u{8086}',
    '\u{842C}',
    '\u{8CAE}',
    '\u{8CB3}',
    '\u{8D30}',
    '\u{920E}',
    '\u{94A9}',
    '\u{9621}',
    '\u{9646}',
    '\u{964C}',
    '\u{9678}',
    '\u{96F6}',
    '\u{F96B}',
    '\u{F973}',
    '\u{F978}',
    '\u{F9B2}',
    '\u{F9D1}',
    '\u{F9D3}',
    '\u{F9FD}',
    '\u{20001}',
    '\u{20064}',
    '\u{200E2}',
    '\u{20121}',
    '\u{2092A}',
    '\u{20983}',
    '\u{2098C}',
    '\u{2099C}',
    '\u{20AEA}',
    '\u{20AFD}',
    '\u{20B19}',
    '\u{22390}',
    '\u{22998}',
    '\u{23B1B}',
    '\u{2626D}',
    '\u{2F890}',
];

/// `Character.isNumber`: its first scalar has a numeric type.
pub fn is_number(c: &str) -> bool {
    let f = first_scalar(c);
    f.is_numeric() || NUMERIC_IDEOGRAPHS.binary_search(&f).is_ok()
}

/// `Character.isWhitespace`.
pub fn is_whitespace(c: &str) -> bool {
    !c.is_empty() && first_scalar(c).is_whitespace()
}

/// `Character.isNewline`.
pub fn is_newline(c: &str) -> bool {
    matches!(
        first_scalar(c),
        '\u{A}'..='\u{D}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Titlecase letters (Lt), which are cased but neither upper- nor lowercase.
fn is_titlecase(c: char) -> bool {
    matches!(c,
        '\u{1C5}' | '\u{1C8}' | '\u{1CB}' | '\u{1F2}'
        | '\u{1F88}'..='\u{1F8F}' | '\u{1F98}'..='\u{1F9F}' | '\u{1FA8}'..='\u{1FAF}'
        | '\u{1FBC}' | '\u{1FCC}' | '\u{1FFC}')
}

fn is_cased(c: &str) -> bool {
    let f = first_scalar(c);
    if single_scalar(c) && (f.is_uppercase() || f.is_lowercase() || is_titlecase(f)) {
        return true;
    }
    !eq(c, &uppercased(c)) || !eq(c, &lowercased(c))
}

/// `Character.isUppercase`.
pub fn is_uppercase(c: &str) -> bool {
    if single_scalar(c) && first_scalar(c).is_uppercase() {
        return true;
    }
    !c.is_empty() && eq(c, &uppercased(c)) && is_cased(c)
}

/// `Character.isLowercase`.
pub fn is_lowercase(c: &str) -> bool {
    if single_scalar(c) && first_scalar(c).is_lowercase() {
        return true;
    }
    !c.is_empty() && eq(c, &lowercased(c)) && is_cased(c)
}

/// `lowercased()`: each scalar's full lowercase mapping, without context.
pub fn lowercased(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    s.chars().flat_map(char::to_lowercase).collect()
}

/// `uppercased()`: each scalar's full uppercase mapping.
pub fn uppercased(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_uppercase();
    }
    s.chars().flat_map(char::to_uppercase).collect()
}

/// The first `Character`, or "".
pub fn first(s: &str) -> &str {
    s.graphemes(true).next().unwrap_or("")
}

/// The last `Character`, or "".
pub fn last(s: &str) -> &str {
    s.graphemes(true).next_back().unwrap_or("")
}

/// `dropFirst()`: all but the first `Character`.
pub fn drop_first(s: &str) -> &str {
    &s[first(s).len()..]
}

/// `dropLast(n)`.
pub fn drop_last(s: &str, n: usize) -> &str {
    let mut end = s.len();
    for (i, _) in s.grapheme_indices(true).rev().take(n) {
        end = i;
    }
    &s[..end]
}

/// `suffix(n)`.
pub fn suffix(s: &str, n: usize) -> &str {
    let mut start = s.len();
    for (i, _) in s.grapheme_indices(true).rev().take(n) {
        start = i;
    }
    &s[start..]
}

/// `prefix(n)`.
pub fn prefix(s: &str, n: usize) -> &str {
    match s.grapheme_indices(true).nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// `hasPrefix`: bytes when both are ASCII, else `Character` by `Character`.
pub fn has_prefix(s: &str, p: &str) -> bool {
    if s.is_ascii() && p.is_ascii() {
        return s.as_bytes().starts_with(p.as_bytes());
    }
    let mut a = s.graphemes(true);
    p.graphemes(true)
        .all(|g| a.next().is_some_and(|c| eq(c, g)))
}

/// `hasSuffix`: bytes when both are ASCII, else `Character` by `Character` from the end.
pub fn has_suffix(s: &str, p: &str) -> bool {
    if s.is_ascii() && p.is_ascii() {
        return s.as_bytes().ends_with(p.as_bytes());
    }
    let mut a = s.graphemes(true).rev();
    p.graphemes(true)
        .rev()
        .all(|g| a.next().is_some_and(|c| eq(c, g)))
}

/// Whether one of `s`'s `Character`s is `c` (a one-character needle: `contains(" ")`).
pub fn contains_char(s: &str, c: &str) -> bool {
    if s.is_ascii() && c.is_ascii() {
        return s.contains(c);
    }
    s.graphemes(true).any(|g| eq(g, c))
}

/// `contains(where:)` over `Character`s.
pub fn any_char(s: &str, f: impl Fn(&str) -> bool) -> bool {
    s.graphemes(true).any(f)
}

/// `split(separator:)` (empty pieces left out) on one `Character`.
pub fn split_on<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    split_where(s, |g| eq(g, sep))
}

/// `split(whereSeparator:)`, empty pieces left out.
pub fn split_where(s: &str, is_separator: impl Fn(&str) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, g) in s.grapheme_indices(true) {
        if is_separator(g) {
            if i > start {
                out.push(&s[start..i]);
            }
            start = i + g.len();
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// `split(separator:, omittingEmptySubsequences: false)` on one `Character`.
pub fn split_keeping_empty<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, g) in s.grapheme_indices(true) {
        if eq(g, sep) {
            out.push(&s[start..i]);
            start = i + g.len();
        }
    }
    out.push(&s[start..]);
    out
}

/// `first.uppercased() + s.dropFirst()`.
pub fn capitalised_first(s: &str) -> String {
    let f = first(s);
    uppercased(f) + &s[f.len()..]
}

/// `first.lowercased() + s.dropFirst()`.
pub fn lowered_first(s: &str) -> String {
    let f = first(s);
    lowercased(f) + &s[f.len()..]
}

/// `s` decomposed and case-folded (full folding), as Foundation compares it, except the iota
/// subscript (U+0345), which Foundation keeps: "ᾀ" is not the fold of "ἈΙ".
fn folded(s: &str) -> String {
    use caseless::Caseless;
    let mut out = String::new();
    for c in s.nfd() {
        if c == '\u{345}' {
            out.push(c);
        } else {
            out.extend(std::iter::once(c).default_case_fold());
        }
    }
    out.nfd().collect()
}

/// `caseInsensitiveCompare(_:) == .orderedSame`: canonically equivalent once case-folded.
pub fn case_insensitive_eq(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    folded(a) == folded(b)
}

/// `folding(options: .diacriticInsensitive, locale: nil)`: each character decomposed and the
/// marks after its first scalar dropped (a lone mark keeps its first), as Foundation answers;
/// Greek's spacing breathings with an accent (U+1FCD–1FCF, U+1FDD–1FDF) stay whole.
pub fn without_diacritics(s: &str) -> String {
    if s.is_ascii() {
        return s.to_owned();
    }
    let mut out = String::new();
    for g in s.graphemes(true) {
        let mut d = g.nfd();
        if matches!(first_scalar(g), '\u{1FCD}'..='\u{1FCF}' | '\u{1FDD}'..='\u{1FDF}') {
            out.push_str(g);
            continue;
        }
        if let Some(first) = d.next() {
            out.push(first);
            out.extend(d.filter(|&c| !is_combining_mark(c)));
        }
    }
    out.nfc().collect()
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`: Foundation's set is White_Space and the
/// zero-width space.
pub fn trimmed(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{200B}')
}

/// `replacingOccurrences(of: "'", with: "")`.
pub fn without_apostrophes(s: &str) -> String {
    if s.is_ascii() {
        return s.replace('\'', "");
    }
    s.graphemes(true).filter(|g| *g != "'").collect()
}

/// The first scalar of a `Character`'s canonical decomposition.
pub fn decomposed_base(c: &str) -> Option<char> {
    c.nfd().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_are_grapheme_clusters() {
        assert_eq!(count("e\u{301}te"), 3);
        assert_eq!(drop_last("cafe\u{301}", 1), "caf");
        assert_eq!(suffix("cafe\u{301}", 1), "e\u{301}");
        assert!(eq("caf\u{e9}", "cafe\u{301}"));
        assert!(has_suffix("le caf\u{e9}", "cafe\u{301}"));
        assert!(!has_prefix("\u{e9}t\u{e9}", "e"));
    }

    #[test]
    fn character_properties() {
        assert!(is_uppercase("A") && !is_uppercase("a") && !is_uppercase("1"));
        assert!(is_uppercase("E\u{301}"));
        assert!(is_lowercase("\u{df}"));
        assert!(is_letter("\u{e9}") && !is_letter("'"));
        assert!(is_newline("\r\n") && is_whitespace("\u{a0}"));
    }

    #[test]
    fn foundation_calls() {
        assert!(case_insensitive_eq("Stra\u{df}e", "STRASSE"));
        assert!(case_insensitive_eq("I'd", "i'd"));
        assert_eq!(without_diacritics("voc\u{ea}"), "voce");
        assert_eq!(trimmed(" omw\n"), "omw");
        assert_eq!(without_apostrophes("don't"), "dont");
        assert_eq!(split_on("a  b", " "), ["a", "b"]);
        assert_eq!(split_keeping_empty("a\t\tb", "\t"), ["a", "", "b"]);
    }
}
