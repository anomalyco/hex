//! The strip's words for a word being typed in one language (`candidates`), its completions
//! (`rankedCompletions`), and the predictions between words (`predictions`).

use std::collections::HashSet;

use super::engine::Engine;
use super::{Suggestion, SuggestionKind, lists, text, tokens};
use crate::model::Model;

impl Engine {
    /// `predictions(_:previous:capitalized:)`: up to three distinct allowed words.
    pub fn predictions<S: AsRef<str>>(
        &self,
        words: &[S],
        previous: Option<&str>,
        capitalized: bool,
    ) -> Vec<Suggestion> {
        let mut out: Vec<Suggestion> = Vec::new();
        for word in words {
            if out.len() >= 3 {
                break;
            }
            let word = word.as_ref();
            if previous.is_some_and(|p| text::eq(&text::lowercased(word), p))
                || !self.may_offer(word, previous)
            {
                continue;
            }
            let shown = if capitalized && self.adds_capitals {
                text::capitalised_first(word)
            } else {
                word.to_owned()
            };
            if out
                .iter()
                .any(|s| text::case_insensitive_eq(&s.replacement, &shown))
            {
                continue;
            }
            out.push(Suggestion {
                display_text: shown.clone(),
                replacement: shown,
                kind: SuggestionKind::NextWord,
            });
        }
        out
    }

    /// `candidates(for:autoFix:guesses:limit:model:corrects:)`.
    pub fn candidates(
        &mut self,
        token: &str,
        auto_fix: Option<&str>,
        guesses: bool,
        limit: isize,
        model: Option<&Model>,
        corrects: bool,
    ) -> Vec<String> {
        let lower = text::lowercased(token);
        let mut pool: Vec<String> = auto_fix.map(|f| vec![f.to_owned()]).unwrap_or_default();
        let start = pool.len();
        let end = start + limit.max(0) as usize;
        let has =
            |pool: &[String], item: &str| pool.iter().any(|p| text::case_insensitive_eq(p, item));

        if corrects && auto_fix.is_none() {
            let learned = self
                .learning
                .typo(&lower)
                .map(str::to_owned)
                .filter(|l| self.may_offer_replacing(l, token));
            if let Some(learned) = learned {
                pool.push(self.cased(&learned, token));
            } else if self.config.english_rules
                && let Some(suggested) = lists::suggest_only_contraction(&lower)
            {
                pool.push(tokens::match_case(token, suggested));
            } else if !self.is_kept(&lower)
                && let Some(accented) = self.config.accent_suggestion(&lower)
            {
                pool.push(self.cased(accented, token));
            }
        }
        if pool.len() < end
            && let Some(term) = self.vocabulary_completion(token)
            && !has(&pool, &term)
        {
            pool.push(term);
        }

        let mut predictions: Vec<String> = Vec::new();
        if pool.len() < end
            && text::count(token) <= 3
            && let Some(model) = model
            && self.config.reads_model(model.language())
        {
            let previous = self.previous_word_of(token);
            predictions = model
                .completions(&lower, previous.as_deref(), |w| {
                    self.may_offer(w, previous.as_deref())
                })
                .into_iter()
                .map(|w| tokens::match_case(token, &w))
                .collect();
            if auto_fix.is_none()
                && let Some(first) = predictions.iter().find(|p| !has(&pool, p))
            {
                pool.push(first.clone());
            }
        }

        if corrects && let Some(decision) = self.decoder_decision(token, false) {
            for reading in &decision.readings {
                if pool.len() < end && self.may_offer_replacing(&reading.text, token) {
                    let shown = self.cased(&reading.text, token);
                    if !has(&pool, &shown) {
                        pool.push(shown);
                    }
                }
            }
        }

        if corrects
            && guesses
            && pool.len() < end
            && text::count(token) >= 2
            && !self.knows_as_typed(token)
        {
            for guess in self.ranked_guesses(token) {
                let fixed = self.cased(&guess, token);
                if !text::case_insensitive_eq(&fixed, token)
                    && self.may_offer_replacing(&guess, token)
                    && !has(&pool, &fixed)
                {
                    pool.push(fixed);
                }
                if pool.len() >= end {
                    break;
                }
            }
        }

        for item in predictions {
            if pool.len() < end && !has(&pool, &item) {
                pool.push(item);
            }
        }
        if pool.len() < end {
            for item in self.ranked_completions(token) {
                if !text::eq(&item, token) && !has(&pool, &item) {
                    pool.push(item);
                    if pool.len() >= end {
                        break;
                    }
                }
            }
        }
        pool.into_iter()
            .skip(start)
            .take(limit.max(0) as usize)
            .collect()
    }

    /// `rankedCompletions(for:)`: words that start with `token`, the dictionary's first, then
    /// yours, then names unless you typed a capital, cased like `token`.
    pub fn ranked_completions(&self, token: &str) -> Vec<String> {
        let lower = text::lowercased(token);
        let typed_capital = text::is_uppercase(text::first(token));
        let is_name = |w: &str| text::is_uppercase(text::first(w));
        let count = text::count(token);
        let mut seen: HashSet<String> = HashSet::from([text::nfc(&lower).into_owned()]);
        let (mut common, mut yours, mut names) = (Vec::new(), Vec::new(), Vec::new());
        for item in self.checker.completions(&lower) {
            let n = text::count(&item);
            if n > count
                && n <= count + 8
                && self.may_offer_replacing(&item, token)
                && seen.insert(text::nfc(&text::lowercased(&item)).into_owned())
            {
                if is_name(&item) && !typed_capital {
                    names.push(item);
                } else {
                    common.push(item);
                }
            }
        }
        for term in &self.lexicon_terms {
            if text::count(term) > count
                && text::has_prefix(&text::lowercased(term), &lower)
                && seen.insert(text::nfc(&text::lowercased(term)).into_owned())
            {
                if is_name(term) && !typed_capital {
                    names.push(term.clone());
                } else {
                    yours.push(term.clone());
                }
            }
        }
        common
            .into_iter()
            .chain(yours)
            .chain(names)
            .map(|w| self.cased(&w, token))
            .collect()
    }
}
