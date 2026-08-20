//! Retained dictation history: an owner-only, bounded, crash-safe store of
//! successful dictation and Voice Action results.
//!
//! History is a product record, deliberately separate from the diagnostic
//! Activity event stream. Entries hold text and bounded metadata only; audio
//! is never retained here. Every retention choice remains subject to hard
//! entry and byte caps, writes are atomic, and files are owner-only.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 2_000;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_LABEL_BYTES: usize = 256;
const MAX_FALLBACK_BYTES: usize = 1_024;
const MAX_TOTAL_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SEARCH_RESULTS: usize = 200;

/// How long successful results remain in retained history.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryRetention {
    Off,
    Day,
    #[default]
    Week,
    Month,
    Forever,
}

impl HistoryRetention {
    pub const ALL: [Self; 5] = [Self::Off, Self::Day, Self::Week, Self::Month, Self::Forever];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::Forever => "Forever",
        }
    }

    pub const fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }

    const fn max_age_ms(self) -> Option<u64> {
        const HOUR_MS: u64 = 60 * 60 * 1_000;
        match self {
            Self::Off | Self::Forever => None,
            Self::Day => Some(24 * HOUR_MS),
            Self::Week => Some(7 * 24 * HOUR_MS),
            Self::Month => Some(30 * 24 * HOUR_MS),
        }
    }
}

/// Which successful output produced an entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Dictation,
    Send,
    VoiceAction,
}

impl HistoryKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dictation => "Dictation",
            Self::Send => "Send",
            Self::VoiceAction => "Voice Action",
        }
    }
}

/// Bounded record of the post-processing that shaped the final text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HistoryProcessing {
    pub profile: String,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// One retained successful result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub kind: HistoryKind,
    /// Corrected local transcript before mode processing.
    pub raw_text: String,
    /// Text that was actually inserted.
    pub final_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<HistoryProcessing>,
    #[serde(default)]
    pub audio_ms: u64,
    #[serde(default)]
    pub inference_ms: u64,
    #[serde(default)]
    pub total_ms: u64,
}

impl HistoryEntry {
    fn text_bytes(&self) -> usize {
        self.raw_text.len() + self.final_text.len()
    }

    fn matches(&self, needle: &str) -> bool {
        let matches_field = |field: &str| field.to_lowercase().contains(needle);
        matches_field(&self.raw_text)
            || matches_field(&self.final_text)
            || self.application.as_deref().is_some_and(matches_field)
            || self
                .processing
                .as_ref()
                .is_some_and(|processing| matches_field(&processing.profile))
    }
}

/// A successful result awaiting a history identity.
#[derive(Clone, Debug)]
pub struct HistoryDraft {
    pub kind: HistoryKind,
    pub raw_text: String,
    pub final_text: String,
    pub application: Option<String>,
    pub processing: Option<HistoryProcessing>,
    pub audio_ms: u64,
    pub inference_ms: u64,
    pub total_ms: u64,
}

#[derive(Serialize)]
struct SavedHistory<'a> {
    version: u32,
    next_id: u64,
    entries: &'a [HistoryEntry],
}

#[derive(Deserialize)]
struct LoadedHistory {
    version: u32,
    next_id: u64,
    entries: Vec<HistoryEntry>,
}

/// Owner-only bounded history store. Entries are held oldest-first.
pub struct HistoryStore {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    next_id: u64,
    retention: HistoryRetention,
}

impl HistoryStore {
    /// Open the store, recover from unreadable content, and prune expired
    /// entries. A malformed file is preserved beside the store instead of
    /// being silently destroyed.
    pub fn open(path: PathBuf, retention: HistoryRetention, now_ms: u64) -> Self {
        let mut store = Self {
            path,
            entries: Vec::new(),
            next_id: 1,
            retention,
        };
        match fs::read(&store.path) {
            Ok(bytes) => match serde_json::from_slice::<LoadedHistory>(&bytes) {
                Ok(loaded) if loaded.version == VERSION => {
                    let max_entry_id = loaded.entries.iter().map(|entry| entry.id).max();
                    store.next_id = loaded
                        .next_id
                        .max(max_entry_id.map_or(1, |id| id.saturating_add(1)));
                    store.entries = loaded.entries;
                    store.entries.sort_by_key(|entry| entry.id);
                }
                Ok(loaded) => {
                    tracing::warn!(version = loaded.version, "unsupported history version");
                    store.preserve_corrupt();
                }
                Err(error) => {
                    tracing::warn!(%error, path = %store.path.display(), "history file is malformed");
                    store.preserve_corrupt();
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = %store.path.display(), "could not read history");
            }
        }
        if store.prune(now_ms)
            && let Err(error) = store.persist()
        {
            tracing::warn!(%error, "could not persist pruned history");
        }
        store
    }

    /// Record one successful result. Returns the stable entry ID, or `None`
    /// while retention is off.
    pub fn record(&mut self, draft: HistoryDraft, now_ms: u64) -> io::Result<Option<u64>> {
        if self.retention.is_off() {
            return Ok(None);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(HistoryEntry {
            id,
            timestamp_ms: now_ms,
            kind: draft.kind,
            raw_text: truncated(&draft.raw_text, MAX_TEXT_BYTES),
            final_text: truncated(&draft.final_text, MAX_TEXT_BYTES),
            application: draft
                .application
                .map(|application| truncated(&application, MAX_LABEL_BYTES)),
            processing: draft.processing.map(|processing| HistoryProcessing {
                profile: truncated(&processing.profile, MAX_LABEL_BYTES),
                latency_ms: processing.latency_ms,
                fallback: processing
                    .fallback
                    .map(|fallback| truncated(&fallback, MAX_FALLBACK_BYTES)),
            }),
            audio_ms: draft.audio_ms,
            inference_ms: draft.inference_ms,
            total_ms: draft.total_ms,
        });
        self.prune(now_ms);
        self.persist()?;
        Ok(Some(id))
    }

    /// Delete one entry. Returns whether it existed.
    pub fn delete(&mut self, id: u64) -> io::Result<bool> {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == before {
            return Ok(false);
        }
        self.persist()?;
        Ok(true)
    }

    /// Delete every entry.
    pub fn clear(&mut self) -> io::Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }
        self.entries.clear();
        self.persist()
    }

    /// Change retention and immediately prune to the new window. Turning
    /// history off stops recording but keeps existing entries until they are
    /// deleted explicitly.
    pub fn set_retention(&mut self, retention: HistoryRetention, now_ms: u64) -> io::Result<()> {
        if self.retention == retention {
            return Ok(());
        }
        self.retention = retention;
        if self.prune(now_ms) {
            self.persist()?;
        }
        Ok(())
    }

    /// All entries, newest first.
    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev()
    }

    #[cfg(test)]
    pub fn entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Case-insensitive substring search over text, application, and
    /// processing profile. Newest first and bounded.
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.entries().take(MAX_SEARCH_RESULTS).collect();
        }
        self.entries()
            .filter(|entry| entry.matches(&needle))
            .take(MAX_SEARCH_RESULTS)
            .collect()
    }

    fn prune(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        if let Some(max_age) = self.retention.max_age_ms() {
            let cutoff = now_ms.saturating_sub(max_age);
            let before = self.entries.len();
            self.entries.retain(|entry| entry.timestamp_ms >= cutoff);
            changed |= self.entries.len() != before;
        }
        while self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
            changed = true;
        }
        let mut total: usize = self.entries.iter().map(HistoryEntry::text_bytes).sum();
        while total > MAX_TOTAL_TEXT_BYTES && self.entries.len() > 1 {
            total -= self.entries.remove(0).text_bytes();
            changed = true;
        }
        changed
    }

    fn persist(&self) -> io::Result<()> {
        let saved = SavedHistory {
            version: VERSION,
            next_id: self.next_id,
            entries: &self.entries,
        };
        let json = serde_json::to_vec(&saved).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temporary)?;
            restrict_to_owner(&file)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        fs::rename(&temporary, &self.path)
    }

    fn preserve_corrupt(&self) {
        let backup = self.path.with_extension("json.corrupt");
        if let Err(error) = fs::rename(&self.path, &backup) {
            tracing::warn!(%error, "could not preserve the malformed history file");
        }
    }
}

/// Thread-safe shared handle over one history store.
#[derive(Clone)]
pub struct History {
    store: Arc<Mutex<HistoryStore>>,
}

impl History {
    pub fn new(store: HistoryStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Open the default store in Application Support.
    pub fn open_default(retention: HistoryRetention) -> color_eyre::Result<Self> {
        let path = crate::app_paths::support_dir()?.join("history.json");
        Ok(Self::new(HistoryStore::open(path, retention, now_ms())))
    }

    pub fn record(&self, draft: HistoryDraft) -> io::Result<Option<u64>> {
        self.locked().record(draft, now_ms())
    }

    pub fn delete(&self, id: u64) -> io::Result<bool> {
        self.locked().delete(id)
    }

    pub fn clear(&self) -> io::Result<()> {
        self.locked().clear()
    }

    pub fn set_retention(&self, retention: HistoryRetention) -> io::Result<()> {
        self.locked().set_retention(retention, now_ms())
    }

    /// Bounded snapshot of matching entries, newest first.
    pub fn search(&self, query: &str) -> Vec<HistoryEntry> {
        self.locked().search(query).into_iter().cloned().collect()
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, HistoryStore> {
        self.store.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn truncated(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(unix)]
fn restrict_to_owner(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_owner(_file: &fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-history-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory.join("history.json")
    }

    fn draft(text: &str) -> HistoryDraft {
        HistoryDraft {
            kind: HistoryKind::Dictation,
            raw_text: format!("raw {text}"),
            final_text: text.to_string(),
            application: Some("Zed".into()),
            processing: Some(HistoryProcessing {
                profile: "Messages".into(),
                latency_ms: 120,
                fallback: None,
            }),
            audio_ms: 900,
            inference_ms: 80,
            total_ms: 1_100,
        }
    }

    #[test]
    fn records_survive_reopen_with_monotonic_ids() {
        let path = temp_path("reopen");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 1_000);
        let first = store.record(draft("first"), 1_000).unwrap().unwrap();
        let second = store.record(draft("second"), 2_000).unwrap().unwrap();
        assert!(second > first);
        store.delete(second).unwrap();

        let mut reopened = HistoryStore::open(path, HistoryRetention::Week, 2_500);
        assert_eq!(reopened.len(), 1);
        let third = reopened.record(draft("third"), 3_000).unwrap().unwrap();
        assert!(third > second, "deleted IDs are never reused");
        let texts: Vec<_> = reopened
            .entries()
            .map(|entry| entry.final_text.as_str())
            .collect();
        assert_eq!(texts, ["third", "first"]);
    }

    #[test]
    fn retention_prunes_expired_entries_on_open_and_write() {
        let path = temp_path("retention");
        let day_ms = 24 * 60 * 60 * 1_000;
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        store.record(draft("old"), 1_000).unwrap();
        store.record(draft("fresh"), day_ms).unwrap();

        // A write one day later prunes the first entry exactly at the cutoff.
        store.record(draft("newest"), day_ms + 1_001).unwrap();
        let texts: Vec<_> = store
            .entries()
            .map(|entry| entry.final_text.as_str())
            .collect();
        assert_eq!(texts, ["newest", "fresh"]);

        let reopened = HistoryStore::open(path, HistoryRetention::Day, 2 * day_ms + 500);
        let texts: Vec<_> = reopened
            .entries()
            .map(|entry| entry.final_text.as_str())
            .collect();
        assert_eq!(texts, ["newest"]);
    }

    #[test]
    fn off_records_nothing_but_preserves_existing_entries() {
        let path = temp_path("off");
        let mut store = HistoryStore::open(path, HistoryRetention::Week, 1_000);
        store.record(draft("kept"), 1_000).unwrap();
        store.set_retention(HistoryRetention::Off, 1_500).unwrap();

        assert_eq!(store.record(draft("dropped"), 2_000).unwrap(), None);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn hard_caps_bound_entries_and_text_regardless_of_retention() {
        let path = temp_path("caps");
        let mut store = HistoryStore::open(path, HistoryRetention::Forever, 0);
        for index in 0..(MAX_ENTRIES + 5) {
            store
                .record(draft(&format!("entry {index}")), index as u64)
                .unwrap();
        }
        assert_eq!(store.len(), MAX_ENTRIES);

        let oversized = "x".repeat(MAX_TEXT_BYTES + 100);
        let id = store.record(draft(&oversized), 9_999_999).unwrap().unwrap();
        assert_eq!(store.entry(id).unwrap().final_text.len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn total_text_bytes_evict_oldest_entries() {
        let path = temp_path("total-bytes");
        let mut store = HistoryStore::open(path, HistoryRetention::Forever, 0);
        let big = "y".repeat(MAX_TEXT_BYTES - 10);
        let per_entry = 2 * big.len() + 8;
        let fits = MAX_TOTAL_TEXT_BYTES / per_entry;
        for index in 0..(fits + 3) {
            store.record(draft(&big), index as u64).unwrap();
        }
        let total: usize = store
            .entries()
            .map(|entry| entry.raw_text.len() + entry.final_text.len())
            .sum();
        assert!(total <= MAX_TOTAL_TEXT_BYTES);
        assert!(store.len() < fits + 3);
    }

    #[test]
    fn search_is_case_insensitive_across_text_application_and_profile() {
        let path = temp_path("search");
        let mut store = HistoryStore::open(path, HistoryRetention::Week, 0);
        store.record(draft("Hello World"), 1_000).unwrap();
        store.record(draft("other text"), 2_000).unwrap();

        assert_eq!(store.search("hello world").len(), 1);
        assert_eq!(store.search("zed").len(), 2);
        assert_eq!(store.search("messages").len(), 2);
        assert_eq!(store.search("absent").len(), 0);
        assert_eq!(store.search("  ").len(), 2, "blank queries list everything");
    }

    #[test]
    fn clear_removes_everything_durably() {
        let path = temp_path("clear");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        store.record(draft("one"), 1_000).unwrap();
        store.record(draft("two"), 2_000).unwrap();
        store.clear().unwrap();

        assert_eq!(store.len(), 0);
        let reopened = HistoryStore::open(path, HistoryRetention::Week, 3_000);
        assert_eq!(reopened.len(), 0);
    }

    #[test]
    fn malformed_files_are_preserved_and_recovered_from() {
        let path = temp_path("malformed");
        fs::write(&path, b"{ not json").unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        assert_eq!(store.len(), 0);
        assert!(path.with_extension("json.corrupt").exists());

        store.record(draft("fresh"), 1_000).unwrap();
        let reopened = HistoryStore::open(path, HistoryRetention::Week, 2_000);
        assert_eq!(reopened.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn history_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("permissions");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        store.record(draft("private"), 1_000).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let text = "é".repeat(MAX_TEXT_BYTES);
        let truncated = truncated(&text, MAX_TEXT_BYTES);
        assert!(truncated.len() <= MAX_TEXT_BYTES);
        assert!(text.starts_with(&truncated));
    }
}
