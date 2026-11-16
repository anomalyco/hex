//! Publish the transcript before starting optional notes. Even an immediate
//! notes result can then only update an existing meeting, never race its show.
use super::{ControllerEvent, SyncSender, meeting};

pub(super) fn finished(manifest: meeting::MeetingManifest, events: &SyncSender<ControllerEvent>) {
    let notes = (manifest.status == meeting::MeetingStatus::Complete && meeting::notes_wanted())
        .then(|| manifest.id.clone());
    let _ = events.send(ControllerEvent::Finished(Box::new(manifest)));
    if let Some(id) = notes {
        start_notes(id, events);
    }
}

pub(super) fn start_notes(id: String, events: &SyncSender<ControllerEvent>) {
    let finished = events.clone();
    let pending = id.clone();
    if meeting::spawn_summary(id.clone(), move |result| {
        let _ = finished.send(match result {
            Ok(manifest) => ControllerEvent::NotesFinished(Box::new(manifest)),
            Err(_) => ControllerEvent::NotesProgress(pending),
        });
    }) {
        let _ = events.send(ControllerEvent::NotesProgress(id));
    }
}
