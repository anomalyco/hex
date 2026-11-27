//! Best-effort protection against editor saves during Markdown rendering.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::meeting::text_sha256;

pub(in crate::meeting) enum RewritePolicy {
    /// Replace a complete generated document only while Hex still owns it.
    /// An absent file can be created, but is rechecked before publication too.
    WholeFileIfOwned,
    /// Merge Hex headings/blocks into an existing, possibly user-edited file.
    /// Such a merge must not acquire ownership of the user's document.
    MergeKnownSections,
}

#[derive(PartialEq)]
pub(in crate::meeting) enum RewriteOutcome {
    Written,
    Unchanged,
    NotOwned,
}

pub(in crate::meeting) fn rewrite(
    path: &Path,
    written: &mut Option<String>,
    policy: RewritePolicy,
    render: impl FnOnce(&str) -> String,
) -> Result<RewriteOutcome> {
    let observed = observe(path)?;
    let owned = observed
        .as_ref()
        .is_some_and(|bytes| written.as_deref() == Some(text_sha256(bytes).as_str()));
    match policy {
        RewritePolicy::WholeFileIfOwned if observed.is_some() && !owned => {
            return Ok(RewriteOutcome::NotOwned);
        }
        RewritePolicy::MergeKnownSections if observed.is_none() => {
            return Err(eyre!("the Markdown file is missing"));
        }
        _ => {}
    }
    let original = std::str::from_utf8(observed.as_deref().unwrap_or_default())?;
    let updated = render(original);
    if observed.is_some() && updated == original {
        return Ok(RewriteOutcome::Unchanged);
    }
    let folder = path
        .parent()
        .ok_or_else(|| eyre!("the Markdown has no folder"))?;
    let temporary = Temporary::write(path, updated.as_bytes())?;
    #[cfg(test)]
    BEFORE_RECHECK.with(|hook| {
        if let Some(hook) = hook.take() {
            hook();
        }
    });
    // This narrows the editor race; a read followed by rename is NOT a
    // filesystem compare-and-swap. Another save can still land between them.
    if observe(path)? != observed {
        return Err(eyre!("the Markdown changed while it was being updated"));
    }
    fs::rename(&temporary.0, path)?;
    if owned || observed.is_none() {
        *written = Some(text_sha256(updated.as_bytes()));
    }
    crate::durable::sync_directory(folder)?;
    Ok(RewriteOutcome::Written)
}

fn observe(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

struct Temporary(PathBuf);

impl Temporary {
    fn write(path: &Path, bytes: &[u8]) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = path
            .file_name()
            .ok_or_else(|| eyre!("the Markdown has no name"))?;
        loop {
            let temporary = path.with_file_name(format!(
                ".{}.{}-{}.tmp",
                name.to_string_lossy(),
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let temporary = Self(temporary);
            file.write_all(bytes)?;
            if let Ok(metadata) = fs::metadata(path) {
                let _ = file.set_permissions(metadata.permissions());
            }
            file.sync_all()?;
            return Ok(temporary);
        }
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != ErrorKind::NotFound
        {
            tracing::warn!(%error, "could not remove temporary meeting Markdown");
        }
    }
}

#[cfg(test)]
thread_local! {
    pub(in crate::meeting) static BEFORE_RECHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}
