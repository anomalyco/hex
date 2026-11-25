//! Undo for an import: both lists from before it, kept owner-only in the
//! data folder so Undo survives a restart. It holds the lists, never the
//! file, and stops applying once either list changes or another file is
//! imported.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Lists;

/// The undo copy's name in the data folder.
const FILE_NAME: &str = "hex-words-undo.json";
/// The most a copy can hold: two lists at the import limits, with room.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The lists from before an import, and the lists it left.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Undo {
    before: Lists,
    after: Lists,
}

impl Undo {
    pub(crate) fn new(before: Lists, after: Lists) -> Self {
        Self { before, after }
    }

    /// The lists Undo puts back.
    pub(crate) fn before(&self) -> &Lists {
        &self.before
    }

    /// Whether the lists are still as the import left them. Once either was
    /// edited, Undo would throw that edit away, so it is no longer offered.
    pub(crate) fn applies_to(&self, current: &Lists) -> bool {
        self.after == *current
    }

    /// Where the copy lives: the data folder, beside `settings.json`.
    pub(crate) fn path() -> Option<PathBuf> {
        crate::app_paths::support_dir()
            .ok()
            .map(|folder| folder.join(FILE_NAME))
    }

    /// Writes the copy where only this user can read it, replacing any
    /// older one in one step.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        crate::durable::write_private(path, &self.to_bytes())
    }

    /// The copy at `path`, or none when it is missing or unreadable.
    pub(crate) fn load(path: &Path) -> Option<Self> {
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        Self::from_bytes(&bytes)
    }

    /// Removes the copy; one already gone is fine.
    pub(crate) fn forget(path: &Path) {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "could not remove the words file undo copy"),
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an undo copy always serializes")
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}
