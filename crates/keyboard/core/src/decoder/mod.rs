//! Spatial autocorrect: a noisy-channel word decoder for taps, ported from `TypingDecoder`
//! (`ios/HEXShared/FullKeyboard/TypingDecoder.swift`). Each tap is scored against every
//! letter with a 2D Gaussian around the key's centre (shifted by where you tend to tap, when
//! known), words are searched in the model's sorted word list with a beam, and a bigram
//! language model on the word before weighs them. A missed space is tried by splitting the
//! taps in two. What you typed is replaced only when the best reading beats it by a margin.
//!
//! Where Swift walks a dictionary, readings that cost exactly the same come out in an order
//! that changes from run to run; here they come out in a fixed one (`search`,
//! `exact_segmentation`). Everything else matches the Swift step for step.

mod keys;
mod merge;
mod search;
mod split;
pub(crate) mod text;

use std::sync::Arc;

pub use keys::{KeyStats, Keys, LayoutKey, Rect, Split, offsets};
pub use search::UserWords;
pub use text::without_accents;

use crate::model::Model;
use crate::never_suggest::NeverSuggest;
use keys::swift_min;

/// `TypingDecoder.Parameters`, field for field, with the same defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
    /// Touch spread as a share of the key pitch (key width plus gap; row height plus gap).
    pub sigma_x: f64,
    pub sigma_y: f64,
    pub lm_weight: f64,
    pub deletion_cost: f64,
    pub insertion_cost: f64,
    pub transposition_cost: f64,
    pub space_omission_cost: f64,
    /// A tap on the bottom row's upper half read as a space the finger missed ("ofbthe"):
    /// this, plus `space_tap_weight` times how much nearer its letter row than the space bar
    /// it landed. Below the row's centre such a tap costs `space_omission_cost`.
    pub space_tap_cost: f64,
    pub space_tap_weight: f64,
    /// A space tap is read as a letter meant on the row above ("deli ate" for "delicate")
    /// only when the row is at most this many nats less likely than the space bar for it.
    pub merge_reach: f64,
    pub max_edits: usize,
    pub beam: usize,
    /// Nats the best reading must beat what you typed by to replace it on space.
    pub auto_margin: f64,
    /// The same when what you typed is itself a word.
    pub valid_margin: f64,
    /// Language-model cost of a word the model lacks that the dictionary, your learned words
    /// or the shorthand list accept, and of one nothing accepts.
    pub unknown_valid_cost: f64,
    pub unknown_cost: f64,
    pub max_tap_cost: f64,
    pub splits: bool,
    /// Extra cost of an accented letter typed as its base letter's key (é from e).
    pub accent_cost: f64,
    /// A one-letter word stands alone in a split only when commoner than this (-ln p).
    pub one_letter_word_cost: f64,
    /// Language-model cost of one of your own words the model lacks (-ln p).
    pub user_word_cost: f64,
    /// The margin when the best reading only adds or moves accents on what you typed
    /// ("esta" -> "está"); ñ for n isn't one ("ano", "año").
    pub accent_margin: f64,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            sigma_x: 0.29,
            sigma_y: 0.26,
            lm_weight: 1.25,
            deletion_cost: 7.0,
            insertion_cost: 7.0,
            transposition_cost: 5.0,
            space_omission_cost: 6.0,
            space_tap_cost: 5.0,
            space_tap_weight: 0.25,
            merge_reach: 3.0,
            max_edits: 1,
            beam: 48,
            auto_margin: 2.0,
            valid_margin: 7.0,
            unknown_valid_cost: 15.0,
            unknown_cost: 22.0,
            max_tap_cost: 9.0,
            splits: true,
            accent_cost: 1.0,
            one_letter_word_cost: 7.0,
            user_word_cost: 11.0,
            accent_margin: 2.0,
        }
    }
}

impl Parameters {
    /// The settings tuned for a language's model on the benchmark's dev sentences: English
    /// leans more on the word before; Spanish's many one- and two-letter words make a letter
    /// read as a space too cheap a split ("vaxes" -> "va es"), so there only below the row's
    /// centre.
    pub fn tuned(language: &str) -> Self {
        let mut p = Self::default();
        if language == "en" {
            p.lm_weight = 1.5;
        }
        if language == "es" {
            p.space_tap_cost = f64::INFINITY;
        }
        p
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub text: String,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// What space should put in place of what you typed, or `None` to keep it.
    pub replacement: Option<String>,
    /// Readings for the suggestion strip, best first: better than what you typed, or a
    /// missed-space fallback that costs more than the flat cost of a word nothing knows.
    pub readings: Vec<Reading>,
    pub literal_cost: f64,
}

#[derive(Debug, Clone)]
pub struct TypingDecoder {
    model: Arc<Model>,
    pub parameters: Parameters,
    /// Your own words, searched beside the model's.
    pub user_words: UserWords,
    /// Words never offered (`OffensiveWords` for the language and Suggest swear words).
    pub never_suggest: NeverSuggest,
}

/// The taps, typed letters and literal reading `decide` and `replaces` share.
struct Literal {
    letters: Vec<u8>,
    costs: Vec<Vec<f64>>,
    prev: Option<usize>,
    index: Option<usize>,
    is_yours: bool,
    cost: f64,
}

impl TypingDecoder {
    /// `parameters` `None` takes `Parameters::tuned` for the model's language.
    pub fn new(model: Arc<Model>, parameters: Option<Parameters>) -> Self {
        let parameters = parameters.unwrap_or_else(|| Parameters::tuned(model.language()));
        Self {
            model,
            parameters,
            user_words: UserWords::default(),
            never_suggest: NeverSuggest::default(),
        }
    }

    pub fn model(&self) -> &Arc<Model> {
        &self.model
    }

    /// What you typed read as it stands: `None` when the model can't read it (a letter it
    /// lacks, an apostrophe, taps that don't match, a tap no key can explain).
    fn literal(
        &self,
        taps: &[(f64, f64)],
        typed: &str,
        previous: Option<&str>,
        literal_is_valid: bool,
        keys: &Keys,
    ) -> Option<Literal> {
        let model = &*self.model;
        let p = &self.parameters;
        if taps.len() != text::count(typed) || typed.is_empty() {
            return None;
        }
        let letters = model.encode(typed)?;
        if letters.iter().any(|&c| Some(c) == model.apostrophe_code()) {
            return None;
        }
        let centres = self.code_centres(keys);
        let costs: Vec<Vec<f64>> = taps
            .iter()
            .map(|&t| self.tap_costs(t, &centres, keys))
            .collect();
        let prev = previous.and_then(|w| model.index(w));
        let mut cost = 0.0;
        for (i, &code) in letters.iter().enumerate() {
            let c = costs[i][usize::from(code)];
            if !c.is_finite() {
                return None;
            }
            cost += c;
        }
        let index = model.index_of_codes(&letters);
        let is_yours = self.user_words.index(&letters).is_some();
        let mut lm = match index {
            Some(i) => self.lm_cost(i, prev),
            None if literal_is_valid => p.unknown_valid_cost,
            None => p.unknown_cost,
        };
        if is_yours {
            lm = swift_min(lm, p.user_word_cost);
        }
        cost += p.lm_weight * lm;
        Some(Literal {
            letters,
            costs,
            prev,
            index,
            is_yours,
            cost,
        })
    }

    /// `typed` is the lowercase letters the keys resolved, one per tap (`taps`, in the keys'
    /// coordinates). `None` when the model can't read it (a letter it lacks, or taps that
    /// don't match).
    pub fn decide(
        &self,
        taps: &[(f64, f64)],
        typed: &str,
        previous: Option<&str>,
        literal_is_valid: bool,
        keys: &Keys,
    ) -> Option<Decision> {
        let model = &*self.model;
        let p = &self.parameters;
        let literal = self.literal(taps, typed, previous, literal_is_valid, keys)?;
        let Literal {
            letters,
            costs,
            prev,
            index: literal_index,
            is_yours: literal_is_yours,
            cost: literal_cost,
        } = literal;
        let never = &self.never_suggest;

        let mut readings: Vec<Reading> = Vec::new();
        let all = 0..costs.len();
        for (word, spatial) in self.search(
            model,
            &costs,
            all.clone(),
            5,
            p.max_edits,
            Some(&never.words),
        ) {
            if Some(word) == literal_index || never.follows(word, prev) {
                continue;
            }
            readings.push(Reading {
                text: model.word(word),
                cost: spatial + p.lm_weight * self.lm_cost(word, prev),
            });
        }
        if !self.user_words.is_empty() {
            for (word, spatial) in self.search(&self.user_words, &costs, all, 3, p.max_edits, None)
            {
                if self.user_words.codes(word) == letters.as_slice() {
                    continue;
                }
                readings.push(Reading {
                    text: self.user_words.text(word).to_owned(),
                    cost: spatial + p.lm_weight * p.user_word_cost,
                });
            }
        }
        if p.splits && letters.len() >= 3 {
            readings.extend(self.split_readings(&costs, taps, &letters, keys, prev));
        }
        readings.sort_by(|a, b| search::ascending(a.cost, b.cost));
        let typed_key = text::canonical(typed);
        let mut seen = std::collections::HashSet::new();
        readings.retain(|r| {
            let lower = text::canonical(&text::lowercased(&r.text));
            lower != typed_key && r.cost < literal_cost && seen.insert(lower)
        });

        let mut replacement = None;
        if let Some(best) = readings.first() {
            let mut margin = if literal_index.is_some() || literal_is_valid || literal_is_yours {
                p.valid_margin
            } else {
                p.auto_margin
            };
            if without_accents(&text::lowercased(&best.text)) == without_accents(typed) {
                margin = swift_min(margin, p.accent_margin);
            }
            if literal_cost - best.cost >= margin {
                replacement = Some(best.text.clone());
            }
        }
        // Nothing above could read a word that is no word and no dictionary's: a missed
        // space may still explain it, with a slip in the second word or three or more words
        // run together. Never second-guess a dictionary word, one of yours, or a likelier
        // reading that only missed the margin. German is left out: its model is lowercase and
        // its nouns take their capital in the single-word path.
        if replacement.is_none()
            && literal_index.is_none()
            && !literal_is_valid
            && !literal_is_yours
            && model.language() != "de"
            && p.splits
            && letters.len() >= 5
            && let Some(fallback) = self.missed_space_reading(&costs, &letters, prev, literal_cost)
        {
            let fallback_key = text::canonical(&text::lowercased(&fallback.text));
            let takes = match readings.first() {
                None => true,
                Some(best) => {
                    fallback.cost <= best.cost
                        || text::canonical(&text::lowercased(&best.text)) == fallback_key
                }
            };
            if takes {
                replacement = Some(fallback.text.clone());
                readings.retain(|r| text::canonical(&text::lowercased(&r.text)) != fallback_key);
                readings.insert(0, fallback);
            }
        }
        readings.truncate(3);
        Some(Decision {
            replacement,
            readings,
            literal_cost,
        })
    }

    /// -ln P(word | previous) with stupid backoff (0.4) to the unigram.
    fn lm_cost(&self, word: usize, prev: Option<usize>) -> f64 {
        let backoff = 0.92 + self.model.unigram_cost(word);
        match prev.and_then(|prev| self.model.bigram_cost(prev, word)) {
            Some(bigram) => swift_min(bigram, backoff),
            None => backoff,
        }
    }

    /// Key centres by the model's letter codes; `None` for letters without a key.
    fn code_centres(&self, keys: &Keys) -> Vec<Option<(f64, f64)>> {
        let mut out = vec![None; self.model.code_count()];
        for &(c, x, y) in &keys.centres {
            if let Some(code) = self.model.code_of(c) {
                out[usize::from(code)] = Some((x, y));
            }
        }
        out
    }

    /// Each letter code's cost for one tap, in nats above the likeliest letter's. An accented
    /// letter costs at most its base letter's plus `accent_cost` (é on an English or Spanish
    /// layout, ñ typed with n); one with neither key is impossible (infinity).
    fn tap_costs(&self, tap: (f64, f64), centres: &[Option<(f64, f64)>], keys: &Keys) -> Vec<f64> {
        let p = &self.parameters;
        let sx = keys.pitch.0 * p.sigma_x;
        let sy = keys.pitch.1 * p.sigma_y;
        let mut out: Vec<f64> = centres
            .iter()
            .map(|centre| match centre {
                None => f64::INFINITY,
                Some((cx, cy)) => {
                    let dx = (tap.0 - cx) / sx;
                    let dy = (tap.1 - cy) / sy;
                    0.5 * (dx * dx + dy * dy)
                }
            })
            .collect();
        // Also from its base key even when it has its own (ñ from n on the Spanish layout):
        // whichever is cheaper.
        let raw = out.clone();
        for code in 1..out.len() {
            let base = usize::from(self.model.base_code(code));
            if base != code && raw[base].is_finite() {
                out[code] = swift_min(raw[code], raw[base] + p.accent_cost);
            }
        }
        let best = out
            .iter()
            .copied()
            .filter(|c| c.is_finite())
            .reduce(swift_min)
            .unwrap_or(0.0);
        for c in &mut out {
            *c -= best;
        }
        out
    }

    /// A word that may stand alone in a split: two or more letters, or a common one-letter
    /// word ("a" and "i" in English; "y", "a", "o" in Spanish).
    fn can_stand_alone(&self, i: usize) -> bool {
        use crate::model::SortedWords;
        self.model.length(i) >= 2
            || self.model.unigram_cost(i) < self.parameters.one_letter_word_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(language: &str) -> Arc<Model> {
        let path = format!(
            "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        Arc::new(Model::new(Box::new(bytes)).expect("a valid model"))
    }

    /// QWERTY on a 390 pt iPhone, roughly: rows offset half a column and one and a half.
    fn keys() -> Keys {
        let mut centres = Vec::new();
        for (row, (letters, offset)) in [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)]
            .into_iter()
            .enumerate()
        {
            for (i, c) in letters.chars().enumerate() {
                centres.push((
                    c,
                    19.5 + 39.0 * (i as f64 + offset),
                    30.0 + 54.0 * row as f64,
                ));
            }
        }
        Keys {
            centres,
            pitch: (39.0, 54.0),
            bottom_row_y: 138.0,
            space: Some(Rect {
                x: 100.0,
                y: 167.0,
                width: 190.0,
                height: 42.0,
            }),
        }
    }

    fn taps(word: &str, keys: &Keys) -> Vec<(f64, f64)> {
        word.chars()
            .map(|c| {
                let &(_, x, y) = keys.centres.iter().find(|k| k.0 == c).expect("a key");
                (x, y)
            })
            .collect()
    }

    #[test]
    fn reads_slips_and_keeps_words() {
        let decoder = TypingDecoder::new(model("en"), None);
        let keys = keys();
        let read = |typed: &str, previous: Option<&str>| {
            decoder
                .decide(&taps(typed, &keys), typed, previous, false, &keys)
                .expect("a decision")
        };
        assert_eq!(read("teh", None).replacement.as_deref(), Some("the"));
        assert_eq!(read("hello", None).replacement, None);
        assert_eq!(
            read("seeyoutomorrow", None).replacement.as_deref(),
            Some("see you tomorrow")
        );
        assert!(
            decoder
                .decide(&taps("the", &keys), "th", None, false, &keys)
                .is_none()
        );
        let teh = taps("teh", &keys);
        assert_eq!(
            decoder.replaces("the", &teh, "teh", None, false, &keys),
            Some(true)
        );
        assert_eq!(
            decoder.replaces("the end", &teh, "teh", None, false, &keys),
            None
        );
    }

    #[test]
    fn tuned_for_each_language() {
        assert_eq!(Parameters::tuned("en").lm_weight, 1.5);
        assert!(Parameters::tuned("es").space_tap_cost.is_infinite());
        assert_eq!(Parameters::tuned("de"), Parameters::default());
    }
}
