//! Deleting a meeting: the Meetings pane's Delete, and `hex meetings delete`
//! through the running Hex's control host. One path, so both move the same
//! files to the Trash and refuse for the same reasons.

use std::path::Path;

use color_eyre::eyre::{Result, eyre};

use super::{
    FINAL_PENDING_FILE, MeetingStatus, display_title, exported_markdown, meeting_is_active,
    notes_file, notes_in_progress, os, read_manifest, root,
};

/// Why a meeting can't be deleted, as `hex meetings delete` reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteRefusal {
    /// No meeting has that id.
    Unknown,
    /// It is still recording.
    Recording,
    /// It is still transcribing.
    Transcribing,
    /// Hex is writing its notes.
    Notes,
    /// It is waiting for the final transcript of a run Hex quit during.
    FinalPending,
    /// Another Hex holds its folder.
    Busy,
}

impl DeleteRefusal {
    /// The control error code, stable for `hex meetings delete --json`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown => "not-found",
            Self::Recording => "meeting-recording",
            Self::Transcribing => "meeting-transcribing",
            Self::Notes => "meeting-notes",
            Self::FinalPending => "meeting-final-pending",
            Self::Busy => "busy",
        }
    }

    /// What the person reads, in the pane and on the command line.
    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "No meeting has that id",
            Self::Recording => "Stop the recording before deleting it",
            Self::Transcribing => "Wait for the transcript to finish before deleting it",
            Self::Notes => "Wait for the notes to finish before deleting",
            Self::FinalPending => "Wait for its final transcript before deleting it",
            Self::Busy => "Hex is still working on this meeting",
        }
    }
}

/// Why a delete didn't happen: a refusal, or a move that failed.
#[derive(Debug)]
pub enum DeleteError {
    Refused(DeleteRefusal),
    Failed(color_eyre::Report),
}

impl DeleteError {
    /// The control error code, stable for `hex meetings delete --json`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Failed(_) => "failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.message().into(),
            Self::Failed(error) => format!("couldn't delete the meeting: {error:#}"),
        }
    }
}

impl From<DeleteRefusal> for DeleteError {
    fn from(refusal: DeleteRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl std::fmt::Display for DeleteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for DeleteError {}

/// A meeting that went to the Trash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedMeeting {
    pub id: String,
    pub title: String,
    /// Whether an exported Markdown copy went with it.
    pub markdown: bool,
}

/// Deletes the finished meeting with exactly this id, as the Meetings pane's
/// Delete does: its exported Markdown, its tracked notes file, then its
/// recording folder, all to the Trash (never `rm`). Refuses while it records,
/// transcribes, writes notes, or waits for its final transcript.
pub fn delete(id: &str) -> Result<TrashedMeeting, DeleteError> {
    let root = root().map_err(DeleteError::Failed)?;
    delete_in(&root, id, os::move_to_trash)
}

/// Moves a finished meeting to the Trash with its Markdown copy; the pane's
/// Delete, whose refusal reads as a message.
pub fn trash(id: &str) -> Result<()> {
    let root = root()?;
    delete_in(&root, id, os::move_to_trash)
        .map(|_| ())
        .map_err(|error| eyre!(error.message()))
}

/// `delete` against `root`, with the id checked as one folder name under it.
pub(super) fn delete_in(
    root: &Path,
    id: &str,
    move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<TrashedMeeting, DeleteError> {
    if !plausible_id(id) {
        return Err(DeleteRefusal::Unknown.into());
    }
    let directory = root.join(id);
    if !directory.join("manifest.json").is_file() {
        return Err(DeleteRefusal::Unknown.into());
    }
    delete_meeting(&directory, move_to_trash)
}

/// An id comes from `hex meetings list --json` and names one folder directly
/// under the meetings root: no separators, no `.` or `..`, no empty name, so
/// a caller can't point deletion anywhere else.
pub(super) fn plausible_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\']) && !id.contains('\0')
}

pub(super) fn delete_meeting(
    directory: &Path,
    mut move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<TrashedMeeting, DeleteError> {
    let manifest = read_manifest(directory).map_err(DeleteError::Failed)?;
    match meeting_is_active(directory) {
        Some(false) => {}
        Some(true) => return Err(refusal_for(manifest.status).into()),
        None => return Err(DeleteRefusal::Busy.into()),
    }
    if notes_in_progress(&manifest.id) {
        return Err(DeleteRefusal::Notes.into());
    }
    if directory.join(FINAL_PENDING_FILE).exists() {
        return Err(DeleteRefusal::FinalPending.into());
    }
    let markdown = exported_markdown(&manifest);
    if let Some(path) = &markdown {
        move_to_trash(path).map_err(DeleteError::Failed)?;
    }
    if let Some(path) = notes_file::tracked(&manifest) {
        move_to_trash(&path).map_err(DeleteError::Failed)?;
    }
    move_to_trash(directory).map_err(DeleteError::Failed)?;
    Ok(TrashedMeeting {
        id: manifest.id.clone(),
        title: display_title(&manifest),
        markdown: markdown.is_some(),
    })
}

/// An active folder refuses for what its manifest says it is doing.
pub(super) fn refusal_for(status: MeetingStatus) -> DeleteRefusal {
    match status {
        MeetingStatus::Starting | MeetingStatus::Recording => DeleteRefusal::Recording,
        MeetingStatus::Transcribing => DeleteRefusal::Transcribing,
        MeetingStatus::Complete | MeetingStatus::Interrupted | MeetingStatus::Failed => {
            DeleteRefusal::Busy
        }
    }
}
