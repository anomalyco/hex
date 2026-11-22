//! Swipe typing: reads a finger's path across the letter keys as a word.
//!
//! A shape-matching decoder after SHARK2 (Kristensson and Zhai, 2004) and the template
//! matching of Gboard's published gesture decoders, written for Hex:
//! 1. The path is resampled to evenly spaced points in key pitches (a key's width plus a gap
//!    across, a row's height plus a gap down), so a slip of half a key counts the same either
//!    way.
//! 2. Words come from the typing model's word list, which is sorted letter by letter: the
//!    words starting on a key near where the finger went down are one range of it per letter.
//!    Of those, only words ending on a key near where it lifted, with every key near the path
//!    and an ideal path (the polyline through their keys' centres) about as long, are scored.
//! 3. Each is scored on where its ideal path runs against the gesture (location: a quick
//!    point-by-point pass, then dynamic time warping for the best few, which follows a finger
//!    that hurries and slows), on its shape alone (both paths moved to one centre and size),
//!    and on how near the finger went down and lifted to its first and last keys;
//! 4. the best few also on how their keys account for where the finger turned and where it
//!    slowed (the gesture's evidence, from the touches' times): a turn or a pause needs a key
//!    of the word near it, and a key the word goes straight through needs a pause on it. A
//!    straight stroke runs over the letters between its ends, so the path alone can't tell
//!    "tired" from "tied" (one line from i to e) or "wire" from "write"; the finger slowing on
//!    r can,
//! 5. and weighed with the model's frequency of the word after the word before.
//!
//! Nothing is built ahead or kept: each swipe reads the memory-mapped model in place, so the
//! decoder holds no memory between swipes.
//!
//! Ported line by line from the Swift `SwipeDecoder` (Hex for iOS's, before it called this):
//! every reading is the Swift's, the same candidates in the same order with costs equal bit
//! for bit (`tests/swipe_parity.rs`), so the arithmetic keeps Swift's operation order,
//! Swift's `min`/`max` and its stable sorts. Tune it here.
//!
//! API contract shared by the FFI (`hex-keyboard-ffi`) and the port; the signatures change
//! only by agreement.

// Ported line by line: `!(a <= b)` is Swift's `guard a <= b else`, which gives up on a NaN
// too, and the loops index as Swift's do.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

mod geometry;
mod gesture;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::model::{Model, SortedWords};
use crate::never_suggest::NeverSuggest;
use geometry::{
    Point, distance, distance_to_polyline, max, mean_squared_distance, min, nearest, resample,
    shape_distance, sort_by_less, warped_distance,
};
use gesture::{Gesture, evidence_cost};

/// How swipes are read: lengths in key pitches (a key's width plus a gap across, a row's
/// height plus a gap down), costs in nats. Tuned on synthetic swipes of the autocorrect
/// benchmark's held-out dev sentences (English and Spanish; clean, neat, typical and sloppy
/// fingers; experiment folder 2026-11-20-hex-swipe-typing), never on its test sentences; the
/// turn and pause parameters also on mined dev sets of words that share a path, with fingers
/// that stop at every key, keep most of their speed through a straight-on key, or hesitate
/// between keys. The C interface's `HkSwipeParameters` and Swift's `SwipeDecoder.Parameters`
/// mirror it field for field.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
    /// Points the gesture and each word's ideal path are resampled to.
    pub samples: usize,
    /// Keys this many pitches from where the finger went down (lifted) may start (end)
    /// the word: the nearest few.
    pub start_radius: f64,
    pub end_radius: f64,
    pub max_start_keys: usize,
    pub max_end_keys: usize,
    /// Every key of a word passes at most this far from the path (pitches).
    pub near_radius: f64,
    /// How much shorter or longer than the gesture a word's ideal path may be.
    pub min_length_ratio: f64,
    pub max_length_ratio: f64,
    /// How far from its first and last keys' centres a finger goes down and lifts: most
    /// within the first spread, the rest (`stray`) within the wider second, so a near
    /// miss costs much and a far one no more than the path's shape can make up.
    pub start_sigma: f64,
    pub end_sigma: f64,
    pub start_stray_sigma: f64,
    pub end_stray_sigma: f64,
    pub stray: f64,
    /// Nats per mean squared pitch between the paths, and per mean squared distance
    /// between their shapes (each scaled to a unit box).
    pub location_weight: f64,
    pub shape_weight: f64,
    /// Weight of the word model's cost, -ln P(word | word before).
    pub lm_weight: f64,
    /// Words of the quick pass the aligned pass scores again.
    pub shortlist: usize,
    /// Past this many words, only the likeliest by how near their keys lie to the path
    /// (and their frequency) get the quick pass, which keeps a long, loose swipe fast.
    pub prefilter: usize,
    /// How far (a share of the samples) the aligned pass may run ahead or behind.
    pub band: f64,
    /// The shortest path read as a swipe (pitches).
    pub minimum_length: f64,
    /// A contraction ("I'm") whose first word ("I") never follows the word before in the
    /// model's pairs costs this much more: those first words follow thousands of words, so
    /// a missing pair is evidence against it ("with us in", not "with us I'm").
    pub contraction_missing_pair: f64,
    // Where the finger turned or slowed (`Evidence`): the keys of the word read must
    // account for them, in order.
    /// The path is read for turns and pauses every this many pitches.
    pub evidence_step: f64,
    /// A turn is the change of direction between this much path before a point and after it:
    /// none under `turn_from` degrees, full evidence from `turn_full`.
    pub turn_window: f64,
    pub turn_from: f64,
    pub turn_full: f64,
    /// A pause is time spent per pitch over this much path each side, against the swipe's
    /// median: none under `dwell_from` times the median, full evidence from `dwell_full`.
    pub dwell_window: f64,
    pub dwell_from: f64,
    pub dwell_full: f64,
    /// Weaker turns and pauses are ignored, and of two nearer than `evidence_spacing`
    /// (pitches along the path) only the stronger counts; none counts this near either end,
    /// which the first and last keys account for.
    pub evidence_floor: f64,
    pub evidence_spacing: f64,
    pub evidence_guard: f64,
    /// How far a key may lie from the turn or pause it accounts for (pitches): a spread,
    /// and the farthest.
    pub evidence_sigma: f64,
    pub evidence_reach: f64,
    /// Nats for a full-strength turn or pause no key of the word accounts for (the finger
    /// slowed on a letter the word lacks), for a key the word goes straight through with
    /// no pause there (only when the path has times), and for a key the word turns at
    /// without a turn or pause there.
    pub unexplained_cost: f64,
    pub straight_key_cost: f64,
    pub corner_key_cost: f64,
    /// A word's key it turns at by less than this (degrees) is one it goes straight through.
    pub straight_angle: f64,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            samples: 32,
            start_radius: 1.6,
            end_radius: 2.1,
            max_start_keys: 4,
            max_end_keys: 8,
            near_radius: 1.5,
            min_length_ratio: 0.4,
            max_length_ratio: 1.8,
            start_sigma: 0.3,
            end_sigma: 0.3,
            start_stray_sigma: 0.8,
            end_stray_sigma: 0.9,
            stray: 0.25,
            location_weight: 30.0,
            shape_weight: 60.0,
            lm_weight: 1.0,
            shortlist: 80,
            prefilter: 1500,
            band: 0.1,
            minimum_length: 0.6,
            contraction_missing_pair: 3.0,
            evidence_step: 0.1,
            turn_window: 0.2,
            turn_from: 35.0,
            turn_full: 120.0,
            dwell_window: 0.1,
            dwell_from: 1.3,
            dwell_full: 4.0,
            evidence_floor: 0.1,
            evidence_spacing: 0.8,
            evidence_guard: 0.7,
            evidence_sigma: 0.35,
            evidence_reach: 1.4,
            unexplained_cost: 6.0,
            straight_key_cost: 2.5,
            corner_key_cost: 2.0,
            straight_angle: 20.0,
        }
    }
}

impl Parameters {
    /// -ln of a point landing `squared` (pitches²) from a key's centre, under the mixture of
    /// a tight spread and a stray one; 0 at the centre.
    fn cost(&self, squared: f64, sigma: f64, straying_sigma: f64) -> f64 {
        let near = (1.0 - self.stray) / (sigma * sigma);
        let far = self.stray / (straying_sigma * straying_sigma);
        let density = near * (-squared / (2.0 * sigma * sigma)).exp()
            + far * (-squared / (2.0 * straying_sigma * straying_sigma)).exp();
        ((near + far) / max(density, f64::MIN_POSITIVE)).ln()
    }
}

/// The letter keys a swipe is read against (`TypingDecoder.Keys`: only the centres and pitch
/// matter to a swipe), in the touch's coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    /// Each letter key's character and centre, each character once (Swift's are a
    /// dictionary); in any order.
    pub centres: Vec<(char, f64, f64)>,
    /// Key width plus gap across; row height plus gap down.
    pub pitch: (f64, f64),
}

/// One word read beside the model's (`SwipeDecoder.ExtraWords`): an English contraction or
/// one of your own words, with its cost (-ln P(word)).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraWord {
    pub text: String,
    pub cost: f64,
    /// Whether its first word ("i" for "I'm") weighs it after the word before.
    pub contraction: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The word as the model spells it (lowercase, accents, apostrophes), or an extra word as
    /// given.
    pub text: String,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    /// Best first.
    pub candidates: Vec<Candidate>,
    /// Words that passed the key filters and were scored.
    pub scored: usize,
}

/// `SwipeDecoder.ExtraWords`, built once: each word's codes, text, cost, the model's index
/// of the same word (for its pairs) and a contraction's first word in the model.
#[derive(Debug, Clone, Default)]
struct ExtraWords {
    codes: Vec<Vec<u8>>,
    texts: Vec<String>,
    costs: Vec<f64>,
    model_index: Vec<Option<usize>>,
    heads: Vec<Option<usize>>,
}

impl ExtraWords {
    /// `ExtraWords.add` over every word in order: a word with a letter the model lacks is
    /// left out, and a repeat (by its codes) once.
    fn new(words: Vec<ExtraWord>, model: &Model) -> Self {
        let mut out = Self::default();
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        for word in words {
            // Swift's `lowercased()` is the full default lowercase mapping, as `to_lowercase`
            // is, except that Rust also maps a word-final Σ to ς: only Greek has it, and the
            // swipe models (English and Spanish) have no Greek letters.
            let lower = word.text.to_lowercase();
            let Some(encoded) = model.encode(&lower) else {
                continue;
            };
            // Swift asks `word.count >= 2` (Characters). Each Character of an encoded word is
            // one letter code (`Model::encode` matches Swift's canonically equivalent lookup),
            // and lowercasing never splits or joins Characters, so that's the codes' count.
            if encoded.len() < 2 || !seen.insert(encoded.clone()) {
                continue;
            }
            out.model_index.push(model.index_of_codes(&encoded));
            out.heads.push(if word.contraction {
                head_of(&lower).and_then(|head| model.index(head))
            } else {
                None
            });
            out.codes.push(encoded);
            out.texts.push(word.text);
            out.costs.push(word.cost);
        }
        out
    }
}

/// A contraction's first word: "do" for "don't", "i" for "i'm" (`ExtraWords.head(of:)`).
fn head_of(contraction: &str) -> Option<&str> {
    if let Some(head) = contraction.strip_suffix("n't") {
        return Some(head);
    }
    contraction.find('\'').map(|at| &contraction[..at])
}

/// A decoder can't be changed once made: a new word list makes a new decoder, so a read in
/// progress on another thread keeps the one it started with.
#[derive(Debug, Clone)]
pub struct SwipeDecoder {
    model: Arc<Model>,
    parameters: Parameters,
    never_suggest: NeverSuggest,
    extra: ExtraWords,
    /// The model's words that `extra` also has, read only as extra words.
    extra_indices: HashSet<usize>,
}

impl SwipeDecoder {
    /// `extra` in order: a repeat of an earlier word (by its letters) is left out, as is a word
    /// with a letter the model lacks (`ExtraWords.add`).
    pub fn new(
        model: Arc<Model>,
        parameters: Parameters,
        never_suggest: NeverSuggest,
        extra: Vec<ExtraWord>,
    ) -> Self {
        let extra = ExtraWords::new(extra, &model);
        let extra_indices = extra.model_index.iter().flatten().copied().collect();
        Self {
            model,
            parameters,
            never_suggest,
            extra,
            extra_indices,
        }
    }

    pub fn model(&self) -> &Arc<Model> {
        &self.model
    }

    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }

    pub fn never_suggest(&self) -> &NeverSuggest {
        &self.never_suggest
    }

    /// The likeliest words for a swipe along `path` (the touch's points, first to last, in the
    /// keys' coordinates), best first; empty when it can't be read as one. `times` (seconds,
    /// one per point) show where the finger paused; without them only its turns count.
    pub fn read(
        &self,
        path: &[(f64, f64)],
        times: Option<&[f64]>,
        keys: &Keys,
        previous: Option<&str>,
        limit: usize,
    ) -> Reading {
        let p = &self.parameters;
        let model = &*self.model;
        let none = Reading::default();
        let pitch = Point::new(keys.pitch.0, keys.pitch.1);
        if !(pitch.x > 0.0) || !(pitch.y > 0.0) || path.len() < 2 || limit == 0 || p.samples < 4 {
            return none;
        }
        let grid = KeyGrid::new(keys, model, pitch);
        if grid.centres.len() < 2 || grid.centres.len() > 64 {
            return none;
        }
        let Some(gesture) = Gesture::new(path, times, pitch, p) else {
            return none;
        };
        if !(gesture.length >= p.minimum_length) {
            return none;
        }
        let (start, end) = (gesture.raw[0], gesture.raw[gesture.raw.len() - 1]);

        // 1. The keys the word may start and end on, and how near the path passes each key.
        let key_count = grid.centres.len();
        let near: Vec<f64> = grid
            .centres
            .iter()
            .map(|&c| distance_to_polyline(c, &gesture.raw))
            .collect();
        let mut start_cost = vec![f64::INFINITY; key_count];
        let mut end_cost = vec![f64::INFINITY; key_count];
        let starts = nearest(&grid.centres, start, p.start_radius, p.max_start_keys);
        for &(k, squared) in &starts {
            start_cost[k] = p.cost(squared, p.start_sigma, p.start_stray_sigma);
        }
        let mut end_mask: u64 = 0;
        for (k, squared) in nearest(&grid.centres, end, p.end_radius, p.max_end_keys) {
            end_cost[k] = p.cost(squared, p.end_sigma, p.end_stray_sigma);
            end_mask |= 1 << k;
        }
        if starts.is_empty() || end_mask == 0 {
            return none;
        }

        // 2. Words that start on a start key, end on an end key, keep near the path and are
        // about as long: their keys (repeats run together, apostrophes left out). An id is a
        // model word's index, or -1 - n for extra word n.
        let mut found = Found::default();
        let filter = Filter {
            grid: &grid,
            near: &near,
            start_cost: &start_cost,
            end_mask,
            gesture_length: gesture.length,
            p,
        };
        for &(s, _) in &starts {
            for &code in &grid.codes_of_key[s] {
                let (lo, hi) = model.narrow(0, model.word_count(), 0, i32::from(code));
                for i in lo..hi {
                    if !self.never_suggest.words.contains(&i) && !self.extra_indices.contains(&i) {
                        filter.consider(model.codes(i), i as i64, &mut found);
                    }
                }
            }
        }
        for (n, codes) in self.extra.codes.iter().enumerate() {
            filter.consider(codes, -1 - n as i64, &mut found);
        }
        if found.words.is_empty() {
            return none;
        }

        // 3. A quick point-by-point pass over them all, then the aligned pass for the best.
        let n = p.samples;
        let mut template = vec![Point::default(); n];
        let mut vertices = [Point::default(); 64];
        let mut rows = (vec![0.0; n], vec![0.0; n]);
        let evidence_rows = gesture.evidence.len() + 1;
        let mut evidence_rows = (vec![0.0; evidence_rows], vec![0.0; evidence_rows]);
        // Swift's `Int(_:)` truncates toward zero, as `as` does.
        let band = 1.max((p.band * n as f64) as usize);
        let mut spatial = |c: usize, aligned: bool| -> f64 {
            let (from, to) = (found.offsets[c], found.offsets[c + 1]);
            for v in from..to {
                vertices[v - from] = grid.centres[found.sequence[v] as usize];
            }
            let line = &vertices[..to - from];
            resample(line, found.lengths[c], &mut template);
            let location = if aligned {
                warped_distance(&gesture.points, &template, band, &mut rows)
            } else {
                mean_squared_distance(&gesture.points, &template)
            };
            let shape = shape_distance(&gesture.shape, &template);
            let evidence = if aligned {
                evidence_cost(line, &gesture, p, &mut evidence_rows)
            } else {
                0.0
            };
            start_cost[found.sequence[from] as usize]
                + end_cost[found.sequence[to - 1] as usize]
                + p.location_weight * location
                + p.shape_weight * shape
                + evidence
        };
        let mut pool: Vec<usize> = (0..found.words.len()).collect();
        if pool.len() > p.prefilter {
            let rough: Vec<f64> = pool
                .iter()
                .map(|&c| {
                    let (from, to) = (found.offsets[c], found.offsets[c + 1]);
                    start_cost[found.sequence[from] as usize]
                        + end_cost[found.sequence[to - 1] as usize]
                        + p.location_weight * found.nearness[c]
                        + p.lm_weight * self.lm_cost(found.words[c], None)
                })
                .collect();
            sort_by_less(&mut pool, |&a, &b| rough[a] < rough[b]);
            pool.truncate(p.prefilter);
        }
        let mut quick: Vec<(usize, f64)> = Vec::with_capacity(pool.len());
        for &c in &pool {
            quick.push((
                c,
                spatial(c, false) + p.lm_weight * self.lm_cost(found.words[c], None),
            ));
        }
        sort_by_less(&mut quick, |a, b| a.1 < b.1);

        let previous_word = previous.and_then(|w| model.index(&w.to_lowercase()));
        let mut scored: Vec<(i64, f64)> = Vec::new();
        for &(c, _) in quick.iter().take(limit.max(p.shortlist)) {
            let word = found.words[c];
            if word >= 0 && self.never_suggest.follows(word as usize, previous_word) {
                continue;
            }
            scored.push((
                word,
                spatial(c, true) + p.lm_weight * self.lm_cost(word, previous_word),
            ));
        }
        sort_by_less(&mut scored, |a, b| a.1 < b.1);
        let candidates = scored
            .iter()
            .take(limit)
            .map(|&(word, cost)| Candidate {
                text: if word >= 0 {
                    model.word(word as usize)
                } else {
                    self.extra.texts[(-1 - word) as usize].clone()
                },
                cost,
            })
            .collect();
        Reading {
            candidates,
            scored: found.words.len(),
        }
    }

    /// -ln P(word | previous) with stupid backoff (0.4) to the unigram, as Spatial
    /// autocorrect's; an extra word's own cost when lower.
    fn lm_cost(&self, id: i64, previous: Option<usize>) -> f64 {
        let model = &*self.model;
        let of_model = |word: usize| -> f64 {
            let backoff = 0.92 + model.unigram_cost(word);
            match previous.and_then(|previous| model.bigram_cost(previous, word)) {
                Some(bigram) => min(bigram, backoff),
                None => backoff,
            }
        };
        if id >= 0 {
            return of_model(id as usize);
        }
        let n = (-1 - id) as usize;
        let mut own = 0.92 + self.extra.costs[n];
        // A contraction is as much likelier after the word before as its first word is.
        if let (Some(head), Some(previous)) = (self.extra.heads[n], previous) {
            let alone = 0.92 + model.unigram_cost(head);
            own += model
                .bigram_cost(previous, head)
                .unwrap_or(alone + self.parameters.contraction_missing_pair)
                - alone;
        }
        match self.extra.model_index[n] {
            Some(index) => min(own, of_model(index)),
            None => own,
        }
    }
}

/// The words that passed `Filter::consider`: ids, their keys run together in `sequence`
/// (word c's from `offsets[c]` to `offsets[c + 1]`), their ideal paths' lengths, and the
/// mean squared distance of their keys from the path, for the prefilter.
struct Found {
    words: Vec<i64>,
    offsets: Vec<usize>,
    lengths: Vec<f64>,
    sequence: Vec<u8>,
    nearness: Vec<f64>,
}

impl Default for Found {
    fn default() -> Self {
        Self {
            words: Vec::new(),
            offsets: vec![0],
            lengths: Vec::new(),
            sequence: Vec::new(),
            nearness: Vec::new(),
        }
    }
}

/// The key filters of `read`'s step 2 (Swift's nested `consider`).
struct Filter<'a> {
    grid: &'a KeyGrid,
    near: &'a [f64],
    start_cost: &'a [f64],
    end_mask: u64,
    gesture_length: f64,
    p: &'a Parameters,
}

impl Filter<'_> {
    fn consider(&self, codes: &[u8], id: i64, found: &mut Found) {
        let grid = self.grid;
        if codes.len() < 2 {
            return; // Swift: no code past the first to end on.
        }
        let mut j = codes.len() - 1;
        while j > 0 && grid.key_of_code[codes[j] as usize] == KeyGrid::SKIP {
            j -= 1;
        }
        if j == 0 {
            return;
        }
        let end_key = grid.key_of_code[codes[j] as usize];
        if end_key < 0 || self.end_mask & (1 << end_key) == 0 {
            return;
        }
        let mut scratch = [0u8; 64];
        let mut count = 0;
        let mut last: i64 = -1;
        let mut length = 0.0;
        let mut squares = 0.0;
        for &c in codes {
            let k = i64::from(grid.key_of_code[c as usize]);
            if k == i64::from(KeyGrid::SKIP) || k == last {
                continue;
            }
            if k < 0 || count >= scratch.len() || !(self.near[k as usize] <= self.p.near_radius) {
                return;
            }
            if last >= 0 {
                length += distance(grid.centres[last as usize], grid.centres[k as usize]);
            }
            squares += self.near[k as usize] * self.near[k as usize];
            scratch[count] = k as u8;
            count += 1;
            last = k;
        }
        if count < 2 || !self.start_cost[scratch[0] as usize].is_finite() {
            return;
        }
        let ratio = length / self.gesture_length;
        if !(ratio >= self.p.min_length_ratio) || !(ratio <= self.p.max_length_ratio) {
            return;
        }
        found.sequence.extend_from_slice(&scratch[..count]);
        found.words.push(id);
        found.lengths.push(length);
        found.offsets.push(found.sequence.len());
        found.nearness.push(squares / count as f64);
    }
}

/// The layout's letter keys in pitches, and which key types each of the model's letters: its
/// own, else its unaccented letter's (é on e); the apostrophe has none and is skipped.
struct KeyGrid {
    centres: Vec<Point>,
    key_of_code: Vec<i8>,
    codes_of_key: Vec<Vec<u8>>,
}

impl KeyGrid {
    const SKIP: i8 = -1;
    const MISSING: i8 = -2;

    fn new(keys: &Keys, model: &Model, pitch: Point) -> Self {
        // Swift sorts its keys by `Character`'s `<`, which for these one-scalar letters is
        // their scalars' order.
        let mut sorted = keys.centres.clone();
        sort_by_less(&mut sorted, |a, b| a.0 < b.0);
        let mut index = HashMap::new();
        let mut centres = Vec::with_capacity(sorted.len());
        for &(character, x, y) in &sorted {
            index.insert(character, centres.len());
            centres.push(Point::new(x / pitch.x, y / pitch.y));
        }
        let mut key_of_code = vec![Self::MISSING; model.code_count()];
        let mut codes_of_key = vec![Vec::new(); centres.len()];
        for code in 1..model.code_count() {
            if Some(code as u8) == model.apostrophe_code() {
                key_of_code[code] = Self::SKIP;
                continue;
            }
            let own = index.get(&model.character(code));
            let base = || index.get(&model.character(model.base_code(code) as usize));
            let Some(&key) = own.or_else(base) else {
                continue;
            };
            if key >= 64 {
                continue;
            }
            key_of_code[code] = key as i8;
            codes_of_key[key].push(code as u8);
        }
        Self {
            centres,
            key_of_code,
            codes_of_key,
        }
    }
}

/// The cost (-ln P) of a common English contraction (`SwipeDecoder.contractionCost`).
pub const CONTRACTION_COST: f64 = 7.5;
/// The cost of one of your own words (`SwipeDecoder.ownWordCost`).
pub const OWN_WORD_COST: f64 = 11.0;

const COMMONEST_CONTRACTIONS: [&str; 10] = [
    "i'm", "don't", "it's", "i'll", "can't", "that's", "didn't", "you're", "i've", "doesn't",
];
const COMMON_CONTRACTIONS: [&str; 19] = [
    "isn't",
    "there's",
    "let's",
    "won't",
    "what's",
    "he's",
    "she's",
    "wasn't",
    "i'd",
    "we're",
    "they're",
    "haven't",
    "couldn't",
    "wouldn't",
    "aren't",
    "you'll",
    "we'll",
    "shouldn't",
    "you've",
];

/// `SwipeDecoder.costOfContraction(_:base:)`: the commonest ("I'm", "don't", "it's") like a
/// word ranked about 300th, common ones at `base`, the rest ("he'll", "ma'am") about
/// 10,000th.
pub fn cost_of_contraction(word: &str, base: f64) -> f64 {
    let lower = word.to_lowercase();
    if COMMONEST_CONTRACTIONS.contains(&lower.as_str()) {
        return base - 1.5;
    }
    if COMMON_CONTRACTIONS.contains(&lower.as_str()) {
        return base;
    }
    base + 2.0
}
