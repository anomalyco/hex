//! The keyboard-distance cost of a one-edit typo (`AutocorrectEngine+Distance`): what cautious
//! autocorrect, bilingual choices and the strip's spelling guesses measure a slip by.

use super::language::{KeyGrid, LanguageConfig};
use super::text;

/// A weighted cost when `source` and `target` are within one Damerau-Levenshtein edit
/// (`singleEditKeyboardCost`), else `None`. `language` says which accents fold; `grid` holds
/// the keys a slip is measured on.
pub fn single_edit_keyboard_cost(
    source: &str,
    target: &str,
    language: &LanguageConfig,
    grid: &KeyGrid,
) -> Option<f64> {
    if text::eq(source, target) {
        return Some(0.0);
    }
    let s = text::chars(source);
    let t = text::chars(target);
    // A language whose letters fold: accents left out, however many, cost as one missing
    // Spanish accent, a little more for each further one (Polish "zolw" -> "żółw", Greek
    // "καλημερα" -> "καλημέρα", Czech "cesky" -> "český", Hungarian "koszonom" ->
    // "köszönöm"), and so do its `fold_pairs` ("l" for "ł", "σ" for "ς").
    if language.fold_diacritics && s.len() == t.len() {
        let same = |a: &str, b: &str| {
            same_letter_ignoring_accents(a, b)
                || language.fold_pairs.iter().any(|(x, y)| is_pair(a, b, x, y))
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
            return Some(if are_adjacent_keys(s[i], t[i], grid) {
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
    let left = extra_idx > 0 && are_adjacent_keys(extra, longer[extra_idx - 1], grid);
    let right =
        extra_idx + 1 < longer.len() && are_adjacent_keys(extra, longer[extra_idx + 1], grid);
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

/// Whether two keys neighbour on the host's grid.
pub fn are_adjacent_keys(a: &str, b: &str, grid: &KeyGrid) -> bool {
    let (Some(pa), Some(pb)) = (grid.get(a), grid.get(b)) else {
        return false;
    };
    let dx = pa.1 - pb.1;
    let dy = pa.0 - pb.0;
    (dx * dx + dy * dy).sqrt() <= 1.6
}
