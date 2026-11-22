//! The keyboard-distance cost of a one-edit typo (`AutocorrectEngine+Distance`): what cautious
//! autocorrect, bilingual choices and the strip's spelling guesses measure a slip by.

use super::language::{Language, LetterLayout};
use super::text;

/// A weighted cost when `source` and `target` are within one Damerau-Levenshtein edit
/// (`singleEditKeyboardCost`), else `None`.
pub fn single_edit_keyboard_cost(
    source: &str,
    target: &str,
    language: Language,
    letters: Option<LetterLayout>,
) -> Option<f64> {
    if text::eq(source, target) {
        return Some(0.0);
    }
    let s = text::chars(source);
    let t = text::chars(target);
    if matches!(
        language,
        Language::Polish | Language::Greek | Language::Czech
    ) && s.len() == t.len()
    {
        let same = if language == Language::Greek {
            same_greek_letter_ignoring_accents
        } else {
            same_polish_letter_ignoring_diacritics
        };
        let missed: Vec<(&str, &str)> = s
            .iter()
            .zip(&t)
            .filter(|(a, b)| !text::eq(a, b))
            .map(|(a, b)| (*a, *b))
            .collect();
        if !missed.is_empty() && missed.iter().all(|(a, b)| same(a, b)) {
            return Some(0.15 + 0.05 * (missed.len() - 1) as f64);
        }
    }
    let ds = s.len() as isize - t.len() as isize;
    if ds.abs() > 1 {
        return None;
    }
    if ds == 0 {
        let mut diffs: Vec<usize> = Vec::new();
        for i in 0..s.len() {
            if !text::eq(s[i], t[i]) {
                diffs.push(i);
                if diffs.len() > 2 {
                    return None;
                }
            }
        }
        if diffs.len() == 1 {
            let i = diffs[0];
            if same_letter_ignoring_accents(s[i], t[i]) {
                return Some(0.15);
            }
            return Some(if are_adjacent_keys(s[i], t[i], language, letters) {
                0.65
            } else {
                1.15
            });
        }
        if diffs.len() == 2
            && diffs[1] == diffs[0] + 1
            && text::eq(s[diffs[0]], t[diffs[1]])
            && text::eq(s[diffs[1]], t[diffs[0]])
        {
            return Some(0.60);
        }
        return None;
    }
    let (shorter, longer) = if s.len() < t.len() {
        (&s, &t)
    } else {
        (&t, &s)
    };
    let (mut i, mut j) = (0, 0);
    let mut skipped: Option<usize> = None;
    while i < shorter.len() && j < longer.len() {
        if text::eq(shorter[i], longer[j]) {
            i += 1;
            j += 1;
        } else if skipped.is_none() {
            skipped = Some(j);
            j += 1;
        } else {
            return None;
        }
    }
    let extra_idx = skipped.unwrap_or(longer.len() - 1);
    let extra = longer[extra_idx];
    let left = extra_idx > 0 && are_adjacent_keys(extra, longer[extra_idx - 1], language, letters);
    let right = extra_idx + 1 < longer.len()
        && are_adjacent_keys(extra, longer[extra_idx + 1], language, letters);
    Some(if left || right { 0.70 } else { 0.90 })
}

/// Whether two letters differ only by an accent, diaeresis or tilde (e/é, u/ü, n/ñ).
pub fn same_letter_ignoring_accents(a: &str, b: &str) -> bool {
    if text::eq(a, b) {
        return false;
    }
    text::decomposed_base(a) == text::decomposed_base(b)
}

fn is_pair(a: &str, b: &str, x: &str, y: &str) -> bool {
    (text::eq(a, x) && text::eq(b, y)) || (text::eq(a, y) && text::eq(b, x))
}

/// One Polish letter with or without its diacritic (a/ą, z/ż, l/ł).
pub fn same_polish_letter_ignoring_diacritics(a: &str, b: &str) -> bool {
    same_letter_ignoring_accents(a, b) || is_pair(a, b, "l", "ł")
}

/// One Greek letter with or without its tonos or diaeresis, or σ and ς.
pub fn same_greek_letter_ignoring_accents(a: &str, b: &str) -> bool {
    same_letter_ignoring_accents(a, b) || is_pair(a, b, "σ", "ς")
}

/// Whether two keys neighbour on `language`'s keys in `letters`' arrangement.
pub fn are_adjacent_keys(
    a: &str,
    b: &str,
    language: Language,
    letters: Option<LetterLayout>,
) -> bool {
    let keys = language.cached_key_coordinates(letters);
    let (Some(pa), Some(pb)) = (
        keys.get(text::nfc(a).as_ref()),
        keys.get(text::nfc(b).as_ref()),
    ) else {
        return false;
    };
    let dx = pa.1 - pb.1;
    let dy = pa.0 - pb.0;
    (dx * dx + dy * dy).sqrt() <= 1.6
}
