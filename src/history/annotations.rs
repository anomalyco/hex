//! Annotate's captures (annotations and replays) for the Captures pane: the
//! Hex and legacy packages from the last 30 days, read in place.
//! Nothing is copied or indexed; the
//! bundles are the record. A scan runs on a thread of its own, at most every
//! few seconds while the pane asks, and reads again only the bundles whose
//! manifest or movie availability changed.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use scrawl_capture::bundle::{MANIFEST, manifest_name, read_manifest};

use super::captures::{CaptureWord, WINDOW_MS};

/// How often a scan may run while the pane asks.
const RESCAN: Duration = Duration::from_secs(5);

/// One annotation or replay on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Annotation {
    /// Stable for the bundle, and never one of the Lab index's ids.
    pub id: u64,
    /// Exact manifest identifier, shared across moved or copied packages.
    pub manifest_id: String,
    /// The bundle folder.
    pub bundle: PathBuf,
    pub at_ms: u64,
    pub duration_ms: u64,
    /// The app it was of, when it was one.
    pub app: Option<String>,
    /// What was said over it.
    pub words: String,
    pub timed: Vec<CaptureWord>,
    /// The session it was sent to.
    pub sent_to: Option<String>,
    /// Its movie, while it's still there.
    pub movie: Option<PathBuf>,
    /// It had a movie that retention has since removed; its words and
    /// marks remain.
    pub expired: bool,
    /// A picture of it for the row: its first keyframe, else its contact
    /// sheet.
    pub picture: Option<PathBuf>,
}

impl Annotation {
    /// Whether every word of `words` (lowercased) is in its words, app,
    /// destination, or kind.
    pub fn matches(&self, words: &[String]) -> bool {
        let haystack = format!(
            "{} {} {} annotation",
            self.words,
            self.app.as_deref().unwrap_or(""),
            self.sent_to.as_deref().unwrap_or("")
        )
        .to_lowercase();
        words.iter().all(|word| haystack.contains(word.as_str()))
    }
}

/// Reads the bundle at `bundle`; `None` while it has no readable
/// `annotation.json` or `scrawl.json` (still being written, or not a capture).
pub fn read(bundle: &Path) -> Option<Annotation> {
    Some(from_manifest(bundle, read_manifest(bundle).ok()?))
}

pub(crate) fn from_manifest(
    bundle: &Path,
    manifest: scrawl_capture::model::ScrawlBundle,
) -> Annotation {
    let movie = manifest.video.as_ref().map(|video| bundle.join(video));
    let present = movie.as_ref().filter(|movie| movie.is_file()).cloned();
    let picture = manifest
        .keyframes
        .iter()
        .map(|keyframe| bundle.join(&keyframe.file))
        .find(|path| path.is_file())
        .or_else(|| {
            (!manifest.contact_sheet.is_empty())
                .then(|| bundle.join(&manifest.contact_sheet))
                .filter(|path| path.is_file())
        });
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(manifest.id.as_bytes());
    Annotation {
        id: u64::from_be_bytes(digest[..8].try_into().unwrap()) | 1 << 63,
        manifest_id: manifest.id,
        bundle: bundle.to_path_buf(),
        at_ms: manifest.created_at_ms,
        duration_ms: manifest.duration_ms,
        app: manifest.target.app,
        words: manifest.transcript.text.trim().to_owned(),
        timed: manifest
            .transcript
            .words
            .into_iter()
            .map(|w| CaptureWord {
                t0: w.t0.saturating_sub(manifest.video_offset_ms),
                t1: w.t1.saturating_sub(manifest.video_offset_ms),
                text: w.text,
            })
            .collect(),
        sent_to: manifest
            .destination
            .map(|destination| destination.title)
            .filter(|title| !title.is_empty()),
        expired: movie.is_some() && present.is_none(),
        movie: present,
        picture,
    }
}

/// The bundles in `folder` from the window before `now_ms`, newest first,
/// reusing `cache`'s reads of unchanged `scrawl.json` files. `cache` keeps
/// only what's still listed.
fn scan(
    folders: &[PathBuf],
    now_ms: u64,
    cache: &mut HashMap<PathBuf, (SystemTime, Option<Annotation>)>,
) -> Vec<Annotation> {
    let mut seen = HashMap::new();
    for entry in folders
        .iter()
        .filter_map(|p| fs::read_dir(p).ok())
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let Some(name) = manifest_name(&path) else {
            continue;
        };
        let Ok(modified) = fs::symlink_metadata(path.join(name)).and_then(|meta| meta.modified())
        else {
            continue;
        };
        let read = match cache.remove(&path) {
            Some((at, read))
                if at == modified
                    && !read.as_ref().is_some_and(|a| {
                        a.movie.as_ref().is_some_and(|p| !p.is_file()) || a.expired
                    }) =>
            {
                read
            }
            _ => read(&path),
        };
        seen.insert(path, (modified, read));
    }
    *cache = seen;
    let mut found: Vec<Annotation> = cache
        .values()
        .filter_map(|(_, read)| read.clone())
        .filter(|a| now_ms.saturating_sub(a.at_ms) < WINDOW_MS)
        .collect();
    found.sort_by_key(|a| (manifest_name(&a.bundle) != Some(MANIFEST), a.bundle.clone()));
    let mut ids = std::collections::HashSet::new();
    found.retain(|a| ids.insert(a.manifest_id.clone()));
    found.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.bundle.cmp(&b.bundle)));
    found
}

/// The Captures view's bounded annotation source across `folders`, read by the
/// mixed History reader off the window thread. Presence here never authorizes paste.
#[cfg(all(target_os = "macos", hex_capture))]
pub(crate) fn in_folders(folders: &[PathBuf], now: u64) -> Vec<Annotation> {
    scan(folders, now, &mut HashMap::new())
}

#[derive(Default)]
struct Shared {
    listed: Vec<Annotation>,
    cache: HashMap<PathBuf, (SystemTime, Option<Annotation>)>,
    scanned: Option<Instant>,
    scanning: bool,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
/// Moves when a scan finds something different.
static REVISION: AtomicU64 = AtomicU64::new(0);

fn shared() -> MutexGuard<'static, Option<Shared>> {
    SHARED.lock().unwrap_or_else(|error| error.into_inner())
}

/// The listed annotations' revision. Safe from any thread.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

/// Starts a scan of the captures folder on its own thread, unless one runs
/// or ran in the last few seconds. Safe from any thread.
pub fn refresh() {
    {
        let mut shared = shared();
        let shared = shared.get_or_insert_with(Shared::default);
        if shared.scanning || shared.scanned.is_some_and(|at| at.elapsed() < RESCAN) {
            return;
        }
        shared.scanning = true;
    }
    let spawned = std::thread::Builder::new()
        .name("hex-captures-scan".into())
        .spawn(|| {
            let folders = crate::app_settings::capture_settings().annotation_roots();
            let mut cache = shared()
                .as_mut()
                .map(|shared| std::mem::take(&mut shared.cache))
                .unwrap_or_default();
            let listed = scan(&folders, super::now_ms(), &mut cache);
            let mut shared = shared();
            let shared = shared.get_or_insert_with(Shared::default);
            shared.cache = cache;
            shared.scanned = Some(Instant::now());
            shared.scanning = false;
            if shared.listed != listed {
                shared.listed = listed;
                REVISION.fetch_add(1, Ordering::AcqRel);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not read the captures folder");
        if let Some(shared) = shared().as_mut() {
            shared.scanning = false;
        }
    }
}

/// The annotations the last scan found, newest first, matching every word
/// of `query`. One trashed while that scan ran is left out.
pub fn list(query: &str) -> Vec<Annotation> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    shared()
        .as_ref()
        .map(|shared| {
            shared
                .listed
                .iter()
                .filter(|annotation| annotation.matches(&words) && annotation.bundle.exists())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Forgets the bundle at `bundle` (it went to the Trash), so it leaves the
/// list before the next scan.
pub fn forget(bundle: &Path) {
    if let Some(shared) = shared().as_mut() {
        shared
            .listed
            .retain(|annotation| annotation.bundle != bundle);
        shared.cache.remove(bundle);
        REVISION.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hex-annotations-{name}-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn write(folder: &Path, name: &str, at_ms: u64, extra: &str) -> PathBuf {
        let bundle = folder.join(name);
        fs::create_dir_all(bundle.join("frames")).unwrap();
        fs::write(bundle.join("frames/frame-01.png"), b"png").unwrap();
        fs::write(
            bundle.join("scrawl.json"),
            format!(
                r#"{{"version":1,"id":"{at_ms}","createdAtMs":{at_ms},"durationMs":4000,
                "target":{{"mode":"window","app":"Brave","displayId":1,
                  "frame":{{"x":0,"y":0,"width":1,"height":1,"scale":2}}}},
                "transcript":{{"source":"parakeet","text":" make it blue ","segments":[],
                  "words":[{{"text":"make","t0":0,"t1":1}}]}},
                "strokes":[],"marks":[],"events":[],
                "keyframes":[{{"index":0,"t":0,"reason":"start","file":"frames/frame-01.png","event":"e"}}],
                "contactSheet":"contact-sheet.png"{extra}}}"#
            ),
        )
        .unwrap();
        bundle
    }

    #[test]
    fn it_lists_bundles_in_the_window_newest_first_with_what_remains() {
        let folder = folder("list");
        let now = 1_800_000_000_000;
        let older = write(
            &folder,
            &format!("scrawl-{}-1-1.scrawl", now - 60_000),
            now - 60_000,
            r#","video":"recording.mp4","destination":{"session":"ses_1","title":"Hex","via":"chosen"}"#,
        );
        let newer = write(&folder, &format!("{}.scrawl", now - 1_000), now - 1_000, "");
        fs::write(newer.join("recording.mp4"), b"mp4").unwrap();
        let stale = now - WINDOW_MS - 1;
        write(&folder, &format!("{stale}.scrawl"), stale, "");
        fs::create_dir_all(folder.join(format!("{}.scrawl", now - 5))).unwrap();

        let mut cache = HashMap::new();
        let roots = std::slice::from_ref(&folder);
        let listed = scan(roots, now, &mut cache);
        assert_eq!(
            listed.iter().map(|a| &a.bundle).collect::<Vec<_>>(),
            [&newer, &older],
            "newest first; past the window or unwritten, left out"
        );
        let older = &listed[1];
        assert_eq!(older.words, "make it blue");
        assert_eq!(older.app.as_deref(), Some("Brave"));
        assert_eq!(older.sent_to.as_deref(), Some("Hex"));
        assert!(older.expired && older.movie.is_none(), "its movie is gone");
        assert_eq!(
            older.picture,
            Some(older.bundle.join("frames/frame-01.png"))
        );
        assert!(
            !listed[0].expired,
            "a bundle that never had a movie hasn't lost one"
        );
        assert!(older.matches(&["blue".into(), "hex".into()]));
        assert!(older.matches(&["annotation".into()]));
        assert!(!older.matches(&["red".into()]));
        assert_ne!(listed[0].id, listed[1].id);

        assert_eq!(
            scan(roots, now, &mut cache),
            listed,
            "a rescan reads the same"
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn hex_annotation_mixed_roots_show_one_record_at_its_original_time() {
        let tmp = folder("mixed");
        let roots = [tmp.join("own"), tmp.join("legacy")];
        let legacy = roots[1].join("original.scrawl");
        let hex = roots[0].join("renamed.hexannotation");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&hex).unwrap();
        fs::write(
            legacy.join("scrawl.json"),
            include_bytes!("../../tests/fixtures/annotations/marked-v1.json"),
        )
        .unwrap();
        let b = read_manifest(&legacy).unwrap();
        scrawl_capture::bundle::publish(&hex, &b).unwrap();
        let listed = scan(&roots, b.created_at_ms + 1, &mut HashMap::new());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, read(&legacy).unwrap().id);
        assert_eq!(listed[0].bundle, hex);
        assert_eq!(listed[0].at_ms, b.created_at_ms);
        assert!(listed[0].expired);
        assert_eq!(listed[0].words, "Move this");
        assert_eq!(
            listed[0].timed[1].t1, 930,
            "word on the editor's movie clock"
        );
        assert_eq!(
            fs::read_dir(legacy).unwrap().count(),
            1,
            "read-only discovery"
        );
        fs::remove_dir_all(tmp).unwrap();
    }
}
