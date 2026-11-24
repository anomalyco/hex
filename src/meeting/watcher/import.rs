//! Turning a chosen audio file into a finished meeting on its own worker
//! thread (macOS; Labs › Import audio). The controller starts one, polls it,
//! and reports its failure in the Meetings pane.
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::thread::JoinHandle;

use color_eyre::eyre::Result;

use super::{ActiveRecording, ControllerEvent, meeting, start_notes};

/// An audio file being transcribed on a worker thread.
pub(super) struct ImportWorker {
    pub(super) worker: JoinHandle<Result<meeting::MeetingManifest>>,
    /// The chosen file's name, for the message when it held no speech.
    pub(super) file: String,
}

/// Starts importing `path`, refusing while a recording or another import
/// runs. Failures reach the Meetings pane as an import error.
pub(super) fn start_import(
    recording: &mut Option<ActiveRecording>,
    importing: &mut Option<ImportWorker>,
    path: PathBuf,
    events: &SyncSender<ControllerEvent>,
) {
    if recording.is_some() {
        let _ = events.send(ControllerEvent::ImportFailed {
            error: "Stop the recording before importing audio".into(),
        });
        return;
    }
    if importing.is_some() {
        let _ = events.send(ControllerEvent::ImportFailed {
            error: "An audio import is already running".into(),
        });
        return;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        let _ = events.send(ControllerEvent::ImportFailed {
            error: "Importing audio is only available on macOS".into(),
        });
    }
    #[cfg(target_os = "macos")]
    {
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let started = std::thread::Builder::new()
            .name("meeting-import".into())
            .spawn(move || meeting::import::import(&path));
        match started {
            Ok(worker) => *importing = Some(ImportWorker { worker, file }),
            Err(error) => {
                let _ = events.send(ControllerEvent::ImportFailed {
                    error: error.to_string(),
                });
            }
        }
    }
}

/// Finishes an import worker, starting notes as a finished recording would.
pub(super) fn poll_import(
    importing: &mut Option<ImportWorker>,
    events: &SyncSender<ControllerEvent>,
) {
    if !importing
        .as_ref()
        .is_some_and(|worker| worker.worker.is_finished())
    {
        return;
    }
    let Some(worker) = importing.take() else {
        return;
    };
    let file = worker.file;
    match worker.worker.join() {
        Ok(Ok(manifest)) if manifest.status == meeting::MeetingStatus::Complete => {
            // An imported file with no speech leaves nothing to keep, exactly
            // as an empty recording does; notes never run for it.
            if !meeting::keep_or_discard(&manifest) {
                let _ = events.send(ControllerEvent::ImportFailed {
                    error: format!("No speech was found in {file}."),
                });
                return;
            }
            tracing::info!(meeting = manifest.id, "imported audio was transcribed");
            if meeting::notes_wanted() {
                start_notes(manifest.id, events);
            }
        }
        Ok(Ok(manifest)) => {
            tracing::info!(
                meeting = manifest.id,
                status = ?manifest.status,
                "left an imported meeting for the next launch"
            );
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "could not import audio as a meeting");
            let _ = events.send(ControllerEvent::ImportFailed {
                error: format!("{error:#}"),
            });
        }
        Err(_) => {
            tracing::error!("an audio import panicked");
            let _ = events.send(ControllerEvent::ImportFailed {
                error: "the audio import stopped".into(),
            });
        }
    }
}
