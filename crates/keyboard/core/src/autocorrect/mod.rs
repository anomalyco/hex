//! Cautious on-device suggestions and replace-on-space, ported from `AutocorrectEngine`
//! (`ios/HEXShared/FullKeyboard/AutocorrectEngine.swift` and its extensions) decision for
//! decision. The host keeps what only the OS can do: the spelling dictionary answers through
//! `TextChecker` (iOS: `UITextChecker`), and your words, settings and field come in as data.
//!
//! `Autocorrect` is one keyboard's autocorrect: the layout language's engine and, with
//! bilingual autocorrect, the partner language's (each with its own dictionary), which share
//! your words and the keys.

mod bilingual;
mod context;
mod distance;
mod engine;
mod german;
mod language;
mod learning;
pub mod lists;
mod offensive;
mod replace;
mod suggest;
pub mod text;
mod tokens;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use distance::{are_adjacent_keys, single_edit_keyboard_cost};
pub use engine::Bundled;
pub use german::GermanCapitals;
pub use language::{KeyCell, KeyGrid, LanguageConfig, LanguageId};
pub use learning::{AppliedCorrection, Change, LearnedList, TypingLearning};
pub use offensive::{NeverSuggestList, OffensiveKind, OffensiveWords};
pub use tokens::{
    current_token, has_vowel, is_sentence_start, match_case, previous_word, starts_sentence,
    vocabulary_spelling, vocabulary_words,
};

use crate::decoder::Keys;
use crate::model::Model;
use engine::Engine;
use learning::LearnedWords;

/// The spelling dictionary, which stays the host's (`TextChecking`): iOS answers with
/// `UITextChecker`. The engine asks in the same order the Swift did.
pub trait TextChecker {
    fn is_word_valid(&self, word: &str) -> bool;
    fn guesses(&self, word: &str) -> Vec<String>;
    fn completions(&self, prefix: &str) -> Vec<String>;
    /// Switches the dictionary (a `UITextChecker` language code such as `nb`).
    fn use_language(&mut self, code: &str);
}

/// The kind of text field, as far as autocorrect cares (`KeyboardFieldKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Url,
    Email,
}

/// One slot of the suggestion strip (`KeyboardSuggestion.Kind`, the ones autocorrect makes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// Keeps the exact characters typed.
    KeepLiteral,
    /// The best replacement or completion; `auto_replace` when space will apply it.
    Primary {
        auto_replace: bool,
    },
    Secondary,
    NextWord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub display_text: String,
    pub replacement: String,
    pub kind: SuggestionKind,
}

/// One keyboard's autocorrect (`AutocorrectEngine` with its partner).
pub struct Autocorrect {
    me: Engine,
    partner: Option<Engine>,
    /// Learn each language separately: each language reads its own kept words.
    learns_per_language: bool,
}

impl Autocorrect {
    pub fn new(bundled: Arc<Bundled>, checker: Box<dyn TextChecker>) -> Self {
        Self {
            me: Engine::new(bundled, checker, LearnedList::Shared),
            partner: None,
            learns_per_language: false,
        }
    }

    // MARK: settings and data

    /// Settings › Keyboard › Autocorrect (the partner keeps its own, always on).
    pub fn set_enabled(&mut self, on: bool) {
        self.me.is_enabled = on;
    }

    pub fn is_enabled(&self) -> bool {
        self.me.is_enabled
    }

    /// Settings › Keyboard › Suggest swear words.
    pub fn set_allows_swearing(&mut self, allows: bool) {
        self.me.set_allows_swearing(allows);
        if let Some(partner) = &mut self.partner {
            partner.set_allows_swearing(allows);
        }
    }

    pub fn allows_swearing(&self) -> bool {
        self.me.allows_swearing
    }

    /// Settings › Keyboard › Auto-capitalization.
    pub fn set_adds_capitals(&mut self, adds: bool) {
        self.me.adds_capitals = adds;
        if let Some(partner) = &mut self.partner {
            partner.adds_capitals = adds;
        }
    }

    /// Learn each language separately.
    pub fn set_learns_per_language(&mut self, on: bool) {
        if on != self.learns_per_language {
            self.learns_per_language = on;
            self.scope_learned_words();
        }
    }

    pub fn learns_per_language(&self) -> bool {
        self.learns_per_language
    }

    fn list_for(&self, language: &LanguageId) -> LearnedList {
        if self.learns_per_language {
            LearnedList::Language(language.clone())
        } else {
            LearnedList::Shared
        }
    }

    /// `scopeLearnedWords`: which list each engine reads, counting a change as a change of
    /// your words. The host then hands over the lists' words (`set_learned_words`).
    fn scope_learned_words(&mut self) {
        let store = self.list_for(&self.me.language);
        if store != self.me.learned_list {
            self.me.learned_list = store;
            self.me.user_words_revision += 1;
        }
        if let Some(language) = self.partner.as_ref().map(|p| p.language.clone()) {
            let theirs = self.list_for(&language);
            if let Some(partner) = &mut self.partner
                && theirs != partner.learned_list
            {
                partner.learned_list = theirs;
                partner.user_words_revision += 1;
            }
        }
    }

    /// The list of kept words this engine reads, and the partner's.
    pub fn learned_list(&self) -> &LearnedList {
        &self.me.learned_list
    }

    pub fn partner_learned_list(&self) -> Option<&LearnedList> {
        self.partner.as_ref().map(|p| &p.learned_list)
    }

    /// The words of `learned_list()` (`LearnedWordsStore.words`), whenever they change.
    pub fn set_learned_words<S: AsRef<str>>(&mut self, words: &[S]) {
        self.me.learned = LearnedWords::new(words);
    }

    pub fn set_partner_learned_words<S: AsRef<str>>(&mut self, words: &[S]) {
        if let Some(partner) = &mut self.partner {
            partner.learned = LearnedWords::new(words);
        }
    }

    /// Follows the keyboard layout (`setLanguage`): `tag` is the language's BCP 47 tag (an
    /// unknown one turns everything off but your text replacements); `available` is the
    /// device's dictionaries. The keys stay the host's: it hands the new language's grid over
    /// with `set_keys`.
    pub fn set_language<S: AsRef<str>>(&mut self, tag: &str, available: &[S]) {
        let language = LanguageId::new(tag);
        self.me.language = language.clone();
        self.me.config = language.config();
        self.scope_learned_words();
        if self
            .partner
            .as_ref()
            .is_some_and(|p| p.language == language)
        {
            self.partner = None;
        } else if let Some(partner) = &mut self.partner {
            partner.layout_language = language.clone();
            partner.decoder_cache = None;
        }
        if !self.me.set_language(&language, available) {
            self.partner = None;
        }
    }

    pub fn language(&self) -> &str {
        self.me.language.as_str()
    }

    /// False when the device has no dictionary for the language: then only your text
    /// replacements apply.
    pub fn has_dictionary(&self) -> bool {
        self.me.has_dictionary
    }

    /// The keys the letters are typed on (`setKeys`): the layout's language (`layout`, whose
    /// manifest says which accents a slip may leave out) and the grid of letters the host shows,
    /// for the neighbouring-key cost. The partner types on the same ones.
    pub fn set_keys(&mut self, layout: &str, cells: &[KeyCell]) {
        let grid = Arc::new(KeyGrid::new(cells));
        for engine in std::iter::once(&mut self.me).chain(self.partner.as_mut()) {
            engine.layout_language = LanguageId::new(layout);
            engine.grid = grid.clone();
        }
    }

    /// Bilingual autocorrect's second language (`setPartner`), with its own dictionary.
    pub fn set_partner<S: AsRef<str>>(
        &mut self,
        other: Option<&str>,
        available: &[S],
        checker: Box<dyn TextChecker>,
    ) {
        let Some(other) = other.map(LanguageId::new) else {
            self.partner = None;
            return;
        };
        if other == self.me.language
            || !self.me.has_dictionary
            || other.config().spell_check_language(available).is_none()
        {
            self.partner = None;
            return;
        }
        if self.partner.as_ref().is_some_and(|p| p.language == other) {
            return;
        }
        let me = &self.me;
        let mut engine = Engine::new(me.bundled.clone(), checker, self.list_for(&other));
        engine.set_language(&other, available);
        engine.layout_language = me.layout_language.clone();
        engine.grid = me.grid.clone();
        engine.shortcuts = me.shortcuts.clone();
        engine.shortcut_index = me.shortcut_index.clone();
        engine.system_lexicon_terms = me.system_lexicon_terms.clone();
        engine.vocabulary = me.vocabulary.clone();
        engine.lexicon_terms = me.lexicon_terms.clone();
        if !self.learns_per_language {
            engine.learning = me.learning.clone();
        }
        engine.set_allows_swearing(me.allows_swearing);
        engine.adds_capitals = me.adds_capitals;
        engine.decoder_keys = me.decoder_keys.clone();
        engine.decoder_baseline_keys = me.decoder_baseline_keys.clone();
        engine.decoder_taps = me.decoder_taps.clone();
        self.partner = Some(engine);
    }

    pub fn partner_language(&self) -> Option<&str> {
        self.partner.as_ref().map(|p| p.language.as_str())
    }

    /// Your `UILexicon` entries: text replacements and terms (`loadLexiconEntries`).
    pub fn load_lexicon_entries<S: AsRef<str>>(&mut self, entries: &[(S, S)]) {
        self.me.load_lexicon_entries(entries);
        if let Some(partner) = &mut self.partner {
            partner.load_lexicon_entries(entries);
        }
    }

    /// Hex's vocabulary list for typing.
    pub fn set_vocabulary<S: AsRef<str>>(&mut self, terms: &[S]) {
        if terms.len() == self.me.vocabulary.len()
            && terms
                .iter()
                .zip(&self.me.vocabulary)
                .all(|(a, b)| text::eq(a.as_ref(), b))
        {
            return;
        }
        self.me.set_vocabulary(terms);
        if let Some(partner) = &mut self.partner {
            partner.set_vocabulary(terms);
        }
    }

    pub fn vocabulary(&self) -> &[String] {
        &self.me.vocabulary
    }

    /// What Learn how I type learned (`applyLearning`); with Learn each language separately,
    /// the partner gets `partner` instead.
    pub fn apply_learning(&mut self, learning: &TypingLearning, partner: Option<&TypingLearning>) {
        self.me.apply_learning(learning);
        if self.learns_per_language
            && let Some(theirs) = partner
        {
            if let Some(engine) = &mut self.partner {
                engine.apply_learning(theirs);
            }
            return;
        }
        if let Some(engine) = &mut self.partner {
            engine.apply_learning(learning);
        }
    }

    /// Keeps `word` as typed this session (`suppressAndKeep`); the host counts the keep in its
    /// `LearnedWordsStore`.
    pub fn suppress_and_keep(&mut self, word: &str) {
        let lower = text::nfc(&text::lowercased(word)).into_owned();
        self.me.suppressed.insert(lower.clone());
        self.me.user_words_revision += 1;
        if let Some(partner) = &mut self.partner {
            partner.suppressed.insert(lower);
            partner.user_words_revision += 1;
        }
    }

    /// Spatial autocorrect's model for the layout language, or none (`decoder`).
    pub fn set_decoder(&mut self, model: Option<Arc<Model>>) {
        self.me.set_decoder(model);
    }

    pub fn decoder_model(&self) -> Option<&Arc<Model>> {
        self.me.decoder.as_ref().map(|d| d.model())
    }

    /// The partner's Spatial autocorrect model (`partner.decoder`).
    pub fn set_partner_decoder(&mut self, model: Option<Arc<Model>>) {
        if let Some(partner) = &mut self.partner {
            partner.set_decoder(model);
        }
    }

    pub fn partner_decoder_model(&self) -> Option<&Arc<Model>> {
        self.partner
            .as_ref()
            .and_then(|p| p.decoder.as_ref().map(|d| d.model()))
    }

    /// The layout language's word list and pairs (`contextModel`).
    pub fn set_context_model(&mut self, model: Option<Arc<Model>>) {
        self.me.context_model = model;
    }

    pub fn set_decoder_keys(&mut self, keys: Option<Keys>) {
        if let Some(partner) = &mut self.partner {
            partner.decoder_keys = keys.clone();
        }
        self.me.decoder_keys = keys;
    }

    pub fn set_decoder_baseline_keys(&mut self, keys: Option<Keys>) {
        if let Some(partner) = &mut self.partner {
            partner.decoder_baseline_keys = keys.clone();
        }
        self.me.decoder_baseline_keys = keys;
    }

    /// Where each letter of the word being typed was tapped, or `None`.
    pub fn set_decoder_taps(&mut self, taps: Option<Vec<(f64, f64)>>) {
        if let Some(partner) = &mut self.partner {
            partner.decoder_taps = taps.clone();
        }
        self.me.decoder_taps = taps;
    }

    /// German's capitals for this engine (`germanCapitals`); unset, the bundled ones.
    pub fn set_german_capitals(&mut self, capitals: Option<Arc<GermanCapitals>>) {
        self.me.german_capitals = Some(capitals);
    }

    pub fn has_german_capitals(&mut self) -> bool {
        self.me.german_capitals.clone().flatten().is_some()
    }

    // MARK: corrections the strip may revert

    pub fn forget_correction(&mut self) {
        self.me.last_correction = None;
    }

    pub fn record_applied_correction(
        &mut self,
        original: &str,
        corrected: &str,
        anchor: &str,
        changes: Vec<Change>,
    ) {
        self.me.last_correction = Some(AppliedCorrection {
            original: original.to_owned(),
            corrected: corrected.to_owned(),
            changes,
            anchor: anchor.to_owned(),
        });
    }

    /// The latest autocorrection when the cursor is right after it.
    pub fn revertible_correction(&self, before: Option<&str>) -> Option<&AppliedCorrection> {
        self.me
            .last_correction
            .as_ref()
            .filter(|c| c.is_at_end(before))
    }

    // MARK: what the engine answers directly

    pub fn last_fix_language(&self) -> Option<&str> {
        self.me.last_fix_language.as_ref().map(LanguageId::as_str)
    }

    pub fn last_revision_language(&self) -> Option<&str> {
        self.me
            .last_revision_language
            .as_ref()
            .map(LanguageId::as_str)
    }

    pub fn may_offer(&self, candidate: &str, previous: Option<&str>) -> bool {
        self.me.may_offer(candidate, previous)
    }

    pub fn knows_as_typed(&self, token: &str) -> bool {
        self.me.knows_as_typed(token)
    }

    pub fn text_replacement(&self, before: Option<&str>) -> Option<(String, String)> {
        self.me.text_replacement(before)
    }

    pub fn without_stray_capital(&self, word: &str, typed: &str) -> String {
        self.me.without_stray_capital(word, typed)
    }

    pub fn without_added_capitals(&self, fix: &str, typed: &str) -> Option<String> {
        self.me.without_added_capitals(fix, typed)
    }

    pub fn without_added_capitals_pool(&self, pool: Vec<String>, typed: &str) -> Vec<String> {
        self.me.without_added_capitals_pool(pool, typed)
    }

    pub fn is_own_spelling(&self, word: &str) -> bool {
        self.me.is_own_spelling(word)
    }

    pub fn vocabulary_term(&self, token: &str) -> Option<String> {
        self.me.vocabulary_term(token)
    }

    pub fn context_choice(
        &self,
        word: &str,
        previous: Option<&str>,
        next: Option<&str>,
        starts_sentence: bool,
    ) -> Option<String> {
        self.me
            .context_choice(word, previous, next, starts_sentence)
    }

    /// Your words for Swipe typing (`ownWords`).
    pub fn own_words(&self) -> Vec<String> {
        self.me.own_words()
    }

    /// Bumped whenever your own words, the language or Suggest swear words change.
    pub fn own_words_revision(&self) -> u64 {
        self.me.user_words_revision
    }

    /// The words never offered in the layout language (`offensiveWords.blocked`).
    pub fn blocked_words(&self) -> Vec<String> {
        self.me.offensive.blocked(self.me.allows_swearing)
    }

    /// `hasWords`: the phone's dictionary, or Hex's own model, for the language.
    pub fn has_words(&self) -> bool {
        self.me.has_words()
    }

    pub fn is_own_capitals(&self, word: &str, typed: &str) -> bool {
        self.me.is_own_capitals(word, typed)
    }

    /// A swipe's readings with your own words in capitals never first unless caps lock is on
    /// (`readingsOfferingOwnCapitals`).
    pub fn readings_offering_own_capitals(
        &self,
        readings: Vec<String>,
        caps_lock: bool,
        model: &Model,
    ) -> Vec<String> {
        self.me
            .readings_offering_own_capitals(readings, caps_lock, model)
    }

    /// Whether the dictionary knows the word as typed (`checker.isWordValid`).
    pub fn is_word_valid(&self, word: &str) -> bool {
        self.me.checker.is_word_valid(word)
    }
}
