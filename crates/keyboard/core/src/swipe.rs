//! Swipe typing: reads a finger's path across the letter keys as a word. A shape-matching
//! decoder after SHARK2, ported from `SwipeDecoder` (`ios/HEXShared/FullKeyboard/SwipeDecoder.swift`),
//! which documents the method.
//!
//! API contract shared by the FFI (`hex-keyboard-ffi`) and the port. The bodies are filled in
//! by the port; the signatures change only by agreement.

use std::sync::Arc;

use crate::model::Model;
use crate::never_suggest::NeverSuggest;

/// `SwipeDecoder.Parameters`, field for field, with the same defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
    pub samples: usize,
    pub start_radius: f64,
    pub end_radius: f64,
    pub max_start_keys: usize,
    pub max_end_keys: usize,
    pub near_radius: f64,
    pub min_length_ratio: f64,
    pub max_length_ratio: f64,
    pub start_sigma: f64,
    pub end_sigma: f64,
    pub start_stray_sigma: f64,
    pub end_stray_sigma: f64,
    pub stray: f64,
    pub location_weight: f64,
    pub shape_weight: f64,
    pub lm_weight: f64,
    pub shortlist: usize,
    pub prefilter: usize,
    pub band: f64,
    pub minimum_length: f64,
    pub contraction_missing_pair: f64,
    pub evidence_step: f64,
    pub turn_window: f64,
    pub turn_from: f64,
    pub turn_full: f64,
    pub dwell_window: f64,
    pub dwell_from: f64,
    pub dwell_full: f64,
    pub evidence_floor: f64,
    pub evidence_spacing: f64,
    pub evidence_guard: f64,
    pub evidence_sigma: f64,
    pub evidence_reach: f64,
    pub unexplained_cost: f64,
    pub straight_key_cost: f64,
    pub corner_key_cost: f64,
    pub straight_angle: f64,
}

impl Default for Parameters {
    fn default() -> Self {
        todo!("port SwipeDecoder.Parameters defaults")
    }
}

/// The letter keys a swipe is read against (`TypingDecoder.Keys`: only the centres and pitch
/// matter to a swipe), in the touch's coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    /// Each letter key's character and centre.
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

/// A decoder can't be changed once made: a new word list makes a new decoder, so a read in
/// progress on another thread keeps the one it started with.
#[derive(Debug, Clone)]
pub struct SwipeDecoder {
    model: Arc<Model>,
    parameters: Parameters,
    never_suggest: NeverSuggest,
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
        let _ = (&model, &parameters, &never_suggest, &extra);
        todo!("port SwipeDecoder.init + ExtraWords")
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
        let _ = (path, times, keys, previous, limit);
        todo!("port SwipeDecoder.read")
    }
}

/// The cost (-ln P) of a common English contraction (`SwipeDecoder.contractionCost`).
pub const CONTRACTION_COST: f64 = 7.5;
/// The cost of one of your own words (`SwipeDecoder.ownWordCost`).
pub const OWN_WORD_COST: f64 = 11.0;

/// `SwipeDecoder.costOfContraction(_:base:)`.
pub fn cost_of_contraction(word: &str, base: f64) -> f64 {
    let _ = (word, base);
    todo!("port SwipeDecoder.costOfContraction")
}
