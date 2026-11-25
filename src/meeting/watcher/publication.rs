//! Publish the transcript before starting optional notes. Even an immediate
//! notes result can then only update an existing meeting, never race its show.
use std::sync::atomic::Ordering;

use super::{ActiveRecording, ControllerEvent, MeetingDetector, SyncSender, meeting};

impl ActiveRecording {
    /// Discard from the capsule's ✕: ends capture and skips the final pass.
    /// Only a recording still capturing can skip it.
    pub(super) fn throw_away(
        &self,
        detector: &mut MeetingDetector,
        events: &SyncSender<ControllerEvent>,
    ) {
        if self.stop.load(Ordering::Relaxed) {
            return;
        }
        self.discard.store(true, Ordering::Relaxed);
        tracing::info!("meeting recording discarded from its capsule");
        self.end(detector, events);
    }
}

/// A recording discarded from its capsule: its folder goes to the Trash
/// (no Markdown was written yet), and nothing offers a finished meeting.
pub(super) fn discarded(manifest: meeting::MeetingManifest, events: &SyncSender<ControllerEvent>) {
    if let Err(error) = meeting::trash(&manifest.id) {
        tracing::warn!(%error, meeting = manifest.id, "could not move a discarded meeting to the Trash");
    }
    let _ = events.send(ControllerEvent::Discarded(manifest.id));
}

pub(super) fn finished(manifest: meeting::MeetingManifest, events: &SyncSender<ControllerEvent>) {
    // A completed pass with no speech leaves nothing to keep: its files go to
    // the Trash and no finished capsule or notes offer a meeting that is gone.
    if !meeting::keep_or_discard(&manifest) {
        let _ = events.send(ControllerEvent::Discarded(manifest.id));
        return;
    }
    let notes = (manifest.status == meeting::MeetingStatus::Complete && meeting::notes_wanted())
        .then(|| manifest.id.clone());
    let _ = events.send(ControllerEvent::Finished(Box::new(manifest)));
    if let Some(id) = notes {
        super::start_notes(id, events);
    }
}
