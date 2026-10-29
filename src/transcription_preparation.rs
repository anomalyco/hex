use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crate::transcription_models::{ModelRuntime, TranscriptionModelId, TranscriptionSelection};

type Prepare = dyn Fn(&TranscriptionSelection, bool, &AtomicBool, &AtomicU64) -> Result<(), String>
    + Send
    + Sync;

/// Application-owned preparation shared by Settings and the menu bar.
/// At most one worker runs; a newer request replaces the single pending request.
/// Downloads belong to [`crate::model_downloads`]: a replaced choice leaves
/// its model downloading, and only Cancel stops it.
#[derive(Clone)]
pub struct TranscriptionPreparation(Rc<RefCell<State>>);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreparationStatus {
    pub model: Option<TranscriptionModelId>,
    pub downloaded_bytes: u64,
    pub error: Option<String>,
}

struct Request {
    selection: TranscriptionSelection,
    installed_only: bool,
}

struct Task {
    request: Request,
    canceled: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    worker: JoinHandle<Result<(), String>>,
}

struct State {
    prepare: Arc<Prepare>,
    active: Option<Task>,
    pending: Option<Request>,
    error: Option<String>,
}

impl Default for TranscriptionPreparation {
    fn default() -> Self {
        Self::with_prepare(Arc::new(prepare_selection))
    }
}

impl TranscriptionPreparation {
    pub(crate) fn with_prepare(prepare: Arc<Prepare>) -> Self {
        Self(Rc::new(RefCell::new(State {
            prepare,
            active: None,
            pending: None,
            error: None,
        })))
    }

    pub fn start(&self, selection: TranscriptionSelection, installed_only: bool) {
        self.cancel();
        let mut state = self.0.borrow_mut();
        state.error = None;
        state.pending = Some(Request {
            selection,
            installed_only,
        });
        state.start_pending();
    }

    pub fn cancel(&self) {
        let mut state = self.0.borrow_mut();
        state.pending = None;
        if let Some(task) = &state.active {
            task.canceled.store(true, Ordering::Release);
        }
    }

    /// Cancels the choice and stops its model's download.
    pub fn cancel_with_download(&self) {
        if let Some(model) = self.status().model {
            crate::model_downloads::shared().cancel(model);
        }
        self.cancel();
    }

    /// Stops `model`'s download, and the choice when it waits on it.
    pub fn cancel_download(&self, model: TranscriptionModelId) {
        crate::model_downloads::shared().cancel(model);
        if self.status().model == Some(model) {
            self.cancel();
        }
    }

    /// Only the application controller consumes completions and commits settings.
    pub fn poll(&self) -> Option<Result<TranscriptionSelection, String>> {
        let mut state = self.0.borrow_mut();
        if !state.active.as_ref()?.worker.is_finished() {
            return None;
        }
        let task = state.active.take().expect("finished preparation exists");
        let result = match task.worker.join() {
            Ok(result) => result,
            Err(_) => Err("Model preparation worker failed.".into()),
        };
        let accepted = !task.canceled.load(Ordering::Acquire);
        state.start_pending();
        accepted.then(|| result.map(|()| task.request.selection))
    }

    pub fn status(&self) -> PreparationStatus {
        let state = self.0.borrow();
        let request = state.pending.as_ref().or_else(|| {
            state
                .active
                .as_ref()
                .filter(|task| !task.canceled.load(Ordering::Acquire))
                .map(|task| &task.request)
        });
        PreparationStatus {
            model: request.map(|request| request.selection.model),
            downloaded_bytes: if state.pending.is_none() && request.is_some() {
                state
                    .active
                    .as_ref()
                    .map_or(0, |task| task.progress.load(Ordering::Relaxed))
            } else {
                0
            },
            error: state.error.clone(),
        }
    }

    pub fn set_error(&self, error: Option<String>) {
        self.0.borrow_mut().error = error;
    }
}

impl State {
    fn start_pending(&mut self) {
        if self.active.is_some() {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let prepare = self.prepare.clone();
        let selection = request.selection.clone();
        let installed_only = request.installed_only;
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        match thread::Builder::new()
            .name("transcription-preparation".into())
            .spawn(move || {
                prepare(
                    &selection,
                    installed_only,
                    &worker_canceled,
                    &worker_progress,
                )
            }) {
            Ok(worker) => {
                self.active = Some(Task {
                    request,
                    canceled,
                    progress,
                    worker,
                });
            }
            Err(error) => self.error = Some(format!("Could not start model preparation: {error}")),
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = &self.active {
            task.canceled.store(true, Ordering::Release);
        }
    }
}

fn prepare_selection(
    selection: &TranscriptionSelection,
    installed_only: bool,
    canceled: &AtomicBool,
    progress: &AtomicU64,
) -> Result<(), String> {
    let check_canceled = || {
        if canceled.load(Ordering::Acquire) {
            color_eyre::eyre::bail!("Model preparation canceled.");
        }
        Ok(())
    };
    (|| -> color_eyre::Result<()> {
        let model = crate::transcription_models::validate(selection)?;
        if matches!(model.runtime, ModelRuntime::Gguf(_)) {
            if installed_only {
                crate::transcription_models::verify_installed(model, canceled)?;
                progress.store(model.download_bytes().unwrap_or(0), Ordering::Relaxed);
            } else {
                crate::model_downloads::shared()
                    .wait(model.id, canceled, progress, &AtomicU8::new(0))
                    .map_err(|error| color_eyre::eyre::eyre!(error))?;
            }
        }
        check_canceled()?;
        crate::speech_detector::prewarm();
        // macOS warms the model to prove it loads. Windows and Linux load it
        // once, when the listener restarts with the new selection.
        #[cfg(target_os = "macos")]
        crate::transcription::Transcriber::load_selection(selection).map(drop)?;
        check_canceled()
    })()
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn selection(model: TranscriptionModelId) -> TranscriptionSelection {
        TranscriptionSelection {
            model,
            ..Default::default()
        }
    }

    fn completion(
        preparation: &TranscriptionPreparation,
    ) -> Result<TranscriptionSelection, String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = preparation.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "preparation did not complete");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn preparation_has_one_worker_and_only_keeps_the_latest_pending_choice() {
        let (started, starts) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let releases = Mutex::new(releases);
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(
            move |selection, installed, _, progress| {
                assert!(installed);
                progress.store(123, Ordering::Relaxed);
                started.send(selection.model).unwrap();
                releases.lock().unwrap().recv().unwrap();
                Ok(())
            },
        ));
        preparation.start(selection(TranscriptionModelId::ParakeetV2), true);
        assert_eq!(
            starts.recv_timeout(Duration::from_secs(3)).unwrap(),
            TranscriptionModelId::ParakeetV2
        );
        assert_eq!(preparation.status().downloaded_bytes, 123);
        preparation.start(selection(TranscriptionModelId::ParakeetV3), true);
        preparation.start(selection(TranscriptionModelId::WhisperLargeV3Turbo), true);
        assert_eq!(
            preparation.status().model,
            Some(TranscriptionModelId::WhisperLargeV3Turbo)
        );
        assert_eq!(preparation.status().downloaded_bytes, 0);
        assert!(starts.try_recv().is_err());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(preparation.poll().is_none(), "superseded selection escaped");
            if let Ok(model) = starts.try_recv() {
                assert_eq!(model, TranscriptionModelId::WhisperLargeV3Turbo);
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        release.send(()).unwrap();
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::WhisperLargeV3Turbo
        );
        assert!(preparation.status().model.is_none());
        assert!(starts.try_recv().is_err());
    }

    #[test]
    fn cancelling_a_completed_unpolled_preparation_never_commits_it() {
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|_, _, _, _| Ok(())));
        preparation.start(selection(TranscriptionModelId::ParakeetV2), true);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !preparation
            .0
            .borrow()
            .active
            .as_ref()
            .unwrap()
            .worker
            .is_finished()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        preparation.cancel();
        assert!(preparation.poll().is_none());
        assert!(preparation.0.borrow().active.is_none());
    }

    #[test]
    fn preparation_failure_and_worker_panic_allow_a_later_selection() {
        let preparation =
            TranscriptionPreparation::with_prepare(Arc::new(|selection, _, _, _| match selection
                .model
            {
                TranscriptionModelId::ParakeetV2 => Err("fixture load failed".into()),
                TranscriptionModelId::ParakeetV3 => panic!("fixture worker panic"),
                _ => Ok(()),
            }));
        preparation.start(selection(TranscriptionModelId::ParakeetV2), true);
        assert_eq!(completion(&preparation).unwrap_err(), "fixture load failed");
        preparation.start(selection(TranscriptionModelId::ParakeetV3), true);
        assert_eq!(
            completion(&preparation).unwrap_err(),
            "Model preparation worker failed."
        );
        preparation.set_error(Some("save failed".into()));
        assert_eq!(preparation.status().error.as_deref(), Some("save failed"));
        preparation.start(selection(TranscriptionModelId::WhisperLargeV3Turbo), true);
        assert!(preparation.status().error.is_none());
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::WhisperLargeV3Turbo
        );
    }

    #[test]
    fn cancelling_another_models_download_keeps_the_choice() {
        let (release, releases) = mpsc::channel();
        let releases = Mutex::new(releases);
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(move |_, _, _, _| {
            releases.lock().unwrap().recv().unwrap();
            Ok(())
        }));
        preparation.start(selection(TranscriptionModelId::ParakeetV2), false);
        preparation.cancel_download(TranscriptionModelId::ParakeetV3);
        release.send(()).unwrap();
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::ParakeetV2
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        for with_download in [false, true] {
            preparation.start(selection(TranscriptionModelId::ParakeetV2), false);
            if with_download {
                preparation.cancel_with_download();
            } else {
                preparation.cancel_download(TranscriptionModelId::ParakeetV2);
            }
            assert!(preparation.status().model.is_none());
            release.send(()).unwrap();
            while preparation.0.borrow().active.is_some() {
                assert!(preparation.poll().is_none(), "a canceled choice escaped");
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    #[test]
    fn dropping_a_settings_reference_does_not_cancel_application_preparation() {
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|_, _, _, _| Ok(())));
        let settings_reference = preparation.clone();
        settings_reference.start(selection(TranscriptionModelId::ParakeetV2), true);
        drop(settings_reference);
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::ParakeetV2
        );
    }
}
