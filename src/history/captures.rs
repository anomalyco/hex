//! Labs › Screenshots' capture history, as CleanShot's Capture History
//! keeps one: every shot and recording the Lab saved in the last 30 days,
//! newest first, with a small thumbnail and the words said over it.
//!
//! The files themselves stay where they were saved (`screenshots.folder`);
//! this keeps only an owner-only index (`captures/index.json` in
//! Application Support) and a 240 px PNG thumbnail per entry beside it. An
//! entry whose file was moved or deleted is left out of the list. Writes
//! are atomic, and the index is capped in entries and in each entry's words
//! like History is.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
/// How long a capture stays listed.
pub const WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const MAX_ENTRIES: usize = 500;
const MAX_WORDS_BYTES: usize = 4 * 1024;
const MAX_TIMED_WORDS: usize = 2_000;
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
/// A thumbnail's long edge, in pixels.
pub const THUMB_PX: u32 = 240;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Shot,
    Recording,
}

impl CaptureKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shot => "Screenshot",
            Self::Recording => "Screen recording",
        }
    }
}

/// A word said over a recording, on the movie's clock.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaptureWord {
    pub t0: u64,
    pub t1: u64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaptureEntry {
    pub id: u64,
    pub at_ms: u64,
    pub kind: CaptureKind,
    /// The saved PNG or movie.
    pub file: PathBuf,
    /// Its size in pixels.
    pub size: [u32; 2],
    #[serde(default)]
    pub duration_ms: u64,
    /// Everything said over it, joined.
    #[serde(default)]
    pub words: String,
    /// A recording's words with their times, for the editor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timed: Vec<CaptureWord>,
}

impl CaptureEntry {
    /// The file's name without its extension.
    pub fn name(&self) -> String {
        self.file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn matches(&self, words: &[String]) -> bool {
        let haystack =
            format!("{} {} {}", self.words, self.name(), self.kind.label()).to_lowercase();
        words.iter().all(|word| haystack.contains(word.as_str()))
    }
}

/// A capture to remember, with its thumbnail's PNG.
pub struct CaptureDraft {
    pub kind: CaptureKind,
    pub file: PathBuf,
    pub size: [u32; 2],
    pub duration_ms: u64,
    pub words: String,
    pub timed: Vec<CaptureWord>,
    pub thumbnail_png: Vec<u8>,
}

#[derive(Default, Deserialize, Serialize)]
struct Saved {
    version: u32,
    next_id: u64,
    entries: Vec<CaptureEntry>,
}

/// The index and its thumbnails in one folder.
pub struct CaptureStore {
    folder: PathBuf,
    next_id: u64,
    /// Oldest first.
    entries: Vec<CaptureEntry>,
}

impl CaptureStore {
    /// The index in `folder`; empty when there's none, and a malformed one
    /// is kept aside as `index.json.corrupt`.
    pub fn open(folder: PathBuf) -> Self {
        let mut store = Self {
            folder,
            next_id: 1,
            entries: Vec::new(),
        };
        let index = store.index();
        let too_big = fs::metadata(&index).is_ok_and(|meta| meta.len() > MAX_INDEX_BYTES);
        match fs::read(&index) {
            Ok(_) if too_big => store.preserve_corrupt(),
            Ok(bytes) => match serde_json::from_slice::<Saved>(&bytes) {
                Ok(saved) if saved.version == VERSION => {
                    store.next_id = saved.next_id.max(1);
                    store.entries = saved.entries;
                }
                Ok(_) | Err(_) => store.preserve_corrupt(),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "could not read the capture history"),
        }
        store
    }

    fn index(&self) -> PathBuf {
        self.folder.join("index.json")
    }

    /// Entry `id`'s thumbnail.
    pub fn thumbnail(&self, id: u64) -> PathBuf {
        self.folder.join(format!("{id}.png"))
    }

    /// Remembers `draft`, its thumbnail first, dropping what's past the
    /// window or the cap.
    pub fn record(&mut self, draft: CaptureDraft, now_ms: u64) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        create_owner_folder(&self.folder)?;
        write_owner_file(&self.thumbnail(id), &draft.thumbnail_png)?;
        let mut timed = draft.timed;
        timed.truncate(MAX_TIMED_WORDS);
        self.entries.push(CaptureEntry {
            id,
            at_ms: now_ms,
            kind: draft.kind,
            file: draft.file,
            size: draft.size,
            duration_ms: draft.duration_ms,
            words: truncated(&draft.words, MAX_WORDS_BYTES),
            timed,
        });
        self.expire(now_ms);
        self.persist()?;
        Ok(id)
    }

    /// Captures inside the window whose files are still there, newest
    /// first, matching every word of `query` in their words, file name, or
    /// kind.
    pub fn list(&self, query: &str, now_ms: u64) -> Vec<CaptureEntry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .rev()
            .filter(|entry| now_ms.saturating_sub(entry.at_ms) < WINDOW_MS)
            .filter(|entry| entry.matches(&words))
            .filter(|entry| entry.file.exists())
            .cloned()
            .collect()
    }

    /// Forgets the capture saved at `file` (it went to the Trash).
    pub fn forget(&mut self, file: &Path) -> io::Result<bool> {
        let before = self.entries.len();
        let gone: Vec<u64> = self
            .entries
            .iter()
            .filter(|entry| entry.file == file)
            .map(|entry| entry.id)
            .collect();
        self.entries.retain(|entry| entry.file != file);
        for id in gone {
            let _ = fs::remove_file(self.thumbnail(id));
        }
        if self.entries.len() == before {
            return Ok(false);
        }
        self.persist().map(|()| true)
    }

    /// What was said over the shot saved at `file`, in place of what was.
    pub fn set_words(&mut self, file: &Path, words: &str) -> io::Result<bool> {
        let words = truncated(words, MAX_WORDS_BYTES);
        let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.file == file)
        else {
            return Ok(false);
        };
        if entry.words == words {
            return Ok(false);
        }
        entry.words = words;
        self.persist().map(|()| true)
    }

    /// Drops entries past the window, then the oldest past the cap, with
    /// their thumbnails.
    fn expire(&mut self, now_ms: u64) {
        let over = self.entries.len().saturating_sub(MAX_ENTRIES);
        let mut index = 0;
        let mut dropped = Vec::new();
        self.entries.retain(|entry| {
            let keep = index >= over && now_ms.saturating_sub(entry.at_ms) < WINDOW_MS;
            index += 1;
            if !keep {
                dropped.push(entry.id);
            }
            keep
        });
        for id in dropped {
            let _ = fs::remove_file(self.thumbnail(id));
        }
    }

    fn persist(&self) -> io::Result<()> {
        REVISION.fetch_add(1, Ordering::AcqRel);
        let saved = Saved {
            version: VERSION,
            next_id: self.next_id,
            entries: self.entries.clone(),
        };
        let json = serde_json::to_vec(&saved).map_err(io::Error::other)?;
        create_owner_folder(&self.folder)?;
        let temporary = self.index().with_extension("json.tmp");
        write_owner_file(&temporary, &json)?;
        fs::rename(&temporary, self.index())
    }

    fn preserve_corrupt(&self) {
        let backup = self.index().with_extension("json.corrupt");
        if let Err(error) = fs::rename(self.index(), &backup) {
            tracing::warn!(%error, "could not preserve the malformed capture history");
        }
    }
}

/// Moves on every change, so a pane reads the list again only then.
static REVISION: AtomicU64 = AtomicU64::new(0);

/// The capture history's revision. Safe from any thread.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

/// The one store in Application Support, opened on first use; `None` when
/// there's no Application Support folder.
fn shared() -> Option<MutexGuard<'static, CaptureStore>> {
    static STORE: OnceLock<Option<Mutex<CaptureStore>>> = OnceLock::new();
    STORE
        .get_or_init(|| match crate::app_paths::support_dir() {
            Ok(support) => Some(Mutex::new(CaptureStore::open(support.join("captures")))),
            Err(error) => {
                tracing::warn!(%error, "no capture history");
                None
            }
        })
        .as_ref()
        .map(|store| store.lock().unwrap_or_else(|error| error.into_inner()))
}

/// Remembers a capture. Safe from any thread; a failure is logged.
pub fn record(draft: CaptureDraft) {
    if let Some(mut store) = shared()
        && let Err(error) = store.record(draft, super::now_ms())
    {
        tracing::warn!(%error, "could not remember the capture");
    }
}

/// [`CaptureStore::list`] on the shared store.
pub fn list(query: &str) -> Vec<CaptureEntry> {
    shared()
        .map(|store| store.list(query, super::now_ms()))
        .unwrap_or_default()
}

/// Entry `id`'s thumbnail in the shared store.
pub fn thumbnail(id: u64) -> Option<PathBuf> {
    shared().map(|store| store.thumbnail(id))
}

/// [`CaptureStore::forget`] on the shared store.
pub fn forget(file: &Path) {
    if let Some(mut store) = shared()
        && let Err(error) = store.forget(file)
    {
        tracing::warn!(%error, "could not forget the capture");
    }
}

/// [`CaptureStore::set_words`] on the shared store.
pub fn set_words(file: &Path, words: &str) {
    if let Some(mut store) = shared()
        && let Err(error) = store.set_words(file, words)
    {
        tracing::warn!(%error, "could not keep the words said over the capture");
    }
}

fn truncated(text: &str, max_bytes: usize) -> String {
    text[..text.floor_char_boundary(max_bytes)].to_string()
}

fn create_owner_folder(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_owner_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    super::restrict_to_owner(&file)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60 * 1_000;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hex-captures-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn draft(folder: &Path, name: &str, words: &str) -> CaptureDraft {
        let file = folder.join(format!("{name}.png"));
        fs::write(&file, b"png").unwrap();
        CaptureDraft {
            kind: CaptureKind::Shot,
            file,
            size: [1440, 900],
            duration_ms: 0,
            words: words.into(),
            timed: Vec::new(),
            thumbnail_png: b"thumb".to_vec(),
        }
    }

    #[test]
    fn it_lists_the_newest_first_and_survives_a_reopen() {
        let saved = folder("saved");
        let mut store = CaptureStore::open(saved.join("captures"));
        store.record(draft(&saved, "a", ""), 10 * DAY).unwrap();
        let b = store
            .record(draft(&saved, "b", "the deploy button"), 11 * DAY)
            .unwrap();
        let store = CaptureStore::open(saved.join("captures"));
        let names: Vec<String> = store.list("", 12 * DAY).iter().map(|e| e.name()).collect();
        assert_eq!(names, ["b", "a"]);
        assert_eq!(fs::read(store.thumbnail(b)).unwrap(), b"thumb");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&saved.join("captures")), 0o700);
            assert_eq!(mode(&saved.join("captures/index.json")), 0o600);
        }
        fs::remove_dir_all(&saved).unwrap();
    }

    #[test]
    fn it_searches_the_words_and_skips_what_is_gone_or_old() {
        let saved = folder("search");
        let mut store = CaptureStore::open(saved.join("captures"));
        store.record(draft(&saved, "old", "deploy"), 0).unwrap();
        store
            .record(draft(&saved, "moved", "deploy"), 40 * DAY)
            .unwrap();
        store
            .record(draft(&saved, "kept", "The Deploy button"), 40 * DAY)
            .unwrap();
        fs::remove_file(saved.join("moved.png")).unwrap();
        let names = |query: &str| -> Vec<String> {
            store
                .list(query, 41 * DAY)
                .iter()
                .map(CaptureEntry::name)
                .collect()
        };
        // The old one aged out when the others came in.
        assert_eq!(names("deploy button"), ["kept"]);
        assert_eq!(names("screenshot"), ["kept"]);
        assert!(names("nothing").is_empty());
        assert!(!store.thumbnail(1).exists());
        fs::remove_dir_all(&saved).unwrap();
    }

    #[test]
    fn forgetting_or_naming_words_finds_the_file() {
        let saved = folder("forget");
        let mut store = CaptureStore::open(saved.join("captures"));
        let id = store.record(draft(&saved, "a", ""), DAY).unwrap();
        let file = saved.join("a.png");
        assert!(store.set_words(&file, "  what I said  ").unwrap());
        assert_eq!(store.list("said", DAY)[0].id, id);
        assert!(store.forget(&file).unwrap());
        assert!(store.list("", DAY).is_empty());
        assert!(!store.thumbnail(id).exists());
        assert!(!store.forget(&file).unwrap());
        fs::remove_dir_all(&saved).unwrap();
    }

    #[test]
    fn the_index_keeps_its_caps() {
        let saved = folder("caps");
        let mut store = CaptureStore::open(saved.join("captures"));
        let words = "word ".repeat(2_000);
        for index in 0..MAX_ENTRIES + 3 {
            let words = if index == MAX_ENTRIES { &words } else { "" };
            let mut draft = draft(&saved, &format!("{index}"), words);
            draft.thumbnail_png.clear();
            store.record(draft, DAY).unwrap();
        }
        assert_eq!(store.entries.len(), MAX_ENTRIES);
        assert_eq!(store.entries[0].name(), "3");
        let long = store
            .entries
            .iter()
            .find(|e| e.name() == MAX_ENTRIES.to_string());
        assert_eq!(long.unwrap().words.len(), MAX_WORDS_BYTES);
        fs::remove_dir_all(&saved).unwrap();
    }

    #[test]
    fn a_malformed_index_is_kept_aside() {
        let saved = folder("corrupt");
        let captures = saved.join("captures");
        fs::create_dir_all(&captures).unwrap();
        fs::write(captures.join("index.json"), b"{not json").unwrap();
        let store = CaptureStore::open(captures.clone());
        assert!(store.list("", 0).is_empty());
        assert!(captures.join("index.json.corrupt").exists());
        fs::remove_dir_all(&saved).unwrap();
    }
}
