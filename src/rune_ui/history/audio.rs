//! Labs › Copy dictation audio: one kept History recording on the clipboard
//! as a file. The clipboard write runs on a worker like Transcribe again,
//! since another app can hold it for a while, and its answer lands back
//! through the window's waker.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::super::Ui;
use super::HistoryPane;
use crate::desktop::i18n::tr;

/// One entry's kept recording going to the clipboard, on a worker: the copy
/// can wait on another app's clipboard, so the window never does.
pub(crate) struct AudioCopy {
    pub(super) id: u64,
    pub(super) worker: Option<Receiver<Result<(), String>>>,
    /// The worker's answer: `Ok` once the file is on the clipboard.
    pub(super) result: Option<Result<(), String>>,
}

impl AudioCopy {
    /// A finished copy, as if its worker had answered.
    pub(super) fn finished(id: u64, result: Result<(), String>) -> Self {
        Self {
            id,
            worker: None,
            result: Some(result),
        }
    }

    /// A copy still running, with its worker `receiver`, for tests.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn pending(id: u64, worker: Receiver<Result<(), String>>) -> Self {
        Self {
            id,
            worker: Some(worker),
            result: None,
        }
    }
}

impl HistoryPane {
    /// Whether Copy audio is still running for its entry: a text Copy waits
    /// for it, since that worker still lands the WAV on the clipboard.
    pub(super) fn audio_copy_pending(&self) -> bool {
        self.audio
            .as_ref()
            .is_some_and(|audio| audio.worker.is_some())
    }

    /// Takes Copy audio's answer once its worker sends it.
    pub(super) fn poll_audio(&mut self) -> bool {
        let Some(audio) = &mut self.audio else {
            return false;
        };
        let result = match audio.worker.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err(tr("The clipboard copy stopped.").into()),
            Some(Err(TryRecvError::Empty)) | None => return false,
        };
        audio.worker = None;
        if let Err(error) = &result {
            tracing::warn!(%error, id = audio.id, "could not copy a dictation recording");
        }
        audio.result = Some(result);
        true
    }
}

/// Labs › Copy dictation audio: entry `id`'s kept WAV goes on the clipboard
/// as a file, on a worker, since another app can hold the clipboard for a
/// while. A missing file or a refused clipboard says so under the row, as a
/// failed Play does; nothing is copied without a recording.
pub(super) fn copy_audio(ui: &mut Ui, id: u64) {
    if !crate::labs::copy_dictation_audio_enabled(&ui.settings) {
        return;
    }
    if ui.history.audio_copy_pending() {
        return;
    }
    // One "Copied" at a time: this replaces the text Copy's check.
    ui.history.copied = None;
    let Some(path) = ui
        .history
        .store
        .as_ref()
        .and_then(|store| store.saved_recording(id))
    else {
        ui.history.audio = Some(AudioCopy::finished(
            id,
            Err(tr("The recording is gone.").into()),
        ));
        return;
    };
    let (sender, receiver) = channel();
    let waker = ui.modes.waker.clone();
    let started = std::thread::Builder::new()
        .name("hex-copy-audio".into())
        .spawn(move || {
            let copied = crate::history::recordings::copy_to_clipboard(&path)
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(copied);
            if let Some(waker) = &waker {
                waker();
            }
        });
    ui.history.audio = Some(AudioCopy {
        id,
        worker: started.is_ok().then_some(receiver),
        result: started.err().map(|error| Err(error.to_string())),
    });
}
