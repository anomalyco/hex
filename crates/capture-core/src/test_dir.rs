//! Scratch directories for tests. Dev-dependencies of a crate outside the workspace are not
//! resolved, so the tests carry their own.

use std::io;
use std::path::{Path, PathBuf};

use crate::time::HostTime;

/// A directory removed with everything in it on drop.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn tempdir() -> io::Result<TempDir> {
    let base = std::env::temp_dir();
    loop {
        let path = base.join(format!(
            "capture-core-{}-{}",
            std::process::id(),
            HostTime::now().ticks()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(TempDir { path }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}
