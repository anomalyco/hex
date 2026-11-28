//! `hex meetings rename`: the Meetings pane's Rename, through the running
//! Hex's control host. It saves the title the pane's Rename saves, and it
//! offers Rename only when the pane does: for a finished meeting, never while
//! it records, transcribes, writes notes, or waits for its final transcript.

use std::path::Path;

use super::delete::{DeleteRefusal, plausible_id, refusal_for};
use super::notes::InProgress;
use super::write::{MeetingWrite, normalized_title, rename_claimed};
use super::{FINAL_PENDING_FILE, MeetingManifest, meeting_is_active, read_manifest, root};

/// Why a meeting wasn't renamed, as `hex meetings rename` reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenameRefusal {
    /// No meeting has that id.
    Unknown,
    /// The new title is empty or only whitespace.
    EmptyTitle,
    /// It is still recording.
    Recording,
    /// It is still transcribing.
    Transcribing,
    /// Hex is writing its notes or a transcript edit.
    Notes,
    /// It is waiting for the final transcript of a run Hex quit during.
    FinalPending,
    /// Another Hex holds its folder.
    Busy,
}

impl RenameRefusal {
    /// The control error code, stable for `hex meetings rename --json`; the
    /// same codes `hex meetings delete` uses for the same reasons.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown => "not-found",
            Self::EmptyTitle => "invalid-value",
            Self::Recording => "meeting-recording",
            Self::Transcribing => "meeting-transcribing",
            Self::Notes => "meeting-notes",
            Self::FinalPending => "meeting-final-pending",
            Self::Busy => "busy",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "No meeting has that id",
            Self::EmptyTitle => "The title is empty",
            Self::Recording => "Stop the recording before renaming it",
            Self::Transcribing => "Wait for the transcript to finish before renaming it",
            Self::Notes => "Wait for the notes to finish before renaming",
            Self::FinalPending => "Wait for its final transcript before renaming it",
            Self::Busy => "Hex is still working on this meeting",
        }
    }
}

impl From<DeleteRefusal> for RenameRefusal {
    fn from(refusal: DeleteRefusal) -> Self {
        match refusal {
            DeleteRefusal::Unknown => Self::Unknown,
            DeleteRefusal::Recording => Self::Recording,
            DeleteRefusal::Transcribing => Self::Transcribing,
            DeleteRefusal::Notes => Self::Notes,
            DeleteRefusal::FinalPending => Self::FinalPending,
            DeleteRefusal::Busy => Self::Busy,
        }
    }
}

/// Why a rename didn't happen: a refusal, or a write that failed.
#[derive(Debug)]
pub enum RenameError {
    Refused(RenameRefusal),
    Failed(color_eyre::Report),
}

impl RenameError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Failed(_) => "failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.message().into(),
            Self::Failed(error) => format!("couldn't rename the meeting: {error:#}"),
        }
    }
}

impl From<RenameRefusal> for RenameError {
    fn from(refusal: RenameRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for RenameError {}

/// Renames the finished meeting with exactly this id, as the Meetings pane's
/// Rename does. Nothing is written when it is refused.
pub fn rename_finished(id: &str, title: &str) -> Result<MeetingManifest, RenameError> {
    let root = root().map_err(RenameError::Failed)?;
    rename_in(
        &root,
        id,
        title,
        crate::app_settings::meeting_descriptive_filenames(),
    )
}

pub(super) fn rename_in(
    root: &Path,
    id: &str,
    title: &str,
    descriptive_filenames: bool,
) -> Result<MeetingManifest, RenameError> {
    if !plausible_id(id) {
        return Err(RenameRefusal::Unknown.into());
    }
    let directory = root.join(id);
    if !directory.join("manifest.json").is_file() {
        return Err(RenameRefusal::Unknown.into());
    }
    if normalized_title(title).is_empty() {
        return Err(RenameRefusal::EmptyTitle.into());
    }
    // Claim before reading, so notes or an edit that finish meanwhile can't
    // be lost to the manifest this writes.
    let claim = InProgress::claim(id).ok_or(RenameRefusal::Notes)?;
    let manifest = read_manifest(&directory).map_err(RenameError::Failed)?;
    match meeting_is_active(&directory) {
        Some(false) => {}
        Some(true) => return Err(RenameRefusal::from(refusal_for(manifest.status)).into()),
        None => return Err(RenameRefusal::Busy.into()),
    }
    if directory.join(FINAL_PENDING_FILE).exists() {
        return Err(RenameRefusal::FinalPending.into());
    }
    rename_claimed(
        MeetingWrite::claimed(&directory, claim),
        title,
        descriptive_filenames,
    )
    .map_err(RenameError::Failed)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::meeting::{ActiveMeeting, MeetingStatus, display_title, write_manifest};

    const ID: &str = "1760000000000-48";

    fn meeting(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "hex-retitle-{name}-{}-{}",
            std::process::id(),
            crate::desktop::events::now_ms()
        ));
        let directory = base.join(ID);
        fs::create_dir_all(&directory).unwrap();
        let manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": ID,
            "title": "Design sync",
            "status": "complete",
            "created_at_ms": 1_760_000_000_000_u64,
            "ended_at_ms": 1_760_000_060_000_u64,
            "duration_ms": 60_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null,
        }))
        .unwrap();
        write_manifest(&directory, &manifest).unwrap();
        (base, directory)
    }

    fn title(directory: &Path) -> String {
        display_title(&read_manifest(directory).unwrap())
    }

    #[test]
    fn renaming_by_id_saves_the_title_and_refuses_what_the_pane_would() {
        let (base, directory) = meeting("cli");
        for id in ["", "..", "a/b", "1760000000000-2"] {
            let refused = rename_in(&base, id, "Design review", false).unwrap_err();
            assert_eq!(refused.code(), "not-found", "{id:?}");
        }
        let refused = rename_in(&base, ID, "  \n ", false).unwrap_err();
        assert_eq!(refused.code(), "invalid-value");

        let mut manifest = read_manifest(&directory).unwrap();
        manifest.status = MeetingStatus::Recording;
        write_manifest(&directory, &manifest).unwrap();
        let active = ActiveMeeting::acquire(&directory).unwrap();
        let refused = rename_in(&base, ID, "Design review", false).unwrap_err();
        assert_eq!(refused.code(), "meeting-recording");
        drop(active);

        manifest.status = MeetingStatus::Interrupted;
        write_manifest(&directory, &manifest).unwrap();
        fs::write(directory.join(FINAL_PENDING_FILE), b"").unwrap();
        let refused = rename_in(&base, ID, "Design review", false).unwrap_err();
        assert_eq!(refused.code(), "meeting-final-pending");
        fs::remove_file(directory.join(FINAL_PENDING_FILE)).unwrap();

        let claim = InProgress::claim(ID).unwrap();
        let refused = rename_in(&base, ID, "Design review", false).unwrap_err();
        assert_eq!(refused.code(), "meeting-notes");
        drop(claim);
        assert_eq!(title(&directory), "Design sync");

        manifest.status = MeetingStatus::Complete;
        write_manifest(&directory, &manifest).unwrap();
        let renamed = rename_in(&base, ID, "  Design   review ", false).unwrap();
        assert_eq!(renamed.title, "Design review");
        assert_eq!(title(&directory), "Design review");
        fs::remove_dir_all(base).unwrap();
    }
}
