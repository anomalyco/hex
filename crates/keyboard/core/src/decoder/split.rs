//! Readings with a missed space: two words split from the taps (`splitReadings`), and the
//! fallback for taps no word reads (`missedSpaceReading`: a slip in the second word, or
//! three to six words run together).

use std::collections::HashMap;
use std::ops::Range;

use super::keys::swift_max;
use super::{Keys, Reading, TypingDecoder};
use crate::model::SortedWords;

#[derive(Clone)]
struct SegmentPath {
    words: Vec<usize>,
    cost: f64,
}

impl TypingDecoder {
    /// Two words where the taps had no space, or one meant for space ("seehow", "seenhow").
    /// Each half is read with substitutions only, which keeps this cheap; around a tap on the
    /// bottom row's upper half read as a space ("ofbthe"), only as the letters typed, so a
    /// name's middle letter ("rima") is never a space with slips around it.
    pub(super) fn split_readings(
        &self,
        costs: &[Vec<f64>],
        taps: &[(f64, f64)],
        letters: &[u8],
        keys: &Keys,
        prev: Option<usize>,
    ) -> Vec<Reading> {
        let n = costs.len();
        let p = &self.parameters;
        let model = &*self.model;
        let never = &self.never_suggest;
        let mut out = Vec::new();
        let halves = |range: Range<usize>, exact: bool| -> Vec<(usize, f64)> {
            if !exact {
                return self.search(model, costs, range, 3, 0, Some(&never.words));
            }
            match model.index_of_codes(&letters[range.clone()]) {
                Some(word) if !never.words.contains(&word) => {
                    let spatial = range.fold(0.0, |sum, i| sum + costs[i][usize::from(letters[i])]);
                    vec![(word, spatial)]
                }
                _ => Vec::new(),
            }
        };
        let mut pair = |left: Range<usize>, right: Range<usize>, extra: f64, exact: bool| {
            if left.is_empty() || right.is_empty() {
                return;
            }
            let lefts: Vec<(usize, f64)> = halves(left, exact)
                .into_iter()
                .filter(|&(w, _)| self.can_stand_alone(w))
                .collect();
            if lefts.is_empty() {
                return;
            }
            let rights: Vec<(usize, f64)> = halves(right, exact)
                .into_iter()
                .filter(|&(w, _)| self.can_stand_alone(w))
                .collect();
            for &(l, ls) in &lefts {
                // The first word after the word before is checked as `decide`'s one-word
                // readings and `slipped_second_word` check theirs.
                if never.follows(l, prev) {
                    continue;
                }
                let lcost = ls + p.lm_weight * self.lm_cost(l, prev);
                for &(r, rs) in &rights {
                    if never.follows(r, Some(l)) {
                        continue;
                    }
                    let cost = lcost + rs + p.lm_weight * self.lm_cost(r, Some(l)) + extra;
                    out.push(Reading {
                        text: model.word(l) + " " + &model.word(r),
                        cost,
                    });
                }
            }
        };
        for (k, &tap) in taps.iter().enumerate().take(n).skip(1) {
            pair(0..k, k..n, p.space_omission_cost, false);
            if k >= n - 1 {
                continue;
            }
            if tap.1 > keys.bottom_row_y {
                pair(0..k, (k + 1)..n, p.space_omission_cost, false);
            } else if let Some(extra) = self.space_cost(tap, keys) {
                pair(0..k, (k + 1)..n, extra, true);
            }
        }
        out
    }

    /// The cost of reading a tap on the bottom row's upper half as a space the finger missed,
    /// or `None` when it can't be one: above the space bar, priced by how much nearer the
    /// row's centre than the bar's it landed.
    fn space_cost(&self, tap: (f64, f64), keys: &Keys) -> Option<f64> {
        let p = &self.parameters;
        if !p.space_tap_cost.is_finite() {
            return None;
        }
        let bar = keys.space.filter(|bar| bar.width > 0.0)?;
        let slack = keys.pitch.0 / 2.0;
        if !(tap.0 >= bar.min_x() - slack
            && tap.0 <= bar.max_x() + slack
            && tap.1 > keys.bottom_row_y - keys.pitch.1 / 2.0)
        {
            return None;
        }
        let sy = keys.pitch.1 * p.sigma_y;
        let to_bar = (tap.1 - bar.mid_y()) / sy;
        let to_row = (tap.1 - keys.bottom_row_y) / sy;
        let likelier_row = swift_max(0.0, 0.5 * (to_bar * to_bar - to_row * to_row));
        Some(p.space_tap_cost + p.space_tap_weight * likelier_row)
    }

    /// How much likelier the space bar than the bottom letter row a space tap was, in nats
    /// (0 when the row is likelier), or `None` when it isn't above a bottom-row letter.
    pub(super) fn row_cost(&self, tap: (f64, f64), keys: &Keys) -> Option<f64> {
        let bar = keys.space.filter(|bar| bar.width > 0.0)?;
        let slack = keys.pitch.0 / 2.0;
        if !(tap.0 >= bar.min_x() - slack && tap.0 <= bar.max_x() + slack) {
            return None;
        }
        let sy = keys.pitch.1 * self.parameters.sigma_y;
        let to_bar = (tap.1 - bar.mid_y()) / sy;
        let to_row = (tap.1 - keys.bottom_row_y) / sy;
        Some(swift_max(0.0, 0.5 * (to_row * to_row - to_bar * to_bar)))
    }

    /// What a missed space can still make of taps nothing above could read. `decide` asks
    /// only when the word is not a word, not a dictionary word and not one of yours, so this
    /// may take a reading the ordinary margin would not. The missed spaces are the hypothesis,
    /// not evidence: a reading is taken only when its words, without them, cost no more than
    /// the run nothing knows does.
    pub(super) fn missed_space_reading(
        &self,
        costs: &[Vec<f64>],
        letters: &[u8],
        prev: Option<usize>,
        literal_cost: f64,
    ) -> Option<Reading> {
        let slipped = self.slipped_second_word(costs, letters, prev);
        let segmented = self.exact_segmentation(costs, letters, prev);
        let affordable = |reading: Option<Reading>| -> Option<Reading> {
            let reading = reading?;
            let words = reading.text.split(' ').filter(|w| !w.is_empty()).count();
            let words_cost =
                reading.cost - self.parameters.space_omission_cost * (words as f64 - 1.0);
            (words_cost <= literal_cost).then_some(reading)
        };
        match (affordable(slipped), affordable(segmented)) {
            (Some(slipped), Some(segmented)) => Some(if segmented.cost < slipped.cost {
                segmented
            } else {
                slipped
            }),
            (slipped, segmented) => slipped.or(segmented),
        }
    }

    /// The first letters as a word and the rest read as one word after it, when the rest is
    /// one slip from a word and clears the margin it would get typed after that first word
    /// ("specialchsracter" -> "special character"). A rest that is already a word is left to
    /// `split_readings`. The first word is at least two letters: a lone "i" with a slip after
    /// it would read almost any run as two words.
    fn slipped_second_word(
        &self,
        costs: &[Vec<f64>],
        letters: &[u8],
        prev: Option<usize>,
    ) -> Option<Reading> {
        let n = letters.len();
        if n < 5 {
            return None;
        }
        let p = &self.parameters;
        let model = &*self.model;
        let never = &self.never_suggest;
        let mut best: Option<Reading> = None;
        for k in 2..=(n - 3) {
            let Some(left) = model.index_of_codes(&letters[..k]) else {
                continue;
            };
            if !self.can_stand_alone(left)
                || never.words.contains(&left)
                || never.follows(left, prev)
                || model.index_of_codes(&letters[k..]).is_some()
            {
                continue;
            }
            let (mut left_spatial, mut right_literal) = (0.0, 0.0);
            for i in 0..k {
                left_spatial += costs[i][usize::from(letters[i])];
            }
            for i in k..n {
                right_literal += costs[i][usize::from(letters[i])];
            }
            right_literal += p.lm_weight * p.unknown_cost;
            let left_cost = left_spatial + p.lm_weight * self.lm_cost(left, prev);
            for (right, spatial) in
                self.search(model, costs, k..n, 5, p.max_edits, Some(&never.words))
            {
                if never.follows(right, Some(left)) {
                    continue;
                }
                let right_cost = spatial + p.lm_weight * self.lm_cost(right, Some(left));
                if right_literal - right_cost < p.auto_margin {
                    continue;
                }
                let cost = left_cost + right_cost + p.space_omission_cost;
                if best.as_ref().is_none_or(|b| cost < b.cost) {
                    best = Some(Reading {
                        text: model.word(left) + " " + &model.word(right),
                        cost,
                    });
                }
            }
        }
        best
    }

    /// Three to six words typed with no spaces, every one spelled as typed
    /// ("seeyoutomorrow" -> "see you tomorrow"). Dynamic programming keeps the cheapest path
    /// per end position, last word and word count: two paths there can only continue through
    /// that last word, and only their count decides whether one can still grow.
    ///
    /// Swift walks each position's paths in its dictionary's order, so of two that cost
    /// exactly the same the one it keeps varies from run to run. Here they are walked by word
    /// count, then last word (by index), lowest first: the reading with fewer words, then the
    /// one earlier in the word list, wins a tie.
    fn exact_segmentation(
        &self,
        costs: &[Vec<f64>],
        letters: &[u8],
        prev: Option<usize>,
    ) -> Option<Reading> {
        let n = letters.len();
        if !(5..=32).contains(&n) {
            return None;
        }
        let p = &self.parameters;
        let model = &*self.model;
        let never = &self.never_suggest;
        let mut spatial = vec![0.0; n + 1];
        for i in 0..n {
            spatial[i + 1] = spatial[i] + costs[i][usize::from(letters[i])];
        }
        // Keyed by (word count, last word): -1 before the first word.
        let mut states: Vec<HashMap<(usize, i64), SegmentPath>> = vec![HashMap::new(); n + 1];
        states[0].insert(
            (0, -1),
            SegmentPath {
                words: Vec::new(),
                cost: 0.0,
            },
        );
        let mut best: Option<SegmentPath> = None;
        for start in 0..n {
            if states[start].is_empty() {
                continue;
            }
            // Every word that starts here, found by narrowing the model's list one letter at
            // a time: the prefix walk stops as soon as no word carries those letters.
            let mut ends: Vec<(usize, usize)> = Vec::new();
            let (mut lo, mut hi) = (0, model.word_count());
            for end in (start + 1)..=n {
                (lo, hi) = model.narrow(lo, hi, end - start - 1, i32::from(letters[end - 1]));
                if lo >= hi {
                    break;
                }
                if model.length(lo) == end - start
                    && self.can_stand_alone(lo)
                    && !never.words.contains(&lo)
                {
                    ends.push((end, lo));
                }
            }
            if ends.is_empty() {
                continue;
            }
            let mut paths: Vec<((usize, i64), SegmentPath)> = states[start].drain().collect();
            paths.sort_by_key(|(key, _)| *key);
            for ((count, last), path) in paths {
                let before = usize::try_from(last).ok().or(prev);
                for &(end, word) in &ends {
                    if never.follows(word, before) {
                        continue;
                    }
                    let mut words = path.words.clone();
                    words.push(word);
                    let cost = path.cost
                        + (spatial[end] - spatial[start])
                        + p.lm_weight * self.lm_cost(word, before)
                        + if count == 0 {
                            0.0
                        } else {
                            p.space_omission_cost
                        };
                    if end == n {
                        if words.len() >= 3
                            && words.len() <= 6
                            && best.as_ref().is_none_or(|b| cost < b.cost)
                        {
                            best = Some(SegmentPath { words, cost });
                        }
                        continue;
                    }
                    if words.len() >= 6 {
                        continue;
                    }
                    let next = (words.len(), word as i64);
                    if states[end]
                        .get(&next)
                        .is_some_and(|known| known.cost <= cost)
                    {
                        continue;
                    }
                    states[end].insert(next, SegmentPath { words, cost });
                }
            }
        }
        best.map(|b| Reading {
            text: b
                .words
                .iter()
                .map(|&w| model.word(w))
                .collect::<Vec<_>>()
                .join(" "),
            cost: b.cost,
        })
    }
}
