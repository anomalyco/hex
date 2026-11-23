//! What space puts in place of a word in one language (`ownReplacement`): your text
//! replacement, a learned fix, a word the context decides, the English fixes, Spatial
//! autocorrect's reading of your taps, a two-letter slip, a missing accent, or the dictionary's
//! one-edit guess.

use super::engine::{DecoderCache, Engine, model_id};
use super::{distance, lists, text, tokens};
use crate::decoder::{Decision, Keys, TypingDecoder, UserWords};
use crate::model::Model;
use crate::never_suggest::NeverSuggest;

fn is_all_caps(token: &str) -> bool {
    text::count(token) >= 2
        && text::eq(token, &text::uppercased(token))
        && !text::eq(token, &text::lowercased(token))
}

/// ALL-CAPS ("API") or with capitals inside ("iOS") (`isAllCapsOrMixed`).
pub(crate) fn is_all_caps_or_mixed(token: &str) -> bool {
    text::any_char(text::drop_first(token), text::is_uppercase)
}

impl Engine {
    pub fn previous_word_of(&self, token: &str) -> Option<String> {
        tokens::previous_word(self.before_input.as_deref().unwrap_or(""), token)
    }

    /// `ownReplacement`: `cautiousReplacement` in this language alone.
    pub fn own_replacement(&mut self, token: &str) -> Option<String> {
        let lower = text::lowercased(token);
        if let Some(shortcut) = self.shortcut(token) {
            return Some(shortcut);
        }
        let previous = self.previous_word_of(token);
        if self
            .offensive
            .blocks(&lower, self.allows_swearing, previous.as_deref())
        {
            return None;
        }
        if let Some(learned) = self.learning.typo(&lower).map(str::to_owned)
            && self.may_offer_replacing(&learned, token)
            && !self.checker.is_word_valid(token)
        {
            return Some(self.cased(&learned, token));
        }
        let all_caps = is_all_caps(token);
        let internal_caps =
            text::any_char(text::drop_first(token), text::is_uppercase) && !all_caps;
        if internal_caps {
            return None;
        }
        if !all_caps && !text::contains_char(token, "'") {
            let before = self.before_input.clone().unwrap_or_default();
            let starts = tokens::starts_sentence(&before, token);
            if let Some(choice) = self.context_choice(token, previous.as_deref(), None, starts)
                && self.may_offer_replacing(&choice, token)
            {
                return self.without_added_capitals(&choice, token);
            }
        }
        if self.config.english_rules
            && let Some(fixed) = lists::unambiguous_fix(&lower)
        {
            if all_caps && text::count(token) < 3 {
                return None;
            }
            return self.without_added_capitals(&tokens::match_case(token, fixed), token);
        }
        if all_caps {
            return None;
        }
        if let Some(decision) = self.decoder_decision(token, false) {
            if let Some(replacement) = decision.replacement
                && self.may_offer_replacing(&replacement, token)
            {
                let mut cased = self.cased(&replacement, token);
                if self.config.english_rules && text::contains_char(&cased, " ") {
                    cased = text::split_on(&cased, " ")
                        .into_iter()
                        .map(|w| if w == "i" { "I" } else { w })
                        .collect::<Vec<_>>()
                        .join(" ");
                }
                // German: the manifest names its nouns' capitals.
                let nouns = self.config.noun_capitals.is_some();
                if self.adds_capitals && !text::contains_char(&cased, " ") && nouns {
                    cased = self.noun_capitalised(&cased, token).unwrap_or(cased);
                }
                return self.without_added_capitals(&cased, token);
            }
            return if self.adds_capitals {
                self.noun_capitalised(token, token)
            } else {
                None
            };
        }
        if text::count(token) == 2
            && !text::contains_char(token, "'")
            && let Some(fix) = self.common_two_letter_fix(&lower)
            && !self.checker.is_word_valid(token)
        {
            return Some(tokens::match_case(token, fix));
        }
        if let Some(accented) = self.accent_restoration(token) {
            return Some(accented);
        }
        // (a French elision's sides each as a word, `elision_fix`).
        if text::contains_char(token, "'") {
            return self.elision_fix(token);
        }
        if text::count(token) < 3 {
            return None;
        }
        if self.checker.is_word_valid(token) {
            return None;
        }
        let mut best_guess: Option<String> = None;
        let mut best_score = f64::MAX;
        let guesses = self.checker.guesses(token);
        for (rank, guess) in guesses.iter().take(8).enumerate() {
            let Some(raw) = self.usable_guess(guess, token, &guesses) else {
                continue;
            };
            if !self.may_offer_replacing(&raw, token) {
                continue;
            }
            let normalized = text::lowercased(&text::without_apostrophes(&raw));
            let Some(edit) = distance::single_edit_keyboard_cost(
                &lower,
                &normalized,
                self.layout_language.config(),
                &self.grid,
            ) else {
                continue;
            };
            let total = edit + rank as f64 * 0.08;
            if total < best_score {
                best_score = total;
                best_guess = Some(raw);
            }
        }
        let best = best_guess?;
        if best_score > 1.35 {
            return None;
        }
        self.without_added_capitals(&self.cased(&best, token), token)
    }

    /// `frenchElisionFix(for:)`: a word with an elision (the manifest's `elision_heads`) and a
    /// slip on either side of its apostrophe: the word after it corrected as a word of its own
    /// ("l'hpmme" -> "l'homme"), a head that is no elision the commonest one a key away
    /// ("k'homme" -> "l'homme"). `None` when neither side changes, for a word the dictionary
    /// knows whole, and for an apostrophe without a letter on both sides.
    fn elision_fix(&mut self, token: &str) -> Option<String> {
        let heads = self.config.elision_heads;
        let chars = text::chars(token);
        if heads.is_empty() || chars.iter().filter(|c| **c == "'").count() != 1 {
            return None;
        }
        let mark = chars.iter().position(|c| *c == "'")?;
        let head: String = chars[..mark].concat();
        let tail: String = chars[mark + 1..].concat();
        if head.is_empty()
            || tail.is_empty()
            || !text::chars(&head).into_iter().all(text::is_letter)
            || !text::chars(&tail).into_iter().all(text::is_letter)
            || self.checker.is_word_valid(token)
        {
            return None;
        }
        let mut fixed_head = head.clone();
        let lower_head = text::lowercased(&head);
        if !heads.iter().any(|h| text::eq(h, &lower_head)) {
            let mut best: Option<(&str, f64)> = None;
            for &word in heads {
                let Some(cost) = distance::single_edit_keyboard_cost(
                    &lower_head,
                    word,
                    self.layout_language.config(),
                    &self.grid,
                ) else {
                    continue;
                };
                if !(cost > 0.0 && cost <= 0.65) {
                    continue;
                }
                if best.is_none_or(|b| cost < b.1) {
                    best = Some((word, cost));
                }
            }
            fixed_head = tokens::match_case(&head, best?.0);
        }
        let mut fixed_tail = tail.clone();
        if let Some(word) = self.own_replacement(&tail)
            && text::chars(&word).into_iter().all(text::is_letter)
        {
            fixed_tail = word;
        }
        if text::eq(&fixed_head, &head) && text::eq(&fixed_tail, &tail) {
            return None;
        }
        Some(format!("{fixed_head}'{fixed_tail}"))
    }

    /// `accentRestoration(for:)`.
    pub fn accent_restoration(&self, token: &str) -> Option<String> {
        if is_all_caps(token) || text::any_char(text::drop_first(token), text::is_uppercase) {
            return None;
        }
        let lower = text::lowercased(token);
        let accented = self.config.accent_restoration(&lower)?;
        if !self.may_offer_replacing(accented, token) {
            return None;
        }
        self.without_added_capitals(&self.cased(accented, token), token)
    }

    /// `isSureFix`: a contraction or common slip, or a two-letter word.
    pub fn is_sure_fix(&self, token: &str) -> bool {
        text::count(token) == 2
            || (self.config.english_rules
                && lists::unambiguous_fix(&text::lowercased(token)).is_some())
    }

    /// `commonTwoLetterFix(for:)`.
    fn common_two_letter_fix(&self, lower: &str) -> Option<&'static str> {
        let mut best: Option<(&'static str, f64)> = None;
        for &word in self.config.two_letter_words {
            let Some(cost) = distance::single_edit_keyboard_cost(
                lower,
                word,
                self.layout_language.config(),
                &self.grid,
            ) else {
                continue;
            };
            if !(cost > 0.0 && cost <= 0.65) {
                continue;
            }
            if best.is_none_or(|b| cost < b.1) {
                best = Some((word, cost));
            }
        }
        best.map(|b| b.0)
    }

    /// `decoderDecision(for:partnerWord:)`: Spatial autocorrect's reading of `token`'s taps.
    pub fn decoder_decision(&mut self, token: &str, partner_word: bool) -> Option<Decision> {
        self.sync_user_words();
        let (Some(decoder), Some(_), Some(taps)) =
            (&self.decoder, &self.decoder_keys, &self.decoder_taps)
        else {
            return None;
        };
        if !self.config.reads_model(decoder.model().language())
            || text::count(token) < 2
            || taps.len() != text::count(token)
            || text::contains_char(token, "'")
        {
            return None;
        }
        let before = self.before_input.clone().unwrap_or_default();
        if let Some(cache) = &self.decoder_cache
            && text::eq(&cache.before, &before)
            && cache.taps == *taps
            && cache.partner_word == partner_word
        {
            return cache.decision.clone();
        }
        let lower = text::lowercased(token);
        let valid = lists::is_shorthand(&lower)
            || self.learned.contains(&lower)
            || self
                .lexicon_terms
                .iter()
                .any(|t| text::eq(&text::lowercased(t), &lower))
            || self.checker.is_word_valid(token)
            || self.proper_noun(token).is_some()
            || partner_word;
        let previous = tokens::previous_word(&before, token);
        let previous = previous.as_deref();
        let decoder = self.decoder.as_ref()?;
        let keys = self.decoder_keys.as_ref()?;
        let taps = self.decoder_taps.as_ref()?;
        let mut decision =
            self.decision_offering_own_capitals(decoder, taps, token, previous, valid, keys);
        if let Some(baseline) = &self.decoder_baseline_keys
            && let Some(replacement) = decision.as_ref().and_then(|d| d.replacement.clone())
        {
            let clears =
                |reading: &str| decoder.replaces(reading, taps, &lower, previous, valid, baseline);
            match clears(&replacement) {
                Some(true) => {}
                Some(false) => {
                    if let Some(d) = decision.as_mut() {
                        d.replacement = d.readings.iter().map(|r| r.text.clone()).find(|t| {
                            !text::eq(t, &replacement)
                                && !self.is_own_capitals(t, token)
                                && clears(t) == Some(true)
                        });
                    }
                }
                None => {
                    let again = self
                        .decision_offering_own_capitals(
                            decoder, taps, token, previous, valid, baseline,
                        )
                        .and_then(|d| d.replacement);
                    if let Some(d) = decision.as_mut() {
                        d.replacement = again;
                    }
                }
            }
        }
        self.decoder_cache = Some(DecoderCache {
            before,
            taps: taps.clone(),
            partner_word,
            decision: decision.clone(),
        });
        decision
    }

    /// `decisionOfferingOwnCapitals`: Spatial autocorrect's decision for `token`, putting in no
    /// `is_own_capitals` reading: space takes the next reading that clears the margin, if any,
    /// and the strip keeps them all.
    fn decision_offering_own_capitals(
        &self,
        decoder: &TypingDecoder,
        taps: &[(f64, f64)],
        token: &str,
        previous: Option<&str>,
        valid: bool,
        keys: &Keys,
    ) -> Option<Decision> {
        let lower = text::lowercased(token);
        let mut decision = decoder.decide(taps, &lower, previous, valid, keys);
        if let Some(d) = decision.as_mut()
            && let Some(replacement) = &d.replacement
            && self.is_own_capitals(replacement, token)
        {
            d.replacement = d.readings.iter().map(|r| r.text.clone()).find(|t| {
                !self.is_own_capitals(t, token)
                    && decoder.replaces(t, taps, &lower, previous, valid, keys) == Some(true)
            });
        }
        decision
    }

    /// `readingsOfferingOwnCapitals`: a swipe's readings, best first, putting in no
    /// `is_own_capitals` one unless caps lock is on: the model's word of the same letters in
    /// lowercase goes first, else the first ordinary reading; the strip offers yours.
    pub fn readings_offering_own_capitals(
        &self,
        readings: Vec<String>,
        caps_lock: bool,
        model: &Model,
    ) -> Vec<String> {
        let Some(first) = readings.first() else {
            return readings;
        };
        if caps_lock || !self.is_own_capitals(first, "") {
            return readings;
        }
        let lower = text::lowercased(first);
        if model.index(&lower).is_some() {
            return std::iter::once(lower).chain(readings).collect();
        }
        let Some(index) = readings.iter().position(|r| !self.is_own_capitals(r, "")) else {
            return readings;
        };
        let mut ordered = readings;
        let item = ordered.remove(index);
        ordered.insert(0, item);
        ordered
    }

    /// `pronounCapital(before:)`: "i'm", "i'll", "i've" and "i'd" typed with their apostrophe at
    /// the end of `before` take the capital "i", "im" and "ive" take: (what was typed, its fix).
    pub fn pronoun_capital(&self, before: Option<&str>) -> Option<(String, String)> {
        let before = before?;
        if !self.is_enabled
            || !self.has_dictionary
            || !self.adds_capitals
            || !self.config.english_rules
        {
            return None;
        }
        for end in ["m", "ll", "ve", "d"] {
            for mark in ["'", "\u{2019}"] {
                let typed = format!("i{mark}{end}");
                if !text::has_suffix(before, &typed) {
                    continue;
                }
                let previous = text::last(text::drop_last(before, text::count(&typed)));
                let joins = !previous.is_empty()
                    && (text::is_letter(previous)
                        || text::is_number(previous)
                        || previous == "'"
                        || previous == "\u{2019}");
                if joins || self.is_suppressed(&typed) || self.learning.is_rejected(&typed) {
                    return None;
                }
                return Some((typed, format!("I{mark}{end}")));
            }
        }
        None
    }

    /// `properNoun(for:)`: `token` capitalised when the dictionary knows it only that way.
    pub fn proper_noun(&self, token: &str) -> Option<String> {
        if text::count(token) < 2 || !text::eq(token, &text::lowercased(token)) {
            return None;
        }
        let capitalised = text::capitalised_first(token);
        if !self.checker.is_word_valid(&capitalised) || self.checker.is_word_valid(token) {
            return None;
        }
        Some(capitalised)
    }

    /// `nounCapitalised(for:typed:)`.
    pub fn noun_capitalised(&mut self, word: &str, token: &str) -> Option<String> {
        if self.config.noun_capitals.is_some()
            && let Some(capitals) = self.german_nouns()
            && capitals.capitalises(word, self.previous_word_of(token).as_deref()) == Some(true)
            && !word.is_empty()
        {
            return Some(text::capitalised_first(word));
        }
        self.proper_noun(word)
    }

    /// `syncUserWords`: the decoder's copy of your words and the never-suggest list, rebuilt
    /// when one of them or the model changed.
    pub fn sync_user_words(&mut self) {
        let Some(model) = self.decoder.as_ref().map(|d| d.model().clone()) else {
            return;
        };
        let id = model_id(&model);
        if self.user_words_built == Some((self.user_words_revision, id)) {
            return;
        }
        let learned: Vec<String> = self
            .learned
            .words()
            .iter()
            .map(String::as_str)
            .chain(self.learning.fixes())
            .filter(|w| self.may_offer(w, None))
            .map(str::to_owned)
            .collect();
        let mut words: Vec<String> = self.lexicon_terms.clone();
        words.extend(self.shortcut_values().map(str::to_owned));
        words.extend(learned);
        let blocked = self.offensive.blocked(self.allows_swearing);
        if let Some(decoder) = self.decoder.as_mut() {
            decoder.user_words = UserWords::new(&words, &model);
            decoder.never_suggest = NeverSuggest::new(blocked.iter().map(String::as_str), &model);
        }
        self.user_words_built = Some((self.user_words_revision, id));
        self.decoder_cache = None;
    }

    /// `usableGuess(_:typed:among:)`: a dictionary guess as autocorrect may use it.
    pub fn usable_guess(&self, guess: &str, token: &str, guesses: &[String]) -> Option<String> {
        let is_separator = |c: &str| c == " " || c == "-";
        if !text::any_char(guess, is_separator) {
            let typed: Vec<String> = text::chars(token)
                .into_iter()
                .map(|c| text::nfc(c).into_owned())
                .collect();
            let adds_only_letters = text::chars(guess).into_iter().all(|c| {
                text::is_letter(c) || c == "'" || typed.iter().any(|t| *t == text::nfc(c))
            });
            return adds_only_letters.then(|| guess.to_owned());
        }
        let parts: Vec<&str> = text::split_keeping_empty(guess, " ")
            .into_iter()
            .flat_map(|p| text::split_keeping_empty(p, "-"))
            .collect();
        if parts.len() != 2
            || !text::eq(&text::lowercased(&parts.concat()), &text::lowercased(token))
            || !parts.iter().all(|p| self.is_split_word(p))
        {
            return None;
        }
        let spaced = parts.join(" ");
        guesses
            .iter()
            .any(|g| text::case_insensitive_eq(g, &spaced))
            .then_some(spaced)
    }

    /// `isSplitWord`: one half of a split guess.
    fn is_split_word(&self, part: &str) -> bool {
        if part.is_empty() || !text::chars(part).into_iter().all(text::is_letter) {
            return false;
        }
        if text::count(part) == 1 {
            let lower = text::lowercased(part);
            return self.config.english_rules && (lower == "a" || lower == "i");
        }
        self.checker.is_word_valid(part)
    }

    /// `rankedGuesses(for:)`: usable guesses, the cheapest slip first.
    pub fn ranked_guesses(&self, token: &str) -> Vec<String> {
        let lower = text::lowercased(token);
        let (language, grid) = (self.layout_language.config(), &*self.grid);
        let guesses = self.checker.guesses(token);
        let usable: Vec<String> = guesses
            .iter()
            .filter_map(|g| self.usable_guess(g, token, &guesses))
            .collect();
        let count = text::count(token) as f64;
        let cost = |(offset, g): &(usize, String)| {
            let normalized = text::lowercased(&text::without_apostrophes(g));
            distance::single_edit_keyboard_cost(&lower, &normalized, language, grid)
                .unwrap_or_else(|| 2.0 + (text::count(g) as f64 - count).abs() * 0.3)
                + *offset as f64 * 0.08
        };
        let mut ranked: Vec<(usize, String)> = usable.into_iter().take(8).enumerate().collect();
        ranked.sort_by(|a, b| {
            let (ca, cb) = (cost(a), cost(b));
            if ca < cb {
                std::cmp::Ordering::Less
            } else if cb < ca {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        ranked.into_iter().map(|(_, g)| g).collect()
    }
}
