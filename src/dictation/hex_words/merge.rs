//! Merging a file that was read into the desktop's lists, the desktop
//! column of the spec's merge table: in file order, vocabulary first, never
//! removing anything, and keeping the person's own output on a clash unless
//! they picked the file's.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization as _;

use super::{Place, Replacement, Term, clean};
use crate::app_settings::AppSettings;
use crate::dictation::text_replacements::TextReplacement;
use crate::dictation::vocabulary::{self, VocabularyEntry};

/// The two lists a words file changes on the desktop: the shared vocabulary
/// and the Global mode's replacements
/// (`dictation_processing.default_mode.replacements`). Other modes' lists
/// are never read or changed.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Lists {
    pub(crate) vocabulary: Vec<VocabularyEntry>,
    pub(crate) replacements: Vec<TextReplacement>,
}

impl Lists {
    /// The two lists as `settings` holds them.
    pub(crate) fn of(settings: &AppSettings) -> Self {
        Self {
            vocabulary: settings.vocabulary.clone(),
            replacements: settings
                .dictation_processing
                .default_mode
                .replacements
                .clone(),
        }
    }

    /// Puts both lists into `settings`, leaving everything else alone.
    pub(crate) fn put(&self, settings: &mut AppSettings) {
        settings.vocabulary = self.vocabulary.clone();
        settings.dictation_processing.default_mode.replacements = self.replacements.clone();
    }
}

/// How much an import may grow the lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Limits {
    pub(crate) terms: usize,
    pub(crate) spellings: usize,
    /// Applies to importing only: a Global list that already holds more
    /// keeps them all.
    pub(crate) replacements: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            terms: vocabulary::MAX_ENTRIES,
            spellings: vocabulary::MAX_SOUNDS_LIKE,
            replacements: super::MAX_REPLACEMENTS,
        }
    }
}

/// The review sheet's switches: a part switched off is left alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Parts {
    pub(crate) vocabulary: bool,
    pub(crate) replacements: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Self {
            vocabulary: true,
            replacements: true,
        }
    }
}

/// What a merge did, counted as the spec counts it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Summary {
    pub(crate) added_terms: usize,
    /// Terms already listed that gained a spelling.
    pub(crate) updated_terms: usize,
    pub(crate) already_have_terms: usize,
    pub(crate) added_replacements: usize,
    /// Clashes the person switched to the file's output.
    pub(crate) changed_replacements: usize,
    /// Clashes that kept the person's output.
    pub(crate) kept_yours: usize,
    pub(crate) already_have_replacements: usize,
}

impl Summary {
    /// Whether the lists changed.
    pub(crate) fn changed(&self) -> bool {
        self.added_terms + self.updated_terms + self.added_replacements + self.changed_replacements
            > 0
    }
}

/// The same heard words with another output: the person's and the file's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Clash {
    /// The file's replacement, `replacements[index]`.
    pub(crate) index: usize,
    /// The person's own heard words.
    pub(crate) heard: String,
    pub(crate) yours: String,
    pub(crate) theirs: String,
}

/// The lists after a merge, and what it did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Merged {
    pub(crate) lists: Lists,
    pub(crate) summary: Summary,
    pub(crate) clashes: Vec<Clash>,
    /// What didn't fit this device's limits (`overDeviceLimit`), in file
    /// order.
    pub(crate) over_limit: Vec<Place>,
}

/// Merges the file's `terms` and `replacements` into `before`. A clash
/// keeps the person's output unless `theirs` names the file's replacement.
pub(crate) fn merge(
    before: &Lists,
    terms: &[Term],
    replacements: &[Replacement],
    limits: Limits,
    parts: Parts,
    theirs: &BTreeSet<usize>,
) -> Merged {
    let mut merged = Merged {
        lists: before.clone(),
        ..Merged::default()
    };
    if parts.vocabulary {
        merge_terms(&mut merged, terms, limits);
    }
    if parts.replacements {
        merge_replacements(&mut merged, replacements, limits, theirs);
    }
    merged
}

/// The key text compares by, cleaned first: a list's own text may not be,
/// and neither may a file's that didn't come through [`super::read`].
fn key_of(text: &str) -> String {
    clean::key(&clean::words(text))
}

/// Each text's key mapped to its first place in the list.
fn keyed<'a>(texts: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut keys = HashMap::new();
    for (index, text) in texts.enumerate() {
        keys.entry(key_of(text)).or_insert(index);
    }
    keys
}

fn merge_terms(merged: &mut Merged, terms: &[Term], limits: Limits) {
    let list = &mut merged.lists.vocabulary;
    let mut keys = keyed(list.iter().map(|entry| entry.term.as_str()));
    for (index, term) in terms.iter().enumerate() {
        let key = key_of(&term.term);
        match keys.get(&key) {
            Some(&at) => {
                let entry = &mut list[at];
                let mut gained = false;
                for (spelling_index, spelling) in term.sounds_like.iter().enumerate() {
                    let spelling_key = key_of(spelling);
                    if spelling_key == key_of(&entry.term)
                        || entry
                            .sounds_like
                            .iter()
                            .any(|kept| key_of(kept) == spelling_key)
                    {
                        continue;
                    }
                    if entry.sounds_like.len() >= limits.spellings {
                        merged
                            .over_limit
                            .push(Place::Spelling(index, spelling_index));
                        continue;
                    }
                    entry.sounds_like.push(spelling.clone());
                    gained = true;
                }
                if gained {
                    merged.summary.updated_terms += 1;
                } else {
                    merged.summary.already_have_terms += 1;
                }
            }
            None if list.len() >= limits.terms => merged.over_limit.push(Place::Term(index)),
            None => {
                let mut sounds_like = term.sounds_like.clone();
                for spelling_index in limits.spellings..sounds_like.len() {
                    merged
                        .over_limit
                        .push(Place::Spelling(index, spelling_index));
                }
                sounds_like.truncate(limits.spellings);
                keys.insert(key, list.len());
                list.push(VocabularyEntry {
                    term: term.term.clone(),
                    sounds_like,
                });
                merged.summary.added_terms += 1;
            }
        }
    }
}

fn merge_replacements(
    merged: &mut Merged,
    replacements: &[Replacement],
    limits: Limits,
    theirs: &BTreeSet<usize>,
) {
    let list = &mut merged.lists.replacements;
    let mut keys = keyed(list.iter().map(|rule| rule.matched_phrase.as_str()));
    for (index, replacement) in replacements.iter().enumerate() {
        let key = key_of(&replacement.heard);
        match keys.get(&key) {
            Some(&at) => {
                let rule = &mut list[at];
                if rule.output.nfc().collect::<String>() == replacement.write {
                    merged.summary.already_have_replacements += 1;
                    continue;
                }
                merged.clashes.push(Clash {
                    index,
                    heard: rule.matched_phrase.clone(),
                    yours: rule.output.clone(),
                    theirs: replacement.write.clone(),
                });
                if theirs.contains(&index) {
                    rule.output = replacement.write.clone();
                    merged.summary.changed_replacements += 1;
                } else {
                    merged.summary.kept_yours += 1;
                }
            }
            None if list.len() >= limits.replacements => {
                merged.over_limit.push(Place::Replacement(index));
            }
            None => {
                keys.insert(key, list.len());
                list.push(TextReplacement {
                    matched_phrase: replacement.heard.clone(),
                    output: replacement.write.clone(),
                });
                merged.summary.added_replacements += 1;
            }
        }
    }
}
