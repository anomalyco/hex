//! Retained dictation history: an owner-only, bounded, crash-safe store of
//! successful dictation results.
//!
//! History is a product record, deliberately separate from the diagnostic
//! Activity event stream. Entries hold text and bounded metadata; audio is
//! kept only while Labs › Keep recordings is on ([`recordings`]), as a file
//! per entry that follows the entry's retention plus its own byte cap. Every
//! retention choice remains subject to hard entry and byte caps, writes are
//! atomic, and files are owner-only.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, TryLockError, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

pub mod captures;
pub mod playback;
pub mod recordings;
pub mod stats;
pub mod versions;

pub use recordings::Recording;
pub use versions::{HistoryVersion, Transcript};

const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 10_000;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_LABEL_BYTES: usize = 256;
const MAX_FALLBACK_BYTES: usize = 1_024;
const MAX_TOTAL_TEXT_BYTES: usize = 16 * 1024 * 1024;
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
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

    pub const fn max_age_ms(self) -> Option<u64> {
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
    /// `hex dictation stop --print`: never pasted, so Paste Last skips it.
    Printed,
    /// Dictate without processing: pasted as heard, with no step run.
    Raw,
}

impl HistoryKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dictation => "Dictation",
            Self::Send => "Send",
            Self::VoiceAction => "Voice Action",
            Self::Printed => "Printed",
            Self::Raw => "Raw",
        }
    }
}

/// Bounded record of the post-processing that shaped the final text.
pub type HistoryProcessing = crate::desktop::events::DictationProcessing;

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
    /// The size of this entry's kept audio (Labs › Keep recordings); 0
    /// when it has none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub recording_bytes: u64,
    /// The speech model that made the text, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The language chosen for the run that made the text; `None` when the
    /// model or the saved setting chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// When the text was made, if not when the entry was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_ms: Option<u64>,
    /// Earlier transcripts, newest first ([`versions`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<HistoryVersion>,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl HistoryEntry {
    /// Whether this entry kept its audio.
    pub fn has_recording(&self) -> bool {
        self.recording_bytes > 0
    }

    fn text_bytes(&self) -> usize {
        self.raw_text.len()
            + self.final_text.len()
            + self
                .versions
                .iter()
                .map(HistoryVersion::text_bytes)
                .sum::<usize>()
    }

    /// Whether any searched field contains `needle` (already lowercased).
    /// `scratch` is reused across a whole search so plain-ASCII fields,
    /// almost all of them, are lowercased without a new allocation.
    fn matches(&self, needle: &str, scratch: &mut String) -> bool {
        let mut matches_field = |field: &str| field_contains(field, needle, scratch);
        (self.raw_text != self.final_text && matches_field(&self.raw_text))
            || matches_field(&self.final_text)
            || self.application.as_deref().is_some_and(&mut matches_field)
            || self.model.as_deref().is_some_and(&mut matches_field)
            || self
                .processing
                .as_ref()
                .is_some_and(|processing| matches_field(&processing.profile))
    }
}

/// `field.to_lowercase().contains(needle)`, lowercasing an ASCII field
/// into `scratch` instead of a new string.
fn field_contains(field: &str, needle: &str, scratch: &mut String) -> bool {
    if field.is_ascii() {
        if field.len() < needle.len() {
            return false;
        }
        scratch.clear();
        scratch.extend(field.chars().map(|c| c.to_ascii_lowercase()));
        scratch.contains(needle)
    } else {
        field.to_lowercase().contains(needle)
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
    /// The dictation's audio, while Labs › Keep recordings is on.
    pub recording: Option<Recording>,
    /// The speech model that transcribed it ([`dictation_model`]).
    pub model: Option<String>,
}

/// The name of the speech model dictation runs: the one the engine holds,
/// else the saved one (Apple Speech, which the engine never holds).
pub fn dictation_model() -> Option<String> {
    let selection = crate::transcription::engine::TranscriptionEngine::shared()
        .and_then(|engine| engine.held_selection())
        .unwrap_or_else(|| crate::app_settings::transcription_selection().1);
    Some(
        crate::transcription::models::definition(selection.model)
            .name
            .to_string(),
    )
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

/// Read one atomic file snapshot without recovery, pruning writes, or workers.
/// Opening once pins the old or new inode while the app replaces the file.
pub fn read_snapshot(path: &std::path::Path) -> io::Result<Vec<HistoryEntry>> {
    use std::io::Read;
    // Text can expand sixfold when JSON-escaped, plus bounded metadata.
    const MAX_FILE_BYTES: u64 = (MAX_TOTAL_TEXT_BYTES * 6
        + MAX_ENTRIES * (6 * (2 * MAX_LABEL_BYTES + MAX_FALLBACK_BYTES) + 1_024))
        as u64;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::other("history file exceeds the read limit"));
    }
    let loaded: LoadedHistory = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if loaded.version != VERSION {
        return Err(io::Error::other("unsupported history version"));
    }
    Ok(loaded.entries)
}

/// Owner-only bounded history store. Entries are held oldest-first.
pub struct HistoryStore {
    path: PathBuf,
    /// Labs › Keep recordings' audio, one file per entry.
    recordings: PathBuf,
    recording_cap: u64,
    entries: Vec<HistoryEntry>,
    next_id: u64,
    retention: HistoryRetention,
    prune_pending: bool,
    prune_error_reported: bool,
    /// The file as this store last read or wrote it. Linux runs the
    /// listener and the History pane in separate processes, so each store
    /// rereads the file when another process changed it.
    file_state: Option<(SystemTime, u64)>,
    /// Changes whenever what a search could return may have changed: a
    /// record, delete, edit, clear, prune, retention change, or a reread of
    /// another process's write. The History pane searches again only then.
    revision: u64,
}

fn file_state(path: &std::path::Path) -> Option<(SystemTime, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

impl HistoryStore {
    /// Open the store, recover from unreadable content, and prune expired
    /// entries. A malformed file is preserved beside the store instead of
    /// being silently destroyed.
    pub fn open(path: PathBuf, retention: HistoryRetention, now_ms: u64) -> Self {
        let mut store = Self {
            recordings: recordings::directory(&path),
            recording_cap: recordings::MAX_RECORDING_BYTES,
            path,
            entries: Vec::new(),
            next_id: 1,
            retention,
            prune_pending: false,
            prune_error_reported: false,
            file_state: None,
            revision: 0,
        };
        store.file_state = file_state(&store.path);
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
        store.expire(now_ms);
        store
    }

    /// Rereads the file if another process wrote it since this store last
    /// read or wrote it. An unreadable file keeps the entries in memory.
    pub fn sync(&mut self) {
        let current = file_state(&self.path);
        if current == self.file_state {
            return;
        }
        self.file_state = current;
        let loaded = match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice::<LoadedHistory>(&bytes)
                .ok()
                .filter(|loaded| loaded.version == VERSION),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(LoadedHistory {
                version: VERSION,
                next_id: self.next_id,
                entries: Vec::new(),
            }),
            Err(_) => None,
        };
        let Some(loaded) = loaded else {
            tracing::warn!(path = %self.path.display(), "could not reread the changed history");
            return;
        };
        let max_entry_id = loaded.entries.iter().map(|entry| entry.id).max();
        self.next_id = self
            .next_id
            .max(loaded.next_id)
            .max(max_entry_id.map_or(1, |id| id.saturating_add(1)));
        self.entries = loaded.entries;
        self.entries.sort_by_key(|entry| entry.id);
        self.touch();
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The store's revision, which moves whenever a search could answer
    /// differently. An oldest
    /// entry past retention is pruned in memory first, so the revision also
    /// moves when time expires one; the idle worker saves that prune.
    pub fn revision(&mut self, now_ms: u64) -> u64 {
        if let Some(max_age) = self.retention.max_age_ms()
            && self
                .entries
                .first()
                .is_some_and(|entry| entry.timestamp_ms < now_ms.saturating_sub(max_age))
        {
            self.prune(now_ms);
        }
        self.revision
    }

    /// Record one successful result. Returns the stable entry ID, or `None`
    /// while retention is off.
    pub fn record(&mut self, draft: HistoryDraft, now_ms: u64) -> io::Result<Option<u64>> {
        if self.retention.is_off() {
            return Ok(None);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let recording_bytes = draft.recording.as_ref().map_or(0, |recording| {
            recordings::write(&self.recordings, id, recording).unwrap_or_else(|error| {
                tracing::warn!(%error, "could not keep the dictation's recording");
                0
            })
        });
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
            recording_bytes,
            model: draft.model.map(|model| truncated(&model, MAX_LABEL_BYTES)),
            language: None,
            text_ms: None,
            versions: Vec::new(),
        });
        self.prune(now_ms);
        self.persist()?;
        Ok(Some(id))
    }

    /// Delete one entry. Returns whether it existed.
    /// A failed save keeps the entry, so the deletion can be retried.
    pub fn delete(&mut self, id: u64) -> io::Result<bool> {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return Ok(false);
        };
        let removed = self.entries.remove(index);
        if let Err(error) = self.persist() {
            self.entries.insert(index, removed);
            return Err(error);
        }
        Ok(true)
    }

    /// Delete every entry in `ids` with one save. Returns how many existed;
    /// unknown ids are ignored. A failed save restores every removed entry
    /// where it was, so the deletion can be retried.
    pub fn delete_many(&mut self, ids: &[u64]) -> io::Result<usize> {
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(self.entries.len());
        for (index, entry) in std::mem::take(&mut self.entries).into_iter().enumerate() {
            if ids.contains(&entry.id) {
                removed.push((index, entry));
            } else {
                kept.push(entry);
            }
        }
        self.entries = kept;
        let count = removed.len();
        if count == 0 {
            return Ok(0);
        }
        if let Err(error) = self.persist() {
            for (index, entry) in removed {
                let at = index.min(self.entries.len());
                self.entries.insert(at, entry);
            }
            return Err(error);
        }
        Ok(count)
    }

    /// Replace entry `id`'s text with `transcript`, such as a transcription
    /// of its recording with another model, keeping the current text as its
    /// newest version. Returns whether it existed.
    pub fn replace_text(
        &mut self,
        id: u64,
        transcript: Transcript,
        now_ms: u64,
    ) -> io::Result<bool> {
        self.change(id, |entry| {
            entry.replace(transcript, now_ms);
            true
        })
    }

    /// Bring entry `id`'s version `index` back as its text. Returns whether
    /// there was one.
    pub fn restore_version(&mut self, id: u64, index: usize) -> io::Result<bool> {
        self.change(id, |entry| entry.restore(index))
    }

    /// Changes entry `id` and saves; a failed save keeps it as it was.
    fn change(
        &mut self,
        id: u64,
        change: impl FnOnce(&mut HistoryEntry) -> bool,
    ) -> io::Result<bool> {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return Ok(false);
        };
        let previous = self.entries[index].clone();
        if !change(&mut self.entries[index]) {
            return Ok(false);
        }
        if let Err(error) = self.persist() {
            self.entries[index] = previous;
            return Err(error);
        }
        Ok(true)
    }

    /// Where entry `id`'s recording is, if it kept one.
    pub fn recording_path(&self, id: u64) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|entry| entry.id == id && entry.has_recording())
            .map(|_| recordings::path(&self.recordings, id))
    }

    /// Delete every entry. A failed save keeps them, so the clear can be
    /// retried.
    pub fn clear(&mut self) -> io::Result<()> {
        if self.entries.is_empty() && !self.prune_pending {
            return Ok(());
        }
        let entries = std::mem::take(&mut self.entries);
        if let Err(error) = self.persist() {
            self.entries = entries;
            return Err(error);
        }
        Ok(())
    }

    /// Change retention and immediately prune to the new window. Turning
    /// history off stops recording but keeps existing entries until they are
    /// deleted explicitly.
    pub fn set_retention(&mut self, retention: HistoryRetention, now_ms: u64) -> io::Result<()> {
        if self.retention == retention {
            return Ok(());
        }
        self.retention = retention;
        self.touch();
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
    pub fn search(&self, query: &str, now_ms: u64) -> Vec<&HistoryEntry> {
        let needle = query.trim().to_lowercase();
        let cutoff = self
            .retention
            .max_age_ms()
            .map(|max_age| now_ms.saturating_sub(max_age));
        let mut scratch = String::new();
        self.entries()
            .filter(|entry| cutoff.is_none_or(|cutoff| entry.timestamp_ms >= cutoff))
            .filter(|entry| needle.is_empty() || entry.matches(&needle, &mut scratch))
            .take(MAX_SEARCH_RESULTS)
            .collect()
    }

    /// The newest `count` entries within retention that a paste could
    /// reuse: never a printed one or one with no text, as Paste Last.
    /// Nothing while retention is Off.
    pub fn recent(&self, count: usize, now_ms: u64) -> Vec<&HistoryEntry> {
        if self.retention.is_off() {
            return Vec::new();
        }
        let cutoff = self
            .retention
            .max_age_ms()
            .map(|max_age| now_ms.saturating_sub(max_age));
        self.entries()
            .filter(|entry| cutoff.is_none_or(|cutoff| entry.timestamp_ms >= cutoff))
            .filter(|entry| entry.kind != HistoryKind::Printed)
            .filter(|entry| !entry.final_text.trim().is_empty())
            .take(count)
            .collect()
    }

    /// The idle worker's expiry: reread whatever another process wrote first,
    /// so pruning never writes a stale copy over its records or deletions.
    fn expire_shared(&mut self, now_ms: u64) {
        self.sync();
        self.expire(now_ms);
    }

    fn expire(&mut self, now_ms: u64) {
        self.prune(now_ms);
        if self.prune_pending
            && let Err(error) = self.persist()
        {
            if !self.prune_error_reported {
                tracing::warn!(%error, "could not persist pruned history; will retry");
            }
            self.prune_error_reported = true;
        }
    }

    fn prune(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        if let Some(max_age) = self.retention.max_age_ms() {
            let cutoff = now_ms.saturating_sub(max_age);
            let before = self.entries.len();
            self.entries.retain(|entry| entry.timestamp_ms >= cutoff);
            changed |= self.entries.len() != before;
        }
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
            changed = true;
        }
        let mut total: usize = self.entries.iter().map(HistoryEntry::text_bytes).sum();
        let mut remove = 0;
        while total > MAX_TOTAL_TEXT_BYTES && remove + 1 < self.entries.len() {
            total -= self.entries[remove].text_bytes();
            remove += 1;
        }
        self.entries.drain(..remove);
        changed |= remove > 0;
        // Past the recordings' own cap the oldest entries keep their text.
        let mut recorded: u64 = self.entries.iter().map(|entry| entry.recording_bytes).sum();
        for entry in &mut self.entries {
            if recorded <= self.recording_cap {
                break;
            }
            if entry.has_recording() {
                recorded -= entry.recording_bytes;
                entry.recording_bytes = 0;
                changed = true;
            }
        }
        self.prune_pending |= changed;
        if changed {
            self.touch();
        }
        changed
    }

    /// Saves the entries. Every change to them goes through here, so it
    /// moves the revision whether or not the save works (a failed save may
    /// keep the change, as a record does, or roll it back).
    fn persist(&mut self) -> io::Result<()> {
        self.touch();
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
        fs::rename(&temporary, &self.path)?;
        self.file_state = file_state(&self.path);
        self.prune_pending = false;
        self.prune_error_reported = false;
        // Whatever left History, or lost its audio, loses its file.
        let entries = &self.entries;
        recordings::sweep(&self.recordings, self.next_id, |id| {
            entries
                .iter()
                .any(|entry| entry.id == id && entry.has_recording())
        });
        Ok(())
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
    // Only handles own senders: dropping the last clone wakes and stops the worker.
    _shutdown: mpsc::Sender<()>,
}

impl History {
    pub fn new(store: HistoryStore) -> Self {
        Self::with_clock(store, PRUNE_INTERVAL, now_ms)
    }

    fn with_clock(
        store: HistoryStore,
        interval: Duration,
        clock: impl Fn() -> u64 + Send + 'static,
    ) -> Self {
        let store = Arc::new(Mutex::new(store));
        let (shutdown, stopped) = mpsc::channel();
        let worker_store = Arc::clone(&store);
        if let Err(error) = thread::Builder::new()
            .name("history-retention".into())
            .spawn(move || {
                while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                    let mut store = match worker_store.try_lock() {
                        Ok(store) => store,
                        Err(TryLockError::WouldBlock) => continue,
                        Err(TryLockError::Poisoned(error)) => error.into_inner(),
                    };
                    store.expire_shared(clock());
                }
            })
        {
            tracing::warn!(%error, "could not start history retention worker");
        }
        Self {
            store,
            _shutdown: shutdown,
        }
    }

    /// Open the default store in Application Support.
    pub fn open_default(retention: HistoryRetention) -> color_eyre::Result<Self> {
        let path = crate::app_paths::support_dir()?.join("history.json");
        Ok(Self::new(HistoryStore::open(path, retention, now_ms())))
    }

    pub fn record(&self, draft: HistoryDraft) -> io::Result<Option<u64>> {
        self.synced().record(draft, now_ms())
    }

    pub fn delete(&self, id: u64) -> io::Result<bool> {
        self.synced().delete(id)
    }

    /// Delete every entry in `ids` with one save ([`HistoryStore::delete_many`]).
    pub fn delete_many(&self, ids: &[u64]) -> io::Result<usize> {
        self.synced().delete_many(ids)
    }

    pub fn clear(&self) -> io::Result<()> {
        self.synced().clear()
    }

    pub fn replace_text(&self, id: u64, transcript: Transcript) -> io::Result<bool> {
        self.synced().replace_text(id, transcript, now_ms())
    }

    pub fn restore_version(&self, id: u64, index: usize) -> io::Result<bool> {
        self.synced().restore_version(id, index)
    }

    pub fn recording_path(&self, id: u64) -> Option<PathBuf> {
        self.synced().recording_path(id)
    }

    pub fn set_retention(&self, retention: HistoryRetention) -> io::Result<()> {
        self.synced().set_retention(retention, now_ms())
    }

    /// Bounded snapshot of matching entries, newest first.
    #[allow(dead_code)]
    pub fn search(&self, query: &str) -> Vec<HistoryEntry> {
        self.search_at(query).1
    }

    /// The store's revision ([`HistoryStore::revision`]), after picking up
    /// another process's write.
    pub fn revision(&self) -> u64 {
        self.synced().revision(now_ms())
    }

    /// [`History::search`] with the revision it was read at.
    pub fn search_at(&self, query: &str) -> (u64, Vec<HistoryEntry>) {
        let now = now_ms();
        let mut store = self.synced();
        let revision = store.revision(now);
        let entries = store.search(query, now).into_iter().cloned().collect();
        (revision, entries)
    }

    /// The newest `count` reusable entries, newest first
    /// ([`HistoryStore::recent`]).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn recent(&self, count: usize) -> Vec<HistoryEntry> {
        self.synced()
            .recent(count, now_ms())
            .into_iter()
            .cloned()
            .collect()
    }

    /// The text entry `id` inserted, if it is still retained.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn final_text(&self, id: u64) -> Option<String> {
        self.synced()
            .entries()
            .find(|entry| entry.id == id)
            .map(|entry| entry.final_text.clone())
    }

    fn synced(&self) -> std::sync::MutexGuard<'_, HistoryStore> {
        let mut store = self.store.lock().unwrap_or_else(|error| error.into_inner());
        store.sync();
        store
    }
}

pub use crate::desktop::events::now_ms;

fn truncated(text: &str, max_bytes: usize) -> String {
    text[..text.floor_char_boundary(max_bytes)].to_string()
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
    use std::time::UNIX_EPOCH;

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
            recording: None,
            model: None,
        }
    }

    fn recorded(text: &str) -> HistoryDraft {
        HistoryDraft {
            recording: Some(Recording::from_samples(vec![0.25; 1_600])),
            ..draft(text)
        }
    }

    fn recording_ids(path: &std::path::Path) -> Vec<u64> {
        let mut ids: Vec<u64> = fs::read_dir(recordings::directory(path))
            .map(|files| {
                files
                    .flatten()
                    .filter_map(|file| {
                        file.file_name()
                            .to_str()?
                            .strip_suffix(".wav")?
                            .parse()
                            .ok()
                    })
                    .collect()
            })
            .unwrap_or_default();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn field_matching_agrees_with_full_lowercasing() {
        let fields = [
            "",
            "Hello World",
            "HELLO",
            "Zed",
            "Straße",
            "İstanbul",
            "ÉCOLE normale",
            "mixed ASCII then Ω",
        ];
        let queries = [
            "", "hello", "world", "o w", "ss", "straße", "i̇", "école", "ω", "zed!",
        ];
        let mut scratch = String::new();
        for field in fields {
            for query in queries {
                let needle = query.to_lowercase();
                assert_eq!(
                    field_contains(field, &needle, &mut scratch),
                    field.to_lowercase().contains(&needle),
                    "{field:?} / {query:?}"
                );
            }
        }
    }

    #[test]
    fn search_still_finds_raw_text_that_differs_from_the_final_text() {
        let path = temp_path("search-raw");
        let mut store = HistoryStore::open(path, HistoryRetention::Week, 0);
        let mut same = draft("Same Words");
        same.raw_text = same.final_text.clone();
        let same = store.record(same, 1_000).unwrap().unwrap();
        let rewritten = store.record(draft("Rewritten"), 2_000).unwrap().unwrap();
        let ids = |query: &str| -> Vec<u64> {
            store
                .search(query, 3_000)
                .iter()
                .map(|entry| entry.id)
                .collect()
        };
        assert_eq!(ids("same words"), [same]);
        assert_eq!(ids("RAW rewritten"), [rewritten]);
        assert_eq!(ids("no such phrase"), Vec::<u64>::new());
    }

    #[test]
    fn a_kept_recording_is_an_owner_only_wav_that_reads_back() {
        let path = temp_path("recording");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let plain = store.record(draft("plain"), 1_000).unwrap().unwrap();
        let id = store.record(recorded("kept"), 2_000).unwrap().unwrap();
        assert!(!store.entry(plain).unwrap().has_recording());
        assert!(store.recording_path(plain).is_none());
        assert!(store.entry(id).unwrap().has_recording());
        assert_eq!(recording_ids(&path), [id]);

        let file = store.recording_path(id).unwrap();
        let clip = crate::transcription::service::AudioClip::decode_wav(std::io::BufReader::new(
            fs::File::open(&file).unwrap(),
        ))
        .unwrap();
        assert_eq!(clip.duration_ms, 100);
        let reopened = HistoryStore::open(path, HistoryRetention::Week, 3_000);
        assert!(reopened.entry(id).unwrap().has_recording());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode(&file) & 0o777, 0o600);
            assert_eq!(mode(file.parent().unwrap()) & 0o777, 0o700);
        }
    }

    #[test]
    fn recordings_leave_with_their_entries() {
        let path = temp_path("recording-retention");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        let old = store.record(recorded("old"), 1_000).unwrap().unwrap();
        let deleted = store.record(recorded("deleted"), day_ms).unwrap().unwrap();
        let kept = store.record(recorded("kept"), day_ms).unwrap().unwrap();
        assert_eq!(recording_ids(&path), [old, deleted, kept]);

        // Retention takes the first; Delete the second.
        store.expire(day_ms + 1_001);
        assert_eq!(recording_ids(&path), [deleted, kept]);
        store.delete(deleted).unwrap();
        assert_eq!(recording_ids(&path), [kept]);

        // The entry cap takes the oldest entry's audio with it.
        store.entries = (0..MAX_ENTRIES + 1)
            .map(|offset| HistoryEntry {
                id: kept + offset as u64,
                ..store.entries[0].clone()
            })
            .collect();
        store.next_id = kept + MAX_ENTRIES as u64 + 1;
        store.prune(day_ms + 1_002);
        store.persist().unwrap();
        assert!(recording_ids(&path).is_empty());

        let path = temp_path("recording-clear");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        store.record(recorded("one"), 1_000).unwrap();
        store.record(recorded("two"), 2_000).unwrap();
        assert_eq!(recording_ids(&path).len(), 2);
        store.clear().unwrap();
        assert!(
            recording_ids(&path).is_empty(),
            "Clear all deletes the audio"
        );
    }

    #[test]
    fn the_recordings_byte_cap_drops_the_oldest_audio_but_keeps_the_text() {
        let path = temp_path("recording-cap");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Forever, 0);
        let first = store.record(recorded("first"), 1_000).unwrap().unwrap();
        let size = store.entry(first).unwrap().recording_bytes;
        store.recording_cap = 2 * size;
        let second = store.record(recorded("second"), 2_000).unwrap().unwrap();
        let third = store.record(recorded("third"), 3_000).unwrap().unwrap();

        assert_eq!(store.len(), 3, "every entry keeps its text");
        assert!(!store.entry(first).unwrap().has_recording());
        assert!(store.entry(second).unwrap().has_recording());
        assert!(store.entry(third).unwrap().has_recording());
        assert_eq!(recording_ids(&path), [second, third]);
    }

    #[test]
    fn a_recording_another_process_has_not_saved_yet_survives_a_sweep() {
        let path = temp_path("recording-race");
        let mut window = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let mut listener = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let early = listener.record(recorded("early"), 1_000).unwrap().unwrap();
        window.sync();
        // The listener's next recording is on disk before its entry is.
        recordings::write(
            &recordings::directory(&path),
            early + 1,
            &Recording::from_samples(vec![0.1; 160]),
        )
        .unwrap();
        window.delete(early).unwrap();
        assert_eq!(recording_ids(&path), [early + 1]);
    }

    #[test]
    fn replacing_the_text_keeps_the_entry_and_its_recording() {
        let path = temp_path("replace");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let id = store.record(recorded("befor"), 1_000).unwrap().unwrap();
        let fixed = |text: &str| Transcript {
            text: text.into(),
            model: None,
            language: None,
        };
        assert!(store.replace_text(id, fixed("before"), 1_500).unwrap());
        assert!(!store.replace_text(id + 1, fixed("nothing"), 1_500).unwrap());

        let reopened = HistoryStore::open(path.clone(), HistoryRetention::Week, 2_000);
        let entry = reopened.entry(id).unwrap();
        assert_eq!(entry.final_text, "before");
        assert_eq!(entry.raw_text, "before");
        assert!(entry.processing.is_none());
        assert!(entry.has_recording());
        assert_eq!(recording_ids(&path), [id]);
    }

    #[test]
    fn retranscribing_a_missing_recording_says_so() {
        let path = temp_path("retranscribe-missing");
        let missing = recordings::path(&recordings::directory(&path), 7);
        let error = recordings::retranscribe(
            &missing,
            crate::transcription::models::TranscriptionSelection::default(),
        )
        .unwrap_err();
        assert_eq!(error, "The recording is gone.");
        fs::create_dir_all(missing.parent().unwrap()).unwrap();
        fs::write(&missing, b"not a wav").unwrap();
        let error = recordings::retranscribe(
            &missing,
            crate::transcription::models::TranscriptionSelection::default(),
        )
        .unwrap_err();
        assert_eq!(error, "The recording could not be read.");
    }

    #[test]
    fn stores_in_two_processes_see_each_others_writes() {
        let path = temp_path("two-processes");
        let mut listener = HistoryStore::open(path.clone(), HistoryRetention::Week, 1_000);
        let mut window = HistoryStore::open(path.clone(), HistoryRetention::Week, 1_000);
        let first = listener.record(draft("first"), 1_000).unwrap().unwrap();

        window.sync();
        assert!(window.entry(first).is_some());
        window.delete(first).unwrap();

        listener.sync();
        let second = listener.record(draft("second"), 2_000).unwrap().unwrap();
        assert!(second > first);
        window.sync();
        assert!(window.entry(first).is_none(), "a deletion is never undone");
        assert!(window.entry(second).is_some());
    }

    #[test]
    fn expiry_in_a_stale_process_keeps_the_other_processes_writes() {
        let path = temp_path("stale-expiry");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut listener = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        listener.record(draft("old"), 1_000).unwrap();
        listener.record(draft("kept"), 2_000).unwrap();
        let mut window = HistoryStore::open(path.clone(), HistoryRetention::Day, 2_000);
        let newer = listener.record(draft("newer"), 3_000).unwrap().unwrap();

        window.expire_shared(day_ms + 1_001);
        let saved = read_snapshot(&path).unwrap();
        let texts: Vec<_> = saved
            .iter()
            .map(|entry| entry.final_text.as_str())
            .collect();
        assert_eq!(texts, ["kept", "newer"]);
        assert!(window.entry(newer).is_some());

        window.clear().unwrap();
        listener.expire_shared(day_ms + 2_001);
        assert!(
            read_snapshot(&path).unwrap().is_empty(),
            "a clear stays cleared"
        );
    }

    #[test]
    fn the_revision_moves_only_when_a_search_could_change() {
        let path = temp_path("revision");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        let moved = |store: &mut HistoryStore, now: u64, last: &mut u64| {
            let revision = store.revision(now);
            let changed = revision != *last;
            *last = revision;
            changed
        };
        let mut last = store.revision(0);
        assert!(!moved(&mut store, 0, &mut last), "nothing changed");

        let old = store.record(draft("old"), 1_000).unwrap().unwrap();
        assert!(moved(&mut store, 1_000, &mut last), "a record");
        store.record(draft("fresh"), 2_000).unwrap();
        assert!(moved(&mut store, 2_000, &mut last));
        assert!(!moved(&mut store, day_ms + 1_000, &mut last));

        assert!(
            moved(&mut store, day_ms + 1_001, &mut last),
            "time expired the oldest"
        );
        assert!(store.entry(old).is_none());

        let mut other = HistoryStore::open(path, HistoryRetention::Day, day_ms + 1_001);
        other.record(draft("elsewhere"), day_ms + 1_002).unwrap();
        store.sync();
        assert!(
            moved(&mut store, day_ms + 1_002, &mut last),
            "another process"
        );

        store
            .set_retention(HistoryRetention::Forever, day_ms + 1_002)
            .unwrap();
        assert!(moved(&mut store, day_ms + 1_002, &mut last), "retention");
        let id = store.entries().next().unwrap().id;
        store.delete(id).unwrap();
        assert!(moved(&mut store, day_ms + 1_002, &mut last), "a delete");
        assert!(!moved(&mut store, day_ms + 1_002, &mut last));
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
    fn search_hides_expired_entries_before_the_next_idle_prune() {
        let path = temp_path("search-expiry");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        store.record(draft("old"), 1_000).unwrap();
        store.record(draft("fresh"), 2_000).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH)
            .unwrap();

        assert_eq!(store.search("", day_ms + 1_000).len(), 2);
        assert!(store.search("old", day_ms + 1_001).is_empty());
        let matches = store.search("", day_ms + 1_001);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].final_text, "fresh");
        assert_eq!(
            store.len(),
            2,
            "reads filter without writing on the UI thread"
        );
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), UNIX_EPOCH);
    }

    #[test]
    fn idle_worker_expires_entries_on_disk_without_recording_or_reading() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let path = temp_path("idle-expiry");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        store.record(draft("old"), 1_000).unwrap();
        store.record(draft("fresh"), 2_000).unwrap();
        let clock = Arc::new(AtomicU64::new(2_000));
        let worker_clock = Arc::clone(&clock);
        let (ticks, ticked) = mpsc::channel();
        let history = History::with_clock(store, Duration::from_millis(10), move || {
            let now = worker_clock.load(Ordering::SeqCst);
            let _ = ticks.send(now);
            now
        });
        let tick = |now| {
            clock.store(now, Ordering::SeqCst);
            while ticked.recv_timeout(Duration::from_secs(2)).unwrap() != now {}
            // The clock is sampled under the lock; acquiring it waits for that prune.
            drop(history.synced());
        };
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH)
            .unwrap();

        tick(day_ms + 1_000);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), UNIX_EPOCH);

        tick(day_ms + 1_001);
        let saved: LoadedHistory = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.entries.len(), 1);
        assert_eq!(saved.entries[0].final_text, "fresh");

        tick(day_ms + 2_001);
        let saved: LoadedHistory = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.entries.is_empty());
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH)
            .unwrap();
        tick(2 * day_ms);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), UNIX_EPOCH);
    }

    #[test]
    fn failed_expiry_persistence_retries_without_new_removals() {
        let path = temp_path("expiry-retry");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Day, 0);
        store.record(draft("old"), 1_000).unwrap();
        let temporary = path.with_extension("json.tmp");
        fs::create_dir(&temporary).unwrap();

        store.expire(day_ms + 1_001);
        assert!(store.entries.is_empty());
        assert!(store.prune_pending);
        assert!(store.prune_error_reported);
        let saved: LoadedHistory = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.entries.len(), 1);
        store.expire(day_ms + 1_002);
        assert!(store.prune_pending);
        assert!(store.prune_error_reported);

        fs::remove_dir(temporary).unwrap();
        store.expire(day_ms + 1_003);
        assert!(!store.prune_pending);
        assert!(!store.prune_error_reported);
        let saved: LoadedHistory = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.entries.is_empty());
    }

    #[test]
    fn idle_worker_stops_when_the_last_history_handle_is_dropped() {
        let path = temp_path("worker-lifecycle");
        let store = HistoryStore::open(path, HistoryRetention::Day, 0);
        let (alive, stopped) = mpsc::channel::<()>();
        let history = History::with_clock(store, Duration::from_secs(24 * 60 * 60), move || {
            let _alive = &alive;
            0
        });
        let clone = history.clone();
        drop(history);
        assert_eq!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty));

        drop(clone);
        assert_eq!(
            stopped.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected),
            "shutdown must wake the worker rather than wait for its timer"
        );
    }

    #[test]
    fn choosing_week_retention_from_month_keeps_entries_newer_than_a_week() {
        let day_ms = 24 * 60 * 60 * 1_000;
        let now = 30 * day_ms;
        let path = temp_path("month-to-week");
        let mut store = HistoryStore::open(path, HistoryRetention::Month, 0);
        store
            .record(draft("twenty days old"), now - 20 * day_ms)
            .unwrap();
        store
            .record(draft("two days old"), now - 2 * day_ms)
            .unwrap();

        store.set_retention(HistoryRetention::Week, now).unwrap();

        let texts: Vec<_> = store
            .entries()
            .map(|entry| entry.final_text.as_str())
            .collect();
        assert_eq!(texts, ["two days old"]);
    }

    #[test]
    fn off_records_nothing_but_preserves_existing_entries() {
        let path = temp_path("off");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 1_000);
        store.record(draft("kept"), 1_000).unwrap();
        store.set_retention(HistoryRetention::Off, 1_500).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH)
            .unwrap();

        assert_eq!(store.record(draft("dropped"), 2_000).unwrap(), None);
        let later = HistoryRetention::Month.max_age_ms().unwrap() + 2_000;
        store.expire(later);
        assert_eq!(store.len(), 1);
        assert_eq!(store.search("kept", later).len(), 1);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), UNIX_EPOCH);
    }

    #[test]
    fn hard_caps_bound_entries_and_text_regardless_of_retention() {
        let path = temp_path("caps");
        let mut store = HistoryStore::open(path, HistoryRetention::Forever, 0);
        store.record(draft("entry"), 0).unwrap();
        store.entries = (0..MAX_ENTRIES + 5)
            .map(|id| HistoryEntry {
                id: id as u64,
                ..store.entries[0].clone()
            })
            .collect();
        store.prune(0);
        assert_eq!(store.len(), MAX_ENTRIES);
        assert_eq!(store.entries[0].id, 5, "the oldest entries are evicted");
        store.next_id = (MAX_ENTRIES + 5) as u64;

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
        store.record(draft(&big), 0).unwrap();
        store.entries = (0..fits + 3)
            .map(|id| HistoryEntry {
                id: id as u64,
                ..store.entries[0].clone()
            })
            .collect();
        store.prune(0);
        let total: usize = store
            .entries()
            .map(|entry| entry.raw_text.len() + entry.final_text.len())
            .sum();
        assert!(total <= MAX_TOTAL_TEXT_BYTES);
        assert!(store.len() < fits + 3);
        assert!(
            store.entries[0].id > 0,
            "byte pressure also evicts oldest first"
        );
    }

    #[test]
    fn search_is_case_insensitive_across_text_application_and_profile() {
        let path = temp_path("search");
        let mut store = HistoryStore::open(path, HistoryRetention::Week, 0);
        store.record(draft("Hello World"), 1_000).unwrap();
        store.record(draft("other text"), 2_000).unwrap();

        assert_eq!(store.search("hello world", 2_000).len(), 1);
        assert_eq!(store.search("zed", 2_000).len(), 2);
        assert_eq!(store.search("messages", 2_000).len(), 2);
        assert_eq!(store.search("absent", 2_000).len(), 0);
        assert_eq!(
            store.search("  ", 2_000).len(),
            2,
            "blank queries list everything"
        );
    }

    #[test]
    fn recent_lists_the_newest_reusable_entries_within_retention() {
        let path = temp_path("recent");
        let day_ms = HistoryRetention::Day.max_age_ms().unwrap();
        let mut store = HistoryStore::open(path, HistoryRetention::Day, 0);
        store.record(draft("expired"), 1_000).unwrap();
        for (index, text) in ["one", "two", "  ", "three"].into_iter().enumerate() {
            store.record(draft(text), day_ms + index as u64).unwrap();
        }
        store
            .record(
                HistoryDraft {
                    kind: HistoryKind::Printed,
                    ..draft("printed")
                },
                day_ms + 10,
            )
            .unwrap();
        store.record(draft("four"), day_ms + 20).unwrap();

        let texts = |store: &HistoryStore, count| {
            store
                .recent(count, day_ms + 1_001)
                .into_iter()
                .map(|entry| entry.final_text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&store, 5), ["four", "three", "two", "one"]);
        assert_eq!(texts(&store, 2), ["four", "three"]);
        store
            .set_retention(HistoryRetention::Off, day_ms + 1_001)
            .unwrap();
        assert!(texts(&store, 5).is_empty(), "Off lists nothing");
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
    fn delete_many_removes_only_the_listed_entries() {
        let path = temp_path("delete-many");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let first = store.record(draft("one"), 1_000).unwrap().unwrap();
        let second = store.record(draft("two"), 2_000).unwrap().unwrap();
        let third = store.record(draft("three"), 3_000).unwrap().unwrap();

        assert_eq!(
            store.delete_many(&[first, third, 9_999]).unwrap(),
            2,
            "only the listed entries that exist count"
        );
        assert_eq!(store.len(), 1);
        assert!(store.entry(first).is_none());
        assert!(store.entry(second).is_some());
        assert!(store.entry(third).is_none());
        assert_eq!(store.delete_many(&[9_999]).unwrap(), 0);
        assert_eq!(store.len(), 1);

        let reopened = HistoryStore::open(path, HistoryRetention::Week, 4_000);
        assert_eq!(reopened.len(), 1);
        assert!(reopened.entry(second).is_some());
    }

    #[test]
    fn failed_clear_and_delete_keep_entries_for_a_retry() {
        let path = temp_path("failed-clear");
        let mut store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        let first = store.record(draft("one"), 1_000).unwrap().unwrap();
        store.record(draft("two"), 2_000).unwrap();
        let temporary = path.with_extension("json.tmp");
        fs::create_dir(&temporary).unwrap();

        assert!(store.delete(first).is_err());
        assert!(store.delete_many(&[first]).is_err());
        assert!(store.clear().is_err());
        assert_eq!(store.len(), 2, "a failed save keeps the entries");
        assert_eq!(
            store
                .entries()
                .map(|entry| entry.final_text.as_str())
                .collect::<Vec<_>>(),
            ["two", "one"],
            "a failed save puts every removed entry back in place"
        );

        fs::remove_dir(temporary).unwrap();
        store.sync();
        assert!(store.delete(first).unwrap());
        store.clear().unwrap();
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
