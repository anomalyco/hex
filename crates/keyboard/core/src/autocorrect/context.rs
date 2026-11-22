//! English words the words around them decide (`contextChoice`): the same letters with and
//! without an apostrophe ("id", "I'd") and words that sound alike ("your", "you're"), scored by
//! the typing model's word pairs.

use super::engine::Engine;
use super::{lists, text};
use crate::model::Model;

/// Nats the better word must win by, with the word before only and once the next is known.
const CONTEXT_MARGIN: (f64, f64) = (3.0, 1.5);
const CONTEXT_LETTER_MARGIN: (f64, f64) = (6.0, 3.0);
/// -ln P of a word the model lacks.
const UNKNOWN_COST: f64 = 15.0;
/// How much likelier a sentence starts with a contraction's pronoun.
const SENTENCE_START: f64 = 3.0;
/// A next word that never follows "will", "would", "had" or "us" in the pairs.
const MISSING_FOLLOWER: f64 = 5.0;
const RICH_FOLLOWERS: [&str; 4] = ["will", "would", "had", "us"];

/// -ln P(`word` | `previous`) + -ln P(`next` | `word`), a contraction as its words
/// (`contextCost`).
fn context_cost(
    word: &str,
    previous: Option<&str>,
    next: Option<&str>,
    starts_sentence: bool,
    model: &Model,
) -> f64 {
    let lower = text::lowercased(word);
    let expanded = lists::contraction_words(&lower);
    let own: [&str; 1] = [&lower];
    let own_list: [&[&str]; 1] = [&own];
    let readings: &[&[&str]] = expanded.unwrap_or(&own_list);
    let mut best = f64::INFINITY;
    for words in readings {
        let mut cost = model.cost(words[0], previous).unwrap_or(UNKNOWN_COST);
        for k in 1..words.len() {
            cost += model
                .cost(words[k], Some(words[k - 1]))
                .unwrap_or(UNKNOWN_COST);
        }
        if let Some(next) = next {
            let last = words[words.len() - 1];
            cost += model.cost(next, Some(last)).unwrap_or(UNKNOWN_COST);
            if RICH_FOLLOWERS.contains(&last) && !model.has_pair(last, next) {
                cost += MISSING_FOLLOWER;
            }
        }
        if cost < best {
            best = cost;
        }
    }
    if expanded.is_some() && starts_sentence {
        best - SENTENCE_START
    } else {
        best
    }
}

/// `target` cased as `word` reads: a capital first letter carries over, except the "I" of
/// "I'd" or "I'll" (`matchContextCase`).
fn match_context_case(word: &str, target: &str) -> String {
    let starts_with_i = text::has_prefix(word, "I'")
        && text::chars(word)
            .into_iter()
            .skip(2)
            .all(text::is_lowercase);
    let first = text::first(word);
    if first.is_empty() || !text::is_uppercase(first) || starts_with_i || target.is_empty() {
        return target.to_owned();
    }
    text::capitalised_first(target)
}

impl Engine {
    /// `contextChoice(for:previous:next:startsSentence:)`.
    pub fn context_choice(
        &self,
        word: &str,
        previous: Option<&str>,
        next: Option<&str>,
        starts_sentence: bool,
    ) -> Option<String> {
        if !self.is_enabled || !self.language.uses_english_fixes() {
            return None;
        }
        let model = self.context_model.as_ref()?;
        if model.language() != self.language.code() {
            return None;
        }
        let lower = text::lowercased(word);
        let group = lists::context_group(&lower)?;
        if self.is_kept(&lower)
            || self
                .lexicon_terms
                .iter()
                .any(|t| text::eq(&text::lowercased(t), &lower))
        {
            return None;
        }
        let own = context_cost(word, previous, next, starts_sentence, model);
        let mut best: Option<(&str, f64)> = None;
        for &other in group {
            if text::eq(&text::lowercased(other), &lower) {
                continue;
            }
            let cost = context_cost(other, previous, next, starts_sentence, model);
            if cost < best.map_or(f64::INFINITY, |b| b.1) {
                best = Some((other, cost));
            }
        }
        let (best_word, best_cost) = best?;
        let same_letters = text::eq(
            &text::lowercased(&text::without_apostrophes(best_word)),
            &text::without_apostrophes(&lower),
        );
        let margins = if same_letters {
            CONTEXT_MARGIN
        } else {
            CONTEXT_LETTER_MARGIN
        };
        let margin = if next.is_none() { margins.0 } else { margins.1 };
        if own - best_cost < margin {
            return None;
        }
        Some(match_context_case(word, best_word))
    }
}
