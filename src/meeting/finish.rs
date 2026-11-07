//! The final pass of a meeting whose audio is complete: its transcript, the
//! user's Markdown copy, and its finished event, or `final.pending` recovery
//! when Hex is quitting.

use std::fs;
use std::path::Path;

use color_eyre::eyre::Result;

use super::{FINAL_PENDING_FILE, MeetingManifest, MeetingStatus, transcribe, write_manifest};

/// Transcribes and publishes a meeting whose audio is complete, with the
/// folder's lock held. A failure while quitting keeps `final.pending` and
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
            super::finished_events::publish(&manifest.id);
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
