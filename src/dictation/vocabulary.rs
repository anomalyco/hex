//! The shared vocabulary: names Hex should know (people, projects,
//! products), each with spellings a transcript may show instead ("sounds
//! like"). Search uses it to find a term's likely mis-transcriptions,
//! prompts can read it through [`terms`], Parakeet biases its transcripts
//! toward it through [`transcription_bias`], and Whisper and Qwen3-ASR read
//! it as their context prompt through [`transcription_prompt`].

use std::cell::Cell;
use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::dictation::context::ContextSnapshot;

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
#[serde(from = "SavedEntry")]
pub struct VocabularyEntry {
    pub term: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds_like: Vec<String>,
}

/// An entry as `settings.json` holds it: a plain term, or the term with its
/// "sounds like" spellings.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum SavedEntry {
    Term(String),
    Entry {
        term: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sounds_like: Vec<String>,
    },
}

impl From<SavedEntry> for VocabularyEntry {
    fn from(saved: SavedEntry) -> Self {
        match saved {
            SavedEntry::Term(term) => Self {
                term,
                sounds_like: Vec::new(),
            },
            SavedEntry::Entry { term, sounds_like } => Self { term, sounds_like },
        }
    }
}

/// Writes a list for `settings.json`: a term with no "sounds like"
/// spellings as a plain string. Either form loads.
pub(crate) fn serialize_saved<S: serde::Serializer>(
    entries: &[VocabularyEntry],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(entries.iter().map(|entry| {
        if entry.sounds_like.is_empty() {
            SavedEntry::Term(entry.term.clone())
        } else {
            SavedEntry::Entry {
                term: entry.term.clone(),
                sounds_like: entry.sounds_like.clone(),
            }
        }
    }))
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
/// The focused app and page as last seen (macOS), whose mode's own terms a
/// dictation hears with ahead of the shared list.
static FOCUS: OnceLock<RwLock<Option<ContextSnapshot>>> = OnceLock::new();

thread_local! {
    static DICTATING: Cell<bool> = const { Cell::new(false) };
}

/// Records the focused app and page, so the next dictation's transcription
/// hears with its mode's vocabulary.
pub(crate) fn focus(context: &ContextSnapshot) {
    *FOCUS
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|error| error.into_inner()) = Some(context.clone());
}

/// Runs `work` as a dictation's transcription on this thread: its runs hear
/// with the focused mode's terms too. Meetings, captures, and other hosts'
/// audio hear with the shared list alone.
pub(crate) fn for_dictation<R>(work: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            DICTATING.with(|flag| flag.set(self.0));
        }
    }
    let _restore = Restore(DICTATING.with(|flag| flag.replace(true)));
    work()
}

/// The focused mode's own terms first, so the bias cap keeps them, then the
/// shared list, each term once.
fn transcription_entries() -> Vec<VocabularyEntry> {
    let shared = entries();
    if !DICTATING.with(Cell::get) {
        return shared;
    }
    let focused = FOCUS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    #[cfg(target_os = "macos")]
    if let Some(context) = focused {
        let own = crate::dictation::config::dictation_profiles().vocabulary_for(&context);
        return merged(&own, &shared);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = focused;
    shared
}

/// `first`, then the entries of `then` whose term `first` lacks, in any case.
pub(crate) fn merged(first: &[VocabularyEntry], then: &[VocabularyEntry]) -> Vec<VocabularyEntry> {
    if first.is_empty() {
        return then.to_vec();
    }
    let mut list = first.to_vec();
    for entry in then {
        if !contains(&list, &entry.term) {
            list.push(entry.clone());
        }
    }
    list
}

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
    bias_for(&transcription_entries(), boost)
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

/// The context prompt Whisper and Qwen3-ASR runs read: the first terms in
/// list order (at most the bias cap), comma-separated so the echo filter can
/// recognise the list, and only as many as fit `max_chars`. `None` while the
/// list is empty. Read at the start of each run, like the bias.
pub(crate) fn transcription_prompt(max_chars: usize) -> Option<String> {
    prompt_for(&transcription_entries(), max_chars)
}

fn prompt_for(entries: &[VocabularyEntry], max_chars: usize) -> Option<String> {
    let mut prompt = String::new();
    for entry in entries.iter().take(MAX_BIASED_TERMS) {
        let separator = if prompt.is_empty() { "" } else { ", " };
        if prompt.chars().count() + separator.len() + entry.term.chars().count() > max_chars {
            break;
        }
        prompt.push_str(separator);
        prompt.push_str(&entry.term);
    }
    (!prompt.is_empty()).then_some(prompt)
}

/// Adds each name in recognition hints saved before they merged into the
/// vocabulary (comma, semicolon, or line separated), skipping names already
/// listed in any case. Returns whether the list changed.
pub(crate) fn fold_hints(list: &mut Vec<VocabularyEntry>, hints: &str) -> bool {
    let before = list.len();
    for hint in hints.split([',', ';', '\n']) {
        add_clamped(list, hint, &[]);
    }
    list.len() != before
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
    fn plain_terms_load_and_save_as_strings() {
        #[derive(Deserialize, Serialize)]
        struct File {
            #[serde(serialize_with = "serialize_saved")]
            vocabulary: Vec<VocabularyEntry>,
        }
        let file: File = serde_json::from_str(
            r#"{"vocabulary": ["Rune", {"term": "OpenCode", "sounds_like": ["open code"]},
                {"term": "Kaffar"}]}"#,
        )
        .unwrap();
        assert_eq!(
            file.vocabulary,
            [
                entry("Rune", &[]),
                entry("OpenCode", &["open code"]),
                entry("Kaffar", &[])
            ]
        );
        assert_eq!(
            serde_json::to_string(&file).unwrap(),
            r#"{"vocabulary":["Rune",{"term":"OpenCode","sounds_like":["open code"]},"Kaffar"]}"#
        );
        // Replies to `hex vocabulary` keep the object form.
        assert_eq!(
            serde_json::to_string(&entry("Rune", &[])).unwrap(),
            r#"{"term":"Rune"}"#
        );
    }

    #[test]
    fn a_modes_terms_come_first_and_each_term_once() {
        let own = [entry("useEffect", &[]), entry("Rune", &["rooney"])];
        let shared = [entry("rune", &[]), entry("OpenCode", &[])];
        assert_eq!(
            merged(&own, &shared),
            [
                entry("useEffect", &[]),
                entry("Rune", &["rooney"]),
                entry("OpenCode", &[])
            ]
        );
        assert_eq!(merged(&[], &shared), shared);
    }

    #[test]
    fn only_dictation_hears_with_the_focused_modes_terms() {
        apply(&[entry("OpenCode", &[])], VocabularyBoost::Normal);
        assert_eq!(transcription_entries(), [entry("OpenCode", &[])]);
        let inside = for_dictation(|| {
            assert!(DICTATING.with(Cell::get));
            transcription_entries()
        });
        assert!(inside.iter().any(|entry| entry.term == "OpenCode"));
        assert!(!DICTATING.with(Cell::get));
    }

    #[test]
    fn adding_dedupes_by_case_and_merges_spellings() {
        let mut list = Vec::new();
        assert!(add(&mut list, "  Cappuccino ", &["capuchin".into(), "".into()]).unwrap());
        assert_eq!(list, [entry("Cappuccino", &["capuchin"])]);
        assert!(!add(&mut list, "cappuccino", &["Capuchin".into()]).unwrap());
        assert!(add(&mut list, "CAPPUCCINO", &["cap a chino".into()]).unwrap());
        assert_eq!(list, [entry("Cappuccino", &["capuchin", "cap a chino"])]);
        // A spelling identical to its term adds nothing.
        assert!(!add(&mut list, "Cappuccino", &["cappuccino".into()]).unwrap());
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
        assert!(add_clamped(&mut list, "Cappuccino", &with_long));
        assert_eq!(list, [entry("Cappuccino", &["spelling 0", "spelling 1"])]);
        assert!(add_clamped(&mut list, "cappuccino", &spellings));
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
            entry("Cappuccino", &["capuchin", "cap a chino"]),
            entry("Rune", &[]),
        ];
        list.extend((0..70).map(|index| entry(&format!("term {index}"), &["x"])));
        let (terms, options) = bias_for(&list, VocabularyBoost::Strong).unwrap();
        assert_eq!(terms.len(), MAX_BIASED_TERMS);
        assert_eq!(terms[..2], ["Cappuccino", "Rune"]);
        assert_eq!(options.boost, Some(4.0));
        assert_eq!(
            options.sounds_like[..2],
            [(0, "capuchin".into()), (0, "cap a chino".into())]
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
    fn the_prompt_keeps_the_first_terms_that_fit() {
        let list = vec![
            entry("Kaffar", &["Caffer"]),
            entry("Rodolfo", &[]),
            entry("Bragelonne", &[]),
        ];
        assert_eq!(
            prompt_for(&list, 100).as_deref(),
            Some("Kaffar, Rodolfo, Bragelonne")
        );
        assert_eq!(prompt_for(&list, 20).as_deref(), Some("Kaffar, Rodolfo"));
        assert_eq!(prompt_for(&list, 3), None);
        assert_eq!(prompt_for(&[], 100), None);
        let many: Vec<_> = (0..100).map(|n| entry(&format!("T{n}"), &[])).collect();
        let prompt = prompt_for(&many, 10_000).unwrap();
        assert_eq!(prompt.split(", ").count(), MAX_BIASED_TERMS);
    }

    #[test]
    fn hints_fold_into_the_list_without_duplicates() {
        let mut list = vec![entry("OpenCode", &["open code"])];
        assert!(fold_hints(&mut list, "opencode, Effect;Rune\n  ,Effect"));
        assert_eq!(
            list,
            [
                entry("OpenCode", &["open code"]),
                entry("Effect", &[]),
                entry("Rune", &[]),
            ]
        );
        assert!(!fold_hints(&mut list, "Rune, effect"));
        assert!(!fold_hints(&mut list, "  "));
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
            parse_sounds_like(" capuchin,, cap  a chino ,"),
            ["capuchin", "cap a chino"]
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
            r#"{"vocabulary": [{"term": "Rune"}, {"term": "Cappuccino", "sounds_like": ["capuchin"]}]}"#,
        )
        .unwrap();
        assert_eq!(
            read_from(&path).unwrap(),
            [entry("Rune", &[]), entry("Cappuccino", &["capuchin"])]
        );
        std::fs::write(&path, "{").unwrap();
        assert!(read_from(&path).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
