//! What autocorrect knows of a language: its manifest (`crates/keyboard/languages/<tag>.json`,
//! compiled in by `build.rs`), named by its BCP 47 tag ("en", "fr-CA", "sr-Latn"), and the
//! letter grid the host shows for it (`KeyGrid`). An unknown tag has an empty manifest: every
//! capability it would turn on stays off, so what was typed is kept.

use std::collections::HashMap;
use std::sync::Arc;

use super::text;

/// One language's manifest, with `words_from`'s fields filled in (all but `model`).
#[derive(Debug, PartialEq)]
pub struct LanguageConfig {
    pub id: &'static str,
    /// The language of the typing model it reads (HXTM stores four bytes: "fr", never "fr-CA").
    pub model: Option<&'static str>,
    /// `UITextChecker` language codes to try, best first; empty: no dictionary.
    pub system_dictionaries: &'static [&'static str],
    /// A one-edit slip that only leaves out accents is cheap (Polish, Czech, Greek, Hungarian);
    /// `fold_pairs` are letters that count as one too ("l", "ł").
    pub fold_diacritics: bool,
    pub fold_pairs: &'static [(&'static str, &'static str)],
    /// Marks besides . ! ? that end a sentence (Greek's ;).
    pub extra_sentence_enders: &'static [&'static str],
    /// The built-in English contractions, typo list and context words apply.
    pub english_rules: bool,
    /// German's nouns' capitals (`de.capitals`, which the host hands over as `Bundled`).
    pub noun_capitals: Option<&'static str>,
    /// Two-letter words a two-letter slip one key away is fixed to, commonest first.
    pub two_letter_words: &'static [&'static str],
    /// Plain spellings space puts the accent back on ("perche" -> "perché").
    pub accent_restorations: &'static [(&'static str, &'static str)],
    /// Accented forms only offered in the strip ("e" -> "è").
    pub accent_suggestions: &'static [(&'static str, &'static str)],
    /// Elided words before an apostrophe (French "l'", "qu'"), commonest first.
    pub elision_heads: &'static [&'static str],
    pub sentence_starters: &'static [&'static str],
    pub clause_starters: &'static [&'static str],
    pub common_words: &'static [&'static str],
}

static LANGUAGES: &[LanguageConfig] = include!(concat!(env!("OUT_DIR"), "/languages.rs"));

static UNKNOWN: LanguageConfig = LanguageConfig {
    id: "",
    model: None,
    system_dictionaries: &[],
    fold_diacritics: false,
    fold_pairs: &[],
    extra_sentence_enders: &[],
    english_rules: false,
    noun_capitals: None,
    two_letter_words: &[],
    accent_restorations: &[],
    accent_suggestions: &[],
    elision_heads: &[],
    sentence_starters: &[],
    clause_starters: &[],
    common_words: &[],
};

impl LanguageConfig {
    /// The manifest for `tag`, or an empty one.
    pub fn for_tag(tag: &str) -> &'static LanguageConfig {
        LANGUAGES.iter().find(|l| l.id == tag).unwrap_or(&UNKNOWN)
    }

    /// Every compiled-in manifest.
    pub fn all() -> &'static [LanguageConfig] {
        LANGUAGES
    }

    /// The first dictionary the device has (`available`), or `None`.
    pub fn spell_check_language<S: AsRef<str>>(&self, available: &[S]) -> Option<&'static str> {
        self.system_dictionaries
            .iter()
            .copied()
            .find(|c| available.iter().any(|a| text::eq(a.as_ref(), c)))
    }

    /// Whether a typing model built for `language` is this language's.
    pub fn reads_model(&self, language: &str) -> bool {
        self.model == Some(language)
    }

    fn lookup(table: &'static [(&'static str, &'static str)], lower: &str) -> Option<&'static str> {
        let key = text::nfc(lower);
        table
            .iter()
            .find(|(k, _)| text::nfc(k) == key)
            .map(|(_, v)| *v)
    }

    pub fn accent_restoration(&self, lower: &str) -> Option<&'static str> {
        Self::lookup(self.accent_restorations, lower)
    }

    pub fn accent_suggestion(&self, lower: &str) -> Option<&'static str> {
        Self::lookup(self.accent_suggestions, lower)
    }
}

/// A language's BCP 47 tag, as the host names it (and stores learned words under).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LanguageId(Arc<str>);

impl LanguageId {
    pub fn new(tag: &str) -> Self {
        Self(Arc::from(tag))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn config(&self) -> &'static LanguageConfig {
        LanguageConfig::for_tag(&self.0)
    }

    /// The tag without its region or script ("fr" for "fr-CA"), which the never-suggest list
    /// is read by.
    pub fn base(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }
}

/// One letter of the grid the host shows: where it sits, by row and column, rows shifted as
/// the keys are (`KeyboardLanguage.keyCoordinates`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCell {
    pub letter: char,
    pub row: f64,
    pub column: f64,
}

/// The letter keys the host shows, for the neighbouring-key cost of a slip; a repeated letter's
/// last cell counts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyGrid {
    cells: HashMap<String, (f64, f64)>,
}

impl KeyGrid {
    pub fn new(cells: &[KeyCell]) -> Self {
        let mut map = HashMap::new();
        for cell in cells {
            map.insert(
                text::nfc(&cell.letter.to_string()).into_owned(),
                (cell.row, cell.column),
            );
        }
        Self { cells: map }
    }

    /// A letter's (row, column).
    pub fn get(&self, letter: &str) -> Option<(f64, f64)> {
        self.cells.get(text::nfc(letter).as_ref()).copied()
    }
}
