//! The shared vocabulary: names Hex should know (people, projects,
//! products), each with spellings a transcript may show instead ("sounds
//! like"). Search uses it to find a term's likely mis-transcriptions,
//! prompts can read it through [`terms`], and Parakeet biases its
//! transcripts toward it through [`transcription_bias`].

use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// Longest term or "sounds like" entry, in characters.
const MAX_TERM_CHARS: usize = 64;
/// Most entries in the list, and most "sounds like" spellings per entry.
pub const MAX_ENTRIES: usize = 500;
const MAX_SOUNDS_LIKE: usize = 16;
/// Most terms one transcription is biased toward, first in list order: the
/// engine's own cap (`k_vocab_max_terms` in transcribe.cpp), which keeps the
/// biasing a bounded cost per decode.
const MAX_BIASED_TERMS: usize = 64;
/// Read limit for settings.json when reading it without loading `AppSettings`.
const SETTINGS_READ_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct VocabularyEntry {
    pub term: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds_like: Vec<String>,
}

/// How readily transcription swaps heard words for a vocabulary term that
/// sounds like them. Measured on real recordings in
/// `docs/research/vocabulary-boost-results.md`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VocabularyBoost {
    /// Transcripts are left as the model heard them.
    Off,
    Light,
    #[default]
    Normal,
    Strong,
}

impl VocabularyBoost {
    pub const ALL: [Self; 4] = [Self::Off, Self::Light, Self::Normal, Self::Strong];

    /// How much less likely than the heard words, in nats per term piece, a
    /// term may score and still replace them.
    fn nats(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Light => 2.0,
            Self::Normal => 2.5,
            Self::Strong => 4.0,
        }
    }

    fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
}

static SAVED: OnceLock<RwLock<Vec<VocabularyEntry>>> = OnceLock::new();
static BOOST: AtomicU8 = AtomicU8::new(VocabularyBoost::Normal as u8);

/// The saved terms, as last applied from settings.
pub fn terms() -> Vec<String> {
    entries().into_iter().map(|entry| entry.term).collect()
}

/// The saved entries, as last applied from settings.
pub fn entries() -> Vec<VocabularyEntry> {
    SAVED
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The terms and options a Parakeet run biases its transcript with, read
/// at the start of each run and live stream so settings changes apply to the
/// next one. `None` while the list is empty or the boost is off.
pub(crate) fn transcription_bias()
-> Option<(Vec<String>, transcribe_cpp::ParakeetVocabularyOptions)> {
    // Labs › Vocabulary biasing: transcription takes the list only while on.
    if !crate::labs::vocabulary_biasing() {
        return None;
    }
    let boost = VocabularyBoost::from_index(BOOST.load(Ordering::Relaxed));
    if boost == VocabularyBoost::Off {
        return None;
    }
    bias_for(&entries(), boost)
}

fn bias_for(
    entries: &[VocabularyEntry],
    boost: VocabularyBoost,
) -> Option<(Vec<String>, transcribe_cpp::ParakeetVocabularyOptions)> {
    let entries = &entries[..entries.len().min(MAX_BIASED_TERMS)];
    if entries.is_empty() {
        return None;
    }
    let sounds_like = entries
        .iter()
        .enumerate()
        .flat_map(|(index, entry)| {
            entry
                .sounds_like
                .iter()
                .map(move |spelling| (index, spelling.clone()))
        })
        .collect();
    Some((
        entries.iter().map(|entry| entry.term.clone()).collect(),
        transcribe_cpp::ParakeetVocabularyOptions {
            boost: Some(boost.nats()),
            sounds_like,
        },
    ))
}

/// Called by `AppSettings` whenever settings load or save.
pub(crate) fn apply(entries: &[VocabularyEntry], boost: VocabularyBoost) {
    *SAVED
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|error| error.into_inner()) = entries.to_vec();
    BOOST.store(boost as u8, Ordering::Relaxed);
}

/// The entries saved in `root/settings.json`, read without loading
/// `AppSettings` (which migrates and writes), for read-only commands. Empty
/// when the file is missing or unreadable.
pub fn read_saved(root: &Path) -> Vec<VocabularyEntry> {
    match read_from(&root.join("settings.json")) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "could not read the vocabulary; searching without it");
            Vec::new()
        }
    }
}

fn read_from(path: &Path) -> Result<Vec<VocabularyEntry>, String> {
    #[derive(Deserialize)]
    struct Settings {
        #[serde(default)]
        vocabulary: Vec<VocabularyEntry>,
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(SETTINGS_READ_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > SETTINGS_READ_LIMIT {
        return Err("settings file exceeds the read limit".into());
    }
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Ok(settings.vocabulary)
}

/// Splits a comma-separated "sounds like" field into trimmed spellings.
pub fn parse_sounds_like(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(clean)
        .filter(|spelling| !spelling.is_empty())
        .collect()
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn same(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

/// Adds `term`, or adds new `sounds_like` spellings to an existing entry with
/// the same term in any case. Returns whether the list changed.
pub fn add(
    list: &mut Vec<VocabularyEntry>,
    term: &str,
    sounds_like: &[String],
) -> Result<bool, String> {
    let term = clean(term);
    if term.is_empty() {
        return Err("a vocabulary term can't be empty".into());
    }
    let mut spellings: Vec<String> = Vec::new();
    for spelling in sounds_like.iter().map(|spelling| clean(spelling)) {
        if spelling.is_empty()
            || same(&spelling, &term)
            || spellings.iter().any(|kept| same(kept, &spelling))
        {
            continue;
        }
        spellings.push(spelling);
    }
    if let Some(long) = std::iter::once(&term)
        .chain(&spellings)
        .find(|text| text.chars().count() > MAX_TERM_CHARS)
    {
        return Err(format!(
            "“{long}” is longer than {MAX_TERM_CHARS} characters"
        ));
    }
    let existing = list.iter().position(|entry| same(&entry.term, &term));
    if existing.is_none() && list.len() >= MAX_ENTRIES {
        return Err(format!("the vocabulary holds at most {MAX_ENTRIES} terms"));
    }
    let mut merged = existing
        .map(|index| list[index].sounds_like.clone())
        .unwrap_or_default();
    let before = merged.len();
    for spelling in spellings {
        if !merged.iter().any(|kept| same(kept, &spelling)) {
            merged.push(spelling);
        }
    }
    if merged.len() > MAX_SOUNDS_LIKE {
        return Err(format!(
            "a term takes at most {MAX_SOUNDS_LIKE} “sounds like” spellings"
        ));
    }
    match existing {
        Some(index) => {
            list[index].sounds_like = merged;
            Ok(list[index].sounds_like.len() != before)
        }
        None => {
            list.push(VocabularyEntry {
                term,
                sounds_like: merged,
            });
            Ok(true)
        }
    }
}

/// Like [`add`], but keeps what fits instead of refusing: a long term is cut
/// to its first 64 characters, and a long spelling or one past the 16th is
/// left out. A full list still takes no new term. Returns whether anything
/// was cut or left out.
pub fn add_clamped(list: &mut Vec<VocabularyEntry>, term: &str, sounds_like: &[String]) -> bool {
    let mut term = clean(term);
    let mut clamped = false;
    if term.chars().count() > MAX_TERM_CHARS {
        term = clean(&term.chars().take(MAX_TERM_CHARS).collect::<String>());
        clamped = true;
    }
    if term.is_empty() {
        return clamped;
    }
    if add(list, &term, &[]).is_err() {
        return true;
    }
    for spelling in sounds_like {
        clamped |= add(list, &term, std::slice::from_ref(spelling)).is_err();
    }
    clamped
}

/// Whether the list has an entry for `term`, in any case.
pub fn contains(list: &[VocabularyEntry], term: &str) -> bool {
    let term = clean(term);
    list.iter().any(|entry| same(&entry.term, &term))
}

/// Removes the entry for `term`, in any case. Returns whether it was there.
pub fn remove(list: &mut Vec<VocabularyEntry>, term: &str) -> bool {
    let term = clean(term);
    let before = list.len();
    list.retain(|entry| !same(&entry.term, &term));
    list.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, sounds_like: &[&str]) -> VocabularyEntry {
        VocabularyEntry {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        }
    }

    #[test]
    fn adding_dedupes_by_case_and_merges_spellings() {
        let mut list = Vec::new();
        assert!(add(&mut list, "  Abattoir ", &["avatar".into(), "".into()]).unwrap());
        assert_eq!(list, [entry("Abattoir", &["avatar"])]);
        assert!(!add(&mut list, "abattoir", &["Avatar".into()]).unwrap());
        assert!(add(&mut list, "ABATTOIR", &["a bat war".into()]).unwrap());
        assert_eq!(list, [entry("Abattoir", &["avatar", "a bat war"])]);
        // A spelling identical to its term adds nothing.
        assert!(!add(&mut list, "Abattoir", &["abattoir".into()]).unwrap());
        assert!(add(&mut list, "   ", &[]).is_err());
        assert!(add(&mut list, &"x".repeat(65), &[]).is_err());
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn clamped_adds_keep_the_term_and_the_spellings_that_fit() {
        let spellings: Vec<String> = (0..20).map(|index| format!("spelling {index}")).collect();
        let mut list = Vec::new();
        let mut with_long = spellings[..2].to_vec();
        with_long.push("y".repeat(65));
        assert!(add_clamped(&mut list, "Abattoir", &with_long));
        assert_eq!(list, [entry("Abattoir", &["spelling 0", "spelling 1"])]);
        assert!(add_clamped(&mut list, "abattoir", &spellings));
        assert_eq!(list[0].sounds_like, spellings[..MAX_SOUNDS_LIKE]);

        assert!(add_clamped(&mut list, &"x".repeat(70), &["ex".into()]));
        assert_eq!(list[1], entry(&"x".repeat(MAX_TERM_CHARS), &["ex"]));
        assert!(!add_clamped(&mut list, "Rune", &["roon".into()]));
        assert!(!add_clamped(&mut list, "  ", &[]));
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn removing_ignores_case() {
        let mut list = vec![entry("OpenCode", &[]), entry("Rune", &[])];
        assert!(remove(&mut list, "opencode"));
        assert!(!remove(&mut list, "opencode"));
        assert_eq!(list, [entry("Rune", &[])]);
    }

    #[test]
    fn transcription_bias_keeps_the_first_terms_and_their_spellings() {
        assert!(bias_for(&[], VocabularyBoost::Normal).is_none());
        let mut list = vec![
            entry("Abattoir", &["avatar", "a bat war"]),
            entry("Rune", &[]),
        ];
        list.extend((0..70).map(|index| entry(&format!("term {index}"), &["x"])));
        let (terms, options) = bias_for(&list, VocabularyBoost::Strong).unwrap();
        assert_eq!(terms.len(), MAX_BIASED_TERMS);
        assert_eq!(terms[..2], ["Abattoir", "Rune"]);
        assert_eq!(options.boost, Some(4.0));
        assert_eq!(
            options.sounds_like[..2],
            [(0, "avatar".into()), (0, "a bat war".into())]
        );
        assert_eq!(options.sounds_like[2], (2, "x".into()));
        assert!(
            options
                .sounds_like
                .iter()
                .all(|(index, _)| *index < MAX_BIASED_TERMS)
        );
    }

    #[test]
    fn boost_reads_and_writes_as_its_name() {
        for boost in VocabularyBoost::ALL {
            assert_eq!(VocabularyBoost::from_index(boost as u8), boost);
        }
        assert_eq!(
            serde_json::to_value(VocabularyBoost::Strong).unwrap(),
            serde_json::json!("strong")
        );
        assert_eq!(VocabularyBoost::default().nats(), 2.5);
    }

    #[test]
    fn sounds_like_splits_on_commas() {
        assert_eq!(
            parse_sounds_like(" avatar,, a  bat war ,"),
            ["avatar", "a bat war"]
        );
        assert!(parse_sounds_like(" , ").is_empty());
    }

    #[test]
    fn saved_entries_read_without_loading_settings() {
        let directory = std::env::temp_dir().join(format!("hex-vocabulary-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");
        assert_eq!(read_from(&path).unwrap(), []);
        std::fs::write(&path, r#"{"sound_effects": false}"#).unwrap();
        assert_eq!(read_from(&path).unwrap(), []);
        std::fs::write(
            &path,
            r#"{"vocabulary": [{"term": "Rune"}, {"term": "Abattoir", "sounds_like": ["avatar"]}]}"#,
        )
        .unwrap();
        assert_eq!(
            read_from(&path).unwrap(),
            [entry("Rune", &[]), entry("Abattoir", &["avatar"])]
        );
        std::fs::write(&path, "{").unwrap();
        assert!(read_from(&path).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
