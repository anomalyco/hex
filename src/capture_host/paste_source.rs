//! Only an Annotate writer can publish a ready result. Never adopt old
//! folders from a shared library by name, transcript source or age.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use scrawl_capture::bundle::{MANIFEST, manifest_name, read_manifest_bounded};
use scrawl_capture::model::ScrawlBundle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const READY: &str = ".hex-annotate-v1";
const DAY_MS: u64 = 86_400_000;
const MAX_MANIFEST: u64 = 16 << 20;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Stamp {
    ready_at: u64,
    generation: [u8; 16],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<[u8; 32]>,
}

/// Relative to the *current* configured roots; includes its path and receipt
/// binding so a settings change, move, or directory copy cannot retarget or
/// promote a row. No path is accepted from a client.
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
                binding: None,
            },
        }
    }
}

fn compute_binding(
    canonical_dir: &Path,
    meta: &fs::Metadata,
    generation: &[u8; 16],
    ready_at: u64,
    bundle: &ScrawlBundle,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"hex-annotate-ready-v1\0");
    let dir_bytes = canonical_dir.as_os_str().as_encoded_bytes();
    h.update((dir_bytes.len() as u64).to_le_bytes());
    h.update(dir_bytes);
    h.update(meta.dev().to_le_bytes());
    h.update(meta.ino().to_le_bytes());
    h.update(generation);
    h.update(ready_at.to_le_bytes());
    let id_bytes = bundle.id.as_bytes();
    h.update((id_bytes.len() as u64).to_le_bytes());
    h.update(id_bytes);
    h.update(bundle.created_at_ms.to_le_bytes());
    h.finalize().into()
}

fn legacy_summary_matches(dir: &Path, canonical_dir: &Path) -> bool {
    let summary = dir.join("summary.md");
    let meta = match fs::symlink_metadata(&summary) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    if !meta.is_file() {
        return false;
    }
    let Some(bytes) = read_regular(&summary, 1 << 20) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let Some(first) = text.lines().next() else {
        return true;
    };
    if let Some(inner) = first.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
        && (inner.starts_with("hex-annotate") || inner.starts_with("scrawl"))
        && let Some((_, locator)) = inner.rsplit_once(" · ")
        && locator != "."
    {
        let recorded = Path::new(locator);
        let matches = recorded
            .canonicalize()
            .map(|c| c == canonical_dir)
            .unwrap_or_else(|_| recorded == dir || recorded == canonical_dir);
        return matches;
    }
    true
}

/// A receipt belongs to the bundle (so deletion needs no index cleanup).
/// It binds identity, original package inode/path, and completion time.
/// Never rewrite an existing receipt.
pub(crate) fn mark_ready(dir: &Path, ready_at: u64) -> io::Result<()> {
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::other("not a package directory"));
    }
    let canonical = dir.canonicalize()?;
    let bundle =
        read_manifest_bounded(dir, MAX_MANIFEST).map_err(|e| io::Error::other(e.to_string()))?;
    let mut generation = [0; 16];
    getrandom::fill(&mut generation).map_err(|e| io::Error::other(e.to_string()))?;
    let binding = compute_binding(&canonical, &meta, &generation, ready_at, &bundle);
    let stamp = serde_json::to_vec(&Stamp {
        ready_at,
        generation,
        binding: Some(binding),
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

enum Roots<'a> {
    #[cfg(test)]
    One(&'a Path),
    Many(&'a [PathBuf]),
}

pub(crate) struct Scope<'a> {
    roots: Roots<'a>,
    days: Option<u32>,
}

impl<'a> Scope<'a> {
    #[cfg(test)]
    pub(crate) fn new(root: &'a Path, days: Option<u32>) -> Self {
        Self {
            roots: Roots::One(root),
            days,
        }
    }

    pub(crate) fn in_roots(roots: &'a [PathBuf], days: Option<u32>) -> Self {
        Self {
            roots: Roots::Many(roots),
            days,
        }
    }

    fn for_each_root(&self, mut f: impl FnMut(&Path)) {
        let mut seen = HashSet::new();
        let mut visit = |root: &Path| {
            let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            if seen.insert(key) {
                f(root);
            }
        };
        match self.roots {
            #[cfg(test)]
            Roots::One(root) => visit(root),
            Roots::Many(roots) => {
                for root in roots {
                    visit(root);
                }
            }
        }
    }

    fn allows_parent(&self, canonical_dir: &Path) -> bool {
        let Some(parent) = canonical_dir.parent() else {
            return false;
        };
        let mut allowed = false;
        self.for_each_root(|root| {
            if !allowed && root.canonicalize().ok().as_deref() == Some(parent) {
                allowed = true;
            }
        });
        allowed
    }

    fn identity(&self, dir: &Path, now: u64) -> Option<Id> {
        manifest_name(dir)?;
        if !fs::symlink_metadata(dir).ok()?.is_dir() {
            return None;
        }
        let canonical = dir.canonicalize().ok()?;
        if !self.allows_parent(&canonical) {
            return None;
        }
        let stamp: Stamp = serde_json::from_slice(&read_regular(&dir.join(READY), 512)?).ok()?;
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
        let mut ids = Vec::new();
        self.for_each_root(|root| {
            let Ok(entries) = root.read_dir() else {
                return;
            };
            ids.extend(
                entries
                    .flatten()
                    .filter_map(|e| self.identity(&e.path(), now)),
            );
        });
        ids.sort_unstable_by(|a, b| {
            b.stamp
                .ready_at
                .cmp(&a.stamp.ready_at)
                .then_with(|| {
                    (manifest_name(&a.dir) != Some(MANIFEST))
                        .cmp(&(manifest_name(&b.dir) != Some(MANIFEST)))
                })
                .then_with(|| b.cmp(a))
        });
        let mut seen_manifest_ids = HashSet::new();
        ids.into_iter()
            .filter_map(|id| {
                let (_, manifest) = self.resolve(&id, now)?;
                if !seen_manifest_ids.insert(manifest.id.clone()) {
                    return None;
                }
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

    /// The picture the picker's preview shows for `bundle` in `dir`: its
    /// first keyframe, else its contact sheet. Only a plain relative name
    /// inside the bundle is followed, as `resolve` follows no other.
    pub(crate) fn picture(dir: &Path, bundle: &ScrawlBundle) -> Option<PathBuf> {
        let inside = |name: &str| {
            let path = Path::new(name);
            let plain = !name.is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)));
            let path = dir.join(path);
            (plain && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())).then_some(path)
        };
        bundle
            .keyframes
            .first()
            .and_then(|frame| inside(&frame.file))
            .or_else(|| inside(&bundle.contact_sheet))
    }

    pub(crate) fn resolve(&self, id: &Id, now: u64) -> Option<(PathBuf, ScrawlBundle)> {
        if &self.identity(&id.dir, now)? != id {
            return None;
        }
        let dir_meta = fs::symlink_metadata(&id.dir).ok()?;
        if !dir_meta.is_dir() {
            return None;
        }
        let canonical = id.dir.canonicalize().ok()?;
        let bundle = read_manifest_bounded(&id.dir, MAX_MANIFEST).ok()?;
        match &id.stamp.binding {
            Some(expected) => {
                let actual = compute_binding(
                    &canonical,
                    &dir_meta,
                    &id.stamp.generation,
                    id.stamp.ready_at,
                    &bundle,
                );
                if &actual != expected {
                    return None;
                }
            }
            None => {
                // Pre-cutover two-field receipts were issued only for `<id>.scrawl`.
                if manifest_name(&id.dir) != Some("scrawl.json")
                    || id.dir.file_stem() != Some(OsStr::new(&bundle.id))
                    || !legacy_summary_matches(&id.dir, &canonical)
                {
                    return None;
                }
            }
        }
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
