//! Only an Annotate writer can publish a ready result. Never adopt old
//! folders from the shared Scrawl library by name, transcript source or age.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use scrawl_capture::model::ScrawlBundle;
use serde::{Deserialize, Serialize};

const READY: &str = ".hex-annotate-v1";
const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Stamp {
    ready_at: u64,
    generation: [u8; 16],
}

/// Relative to the *current* configured folder; includes its root so a
/// settings change cannot retarget a selected row. No path is accepted from
/// a client. Copying a folder creates a different identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Id {
    dir: PathBuf,
    stamp: Stamp,
}

impl Id {
    pub(crate) fn bundle(&self) -> &Path {
        &self.dir
    }

    /// An identity for the window's fixture scenes. It names no folder, so
    /// it resolves to nothing.
    pub(crate) fn fixture(ready_at: u64) -> Self {
        Self {
            dir: PathBuf::new(),
            stamp: Stamp {
                ready_at,
                generation: [0; 16],
            },
        }
    }
}

/// A receipt belongs to the bundle (so deletion needs no index cleanup).
/// It contains identity and time only. Never rewrite an existing receipt.
pub(crate) fn mark_ready(dir: &Path, ready_at: u64) -> io::Result<()> {
    let mut generation = [0; 16];
    getrandom::fill(&mut generation).map_err(|e| io::Error::other(e.to_string()))?;
    let stamp = serde_json::to_vec(&Stamp {
        ready_at,
        generation,
    })?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(READY))?
        .write_all(&stamp)
}

pub(crate) fn read_regular(path: &Path, max: u64) -> Option<Vec<u8>> {
    // Refuse links and devices. O_NONBLOCK also refuses to hang on a FIFO
    // substituted between discovery and open.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > max {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

pub(crate) struct Scope<'a> {
    root: &'a Path,
    days: Option<u32>,
}

impl<'a> Scope<'a> {
    pub(crate) fn new(root: &'a Path, days: Option<u32>) -> Self {
        Self { root, days }
    }

    fn identity(&self, dir: &Path, now: u64) -> Option<Id> {
        if dir.parent() != Some(self.root)
            || dir.extension()? != "scrawl"
            || !fs::symlink_metadata(dir).ok()?.is_dir()
        {
            return None;
        }
        let stamp: Stamp = serde_json::from_slice(&read_regular(&dir.join(READY), 256)?).ok()?;
        if self
            .days
            .is_some_and(|d| now.saturating_sub(stamp.ready_at) >= u64::from(d.max(1)) * DAY_MS)
        {
            return None;
        }
        Some(Id {
            dir: dir.to_owned(),
            stamp,
        })
    }

    pub(crate) fn rows(&self, limit: usize, now: u64) -> Vec<(Id, u64, String)> {
        let Ok(entries) = self.root.read_dir() else {
            return vec![];
        };
        let mut ids: Vec<_> = entries
            .flatten()
            .filter_map(|e| self.identity(&e.path(), now))
            .collect();
        ids.sort_unstable_by(|a, b| {
            b.stamp
                .ready_at
                .cmp(&a.stamp.ready_at)
                .then_with(|| b.cmp(a))
        });
        ids.into_iter()
            .filter_map(|id| {
                let (_, manifest) = self.resolve(&id, now)?;
                let preview = [
                    Some(manifest.transcript.text.as_str()),
                    manifest.note.as_deref(),
                    manifest.target.app.as_deref(),
                ]
                .into_iter()
                .flatten()
                .map(str::trim)
                .find(|s| !s.is_empty())
                .unwrap_or("Annotate")
                .chars()
                .take(100)
                .collect();
                let ready_at = id.stamp.ready_at;
                Some((id, ready_at, preview))
            })
            .take(limit)
            .collect()
    }

    pub(crate) fn resolve(&self, id: &Id, now: u64) -> Option<(PathBuf, ScrawlBundle)> {
        if &self.identity(&id.dir, now)? != id {
            return None;
        }
        let bundle: ScrawlBundle =
            serde_json::from_slice(&read_regular(&id.dir.join("scrawl.json"), 16 << 20)?).ok()?;
        if !bundle.keyframes.is_empty() {
            // The Annotate writer emits this name; never follow paths supplied
            // by an edited manifest outside the bundle.
            if bundle.contact_sheet != "contact-sheet.png"
                || !fs::symlink_metadata(id.dir.join(&bundle.contact_sheet))
                    .ok()?
                    .is_file()
            {
                return None;
            }
        } else if bundle.transcript.text.trim().is_empty()
            && bundle.note.as_deref().is_none_or(|n| n.trim().is_empty())
            && bundle.marks.is_empty()
        {
            return None;
        }
        Some((id.dir.clone(), bundle))
    }
}
