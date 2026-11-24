//! Deleting selected transcript lines from a finished meeting. One path for
//! the Meetings pane on every OS: the final NDJSON and Markdown are rewritten
//! atomically, rolling-draft entries that match a deleted key are dropped, and
//! the user-facing Markdown copy is refreshed only while Hex still wrote it.
//!
//! A transcript edit clears what was generated *from* the old lines: the
//! notes title and summary, the provisional title, and the chapters. Their
//! stored text could mention a deleted line, and they feed the shown title,
//! chapter sections, search, and MCP. Regenerating the summary reads the
//! edited `transcript.ndjson`, so it starts from what is left.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use color_eyre::eyre::Result;

use super::delete::{DeleteRefusal, plausible_id, refusal_for};
use super::notes::InProgress;
use super::transcript::{ROLLING_FILE, RollingLine};
use super::{
    FINAL_PENDING_FILE, MeetingStatus, TranscriptEntry, TranscriptEntryKey, display_title,
    final_transcript_exists, meeting_is_active, notes_file, read_manifest, root, set_owner_only,
    sync_directory, write_manifest,
};

/// Why transcript lines could not be deleted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditRefusal {
    /// No meeting has that id.
    Unknown,
    /// The transcript edit lab is off.
    Disabled,
    /// The meeting is still recording.
    Recording,
    /// It is still transcribing.
    Transcribing,
    /// Hex is writing its notes.
    Notes,
    /// It is waiting for the final transcript of a run Hex quit during.
    FinalPending,
    /// Its final transcript is not published.
    NotFinished,
    /// Another Hex holds its folder.
    Busy,
    /// The request named no lines.
    NoLines,
}

impl EditRefusal {
    /// What the person reads in the Meetings pane.
    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "No meeting has that id",
            Self::Disabled => "Labs › Delete transcript lines is off",
            Self::Recording => "Stop the recording before deleting lines",
            Self::Transcribing => "Wait for the transcript to finish before deleting lines",
            Self::Notes => "Wait for the notes to finish before deleting lines",
            Self::FinalPending => "Wait for its final transcript before deleting lines",
            Self::NotFinished => "Only a finished transcript can have lines deleted",
            Self::Busy => "Hex is still working on this meeting",
            Self::NoLines => "Choose the lines to delete",
        }
    }
}

/// Why an edit didn't happen: a refusal, or a write that failed.
#[derive(Debug)]
pub enum EditError {
    Refused(EditRefusal),
    Failed(color_eyre::Report),
}

impl EditError {
    pub fn message(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.message().into(),
            Self::Failed(error) => format!("couldn't delete the lines: {error:#}"),
        }
    }
}

impl From<EditRefusal> for EditError {
    fn from(refusal: EditRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<DeleteRefusal> for EditRefusal {
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

impl std::fmt::Display for EditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for EditError {}

/// Deletes the transcript lines identified by `keys` from finished meeting
/// `id`, returning how many lines went. Holds the notes claim, so a summary
/// can't race the rewrite, and refuses while the meeting records,
/// transcribes, waits for its final transcript, or writes notes.
pub fn delete_entries(id: &str, keys: &[TranscriptEntryKey]) -> Result<usize, EditError> {
    if !crate::labs::transcript_edits() {
        return Err(EditRefusal::Disabled.into());
    }
    delete_in(&root().map_err(EditError::Failed)?, id, keys)
}

/// `delete_entries` under an explicit meetings root, for tests and the
/// controller.
pub(super) fn delete_in(
    root: &Path,
    id: &str,
    keys: &[TranscriptEntryKey],
) -> Result<usize, EditError> {
    if keys.is_empty() {
        return Err(EditRefusal::NoLines.into());
    }
    if !plausible_id(id) {
        return Err(EditRefusal::Unknown.into());
    }
    let directory = root.join(id);
    if !directory.join("manifest.json").is_file() {
        return Err(EditRefusal::Unknown.into());
    }
    // The claim comes first: a summary that finished between reading the
    // manifest and claiming it would otherwise be lost to the manifest
    // written below, with a stale export hash and its fields gone.
    let _claim = InProgress::claim(id).ok_or(EditRefusal::Notes)?;
    let mut manifest = read_manifest(&directory).map_err(EditError::Failed)?;
    match meeting_is_active(&directory) {
        Some(false) => {}
        Some(true) => return Err(EditRefusal::from(refusal_for(manifest.status)).into()),
        None => return Err(EditRefusal::Busy.into()),
    }
    if manifest.status != MeetingStatus::Complete || !final_transcript_exists(&directory) {
        return Err(EditRefusal::NotFinished.into());
    }
    if directory.join(FINAL_PENDING_FILE).exists() {
        return Err(EditRefusal::FinalPending.into());
    }

    let mut entries = super::read_ndjson::<TranscriptEntry>(&directory.join("transcript.ndjson"))
        .map_err(EditError::Failed)?;
    let before = entries.len();
    entries.retain(|entry| !keys.iter().any(|key| key.matches(entry)));
    let removed = before - entries.len();
    if removed == 0 {
        return Ok(0);
    }

    let previous_title = display_title(&manifest);
    // These were written from the old transcript; none may leak back through
    // the shown title, chapters, search, MCP, or a regenerated summary.
    manifest.notes = None;
    manifest.notes_error = None;
    manifest.provisional_title = None;
    manifest.chapters.clear();
    manifest.chapters_refined = false;

    // The cleared manifest goes down first: a crash before the transcript
    // rewrite leaves notes that mention nothing the transcript no longer has,
    // never notes that describe deleted lines.
    write_manifest(&directory, &manifest).map_err(EditError::Failed)?;
    remove_from_rolling(&directory, keys).map_err(EditError::Failed)?;
    super::publish_transcript_files(&directory, &display_title(&manifest), &entries)
        .map_err(EditError::Failed)?;

    let had_export = manifest
        .export_path
        .as_ref()
        .is_some_and(|path| path.exists());
    match super::notes::rewrite_export_transcript(&mut manifest, &entries) {
        Ok(()) => {
            if had_export {
                manifest.export_error = None;
            }
        }
        Err(error) => {
            tracing::warn!(%error, meeting = manifest.id, "could not update meeting Markdown");
            manifest.export_error = Some(error.to_string());
        }
    }
    let title = display_title(&manifest);
    if title != previous_title {
        super::export_name::rename_to_title(
            &mut manifest,
            &title,
            crate::app_settings::meeting_descriptive_filenames(),
        );
    }
    notes_file::refresh(&mut manifest);
    write_manifest(&directory, &manifest).map_err(EditError::Failed)?;
    tracing::info!(meeting = manifest.id, removed, "deleted transcript lines");
    Ok(removed)
}

/// Drops the rolling draft's entries whose key matches a deleted line, keeping
/// its coverage line. The draft chunks the same audio differently, so an entry
/// it keeps can still hold words from a deleted final line; no reader shows
/// the draft while the final transcript exists.
fn remove_from_rolling(directory: &Path, keys: &[TranscriptEntryKey]) -> Result<()> {
    let path = directory.join(ROLLING_FILE);
    if !path.exists() {
        return Ok(());
    }
    let lines = super::read_ndjson::<RollingLine>(&path)?;
    let temporary = directory.join(format!("{ROLLING_FILE}.tmp"));
    let mut file = BufWriter::new(File::create(&temporary)?);
    set_owner_only(&temporary, false)?;
    for line in lines {
        if let RollingLine::Entry(entry) = &line
            && keys.iter().any(|key| key.matches(entry))
        {
            continue;
        }
        serde_json::to_writer(&mut file, &line)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    file.get_ref().sync_all()?;
    fs::rename(&temporary, &path)?;
    sync_directory(directory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::meeting::MeetingManifest;
    use crate::meeting::query::Library;

    fn fixture_root(name: &str, id: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hex-meeting-edit-{name}-{}-{}",
            std::process::id(),
            crate::desktop::events::now_ms()
        ));
        fs::create_dir_all(&root).unwrap();
        // Each test gets its own id: the notes claim is per id, and tests in
        // one process run in parallel.
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/meetings/1790000000000-1001");
        let directory = root.join(id);
        fs::create_dir_all(&directory).unwrap();
        for file in ["manifest.json", "transcript.ndjson", "transcript.md"] {
            fs::copy(source.join(file), directory.join(file)).unwrap();
        }
        // The fixture predates retention; keep it current so the query's
        // retention admission never hides it.
        let mut manifest: MeetingManifest =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        manifest.id = id.into();
        manifest.created_at_ms = crate::desktop::events::now_ms();
        manifest.ended_at_ms = Some(manifest.created_at_ms + 65_000);
        manifest.duration_ms = Some(65_000);
        write_manifest(&directory, &manifest).unwrap();
        root
    }

    fn lines(directory: &Path) -> Vec<TranscriptEntry> {
        super::super::read_ndjson(&directory.join("transcript.ndjson")).unwrap()
    }

    #[test]
    fn deleting_turns_rewrites_the_transcript_and_reopens_without_them() {
        let id = "1790000000100-2001";
        let root = fixture_root("storage", id);
        let directory = root.join(id);
        fs::write(directory.join("system.wav"), b"audio stays").unwrap();
        let keys = [
            TranscriptEntryKey {
                source: crate::meeting::MeetingSource::System,
                start_ms: 0,
                end_ms: 4_000,
            },
            TranscriptEntryKey {
                source: crate::meeting::MeetingSource::Microphone,
                start_ms: 32_000,
                end_ms: 36_000,
            },
        ];
        let removed = delete_in(&root, id, &keys).unwrap();
        assert_eq!(removed, 2);

        let remaining = lines(&directory);
        assert_eq!(remaining.len(), 5);
        assert!(
            remaining
                .iter()
                .all(|entry| !entry.text.contains("thanks for joining"))
        );
        assert!(
            remaining
                .iter()
                .all(|entry| !entry.text.contains("write the spec"))
        );
        assert!(
            remaining
                .iter()
                .any(|entry| entry.text.contains("scheduled sessions"))
        );

        let markdown = fs::read_to_string(directory.join("transcript.md")).unwrap();
        assert!(!markdown.contains("thanks for joining"));
        assert!(!markdown.contains("write the spec"));
        assert!(markdown.contains("scheduled sessions"));
        let ndjson = fs::read_to_string(directory.join("transcript.ndjson")).unwrap();
        assert!(!ndjson.contains("thanks for joining"));
        assert!(!ndjson.contains("write the spec"));

        // Text generated from the old lines goes with them.
        let manifest = read_manifest(&directory).unwrap();
        assert!(manifest.notes.is_none());
        assert!(manifest.notes_error.is_none());
        assert!(manifest.chapters.is_empty());
        assert!(manifest.provisional_title.is_none());

        // Everything that reopens the meeting reads only what is left.
        let library = Library::new(&root);
        let meeting = library.meeting(id).unwrap();
        assert_eq!(meeting.entries.len(), 5);
        assert!(
            meeting
                .entries
                .iter()
                .all(|entry| !entry.text.contains("thanks for joining"))
        );
        assert!(meeting.manifest.notes.is_none());
        assert_eq!(
            fs::read(directory.join("system.wav")).unwrap(),
            b"audio stays",
            "audio is never touched"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_edit_waits_for_the_notes_claim() {
        let id = "1790000000400-2004";
        let root = fixture_root("notes-claim", id);
        let keys = [TranscriptEntryKey {
            source: crate::meeting::MeetingSource::System,
            start_ms: 0,
            end_ms: 4_000,
        }];
        let claim = InProgress::claim(id).expect("no other writer holds the meeting");
        assert!(matches!(
            delete_in(&root, id, &keys),
            Err(EditError::Refused(EditRefusal::Notes))
        ));
        drop(claim);
        assert_eq!(delete_in(&root, id, &keys).unwrap(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_summary_prompt_reads_the_edited_transcript_from_disk() {
        let id = "1790000000200-2002";
        let root = fixture_root("summary", id);
        let keys = [
            TranscriptEntryKey {
                source: crate::meeting::MeetingSource::System,
                start_ms: 0,
                end_ms: 4_000,
            },
            TranscriptEntryKey {
                source: crate::meeting::MeetingSource::Microphone,
                start_ms: 32_000,
                end_ms: 36_000,
            },
        ];
        delete_in(&root, id, &keys).unwrap();

        // A fresh read through the same helper `summarize_with` uses before
        // generating notes.
        let entries = crate::meeting::notes::summary_entries(&root.join(id)).unwrap();
        let mut prompts = Vec::new();
        let summary = crate::meeting::notes::summarize_with_calendar(
            &entries,
            None,
            &[],
            12_000,
            std::time::Instant::now() + std::time::Duration::from_secs(60),
            |_, prompt, _| {
                prompts.push(prompt.to_string());
                Ok(crate::meeting::generator::GeneratedNotes {
                    title: "Roadmap review".into(),
                    summary: "They kept the schedule.".into(),
                    ..Default::default()
                })
            },
        )
        .unwrap()
        .expect("the transcript still has lines to summarize");
        assert_eq!(summary.calls, 1);
        assert!(!prompts[0].contains("thanks for joining"), "{}", prompts[0]);
        assert!(!prompts[0].contains("write the spec"), "{}", prompts[0]);
        assert!(prompts[0].contains("scheduled sessions"), "{}", prompts[0]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_export_and_rolling_draft_lose_the_lines_but_a_user_edit_stays() {
        let id = "1790000000300-2003";
        let root = fixture_root("export", id);
        let directory = root.join(id);
        let mut manifest: MeetingManifest =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        let export = directory.join("Weekly sync.md");
        let entries = lines(&directory);
        fs::write(
            &export,
            super::super::markdown::render_document(&manifest, &entries),
        )
        .unwrap();
        manifest.export_path = Some(export.clone());
        manifest.export_sha256 = Some(super::super::file_sha256(&export).unwrap());
        write_manifest(&directory, &manifest).unwrap();
        // A rolling draft that still has the same first line.
        let first = &entries[0];
        let coverage = r#"{"covered_ms":{"system":95000,"microphone":95000}}"#;
        fs::write(
            directory.join(ROLLING_FILE),
            format!(
                "{coverage}\n{}\n{}\n",
                serde_json::to_string(first).unwrap(),
                serde_json::to_string(&entries[1]).unwrap()
            ),
        )
        .unwrap();

        let key = TranscriptEntryKey::of(first);
        delete_in(&root, id, &[key]).unwrap();
        let written = fs::read_to_string(&export).unwrap();
        assert!(!written.contains("thanks for joining"), "{written}");
        assert!(written.contains("scheduled sessions"), "{written}");
        let rolling = fs::read_to_string(directory.join(ROLLING_FILE)).unwrap();
        assert!(!rolling.contains("thanks for joining"), "{rolling}");
        assert!(rolling.contains("covered_ms"), "{rolling}");
        assert!(rolling.contains("walk us through"), "{rolling}");

        // A file the user edited is theirs: a later edit leaves it whole.
        fs::write(&export, "# My notes\n\nKeep this.\n").unwrap();
        let second = TranscriptEntryKey::of(&entries[2]);
        delete_in(&root, id, &[second]).unwrap();
        assert_eq!(
            fs::read_to_string(&export).unwrap(),
            "# My notes\n\nKeep this.\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
