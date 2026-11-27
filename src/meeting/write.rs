//! Process-local read/edit/publish ownership for finished-meeting writers.

use std::path::Path;

use color_eyre::eyre::{Result, eyre};

use super::notes::InProgress;
use super::{MeetingManifest, read_manifest, write_manifest};

pub(super) mod markdown;

pub(super) struct MeetingWrite<'a> {
    directory: &'a Path,
    _claim: InProgress,
}

impl<'a> MeetingWrite<'a> {
    /// Claim before reading: otherwise a just-finished writer can be lost.
    /// This is the same process-local slot used by notes and transcript edits,
    /// not a filesystem lock against another process or the active recorder.
    pub(super) fn claim(directory: &'a Path) -> Result<Self> {
        let id = directory
            .file_name()
            .ok_or_else(|| eyre!("the meeting has no id"))?
            .to_string_lossy();
        let claim = InProgress::claim(&id)
            .ok_or_else(|| eyre!("the meeting is busy writing notes or edits"))?;
        Ok(Self::claimed(directory, claim))
    }

    pub(super) fn claimed(directory: &'a Path, claim: InProgress) -> Self {
        Self {
            directory,
            _claim: claim,
        }
    }

    pub(super) fn update<T>(
        &mut self,
        edit: impl FnOnce(&mut MeetingManifest) -> Result<T>,
    ) -> Result<(MeetingManifest, T)> {
        let mut manifest = read_manifest(self.directory)?;
        #[cfg(test)]
        tests::publication_window(self.directory, tests::Window::Read);
        let result = edit(&mut manifest)?;
        #[cfg(test)]
        tests::publication_window(self.directory, tests::Window::Publish);
        write_manifest(self.directory, &manifest)?;
        Ok((manifest, result))
    }
}

pub(super) fn rename(
    directory: &Path,
    title: &str,
    descriptive_filenames: bool,
) -> Result<MeetingManifest> {
    if normalized_title(title).is_empty() {
        return Err(eyre!("The title is empty"));
    }
    // This short-lived claim also permits the existing active-recording
    // rename path; it is never held for the lifetime of the recording.
    rename_claimed(
        MeetingWrite::claim(directory)?,
        title,
        descriptive_filenames,
    )
}

/// A title as it is saved: runs of whitespace become one space, trimmed.
pub(super) fn normalized_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `rename` under a claim the caller already holds, so it can check the
/// meeting between claiming and writing (`hex meetings rename`).
pub(super) fn rename_claimed(
    mut write: MeetingWrite<'_>,
    title: &str,
    descriptive_filenames: bool,
) -> Result<MeetingManifest> {
    let title = normalized_title(title);
    if title.is_empty() {
        return Err(eyre!("The title is empty"));
    }
    let (manifest, markdown_error) = write.update(|manifest| {
        let previous = super::display_title(manifest);
        manifest.title = title;
        super::set_title_source(manifest, super::TitleSource::User);
        let next = super::display_title(manifest);
        let mut markdown_error = None;
        let title_changed = previous != next;
        let mut heading_updated = title_changed;
        if heading_updated
            && let Some(path) = manifest.export_path.clone().filter(|path| path.exists())
            && let Err(error) = super::notes::retitle_markdown_file(
                &path,
                &mut manifest.export_sha256,
                &previous,
                &next,
            )
        {
            tracing::warn!(%error, meeting = manifest.id, "could not retitle meeting Markdown");
            markdown_error = Some(error);
            heading_updated = false;
        }
        // A failed heading update keeps the old name, as notes do. The
        // separate notes file follows independently while still Hex-owned.
        if heading_updated {
            super::export_name::rename_to_title(manifest, &next, descriptive_filenames);
        }
        if title_changed {
            super::notes_file::refresh(manifest);
        }
        Ok(markdown_error)
    })?;
    match markdown_error {
        Some(error) => Err(error.wrap_err("Renamed, but the Markdown heading was not updated")),
        None => Ok(manifest),
    }
}

#[cfg(test)]
mod tests;
