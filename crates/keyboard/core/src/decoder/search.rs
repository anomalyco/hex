//! The beam search over a sorted word list (`TypingDecoder.search`), and the word lists it
//! searches beside the model's: your own words (`UserWords`) and one word (`OneWord`).

use std::collections::{HashMap, HashSet};

use super::{TypingDecoder, text};
use crate::model::{Model, SortedWords};

/// Your own words for the decoder (Contacts names and other lexicon terms, text replacements,
/// words you kept, fixes Hex learned), as you spell them, each scored as a moderately common
/// word. Words with a letter the model lacks are left out (`TypingDecoder.UserWords`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserWords {
    codes: Vec<Vec<u8>>,
    texts: Vec<String>,
    lookup: HashMap<Vec<u8>, usize>,
}

impl UserWords {
    /// `words` in order: of two with the same letters once lowercased, the first is kept.
    pub fn new<S: AsRef<str>>(words: &[S], model: &Model) -> Self {
        let mut by_codes: HashMap<Vec<u8>, String> = HashMap::new();
        for word in words {
            let word = word.as_ref();
            if text::count(word) < 2
                || !text::characters(word).all(|c| c.is_alphabetic() || c == '\'')
            {
                continue;
            }
            let Some(encoded) = model.encode(&text::lowercased(word)) else {
                continue;
            };
            by_codes.entry(encoded).or_insert_with(|| word.to_owned());
        }
        let mut codes: Vec<Vec<u8>> = by_codes.keys().cloned().collect();
        codes.sort();
        let texts = codes.iter().map(|c| by_codes[c].clone()).collect();
        let lookup = codes
            .iter()
            .enumerate()
            .map(|(i, c)| (c.clone(), i))
            .collect();
        Self {
            codes,
            texts,
            lookup,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    pub fn codes(&self, i: usize) -> &[u8] {
        &self.codes[i]
    }

    pub fn text(&self, i: usize) -> &str {
        &self.texts[i]
    }

    pub fn index(&self, encoded: &[u8]) -> Option<usize> {
        self.lookup.get(encoded).copied()
    }
}

impl SortedWords for UserWords {
    fn word_count(&self) -> usize {
        self.codes.len()
    }

    fn length(&self, i: usize) -> usize {
        self.codes[i].len()
    }

    fn byte(&self, i: usize, depth: usize) -> i32 {
        self.codes[i].get(depth).map_or(-1, |&c| i32::from(c))
    }
}

/// A word list `search` reads, with what breaks a tie between two words found at exactly the
/// same cost: the commoner (lower) first, then the one earlier in the list.
pub(super) trait SearchWords: SortedWords {
    fn tie_rank(&self, _i: usize) -> f64 {
        0.0
    }
}

impl SearchWords for Model {
    fn tie_rank(&self, i: usize) -> f64 {
        self.unigram_cost(i)
    }
}

/// Your own words all count as equally common.
impl SearchWords for UserWords {}

impl SearchWords for OneWord<'_> {}

/// One word, searched as a word list.
pub(super) struct OneWord<'a>(pub &'a [u8]);

impl SortedWords for OneWord<'_> {
    fn word_count(&self) -> usize {
        1
    }

    fn length(&self, _: usize) -> usize {
        self.0.len()
    }

    fn byte(&self, _: usize, depth: usize) -> i32 {
        self.0.get(depth).map_or(-1, |&c| i32::from(c))
    }
}

/// A search state: the words `lo..hi` share the letters so far, `tap` taps are read, with
/// `edits` edits, at `cost` (kept in single precision, as Swift's `State` keeps it).
#[derive(Clone, Copy)]
struct State {
    lo: u32,
    hi: u32,
    tap: u16,
    edits: u8,
    cost: f32,
}

/// Swift's `a < b` as an ordering, for a stable sort with the same result as Swift's.
pub(super) fn ascending<T: PartialOrd>(a: T, b: T) -> std::cmp::Ordering {
    if a < b {
        std::cmp::Ordering::Less
    } else if b < a {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

impl TypingDecoder {
    /// The best words of `words` for `costs[range]`, as (word index, spatial cost incl.
    /// edits), leaving out `skipping` (searched through, never found).
    ///
    /// Words found at exactly the same cost come out commoner first, then by word index
    /// (`SearchWords::tie_rank`). Swift's order there is its dictionary's, which changes from
    /// run to run; it decides which of a tie the `limit` keeps (many words cost the same
    /// letter you didn't type), and the language model then ranks those, so the words it is
    /// likeliest to prefer are the ones kept.
    pub(super) fn search<W: SearchWords>(
        &self,
        words: &W,
        costs: &[Vec<f64>],
        range: std::ops::Range<usize>,
        limit: usize,
        max_edits: usize,
        skipping: Option<&HashSet<usize>>,
    ) -> Vec<(usize, f64)> {
        let p = &self.parameters;
        let model = &*self.model;
        let n = range.len();
        let max_depth = n + max_edits + 2;
        let mut layers: Vec<Vec<State>> = vec![Vec::new(); max_depth + 2];
        layers[0].push(State {
            lo: 0,
            hi: words.word_count() as u32,
            tap: 0,
            edits: 0,
            cost: 0.0,
        });
        let mut found: HashMap<usize, f64> = HashMap::new();
        let letter_choices: Vec<Vec<(usize, f64)>> = range
            .clone()
            .map(|i| {
                let mut choices: Vec<(usize, f64)> = costs[i]
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|&(code, c)| code > 0 && c <= p.max_tap_cost)
                    .collect();
                choices.sort_by(|a, b| ascending(a.1, b.1));
                choices
            })
            .collect();
        // Dropping a clearly aimed, distinct tap needs more evidence than dropping an
        // accidental repeat or a tap between keys. Accents share their physical key.
        let extra_tap_costs: Vec<f64> = if max_edits == 0 {
            Vec::new()
        } else {
            (0..letter_choices.len())
                .map(|tap| {
                    let Some(&(code, _)) = letter_choices[tap].first() else {
                        return p.insertion_cost;
                    };
                    let key = model.base_code(code);
                    for neighbor in [tap.wrapping_sub(1), tap + 1] {
                        if neighbor < letter_choices.len()
                            && letter_choices[neighbor]
                                .first()
                                .is_some_and(|&(other, _)| model.base_code(other) == key)
                        {
                            return p.insertion_cost;
                        }
                    }
                    let gap = letter_choices[tap]
                        .iter()
                        .find(|&&(c, _)| model.base_code(c) != key)
                        .map_or(p.max_tap_cost, |&(_, c)| c);
                    p.insertion_cost + super::keys::swift_min(4.0, gap)
                })
                .collect()
        };
        let apostrophe = model.apostrophe_code().map(usize::from);
        let all_codes: Vec<usize> = (1..model.code_count())
            .filter(|&c| Some(c) != apostrophe)
            .collect();
        for depth in 0..=max_depth {
            let mut layer = std::mem::take(&mut layers[depth]);
            if layer.len() > p.beam {
                layer.sort_by(|a, b| ascending(a.cost, b.cost));
                layer.truncate(p.beam);
            }
            let mut k = 0;
            while k < layer.len() {
                let s = layer[k];
                k += 1;
                let (lo, hi, tap) = (s.lo as usize, s.hi as usize, usize::from(s.tap));
                if tap == n
                    && words.length(lo) == depth
                    && !skipping.is_some_and(|skip| skip.contains(&lo))
                {
                    let c = f64::from(s.cost);
                    if c < found.get(&lo).copied().unwrap_or(f64::INFINITY) {
                        found.insert(lo, c);
                    }
                }
                if depth >= max_depth {
                    continue;
                }
                let next = &mut layers[depth + 1];
                let mut push = |code: usize, next_tap: usize, edits: u8, cost: f64| {
                    let (a, b) = words.narrow(lo, hi, depth, code as i32);
                    if a < b {
                        next.push(State {
                            lo: a as u32,
                            hi: b as u32,
                            tap: next_tap as u16,
                            edits,
                            cost: cost as f32,
                        });
                    }
                };
                let base = f64::from(s.cost);
                if tap < n {
                    for &(code, c) in &letter_choices[tap] {
                        push(code, tap + 1, s.edits, base + c);
                    }
                }
                // Apostrophes are never typed: "dont" reads as "don't" for free.
                if depth > 0
                    && let Some(apostrophe) = apostrophe
                {
                    push(apostrophe, tap, s.edits, base);
                }
                if usize::from(s.edits) >= max_edits {
                    continue;
                }
                let e = s.edits + 1;
                // A letter you didn't type.
                for &code in &all_codes {
                    push(code, tap, e, base + p.deletion_cost);
                }
                // A tap that isn't in the word.
                if tap < n {
                    layer.push(State {
                        lo: s.lo,
                        hi: s.hi,
                        tap: s.tap + 1,
                        edits: e,
                        cost: (base + extra_tap_costs[tap]) as f32,
                    });
                }
                // Two letters typed in the wrong order.
                if tap + 1 < n && depth + 2 <= max_depth {
                    for &(a, ca) in letter_choices[tap + 1].iter().take(3) {
                        let (lo2, hi2) = words.narrow(lo, hi, depth, a as i32);
                        if lo2 >= hi2 {
                            continue;
                        }
                        for &(b, cb) in letter_choices[tap].iter().take(3) {
                            if b == a {
                                continue;
                            }
                            let (lo3, hi3) = words.narrow(lo2, hi2, depth + 1, b as i32);
                            if lo3 >= hi3 {
                                continue;
                            }
                            layers[depth + 2].push(State {
                                lo: lo3 as u32,
                                hi: hi3 as u32,
                                tap: (tap + 2) as u16,
                                edits: e,
                                cost: (base + ca + cb + p.transposition_cost) as f32,
                            });
                        }
                    }
                }
            }
        }
        let mut out: Vec<(usize, f64)> = found.into_iter().collect();
        out.sort_by(|a, b| {
            ascending(a.1, b.1)
                .then(ascending(words.tie_rank(a.0), words.tie_rank(b.0)))
                .then(a.0.cmp(&b.0))
        });
        out.truncate(limit);
        out
    }
}
