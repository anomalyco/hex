//! Publish the transcript before starting optional notes. Even an immediate
//! notes result can then only update an existing meeting, never race its show.
use super::{ControllerEvent, SyncSender, meeting};

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
