//! Two questions besides `decide`: whether a space tap was a letter between two words
//! (`merge`), and whether `decide` would put one reading in place of what you typed
//! (`replaces`).

use super::keys::swift_min;
use super::search::OneWord;
use super::{Keys, TypingDecoder, text, without_accents};
use crate::model::SortedWords;

impl TypingDecoder {
    /// One word where a space was tapped for a letter on the row above ("deli ate" for
    /// "delicate", "a out" for "about"): the likeliest reading of the first word's taps, the
    /// space tap as a letter and the second word's taps, when it beats the two words as they
    /// read by the margin and the tap was near enough the row (`merge_reach`). `None` to keep
    /// them.
    #[allow(clippy::too_many_arguments)]
    pub fn merge(
        &self,
        first: &str,
        first_taps: &[(f64, f64)],
        space: (f64, f64),
        second: &str,
        second_taps: &[(f64, f64)],
        previous: Option<&str>,
        first_is_valid: bool,
        second_is_valid: bool,
        keys: &Keys,
    ) -> Option<String> {
        let model = &*self.model;
        let p = &self.parameters;
        let never = &self.never_suggest;
        let (first_count, second_count) = (text::count(first), text::count(second));
        if first_taps.len() != first_count || second_taps.len() != second_count || second.is_empty()
        {
            return None;
        }
        let first_codes = model.encode(&text::lowercased(first))?;
        let second_codes = model.encode(&text::lowercased(second))?;
        let reach = self.row_cost(space, keys).filter(|&r| r <= p.merge_reach)?;
        let taps: Vec<(f64, f64)> = first_taps
            .iter()
            .copied()
            .chain([space])
            .chain(second_taps.iter().copied())
            .collect();
        let centres = self.code_centres(keys);
        let costs: Vec<Vec<f64>> = taps
            .iter()
            .map(|&t| self.tap_costs(t, &centres, keys))
            .collect();
        let prev = previous.and_then(|w| model.index(w));
        // The two words as space would leave them: each as typed or its likeliest reading
        // ("i dkdnt" is "I didn't", never "obedient").
        let reading =
            |codes: &[u8], start: usize, end: usize, before: Option<usize>, valid: bool| {
                let typed = model.index_of_codes(codes);
                let mut spatial = 0.0;
                for (i, &code) in codes.iter().enumerate() {
                    spatial += costs[start + i][usize::from(code)];
                }
                let lm = match typed {
                    Some(t) => self.lm_cost(t, before),
                    None if valid => p.unknown_valid_cost,
                    None => p.unknown_cost,
                };
                let mut best = (spatial + p.lm_weight * lm, typed);
                for (word, s) in self.search(
                    model,
                    &costs,
                    start..end,
                    3,
                    p.max_edits,
                    Some(&never.words),
                ) {
                    let cost = s + p.lm_weight * self.lm_cost(word, before);
                    if cost < best.0 {
                        best = (cost, Some(word));
                    }
                }
                best
            };
        let first_reading = reading(&first_codes, 0, first_codes.len(), prev, first_is_valid);
        let second_reading = reading(
            &second_codes,
            first_codes.len() + 1,
            taps.len(),
            first_reading.1,
            second_is_valid,
        );
        let split = first_reading.0 + second_reading.0;
        if !split.is_finite() {
            return None;
        }
        let mut best: Option<(usize, f64)> = None;
        let longer = first_count.max(second_count);
        for (word, spatial) in self.search(
            model,
            &costs,
            0..costs.len(),
            3,
            p.max_edits,
            Some(&never.words),
        ) {
            if model.length(word) <= longer || never.follows(word, prev) {
                continue;
            }
            let cost = spatial + p.lm_weight * self.lm_cost(word, prev) + reach;
            if cost < best.map_or(f64::INFINITY, |b| b.1) {
                best = Some((word, cost));
            }
        }
        let best = best?;
        let margin = if first_is_valid && second_is_valid {
            p.valid_margin
        } else {
            p.auto_margin
        };
        (split - best.1 >= margin).then(|| model.word(best.0))
    }

    /// Whether `decide` with `keys` would put `reading` (one word) in place of what you
    /// typed: it beats the literal by the margin. `None` for a two-word reading, which only
    /// `decide` can tell. Adaptive touch asks with the centres read without it, so a
    /// correction it applies is one the keyboard would apply anyway.
    pub fn replaces(
        &self,
        reading: &str,
        taps: &[(f64, f64)],
        typed: &str,
        previous: Option<&str>,
        literal_is_valid: bool,
        keys: &Keys,
    ) -> Option<bool> {
        if reading.contains(' ') {
            return None;
        }
        let model = &*self.model;
        let p = &self.parameters;
        let reading_lower = text::lowercased(reading);
        // `decide`'s guards, then the reading's letters, before any tap is read.
        if taps.len() != text::count(typed) || typed.is_empty() {
            return Some(false);
        }
        let Some(letters) = model.encode(typed) else {
            return Some(false);
        };
        if letters.iter().any(|&c| Some(c) == model.apostrophe_code()) {
            return Some(false);
        }
        let Some(codes) = model.encode(&reading_lower) else {
            return Some(false);
        };
        let Some(literal) = self.literal(taps, typed, previous, literal_is_valid, keys) else {
            return Some(false);
        };
        let mut reading_lm = f64::INFINITY;
        if let Some(index) = model.index_of_codes(&codes)
            && !self.never_suggest.words.contains(&index)
        {
            reading_lm = self.lm_cost(index, literal.prev);
        }
        if self.user_words.index(&codes).is_some() {
            reading_lm = swift_min(reading_lm, p.user_word_cost);
        }
        if !reading_lm.is_finite() {
            return Some(false);
        }
        let mut margin = if literal.index.is_some() || literal_is_valid || literal.is_yours {
            p.valid_margin
        } else {
            p.auto_margin
        };
        if margin > p.accent_margin && without_accents(&reading_lower) == without_accents(typed) {
            margin = p.accent_margin;
        }
        // Usually one alignment the search would also try already clears it: no search then.
        if let Some(bound) = self.aligned_cost(&codes, &literal.costs)
            && literal.cost - (bound + p.lm_weight * reading_lm) >= margin
        {
            return Some(true);
        }
        let all = 0..literal.costs.len();
        let Some(&(_, spatial)) = self
            .search(&OneWord(&codes), &literal.costs, all, 1, p.max_edits, None)
            .first()
        else {
            return Some(false);
        };
        Some(literal.cost - (spatial + p.lm_weight * reading_lm) >= margin)
    }

    /// The cost of `codes` read letter for letter from the taps, or with one letter you
    /// didn't type at its cheapest place: never below what the search finds, so a bound to
    /// clear. `None` when neither fits (another edit, a tap past `max_tap_cost`).
    fn aligned_cost(&self, codes: &[u8], costs: &[Vec<f64>]) -> Option<f64> {
        let p = &self.parameters;
        let n = costs.len();
        let mut letters: Vec<usize> = Vec::with_capacity(codes.len());
        for (depth, &code) in codes.iter().enumerate() {
            if Some(code) == self.model.apostrophe_code() {
                if depth == 0 {
                    return None;
                }
                continue;
            }
            letters.push(usize::from(code));
        }
        let tap = |i: usize, code: usize| {
            let c = costs[i][code];
            (c <= p.max_tap_cost).then_some(c)
        };
        if letters.len() == n {
            let mut total = 0.0;
            for (i, &letter) in letters.iter().enumerate() {
                total += tap(i, letter)?;
            }
            return Some(total);
        }
        if letters.len() != n + 1 || p.max_edits < 1 {
            return None;
        }
        let mut prefix = vec![f64::INFINITY; n + 1];
        prefix[0] = 0.0;
        for i in 0..n {
            let Some(c) = tap(i, letters[i]) else {
                break;
            };
            prefix[i + 1] = prefix[i] + c;
        }
        let mut best = f64::INFINITY;
        let mut suffix = 0.0;
        for j in (0..=n).rev() {
            if j < n {
                let Some(c) = tap(j, letters[j + 1]) else {
                    break;
                };
                suffix += c;
            }
            best = swift_min(best, prefix[j] + suffix);
        }
        best.is_finite().then_some(best + p.deletion_cost)
    }
}
