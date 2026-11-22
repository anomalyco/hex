//! Dictation's text rules, ported from iOS (ios/HEXShared) with the same output: the styles
//! (`TranscriptFormatter`), Format lists (`SpokenLists`), Spoken punctuation
//! (`SpokenPunctuation`), Clean up's rules for Spanish, German and Norwegian and its spoken
//! symbols and counts (`SpokenCleanupRules`, with `CleanupReconstruction`), and French
//! spacing (`FrenchSpacing`).
//!
//! The host runs, as `DictationPipeline` does: [`spoken_symbols`] then [`spoken_counts`]
//! (every style but Verbatim), then Clean up (its English tagger, or [`cleanup_rules`]),
//! then [`format`], then its custom vocabulary and Lowercase, then [`french_spacing`]
//! (every style but Verbatim). A language is passed as the host's locale reads it
//! ([`Language`]); `None` is Swift's `nil`, which most rules take as English.
//!
//! Swift's semantics are kept where they matter: Characters are grapheme clusters, strings
//! compare by canonical equivalence, `folding` is CoreFoundation's (`fold.rs`), and the
//! `NSRegularExpression` patterns are matched with ICU's rules (`regex.rs`) and iOS's Unicode
//! data (`unicode.rs`).

mod casual;
mod cleanup;
mod fold;
mod formatter;
mod french;
mod lists;
mod lists_tables;
mod notes;
mod punctuation;
mod reconstruct;
mod regex;
mod symbols;
mod text;
mod unicode;
mod unicode_generated;

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod rule_tests;

pub use cleanup::{MAX_REACH, bare, clean as cleanup_rules, supports as cleanup_supports};
pub use formatter::{
    continuing_sentence, dropping_final_full_stop, filler_pattern, insertion_text,
    lowercases_continuations, starts_sentence,
};
pub use french::{
    Convention, apply as french_spacing_with, convention as french_convention, french_spacing,
};
pub use lists::format as spoken_lists;
pub use punctuation::format as spoken_punctuation;
pub use reconstruct::reconstruct;
pub use symbols::{spoken_counts, spoken_symbols};

/// A dictation style (`DictationStyle`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// Full sentences, fillers trimmed.
    Polished,
    /// Texting: lowercase start, no final period.
    Casual,
    /// Short lines, a list for several items.
    Notes,
    /// Exact words from the model.
    Verbatim,
}

impl Style {
    /// The style's stored name, as `DictationStyle.rawValue`.
    pub fn name(self) -> &'static str {
        match self {
            Style::Polished => "polished",
            Style::Casual => "casual",
            Style::Notes => "notes",
            Style::Verbatim => "verbatim",
        }
    }

    pub fn from_name(name: &str) -> Option<Style> {
        [
            Style::Polished,
            Style::Casual,
            Style::Notes,
            Style::Verbatim,
        ]
        .into_iter()
        .find(|style| style.name() == name)
    }
}

/// A dictation language as the host's locale reads its BCP 47 identifier: iOS passes
/// `Locale(identifier:).language.languageCode` and `.region`, which canonicalize ("no" is
/// "nb", "iw" is "he"); the rules compare only the code and the region.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Language {
    pub identifier: String,
    pub code: Option<String>,
    pub region: Option<String>,
}

/// What [`format`] does besides the style.
#[derive(Clone, Copy, Debug)]
pub struct FormatOptions<'a> {
    pub style: Style,
    pub language: Option<&'a Language>,
    /// Settings › Dictation › Format lists.
    pub lists: bool,
    /// Settings › Dictation › Spoken punctuation.
    pub punctuation: bool,
    /// Settings › Dictation › End with a period.
    pub ends_with_period: bool,
}

/// `TranscriptFormatter.format`: `raw` in the style. Fillers trimmed (per language), then
/// Format lists and Spoken punctuation (in every style but Verbatim), then the style's
/// sentences; with `ends_with_period` off no line ends with a full stop.
pub fn format(raw: &str, options: &FormatOptions) -> String {
    formatter::format(
        raw,
        options.style,
        options.language,
        options.lists,
        options.punctuation,
        options.ends_with_period,
    )
}

/// `CleanupTagger.words(in:)`: the transcript's whitespace-separated words.
pub fn words(text: &str) -> Vec<String> {
    text::chars(text)
        .split(|c| text::is_whitespace(c))
        .filter(|word| !word.is_empty())
        .map(|word| word.concat())
        .collect()
}

/// The language code a rule reads, `nil` counting as English (`?? "en"`).
fn code_or_english(language: Option<&Language>) -> &str {
    match language {
        None => "en",
        Some(language) => language.code.as_deref().unwrap_or("en"),
    }
}

/// `SpeechLanguagePicker.isEnglish`: `nil` is English, a language without a code isn't.
fn is_english(language: Option<&Language>) -> bool {
    match language {
        None => true,
        Some(language) => language.code.as_deref() == Some("en"),
    }
}
