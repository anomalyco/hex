//! Meeting retention: how long finished meetings, and separately their WAV
//! tracks, are kept before moving to the Trash.

use std::path::Path;

use color_eyre::eyre::{Result, eyre};

use super::{
    FINAL_PENDING_FILE, MeetingManifest, MeetingStatus, exported_markdown, file_sha256, list,
    meeting_is_active, notes_in_progress, now_ms, os, pending_final_transcripts, read_manifest,
    root, write_manifest,
};
use crate::app_settings::MeetingRetention;

/// How long `manifest` is kept: the retention the user chose, else
/// [`MeetingRetention::DEFAULT`] for meetings recorded since it began, and
/// older meetings for good.
pub fn retention_for(
    manifest: &MeetingManifest,
    chosen: Option<MeetingRetention>,
) -> MeetingRetention {
    chosen.unwrap_or(if manifest.retention_applies {
        MeetingRetention::DEFAULT
    } else {
        MeetingRetention::Forever
    })
}

/// Whether a finished meeting outlived its retention, counted from when it
/// ended (or, with no end, started). A meeting that records or transcribes
/// never has.
pub fn expired(
    manifest: &MeetingManifest,
    status: MeetingStatus,
    chosen: Option<MeetingRetention>,
    now_ms: u64,
) -> bool {
    !status.is_active()
        && retention_for(manifest, chosen)
            .max_age_ms()
            .is_some_and(|max_age_ms| {
                let ended = manifest.ended_at_ms.unwrap_or(manifest.created_at_ms);
                now_ms.saturating_sub(ended) > max_age_ms
            })
}

/// Moves every expired meeting to the Trash: its folder, and its exported
/// Markdown while that is still the copy Hex wrote. One that is recording,
/// transcribing, writing notes, or waiting for its final transcript stays
/// for a later pass. Returns how many moved.
pub fn expire(chosen: Option<MeetingRetention>) -> Result<usize> {
    let meetings = list()?;
    let pending = pending_final_transcripts();
    let root = root()?;
    let mut moved = 0;
    for id in expired_meetings(&meetings, chosen, &pending, now_ms()) {
        match trash_expired(&root.join(id), os::move_to_trash) {
            Ok(()) => {
                moved += 1;
                tracing::info!(meeting = id, "moved an expired meeting to the Trash");
            }
            Err(error) => {
                tracing::info!(%error, meeting = id, "kept an expired meeting for now");
            }
        }
    }
    Ok(moved)
}

/// The expired meetings in `meetings`, leaving out `pending` ones.
pub(super) fn expired_meetings<'a>(
    meetings: &'a [MeetingManifest],
    chosen: Option<MeetingRetention>,
    pending: &[String],
    now_ms: u64,
) -> Vec<&'a str> {
    meetings
        .iter()
        .filter(|meeting| !pending.contains(&meeting.id))
        .filter(|meeting| expired(meeting, meeting.status, chosen, now_ms))
        .map(|meeting| meeting.id.as_str())
        .collect()
}

/// [`super::trash_meeting`] for expiry: an exported Markdown the user changed
/// is left where it is.
pub(super) fn trash_expired(
    directory: &Path,
    mut move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let manifest = read_manifest(directory)?;
    if meeting_is_active(directory) != Some(false) {
        return Err(eyre!("the meeting is recording"));
    }
    if notes_in_progress(&manifest.id) {
        return Err(eyre!("its notes are being written"));
    }
    if let Some(path) = exported_markdown(&manifest) {
        if manifest.export_sha256.is_some() && file_sha256(&path) == manifest.export_sha256 {
            move_to_trash(&path)?;
        } else {
            tracing::info!(
                meeting = manifest.id,
                "kept the meeting's Markdown, which changed since Hex wrote it"
            );
        }
    }
    if let Some(path) = super::notes_file::tracked(&manifest) {
        if manifest.notes_export_sha256.is_some()
            && file_sha256(&path) == manifest.notes_export_sha256
        {
            move_to_trash(&path)?;
        } else {
            tracing::info!(
                meeting = manifest.id,
                "kept the meeting's notes file, which changed since Hex wrote it"
            );
        }
    }
    move_to_trash(directory)
}

/// Whether a finished meeting's audio outlived a separate audio retention,
/// counted from when it ended (or, with no end, started). Only a Complete
/// meeting with its final transcript published and no audio removed yet can.
pub fn audio_expired(
    manifest: &MeetingManifest,
    status: MeetingStatus,
    chosen: Option<MeetingRetention>,
    now_ms: u64,
) -> bool {
    status == MeetingStatus::Complete
        && manifest.audio_removed_at_ms.is_none()
        && chosen
            .and_then(MeetingRetention::max_age_ms)
            .is_some_and(|max_age_ms| {
                let ended = manifest.ended_at_ms.unwrap_or(manifest.created_at_ms);
                now_ms.saturating_sub(ended) > max_age_ms
            })
}

/// Moves the WAV tracks of meetings past a separate audio retention to the
/// Trash, keeping their transcripts, notes, summary, and exported Markdown.
/// One that is not Complete, has a pending final transcript, is recording,
/// or is writing notes stays for a later pass. Returns how many lost audio.
pub fn expire_audio(chosen: Option<MeetingRetention>) -> Result<usize> {
    let Some(chosen) = chosen else {
        return Ok(0);
    };
    let meetings = list()?;
    let pending = pending_final_transcripts();
    let root = root()?;
    let mut removed = 0;
    for id in audio_expired_meetings(&meetings, chosen, &pending, now_ms()) {
        match trash_audio_tracks(&root.join(id), notes_in_progress, os::move_to_trash) {
            Ok(()) => {
                removed += 1;
                tracing::info!(
                    meeting = id,
                    "moved an expired meeting's audio to the Trash"
                );
            }
            Err(error) => {
                tracing::info!(%error, meeting = id, "kept an expired meeting's audio for now");
            }
        }
    }
    Ok(removed)
}

/// The meetings in `meetings` whose audio outlived `chosen`, leaving out
/// `pending` ones.
pub(super) fn audio_expired_meetings<'a>(
    meetings: &'a [MeetingManifest],
    chosen: MeetingRetention,
    pending: &[String],
    now_ms: u64,
) -> Vec<&'a str> {
    meetings
        .iter()
        .filter(|meeting| !pending.contains(&meeting.id))
        .filter(|meeting| audio_expired(meeting, meeting.status, Some(chosen), now_ms))
        .map(|meeting| meeting.id.as_str())
        .collect()
}

/// Moves only a finished meeting's WAV tracks to the Trash and records it in
/// the manifest; its text and Markdown stay.
pub(super) fn trash_audio_tracks(
    directory: &Path,
    notes_busy: impl Fn(&str) -> bool,
    mut move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let manifest = read_manifest(directory)?;
    if manifest.status != MeetingStatus::Complete {
        return Err(eyre!("the meeting is not finished"));
    }
    if directory.join(FINAL_PENDING_FILE).exists() {
        return Err(eyre!("its final transcript is still pending"));
    }
    if meeting_is_active(directory) != Some(false) {
        return Err(eyre!("the meeting is recording"));
    }
    if notes_busy(&manifest.id) {
        return Err(eyre!("its notes are being written"));
    }
    if manifest.audio_removed_at_ms.is_some() {
        return Ok(());
    }
    for file in [&manifest.system.file, &manifest.microphone.file] {
        let path = directory.join(file);
        if path.exists() {
            move_to_trash(&path)?;
        }
    }
    // Re-read so concurrent manifest updates are not lost.
    let mut manifest = read_manifest(directory)?;
    manifest.audio_removed_at_ms = Some(now_ms());
    write_manifest(directory, &manifest)
}
