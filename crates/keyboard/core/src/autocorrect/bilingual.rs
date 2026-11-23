//! The decisions that may ask the bilingual partner: the strip (`suggestions`), what space
//! puts in (`replacementOnSpace`, `cautiousReplacement`), which language the words before are
//! in (`languageLean`), the next word's second look at the word before (`revision`), and a
//! space tapped as a letter (`merge`).

use super::engine::Engine;
use super::language::LanguageId;
use super::replace::is_all_caps_or_mixed;
use super::{Autocorrect, FieldKind, Suggestion, SuggestionKind, lists, text, tokens};
use crate::decoder::without_accents;
use crate::model::Model;

/// `languageLean(_:before:commonFrom:)`: above zero the partner's language, below this one's.
fn lean(me: &Engine, partner: &Engine, before: &str, common_from: usize) -> i32 {
    let mut lean = 0;
    let words = text::split_where(text::suffix(before, 120), |c| {
        !text::is_letter(c) && c != "'"
    });
    let from = words.len().saturating_sub(4);
    for word in &words[from..] {
        let count = text::count(word);
        if count < 2 {
            continue;
        }
        let mine = me.checker.is_word_valid(word);
        let theirs = partner.checker.is_word_valid(word);
        if theirs && !mine {
            lean += 1;
        } else if mine && !theirs {
            lean -= 1;
        } else if mine && count >= common_from {
            let my_common = me.is_common(word);
            if my_common != partner.is_common(word) {
                lean += if my_common { -1 } else { 1 };
            }
        }
    }
    lean
}

/// `languageLean(_:token:)`: the words before `token` in the text before the cursor.
fn lean_before_token(me: &Engine, partner: &Engine, token: &str) -> i32 {
    let before = text::suffix(me.before_input.as_deref().unwrap_or(""), 120);
    let rest = if text::has_suffix(before, token) {
        text::drop_last(before, text::count(token))
    } else {
        before
    };
    lean(me, partner, rest, 3)
}

fn without_word<'a>(before: &'a str, word: &str) -> &'a str {
    if text::has_suffix(before, word) {
        text::drop_last(before, text::count(word))
    } else {
        before
    }
}

impl Autocorrect {
    /// `workingPartner`: the partner, unless its dictionary takes any letters for a word.
    fn working_partner(&mut self) -> bool {
        let Some(partner) = &mut self.partner else {
            return false;
        };
        if partner.accepts_any_letters.is_none() {
            partner.accepts_any_letters = Some(partner.checker.is_word_valid("qzxjvkq"));
        }
        partner.accepts_any_letters != Some(true)
    }

    fn engines(&mut self) -> (&mut Engine, Option<&mut Engine>) {
        (&mut self.me, self.partner.as_mut())
    }

    /// `languageLean(_:before:commonFrom:)` with the partner, or `None` without one.
    pub fn language_lean(&self, before: &str, common_from: usize) -> Option<i32> {
        let partner = self.partner.as_ref()?;
        Some(lean(&self.me, partner, before, common_from))
    }

    /// The strip for the text before the cursor (`suggestions(beforeInput:…)`).
    /// `capitalizes_next`: shift would turn itself on for the next letter.
    pub fn suggestions(
        &mut self,
        before_input: Option<&str>,
        field: FieldKind,
        autocorrection_off: bool,
        prediction_model: Option<&Model>,
        capitalizes_next: bool,
    ) -> Vec<Suggestion> {
        self.me.last_fix_language = None;
        if field != FieldKind::Text || autocorrection_off {
            return Vec::new();
        }
        let replaced = self.me.text_replacement(before_input);
        let replacement_only = || -> Vec<Suggestion> {
            match &replaced {
                Some((typed, replacement)) => vec![
                    Suggestion {
                        display_text: format!("\u{201C}{typed}\u{201D}"),
                        replacement: typed.clone(),
                        kind: SuggestionKind::KeepLiteral,
                    },
                    Suggestion {
                        display_text: replacement.clone(),
                        replacement: replacement.clone(),
                        kind: SuggestionKind::Primary { auto_replace: true },
                    },
                ],
                None => Vec::new(),
            }
        };
        if !self.me.has_words() {
            return replacement_only();
        }
        let language = self.me.config;
        if tokens::is_sentence_start(before_input, language) {
            return self
                .me
                .predictions(language.sentence_starters, None, capitalizes_next);
        }
        if let Some(before) = before_input
            && text::last(before) == " "
        {
            let before = text::drop_last(before, 1);
            let mark = text::last(before);
            if !mark.is_empty() && tokens::is_mid_sentence_mark(mark) {
                return self
                    .me
                    .predictions(language.clause_starters, None, capitalizes_next);
            }
            let Some(finished) = tokens::current_token(Some(before)) else {
                return Vec::new();
            };
            let previous = text::lowercased(&finished);
            let mut words: Vec<String> = Vec::new();
            if let Some(model) = prediction_model
                && language.reads_model(model.language())
            {
                words = model.next_words(&previous, |w| self.me.may_offer(w, Some(&previous)));
            }
            words.extend(language.common_words.iter().map(|w| (*w).to_owned()));
            return self
                .me
                .predictions(&words, Some(&previous), capitalizes_next);
        }
        let Some(token) = tokens::current_token(before_input) else {
            return replacement_only();
        };
        if let Some((typed, _)) = &replaced
            && text::count(typed) != text::count(&token)
        {
            return replacement_only();
        }
        self.me.before_input = before_input.map(str::to_owned);
        let lower = text::lowercased(&token);
        let allow_auto = !self.me.is_kept(&lower) && !lists::is_shorthand(&lower);
        if let Some(partner) = &mut self.partner {
            partner.before_input = before_input.map(str::to_owned);
        }
        let auto_fix = match &replaced {
            Some((_, replacement)) => Some(replacement.clone()),
            None if allow_auto && self.me.is_enabled => self.cautious_replacement(&token),
            None => None,
        };
        let mut pool: Vec<String> = auto_fix.iter().cloned().collect();
        let corrects = self.me.is_enabled;
        if self.working_partner() {
            let (me, partner) = self.engines();
            let partner = partner.expect("a working partner");
            let guesses = !partner.knows_as_typed(&token);
            let mine = me.candidates(
                &token,
                auto_fix.as_deref(),
                guesses,
                2,
                prediction_model,
                corrects,
            );
            let guesses = !me.knows_as_typed(&token);
            let model = partner.decoder.as_ref().map(|d| d.model().clone());
            let theirs = partner.candidates(
                &token,
                auto_fix.as_deref(),
                guesses,
                2,
                model.as_deref(),
                corrects,
            );
            let lean = lean_before_token(me, partner, &token);
            let ordered: Vec<String> = if lean < 0 {
                mine
            } else if lean > 0 {
                theirs
            } else {
                let mut out = Vec::new();
                for i in 0..mine.len().max(theirs.len()) {
                    out.extend(mine.get(i).cloned());
                    out.extend(theirs.get(i).cloned());
                }
                out
            };
            for item in ordered {
                if pool.len() < 2 && !pool.iter().any(|p| text::case_insensitive_eq(p, &item)) {
                    pool.push(item);
                }
            }
        } else {
            let limit = 2 - pool.len() as isize;
            let more = self.me.candidates(
                &token,
                auto_fix.as_deref(),
                true,
                limit,
                prediction_model,
                corrects,
            );
            pool.extend(more);
        }
        let pool = self.me.without_added_capitals_pool(pool, &token);
        let literal = Suggestion {
            display_text: format!("\u{201C}{token}\u{201D}"),
            replacement: token.clone(),
            kind: SuggestionKind::KeepLiteral,
        };
        let Some(first) = pool.first() else {
            return vec![literal];
        };
        let mut out = vec![
            literal,
            Suggestion {
                display_text: first.clone(),
                replacement: first.clone(),
                kind: SuggestionKind::Primary {
                    auto_replace: auto_fix.is_some(),
                },
            },
        ];
        if pool.len() >= 2 {
            out.push(Suggestion {
                display_text: pool[1].clone(),
                replacement: pool[1].clone(),
                kind: SuggestionKind::Secondary,
            });
        }
        out
    }

    /// `(token to delete, replacement)` only when cautious autocorrect is sure, or for a text
    /// replacement (`replacementOnSpace`).
    pub fn replacement_on_space(
        &mut self,
        before_input: Option<&str>,
        field: FieldKind,
        autocorrection_off: bool,
    ) -> Option<(String, String)> {
        self.me.last_fix_language = None;
        if field != FieldKind::Text || autocorrection_off {
            return None;
        }
        if let Some(replaced) = self
            .me
            .text_replacement(before_input)
            .or_else(|| self.me.pronoun_capital(before_input))
        {
            return Some(replaced);
        }
        let token = tokens::current_token(before_input)?;
        self.me.before_input = before_input.map(str::to_owned);
        if let Some(partner) = &mut self.partner {
            partner.before_input = before_input.map(str::to_owned);
        }
        let lower = text::lowercased(&token);
        if self.me.is_kept(&lower) {
            return None;
        }
        if !self.me.is_enabled || !self.me.has_words() || lists::is_shorthand(&lower) {
            return None;
        }
        let fix = self.cautious_replacement(&token)?;
        Some((token, fix))
    }

    /// `cautiousReplacement(for:)`: what space puts in place of `token`, asking the partner.
    fn cautious_replacement(&mut self, token: &str) -> Option<String> {
        if let Some(shortcut) = self.me.shortcut(token) {
            self.me.last_fix_language = None;
            return Some(shortcut);
        }
        if let Some(term) = self.me.vocabulary_term(token) {
            self.me.last_fix_language = None;
            return tokens::vocabulary_spelling(token, &term);
        }
        if let Some(accented) = self.me.accent_restoration(token) {
            self.me.last_fix_language = Some(self.me.language.clone());
            return Some(accented);
        }
        if !self.working_partner() {
            self.me.last_fix_language = Some(self.me.language.clone());
            return self.me.own_replacement(token);
        }
        let (me, partner) = self.engines();
        let partner = partner.expect("a working partner");
        let lean = lean_before_token(me, partner, token);
        if text::count(token) >= 2 && partner.knows_as_typed(token) && tokens::has_vowel(token) {
            if lean < 0
                && me.is_sure_fix(token)
                && let Some(own) = me.own_replacement(token)
            {
                me.last_fix_language = Some(me.language.clone());
                return Some(own);
            }
            let replacement = if lean <= 0
                && !is_all_caps_or_mixed(token)
                && !(me.config.english_rules
                    && lists::unambiguous_fix(&text::lowercased(token)).is_some())
            {
                // Never just its accents: the other's word is spelled right ("these" stays,
                // not French "thèse").
                me.decoder_decision(token, true)
                    .and_then(|d| d.replacement)
                    .filter(|r| {
                        !text::eq(
                            &without_accents(&text::lowercased(r)),
                            &without_accents(&text::lowercased(token)),
                        ) && me.may_offer_replacing(r, token)
                    })
            } else {
                None
            };
            let Some(replacement) = replacement else {
                me.last_fix_language = None;
                return None;
            };
            me.last_fix_language = Some(me.language.clone());
            return me.without_added_capitals(&me.cased(&replacement, token), token);
        }
        let own = me.own_replacement(token);
        let theirs = if text::count(token) >= 2 && !me.knows_as_typed(token) {
            partner
                .own_replacement(token)
                .filter(|theirs| !partner.is_name(theirs))
        } else {
            None
        };
        let Some(theirs) = theirs else {
            me.last_fix_language = own.as_ref().map(|_| me.language.clone());
            return own;
        };
        let own = match own {
            Some(own) if !text::case_insensitive_eq(&own, &theirs) => own,
            other => {
                me.last_fix_language = Some(if other.is_none() {
                    partner.language.clone()
                } else {
                    me.language.clone()
                });
                return other.or(Some(theirs));
            }
        };
        if prefers_partner(me, lean, token, &own, &theirs) {
            me.last_fix_language = Some(partner.language.clone());
            return Some(theirs);
        }
        me.last_fix_language = Some(me.language.clone());
        Some(own)
    }

    /// The word before the one being finished, revised now that `next` follows it
    /// (`revision(of:before:next:…)`). `before` is the text up to the end of `word`.
    pub fn revision(
        &mut self,
        word: &str,
        before: &str,
        next: &str,
        field: FieldKind,
        autocorrection_off: bool,
    ) -> Option<String> {
        if field != FieldKind::Text
            || autocorrection_off
            || !self.me.has_words()
            || text::count(word) < 2
        {
            return None;
        }
        let all_caps = text::eq(word, &text::uppercased(word))
            && !text::eq(word, &text::lowercased(word))
            && !text::has_prefix(word, "I'");
        if all_caps {
            return None;
        }
        let next_word = text::split_on(next, " ").first()?.to_string();
        let previous = tokens::previous_word(before, word);
        self.me.last_revision_language = Some(self.me.language.clone());
        if let Some(fixed) = self.bilingual_revision(word, before, &next_word) {
            return Some(fixed);
        }
        if let Some((fixed, into)) = self.accent_revision(word, before, &next_word) {
            self.me.last_revision_language = Some(into);
            return Some(fixed);
        }
        let choice = self.me.context_choice(
            word,
            previous.as_deref(),
            Some(&text::lowercased(&next_word)),
            tokens::starts_sentence(before, word),
        )?;
        if !self.me.may_offer(&choice, previous.as_deref()) {
            return None;
        }
        Some(
            if self.me.adds_capitals || text::is_uppercase(text::first(word)) {
                choice
            } else {
                text::lowercased(&choice)
            },
        )
    }

    /// `bilingualRevision(of:before:next:)`: a sure English fix of a word left as typed at a
    /// sentence's start, once the next word leans English.
    fn bilingual_revision(&mut self, word: &str, before: &str, next: &str) -> Option<String> {
        if !self.working_partner()
            || !self.me.config.english_rules
            || text::contains_char(word, "'")
        {
            return None;
        }
        let fixed = lists::unambiguous_fix(&text::lowercased(word))?;
        if text::count(word) < 2 {
            return None;
        }
        let (me, partner) = self.engines();
        let partner = partner.expect("a working partner");
        let lower = text::lowercased(word);
        if me.is_kept(&lower)
            || !partner.knows_as_typed(word)
            || text::count(next) < 2
            || lean(me, partner, next, 2) >= 0
        {
            return None;
        }
        let rest = without_word(before, word);
        if lean(me, partner, rest, 3) != 0
            || !me.may_offer(fixed, tokens::previous_word(before, word).as_deref())
        {
            return None;
        }
        me.without_added_capitals(&tokens::match_case(word, fixed), word)
    }

    /// `accentRevision(of:before:next:)`: a word only one language knows, respelled with the
    /// other's accents once `next` leans that one. With the language it is in.
    fn accent_revision(
        &mut self,
        word: &str,
        before: &str,
        next: &str,
    ) -> Option<(String, LanguageId)> {
        let lower = text::lowercased(word);
        if !self.working_partner()
            || text::count(word) < 2
            || text::count(next) < 2
            || text::contains_char(word, "'")
            || self.me.is_suppressed(&lower)
            || self.me.learning.is_rejected(&lower)
        {
            return None;
        }
        let (me, partner) = self.engines();
        let partner = partner.expect("a working partner");
        let lean_next = lean(me, partner, next, 2);
        let rest = without_word(before, word);
        if lean_next == 0 || lean(me, partner, rest, 3) * lean_next < 0 {
            return None;
        }
        let (into, other): (&Engine, &Engine) = if lean_next > 0 {
            (partner, me)
        } else {
            (me, partner)
        };
        if !other.knows_as_typed(word) || into.knows_as_typed(word) {
            return None;
        }
        if !text::eq(&text::without_diacritics(&lower), &lower) {
            return None;
        }
        let accented = into.checker.guesses(&lower).into_iter().find(|g| {
            !text::eq(g, &lower)
                && text::eq(g, &text::lowercased(g))
                && text::eq(&text::without_diacritics(g), &lower)
        })?;
        if !into.may_offer(&accented, tokens::previous_word(before, word).as_deref()) {
            return None;
        }
        let language = into.language.clone();
        me.without_added_capitals(&tokens::match_case(word, &accented), word)
            .map(|s| (s, language))
    }

    /// One word in place of `first`, a space tapped as a letter and the word being finished
    /// (`merge(first:firstTaps:space:second:before:)`), or `None` to keep both.
    pub fn merge(
        &mut self,
        first: &str,
        first_taps: &[(f64, f64)],
        space: (f64, f64),
        second: &str,
        before: &str,
    ) -> Option<String> {
        let me = &self.me;
        if !me.is_enabled || !me.has_words() {
            return None;
        }
        let decoder = me.decoder.as_ref()?;
        let keys = me
            .decoder_baseline_keys
            .as_ref()
            .or(me.decoder_keys.as_ref())?;
        let taps = me.decoder_taps.as_ref()?;
        if !me.config.reads_model(decoder.model().language())
            || text::contains_char(first, "'")
            || text::contains_char(second, "'")
        {
            return None;
        }
        for word in [first, second] {
            let lower = text::lowercased(word);
            if me.is_kept(&lower)
                || lists::is_shorthand(&lower)
                || me
                    .lexicon_terms
                    .iter()
                    .any(|t| text::eq(&text::lowercased(t), &lower))
            {
                return None;
            }
        }
        let previous = tokens::previous_word(before, first);
        let first_valid = me.knows_as_typed(first);
        let second_valid = me.knows_as_typed(second);
        let merged = decoder.merge(
            first,
            first_taps,
            space,
            second,
            taps,
            previous.as_deref(),
            first_valid,
            second_valid,
            keys,
        )?;
        if !me.may_offer(&merged, previous.as_deref()) {
            return None;
        }
        me.without_added_capitals(
            &tokens::match_case(first, &merged),
            &format!("{first}{second}"),
        )
    }
}

/// `prefersPartner`: both can fix the typo; the partner's fix when the words before are in its
/// language, or without a lean when its fix is clearly the cheaper slip on these keys.
fn prefers_partner(me: &Engine, lean: i32, token: &str, own: &str, theirs: &str) -> bool {
    if lean != 0 {
        return lean > 0;
    }
    let lower = text::lowercased(token);
    let cost = |fix: &str| {
        super::distance::single_edit_keyboard_cost(
            &lower,
            &text::lowercased(&text::without_apostrophes(fix)),
            me.layout_language.config(),
            &me.grid,
        )
        .unwrap_or(1.0)
    };
    cost(theirs) + 0.2 < cost(own)
}
