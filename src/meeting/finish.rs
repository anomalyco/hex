//! The final pass of a meeting whose audio is complete: its transcript and
//! the user's Markdown copy, or `final.pending` recovery when Hex is
//! quitting. The caller publishes its finished event once it is kept.

use std::fs;
use std::path::Path;

use color_eyre::eyre::Result;

use super::{
    FINAL_PENDING_FILE, MeetingManifest, MeetingStatus, TranscriptEntry, transcribe, write_manifest,
};

/// Moves a meeting whose final pass succeeded with no speech to the Trash,
/// through the pane's delete path, with its exported Markdown. Returns
/// whether it was discarded: a Failed, Interrupted, or unfinished meeting, or
/// one with any transcript entry, is never touched.
pub(crate) fn discard_if_empty(manifest: &MeetingManifest) -> Result<bool> {
    discard_if_empty_in(&super::root()?, manifest, super::os::move_to_trash)
}

/// [`discard_if_empty`] against `root`, moving through `move_to_trash` as
/// [`super::delete::delete_meeting`] does. The folder's lock must be free:
/// callers have already joined the recorder or the recovery worker.
pub(super) fn discard_if_empty_in(
    root: &Path,
    manifest: &MeetingManifest,
    move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<bool> {
    if manifest.status != MeetingStatus::Complete || manifest.error.is_some() {
        return Ok(false);
    }
    // Echo suppression removed utterances from this meeting: its silence is
    // not proof that nobody spoke, so keep it even with no transcript entries.
    if manifest.echo_suppressed_utterances > 0 {
        return Ok(false);
    }
    let directory = root.join(&manifest.id);
    if !super::final_transcript_exists(&directory) {
        return Ok(false);
    }
    let entries = super::read_ndjson::<TranscriptEntry>(&directory.join("transcript.ndjson"))?;
    if !entries.is_empty() {
        return Ok(false);
    }
    let trashed = super::delete::delete_in(root, &manifest.id, move_to_trash)?;
    tracing::info!(
        meeting = trashed.id,
        title = trashed.title,
        markdown = trashed.markdown,
        "discarded a meeting with no speech; moved it to the Trash"
    );
    Ok(true)
}

/// Transcribes a meeting whose audio is complete, with the folder's lock
/// held. A failure while quitting keeps `final.pending` and
/// leaves it Interrupted for the next launch; any other failure keeps the
/// meeting Interrupted with its error.
pub(crate) fn finish_transcription(
    directory: &Path,
    mut manifest: MeetingManifest,
    selection: &crate::transcription::models::TranscriptionSelection,
) -> Result<MeetingManifest> {
    match transcribe(directory, &mut manifest, selection, None) {
        Ok(()) => {
            manifest.status = MeetingStatus::Complete;
            manifest.error = None;
            write_manifest(directory, &manifest)?;
            Ok(manifest)
        }
        Err(error) if crate::desktop::process_exit::quitting() => {
            // Quitting cut the final pass short: like quitting while
            // recording, the transcript finishes at the next launch.
            manifest.status = MeetingStatus::Interrupted;
            manifest.error = None;
            fs::write(directory.join(FINAL_PENDING_FILE), b"")?;
            write_manifest(directory, &manifest)?;
            tracing::info!(
                meeting = manifest.id,
                %error,
                "Hex quit during final transcription; the transcript finishes at the next launch"
            );
            Ok(manifest)
        }
        Err(error) => {
            manifest.status = MeetingStatus::Interrupted;
            manifest.error = Some(error.to_string());
            write_manifest(directory, &manifest)?;
            let _ = fs::remove_file(directory.join(FINAL_PENDING_FILE));
            Err(error)
        }
    }
}
