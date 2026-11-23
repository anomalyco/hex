//! One language's autocorrect state (`AutocorrectEngine`'s stored properties) and the parts
//! of it that never ask the other language: what may be offered, your text replacements,
//! casing a fix and your own spellings. `Autocorrect` holds two: yours and the bilingual
//! partner.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::german::GermanCapitals;
use super::language::{KeyGrid, LanguageConfig, LanguageId};
use super::learning::{AppliedCorrection, LearnedList, LearnedWords, TypingLearning};
use super::offensive::{NeverSuggestList, OffensiveWords};
use super::{TextChecker, lists, text, tokens};
use crate::decoder::{Decision, Keys, TypingDecoder};
use crate::model::Model;

/// Data the host bundles with the keyboard and reads once: the never-suggest list (one list
/// for iOS and Android, `never-suggest.tsv`) and German's capitals.
#[derive(Debug, Default)]
pub struct Bundled {
    pub never_suggest: NeverSuggestList,
    /// `GermanCapitals.bundled()`: what an engine reads when no one set its own.
    pub german_capitals: Option<Arc<GermanCapitals>>,
}

impl Bundled {
    /// The never-suggest list for `language`, by its base language: Apple's three French
    /// layouts read the "fr" rows (Swift read `OffensiveWords.bundled(language.rawValue)`, so
    /// French (Canada) and French (Switzerland) read none).
    pub(crate) fn offensive_words(&self, language: &LanguageId) -> Arc<OffensiveWords> {
        self.never_suggest.bundled(language.base())
    }
}

/// What Spatial autocorrect last decided, for the same text and taps (`decoderCache`).
#[derive(Debug, Clone)]
pub(crate) struct DecoderCache {
    pub before: String,
    pub taps: Vec<(f64, f64)>,
    pub partner_word: bool,
    pub decision: Option<Decision>,
}

pub(crate) struct Engine {
    pub bundled: Arc<Bundled>,
    pub checker: Box<dyn TextChecker>,
    pub learned: LearnedWords,
    pub learned_list: LearnedList,
    pub is_enabled: bool,
    pub language: LanguageId,
    /// `language`'s manifest.
    pub config: &'static LanguageConfig,
    pub has_dictionary: bool,
    pub accepts_any_letters: Option<bool>,
    /// The language of the keys typed on (`KeyboardLayout.language`), whose manifest says
    /// which accents a slip may leave out, and the host's grid of those keys.
    pub layout_language: LanguageId,
    pub grid: Arc<KeyGrid>,
    /// Lowercased shortcut (composed) → your text, ordered by shortcut: the order your text
    /// replacements reach the decoder and Swipe typing in (Swift's was its dictionary's,
    /// random per process).
    pub shortcuts: Vec<(String, String)>,
    pub shortcut_index: HashMap<String, usize>,
    pub lexicon_terms: Vec<String>,
    pub system_lexicon_terms: Vec<String>,
    pub vocabulary: Vec<String>,
    pub suppressed: HashSet<String>,
    pub learning: TypingLearning,
    pub last_correction: Option<AppliedCorrection>,
    pub last_fix_language: Option<LanguageId>,
    pub last_revision_language: Option<LanguageId>,
    pub decoder: Option<TypingDecoder>,
    pub context_model: Option<Arc<Model>>,
    pub decoder_keys: Option<Keys>,
    pub decoder_baseline_keys: Option<Keys>,
    pub decoder_taps: Option<Vec<(f64, f64)>>,
    /// `None` until read: then the bundled list, unless the host set its own.
    pub german_capitals: Option<Option<Arc<GermanCapitals>>>,
    pub before_input: Option<String>,
    pub user_words_revision: u64,
    pub user_words_built: Option<(u64, usize)>,
    pub offensive: Arc<OffensiveWords>,
    pub allows_swearing: bool,
    pub adds_capitals: bool,
    pub decoder_cache: Option<DecoderCache>,
}

/// A model's identity (`ObjectIdentifier`).
pub(crate) fn model_id(model: &Arc<Model>) -> usize {
    Arc::as_ptr(model) as usize
}

impl Engine {
    pub fn new(bundled: Arc<Bundled>, checker: Box<dyn TextChecker>, list: LearnedList) -> Self {
        let english = LanguageId::new("en");
        let offensive = bundled.offensive_words(&english);
        Self {
            bundled,
            checker,
            learned: LearnedWords::default(),
            learned_list: list,
            is_enabled: true,
            config: english.config(),
            language: english.clone(),
            has_dictionary: true,
            accepts_any_letters: None,
            layout_language: english,
            grid: Arc::default(),
            shortcuts: Vec::new(),
            shortcut_index: HashMap::new(),
            lexicon_terms: Vec::new(),
            system_lexicon_terms: Vec::new(),
            vocabulary: Vec::new(),
            suppressed: HashSet::new(),
            learning: TypingLearning::default(),
            last_correction: None,
            last_fix_language: None,
            last_revision_language: None,
            decoder: None,
            context_model: None,
            decoder_keys: None,
            decoder_baseline_keys: None,
            decoder_taps: None,
            german_capitals: None,
            before_input: None,
            user_words_revision: 0,
            user_words_built: None,
            offensive,
            allows_swearing: false,
            adds_capitals: true,
            decoder_cache: None,
        }
    }

    // MARK: settings

    /// `setLanguage` for this engine alone: the partner's part is `Autocorrect`'s.
    pub fn set_language<S: AsRef<str>>(&mut self, language: &LanguageId, available: &[S]) -> bool {
        self.language = language.clone();
        self.config = language.config();
        self.layout_language = language.clone();
        self.accepts_any_letters = None;
        self.offensive = self.bundled.offensive_words(language);
        self.user_words_revision += 1;
        self.decoder_cache = None;
        let code = self.config.spell_check_language(available);
        self.has_dictionary = code.is_some();
        if let Some(code) = code {
            self.checker.use_language(code);
        }
        self.suppressed.clear();
        self.last_correction = None;
        self.has_dictionary
    }

    pub fn set_allows_swearing(&mut self, allows: bool) {
        if allows != self.allows_swearing {
            self.user_words_revision += 1;
            self.decoder_cache = None;
        }
        self.allows_swearing = allows;
    }

    pub fn load_lexicon_entries<S: AsRef<str>>(&mut self, entries: &[(S, S)]) {
        let mut shortcuts: Vec<(String, String)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut terms = Vec::new();
        // A later entry for the same shortcut wins, as in Swift's dictionary.
        for (input, output) in entries {
            let input = text::trimmed(input.as_ref());
            let output = text::trimmed(output.as_ref());
            if input.is_empty() || output.is_empty() {
                continue;
            }
            let lower = text::lowercased(input);
            if !text::eq(&lower, &text::lowercased(output)) {
                let key = text::nfc(&lower).into_owned();
                match index.get(&key) {
                    Some(&i) => shortcuts[i].1 = output.to_owned(),
                    None => {
                        index.insert(key.clone(), shortcuts.len());
                        shortcuts.push((key, output.to_owned()));
                    }
                }
            } else {
                terms.push(output.to_owned());
            }
        }
        shortcuts.sort_by(|a, b| a.0.cmp(&b.0));
        self.shortcut_index = shortcuts
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (k.clone(), i))
            .collect();
        self.shortcuts = shortcuts;
        self.lexicon_terms = terms.clone();
        self.lexicon_terms
            .extend(tokens::vocabulary_words(&self.vocabulary));
        self.system_lexicon_terms = terms;
        self.user_words_revision += 1;
    }

    pub fn set_vocabulary<S: AsRef<str>>(&mut self, terms: &[S]) {
        if terms.len() == self.vocabulary.len()
            && terms
                .iter()
                .zip(&self.vocabulary)
                .all(|(a, b)| text::eq(a.as_ref(), b))
        {
            return;
        }
        self.vocabulary = terms.iter().map(|t| t.as_ref().to_owned()).collect();
        self.lexicon_terms = self.system_lexicon_terms.clone();
        self.lexicon_terms
            .extend(tokens::vocabulary_words(&self.vocabulary));
        self.user_words_revision += 1;
        self.decoder_cache = None;
    }

    /// The decoder for `model` (a new one, as the host makes it), or none.
    pub fn set_decoder(&mut self, model: Option<Arc<Model>>) {
        let old = self.decoder.as_ref().map(|d| model_id(d.model()));
        let new = model.as_ref().map(model_id);
        self.decoder = model.map(|m| TypingDecoder::new(m, None));
        if old != new {
            self.user_words_built = None;
        }
    }

    pub fn apply_learning(&mut self, learning: &TypingLearning) {
        if self.learning != *learning {
            self.learning = learning.clone();
            self.user_words_revision += 1;
        }
    }

    pub fn is_suppressed(&self, lower: &str) -> bool {
        self.suppressed.contains(text::nfc(lower).as_ref())
    }

    /// Kept as typed this session, rejected by Learn how I type, or one you kept twice.
    pub fn is_kept(&self, lower: &str) -> bool {
        self.is_suppressed(lower)
            || self.learning.is_rejected(lower)
            || self.learned.contains(lower)
    }

    pub fn shortcut_values(&self) -> impl Iterator<Item = &str> {
        self.shortcuts.iter().map(|(_, v)| v.as_str())
    }

    fn lexicon_has_lower(&self, lower: &str) -> bool {
        self.lexicon_terms
            .iter()
            .any(|t| text::eq(&text::lowercased(t), lower))
    }

    pub fn german_nouns(&mut self) -> Option<Arc<GermanCapitals>> {
        if self.german_capitals.is_none() {
            self.german_capitals = Some(self.bundled.german_capitals.clone());
        }
        self.german_capitals.clone().flatten()
    }

    // MARK: what may be offered

    /// `mayOffer(_:after:)`.
    pub fn may_offer(&self, candidate: &str, previous: Option<&str>) -> bool {
        if !self
            .offensive
            .blocks(candidate, self.allows_swearing, previous)
        {
            return true;
        }
        self.lexicon_terms
            .iter()
            .any(|t| text::case_insensitive_eq(t, candidate))
            || self
                .shortcut_values()
                .any(|v| text::case_insensitive_eq(v, candidate))
    }

    /// `mayOffer(_:replacing:)`: after the word before `token`.
    pub fn may_offer_replacing(&self, candidate: &str, token: &str) -> bool {
        let previous = tokens::previous_word(self.before_input.as_deref().unwrap_or(""), token);
        self.may_offer(candidate, previous.as_deref())
    }

    /// Your text replacement for `token`, cased as Apple's keyboard writes it (`shortcut(for:)`).
    pub fn shortcut(&self, token: &str) -> Option<String> {
        let key = text::nfc(&text::lowercased(token)).into_owned();
        let stored = &self.shortcuts[*self.shortcut_index.get(&key)?].1;
        let letters: String = text::chars(token)
            .into_iter()
            .filter(|c| text::is_letter(c))
            .collect();
        let leading_capital = text::is_uppercase(text::first(token))
            && (text::count(&letters) < 2 || !text::eq(&letters, &text::uppercased(&letters)));
        let first = text::first(stored);
        if !leading_capital || first.is_empty() || !text::is_lowercase(first) {
            return Some(stored.clone());
        }
        Some(text::capitalised_first(stored))
    }

    /// Your text replacement whose shortcut ends `before` (`textReplacement(before:)`).
    pub fn text_replacement(&self, before: Option<&str>) -> Option<(String, String)> {
        let before = before?;
        if self.shortcuts.is_empty() {
            return None;
        }
        let last = text::last(before);
        if last.is_empty() || text::is_whitespace(last) {
            return None;
        }
        let tail = text::chars(text::suffix(before, 64));
        let mut from = tail.len();
        while from > 0 && !text::is_whitespace(tail[from - 1]) {
            from -= 1;
        }
        let chunk = &tail[from..];
        let joins =
            |c: &str| text::is_letter(c) || text::is_number(c) || c == "'" || c == "\u{2019}";
        for start in 0..chunk.len() {
            let at_boundary = start == 0 || !joins(chunk[start - 1]) || !joins(chunk[start]);
            let typed: String = chunk[start..].concat();
            if at_boundary && let Some(replacement) = self.shortcut(&typed) {
                return Some((typed, replacement));
            }
        }
        None
    }

    /// `knowsAsTyped`: a word of this language as typed, one of yours, or shorthand.
    pub fn knows_as_typed(&self, token: &str) -> bool {
        let lower = text::lowercased(token);
        lists::is_shorthand(&lower)
            || self.learned.contains(&lower)
            || self.lexicon_has_lower(&lower)
            || self.checker.is_word_valid(token)
    }

    /// Whether the dictionary knows `word` only capitalised, and German's list doesn't call
    /// it a noun (`isName`).
    pub fn is_name(&mut self, word: &str) -> bool {
        let lower = text::lowercased(word);
        if text::count(&lower) < 2 {
            return false;
        }
        if !self.checker.is_word_valid(&text::capitalised_first(&lower))
            || self.checker.is_word_valid(&lower)
        {
            return false;
        }
        !(self.config.noun_capitals.is_some()
            && self
                .german_nouns()
                .and_then(|g| g.capitalises(&lower, None))
                == Some(true))
    }

    /// `hasWords`: the phone's dictionary, or Hex's own model for the language. A missing system
    /// dictionary never switches a Hex model off.
    pub fn has_words(&self) -> bool {
        self.has_dictionary
            || self
                .decoder
                .as_ref()
                .is_some_and(|d| self.config.reads_model(d.model().language()))
    }

    /// `isOwnCapitals`: `word`, one of your own words with capitals after its first letter
    /// ("DONT", "McQuorv"), read for `typed`, which has none: the strip offers it, but neither
    /// space nor a swipe puts it in. Your vocabulary's spellings ("GitHub") are as before.
    pub fn is_own_capitals(&self, word: &str, typed: &str) -> bool {
        text::any_char(text::drop_first(word), text::is_uppercase)
            && !text::any_char(text::drop_first(typed), text::is_uppercase)
            && !self.is_vocabulary_spelling(word)
    }

    /// Whether this language's typing model has `word` among its commonest (`isCommon`).
    pub fn is_common(&self, word: &str) -> bool {
        let Some(model) = self
            .context_model
            .as_ref()
            .or(self.decoder.as_ref().map(|d| d.model()))
        else {
            return false;
        };
        if !self.config.reads_model(model.language()) {
            return false;
        }
        match model.index(&text::lowercased(word)) {
            Some(i) => model.unigram_cost(i) <= 10.0,
            None => false,
        }
    }

    // MARK: casing

    /// `withoutStrayCapital(_:typed:)`.
    pub fn without_stray_capital(&self, word: &str, typed: &str) -> String {
        let first = text::first(word);
        if self.config.noun_capitals.is_some()
            || !self.has_dictionary
            || text::is_uppercase(text::first(typed))
            || text::count(word) < 2
            || !text::is_uppercase(first)
            || text::any_char(text::drop_first(word), text::is_uppercase)
            || text::contains_char(word, " ")
            || self.is_vocabulary_spelling(word)
        {
            return word.to_owned();
        }
        let lower = text::lowered_first(word);
        if self.config.english_rules
            && (text::has_prefix(&lower, "i'") || text::has_prefix(&lower, "i\u{2019}"))
        {
            return word.to_owned();
        }
        if self.checker.is_word_valid(&lower) {
            lower
        } else {
            word.to_owned()
        }
    }

    /// `cased(_:typed:)`.
    pub fn cased(&self, candidate: &str, token: &str) -> String {
        self.without_stray_capital(&tokens::match_case(token, candidate), token)
    }

    /// `withoutAddedCapitals(_:typed:)` for a fix.
    pub fn without_added_capitals(&self, fix: &str, token: &str) -> Option<String> {
        if self.adds_capitals
            || text::any_char(token, text::is_uppercase)
            || self.is_own_spelling(fix)
        {
            return Some(fix.to_owned());
        }
        let lowered = text::lowercased(fix);
        (!text::eq(&lowered, token)).then_some(lowered)
    }

    /// `withoutAddedCapitals(_:typed:)` for the strip's words.
    pub fn without_added_capitals_pool(&self, pool: Vec<String>, token: &str) -> Vec<String> {
        if self.adds_capitals || text::any_char(token, text::is_uppercase) {
            return pool;
        }
        let mut kept: Vec<String> = Vec::new();
        for item in pool {
            let item = if self.is_own_spelling(&item) {
                item
            } else {
                text::lowercased(&item)
            };
            if !text::eq(&item, token) && !kept.iter().any(|k| text::eq(k, &item)) {
                kept.push(item);
            }
        }
        kept
    }

    /// `isOwnSpelling`.
    pub fn is_own_spelling(&self, word: &str) -> bool {
        self.shortcut_values().any(|v| text::eq(v, word))
            || ((self.lexicon_terms.iter().any(|t| text::eq(t, word))
                || self.learning.fixes().any(|f| text::eq(f, word))
                || self.learned.words().iter().any(|w| text::eq(w, word)))
                && text::eq(&self.without_stray_capital(word, ""), word))
    }

    // MARK: your vocabulary

    pub fn vocabulary_term(&self, token: &str) -> Option<String> {
        if self.vocabulary.is_empty() {
            return None;
        }
        tokens::vocabulary_words(&self.vocabulary)
            .into_iter()
            .find(|w| text::case_insensitive_eq(w, token))
    }

    pub fn is_vocabulary_spelling(&self, word: &str) -> bool {
        !self.vocabulary.is_empty()
            && tokens::vocabulary_words(&self.vocabulary)
                .iter()
                .any(|w| text::eq(w, word))
    }

    pub fn vocabulary_completion(&self, token: &str) -> Option<String> {
        if text::count(token) < 2 || self.vocabulary.is_empty() {
            return None;
        }
        let lower = text::lowercased(token);
        let count = text::count(token);
        let term = self
            .vocabulary
            .iter()
            .find(|t| text::count(t) > count && text::has_prefix(&text::lowercased(t), &lower))?;
        Some(tokens::match_case(token, term))
    }

    /// `ownWords`: your words for Swipe typing.
    pub fn own_words(&self) -> Vec<String> {
        let mut out = self.lexicon_terms.clone();
        out.extend(
            self.shortcut_values()
                .filter(|v| !text::contains_char(v, " "))
                .map(str::to_owned),
        );
        out.extend(
            self.learned
                .words()
                .iter()
                .filter(|w| self.may_offer(w, None))
                .cloned(),
        );
        out
    }
}
