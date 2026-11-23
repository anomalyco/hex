//! Words the keyboard never suggests or corrects to (`OffensiveWords`, from
//! `never-suggest.tsv`): slurs always, swear words unless Suggest swear words is on. Checked
//! when a suggestion is looked up, not removed from the word models.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffensiveKind {
    /// Never offered, whatever the setting.
    Slur,
    /// Offered only with Suggest swear words on.
    Swear,
}

/// One layout's list: lowercased spellings and two-word expressions, composed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OffensiveWords {
    entries: HashMap<String, OffensiveKind>,
}

impl OffensiveWords {
    /// Reads `layout<TAB>slur|swear<TAB>spelling` lines for `language`, and the `*` lines
    /// every layout shares; `#` starts a comment.
    pub fn parse(tsv: &str, language: &str) -> Self {
        let mut entries: HashMap<String, OffensiveKind> = HashMap::new();
        for line in text::split_where(tsv, text::is_newline) {
            if text::has_prefix(line, "#") {
                continue;
            }
            let fields = text::split_keeping_empty(line, "\t");
            if fields.len() != 3 || !(text::eq(fields[0], "*") || text::eq(fields[0], language)) {
                continue;
            }
            let kind = if text::eq(fields[1], "slur") {
                OffensiveKind::Slur
            } else if text::eq(fields[1], "swear") {
                OffensiveKind::Swear
            } else {
                continue;
            };
            let lowered = text::lowercased(fields[2]);
            let spelling = text::split_on(&lowered, " ").join(" ");
            if spelling.is_empty() {
                continue;
            }
            let key = text::nfc(&spelling).into_owned();
            if entries.get(&key) != Some(&OffensiveKind::Slur) {
                entries.insert(key, kind);
            }
        }
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The kind of a spelling or two-word expression, by its lowercase.
    pub fn kind(&self, word: &str) -> Option<OffensiveKind> {
        self.entries
            .get(text::nfc(&text::lowercased(word)).as_ref())
            .copied()
    }

    /// Whether `candidate` (a word, or words split by spaces or hyphens) may not be offered:
    /// it, one of its words, or an expression it makes (with `previous`), is a slur, or a
    /// swear word while `allow_swearing` is off.
    pub fn blocks(&self, candidate: &str, allow_swearing: bool, previous: Option<&str>) -> bool {
        let blocked = |t: &str| match self.kind(t) {
            Some(OffensiveKind::Slur) => true,
            Some(OffensiveKind::Swear) => !allow_swearing,
            None => false,
        };
        let parts = text::split_where(candidate, |g| g == " " || g == "-");
        let Some(first) = parts.first() else {
            return false;
        };
        if parts.iter().any(|p| blocked(p)) {
            return true;
        }
        if let Some(previous) = previous
            && blocked(&format!("{previous} {first}"))
        {
            return true;
        }
        parts
            .windows(2)
            .any(|w| blocked(&format!("{} {}", w[0], w[1])))
    }

    /// The spellings and two-word expressions to block, as `blocks` would (any order).
    pub fn blocked(&self, allow_swearing: bool) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, kind)| **kind == OffensiveKind::Slur || !allow_swearing)
            .map(|(word, _)| word.clone())
            .collect()
    }
}

/// The bundled `never-suggest.tsv` the host hands over (Android's file, the union of both
/// apps' lists), read once per base language (`OffensiveWords.bundled`).
#[derive(Debug, Default)]
pub struct NeverSuggestList {
    tsv: String,
    loaded: Mutex<HashMap<String, Arc<OffensiveWords>>>,
}

impl NeverSuggestList {
    pub fn new(tsv: impl Into<String>) -> Self {
        Self {
            tsv: tsv.into(),
            loaded: Mutex::default(),
        }
    }

    /// The list for a base language ("en", "fr", "nb"; the shared `*` lines for others).
    pub fn bundled(&self, language: &str) -> Arc<OffensiveWords> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        loaded
            .entry(language.to_owned())
            .or_insert_with(|| Arc::new(OffensiveWords::parse(&self.tsv, language)))
            .clone()
    }
}
