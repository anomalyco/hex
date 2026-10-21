use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::model_downloads::{DownloadStatus, ModelDownloads};
use crate::transcription_models::{ModelRuntime, TranscriptionModelId, TranscriptionSelection};

pub(crate) type Prepare =
    dyn Fn(&TranscriptionSelection, &AtomicBool, &AtomicU64) -> Result<(), String> + Send + Sync;

/// Application-owned preparation shared by Settings and the menu bar.
/// Downloads run independently; activation has one worker and one latest choice.
#[derive(Clone)]
pub struct TranscriptionPreparation(Rc<RefCell<State>>);

impl gpui::Global for TranscriptionPreparation {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreparationStatus {
    pub model: Option<TranscriptionModelId>,
    pub downloaded_bytes: u64,
    pub started: Option<Instant>,
    pub error: Option<String>,
    pub(crate) downloads: Vec<DownloadStatus>,
}

struct Request {
    selection: TranscriptionSelection,
    started: Instant,
}

struct Task {
    request: Request,
    canceled: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    worker: JoinHandle<Result<(), String>>,
}

struct State {
    prepare: Arc<Prepare>,
    downloads: ModelDownloads,
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
            downloads: ModelDownloads::default(),
            active: None,
            pending: None,
            error: None,
        })))
    }

    pub fn start(&self, selection: TranscriptionSelection, installed_only: bool) {
        let mut state = self.0.borrow_mut();
        state.cancel_selection();
        state.error = None;
        if !installed_only
            && matches!(
                crate::transcription_models::definition(selection.model).runtime,
                ModelRuntime::Gguf(_)
            )
            && let Err(error) = state.downloads.start(selection.model)
        {
            state.error = Some(error);
            return;
        }
        state.pending = Some(Request {
            selection,
            started: Instant::now(),
        });
        state.start_pending();
    }

    pub fn cancel(&self) {
        let model = self.status().model;
        if let Some(model) = model {
            self.cancel_model(model);
        }
    }

    #[cfg(target_os = "macos")]
    pub fn cancel_all(&self) {
        let mut state = self.0.borrow_mut();
        state.cancel_selection();
        for download in state.downloads.status() {
            state.downloads.cancel(download.model);
        }
    }

    pub fn cancel_model(&self, model: TranscriptionModelId) {
        let mut state = self.0.borrow_mut();
        state.downloads.cancel(model);
        if state
            .pending
            .as_ref()
            .is_some_and(|request| request.selection.model == model)
        {
            state.pending = None;
        }
        if let Some(task) = &state.active
            && task.request.selection.model == model
        {
            task.canceled.store(true, Ordering::Release);
        }
    }

    /// Only the application controller consumes completions and commits settings.
    pub fn poll(&self) -> Option<Result<TranscriptionSelection, String>> {
        let mut state = self.0.borrow_mut();
        for (model, result) in state.downloads.poll() {
            if let Err(error) = result {
                if state
                    .pending
                    .as_ref()
                    .is_some_and(|request| request.selection.model == model)
                {
                    state.pending = None;
                }
                state.error = Some(format!(
                    "{}: {error}",
                    crate::transcription_models::definition(model).name
                ));
            }
        }
        state.start_pending();
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
        let downloads = state.downloads.status();
        PreparationStatus {
            model: request.map(|request| request.selection.model),
            started: request.map(|request| request.started),
            downloaded_bytes: if let Some(download) = downloads.iter().find(|download| {
                Some(download.model) == request.map(|request| request.selection.model)
            }) {
                download.downloaded_bytes
            } else if state.pending.is_none() && request.is_some() {
                state
                    .active
                    .as_ref()
                    .map_or(0, |task| task.progress.load(Ordering::Relaxed))
            } else {
                0
            },
            error: state.error.clone(),
            downloads,
        }
    }

    pub fn set_error(&self, error: Option<String>) {
        self.0.borrow_mut().error = error;
    }
}

impl State {
    fn cancel_selection(&mut self) {
        self.pending = None;
        if let Some(task) = &self.active {
            task.canceled.store(true, Ordering::Release);
        }
    }

    fn start_pending(&mut self) {
        if self.active.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|request| self.downloads.contains(request.selection.model))
        {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let prepare = self.prepare.clone();
        let selection = request.selection.clone();
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        match thread::Builder::new()
            .name("transcription-preparation".into())
            .spawn(move || prepare(&selection, &worker_canceled, &worker_progress))
        {
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
            crate::transcription_models::verify_installed(model, canceled)?;
            progress.store(model.download_bytes().unwrap_or(0), Ordering::Relaxed);
        }
        check_canceled()?;
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
    use std::time::Duration;

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
    fn downloads_finish_independently_and_only_the_latest_choice_loads() {
        let (started, starts) = mpsc::channel();
        let (loaded, loads) = mpsc::channel();
        let (release_a, releases_a) = mpsc::channel();
        let (release_b, releases_b) = mpsc::channel();
        let releases_a = Mutex::new(releases_a);
        let releases_b = Mutex::new(releases_b);
        let a = TranscriptionModelId::ParakeetV2;
        let b = TranscriptionModelId::ParakeetV3;
        let preparation =
            TranscriptionPreparation::with_prepare(Arc::new(move |selection, _, _| {
                loaded.send(selection.clone()).unwrap();
                Ok(())
            }));
        preparation.0.borrow_mut().downloads =
            ModelDownloads::with_download(Arc::new(move |model, canceled, _| {
                started.send(model).unwrap();
                if model == a {
                    releases_a.lock().unwrap().recv().unwrap();
                } else {
                    releases_b.lock().unwrap().recv().unwrap();
                }
                assert!(!canceled.load(Ordering::Acquire));
                Ok(())
            }));
        preparation.start(selection(a), false);
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), a);
        preparation.start(selection(b), false);
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), b);
        let french = TranscriptionSelection {
            language: "fr".into(),
            ..selection(b)
        };
        preparation.start(french.clone(), false);
        assert_eq!(preparation.status().downloads.len(), 2);
        assert!(
            starts.try_recv().is_err(),
            "a language reused the same model"
        );
        assert!(loads.try_recv().is_err(), "loaded before download finished");
        release_b.send(()).unwrap();
        assert_eq!(completion(&preparation).unwrap(), french);
        assert_eq!(loads.recv_timeout(Duration::from_secs(3)).unwrap(), french);
        assert_eq!(preparation.status().downloads[0].model, a);
        release_a.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !preparation.status().downloads.is_empty() {
            assert!(preparation.poll().is_none(), "older download became active");
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(loads.try_recv().is_err());
    }

    #[test]
    fn a_failed_download_never_loads_and_can_be_retried() {
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|_, _, _| {
            panic!("failed download must not load")
        }));
        preparation.0.borrow_mut().downloads =
            ModelDownloads::with_download(Arc::new(|_, _, _| Err("fixture offline".into())));
        for _ in 0..2 {
            preparation.start(selection(TranscriptionModelId::ParakeetV2), false);
            let deadline = Instant::now() + Duration::from_secs(3);
            while preparation.status().model.is_some() {
                assert!(preparation.poll().is_none());
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            assert!(
                preparation
                    .status()
                    .error
                    .unwrap()
                    .contains("fixture offline")
            );
            assert!(preparation.status().downloads.is_empty());
        }
    }

    #[test]
    fn preparation_has_one_worker_and_only_keeps_the_latest_pending_choice() {
        let (started, starts) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let releases = Mutex::new(releases);
        let preparation =
            TranscriptionPreparation::with_prepare(Arc::new(move |selection, _, progress| {
                progress.store(123, Ordering::Relaxed);
                started.send(selection.model).unwrap();
                releases.lock().unwrap().recv().unwrap();
                Ok(())
            }));
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
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|_, _, _| Ok(())));
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
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|selection, _, _| {
            match selection.model {
                TranscriptionModelId::ParakeetV2 => Err("fixture load failed".into()),
                TranscriptionModelId::ParakeetV3 => panic!("fixture worker panic"),
                _ => Ok(()),
            }
        }));
        preparation.start(selection(TranscriptionModelId::ParakeetV2), true);
        assert_eq!(completion(&preparation).unwrap_err(), "fixture load failed");
        preparation.start(selection(TranscriptionModelId::ParakeetV3), true);
        assert_eq!(
            completion(&preparation).unwrap_err(),
            "Model preparation worker failed."
        );
        preparation.start(selection(TranscriptionModelId::WhisperLargeV3Turbo), true);
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::WhisperLargeV3Turbo
        );
    }

    #[test]
    fn dropping_a_settings_reference_does_not_cancel_application_preparation() {
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(|_, _, _| Ok(())));
        let settings_reference = preparation.clone();
        settings_reference.start(selection(TranscriptionModelId::ParakeetV2), true);
        drop(settings_reference);
        assert_eq!(
            completion(&preparation).unwrap().model,
            TranscriptionModelId::ParakeetV2
        );
    }
}
