//! One transfer per catalog model, shared by every model choice. A choice
//! waits on its model's transfer and only detaches when a newer choice
//! replaces it, so the transfer keeps running: several models can download at
//! once, and a finished one is installed without becoming active.

use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::transcription_models::{ModelPreparationStage, TranscriptionModelId, definition};

type Download = dyn Fn(TranscriptionModelId, &AtomicBool, &AtomicU64, &AtomicU8) -> Result<(), String>
    + Send
    + Sync;

/// A transfer the picker shows: running, or failed with nobody waiting on it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DownloadStatus {
    pub(crate) model: TranscriptionModelId,
    pub(crate) downloaded_bytes: u64,
    pub(crate) stage: ModelPreparationStage,
    pub(crate) error: Option<String>,
}

struct Transfer {
    model: TranscriptionModelId,
    canceled: AtomicBool,
    progress: AtomicU64,
    stage: AtomicU8,
    result: Mutex<Option<Result<(), String>>>,
    finished: Condvar,
}

pub(crate) struct ModelDownloads {
    download: Arc<Download>,
    transfers: Arc<Mutex<Vec<Arc<Transfer>>>>,
}

static SHARED: LazyLock<ModelDownloads> = LazyLock::new(|| {
    ModelDownloads::with_download(Arc::new(|model, canceled, progress, stage| {
        crate::transcription_models::download_with_stage_progress(
            definition(model),
            canceled,
            progress,
            stage,
        )
        .map(drop)
        .map_err(|error| format!("{error:#}"))
    }))
});

/// The process's downloads, which every model choice shares.
pub(crate) fn shared() -> &'static ModelDownloads {
    &SHARED
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

impl ModelDownloads {
    pub(crate) fn with_download(download: Arc<Download>) -> Self {
        Self {
            download,
            transfers: Arc::default(),
        }
    }

    /// Downloads `model`, joining its running transfer, and mirrors that
    /// transfer's progress. Raising `canceled` detaches this caller and
    /// leaves the transfer running; only [`Self::cancel`] stops it.
    pub(crate) fn wait(
        &self,
        model: TranscriptionModelId,
        canceled: &AtomicBool,
        progress: &AtomicU64,
        stage: &AtomicU8,
    ) -> Result<(), String> {
        let transfer = self.transfer(model)?;
        let mut result = lock(&transfer.result);
        loop {
            progress.store(transfer.progress.load(Ordering::Relaxed), Ordering::Relaxed);
            stage.store(transfer.stage.load(Ordering::Acquire), Ordering::Release);
            if let Some(finished) = result.clone() {
                drop(result);
                // This caller reports the failure, so the picker need not.
                self.forget(&transfer);
                return finished;
            }
            if transfer.canceled.load(Ordering::Acquire) {
                return Err("Model download canceled.".into());
            }
            if canceled.load(Ordering::Acquire) {
                return Err("Model preparation canceled.".into());
            }
            result = transfer
                .finished
                .wait_timeout(result, Duration::from_millis(100))
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }

    /// Stops `model`'s transfer, or dismisses its failure.
    pub(crate) fn cancel(&self, model: TranscriptionModelId) {
        lock(&self.transfers).retain(|transfer| {
            if transfer.model != model {
                return true;
            }
            transfer.canceled.store(true, Ordering::Release);
            transfer.finished.notify_all();
            false
        });
    }

    pub(crate) fn status(&self) -> Vec<DownloadStatus> {
        lock(&self.transfers)
            .iter()
            .map(|transfer| DownloadStatus {
                model: transfer.model,
                downloaded_bytes: transfer.progress.load(Ordering::Relaxed),
                stage: ModelPreparationStage::load(&transfer.stage),
                error: lock(&transfer.result)
                    .clone()
                    .and_then(|result| result.err()),
            })
            .collect()
    }

    /// `model`'s running transfer, or a new one. A failed transfer is
    /// retried; a canceled one may still be stopping, and the new transfer
    /// waits for its artifact lock.
    fn transfer(&self, model: TranscriptionModelId) -> Result<Arc<Transfer>, String> {
        let mut transfers = lock(&self.transfers);
        transfers.retain(|transfer| transfer.model != model || lock(&transfer.result).is_none());
        if let Some(transfer) = transfers.iter().find(|transfer| transfer.model == model) {
            return Ok(transfer.clone());
        }
        let transfer = Arc::new(Transfer {
            model,
            canceled: AtomicBool::new(false),
            progress: AtomicU64::new(0),
            stage: AtomicU8::new(ModelPreparationStage::Downloading as u8),
            result: Mutex::new(None),
            finished: Condvar::new(),
        });
        let download = self.download.clone();
        let worker = transfer.clone();
        let registry = self.transfers.clone();
        thread::Builder::new()
            .name("model-download".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    download(model, &worker.canceled, &worker.progress, &worker.stage)
                }))
                .unwrap_or_else(|_| Err("Model download worker failed.".into()));
                let failed = result.is_err() && !worker.canceled.load(Ordering::Acquire);
                *lock(&worker.result) = Some(result);
                worker.finished.notify_all();
                // A failure stays for the picker until a caller reports it,
                // a retry replaces it, or Cancel dismisses it.
                if !failed {
                    lock(&registry).retain(|transfer| !Arc::ptr_eq(transfer, &worker));
                }
            })
            .map_err(|error| format!("Could not start model download: {error}"))?;
        transfers.push(transfer.clone());
        Ok(transfer)
    }

    fn forget(&self, finished: &Arc<Transfer>) {
        lock(&self.transfers).retain(|transfer| !Arc::ptr_eq(transfer, finished));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    const WAIT: Duration = Duration::from_secs(3);
    const A: TranscriptionModelId = TranscriptionModelId::ParakeetV2;
    const B: TranscriptionModelId = TranscriptionModelId::ParakeetV3;

    /// Each transfer reports its start, then holds until released with its
    /// result; `canceled` reports whether it saw cancellation.
    struct Fixture {
        downloads: Arc<ModelDownloads>,
        started: mpsc::Receiver<TranscriptionModelId>,
        release: mpsc::Sender<(TranscriptionModelId, Result<(), String>)>,
        canceled: mpsc::Receiver<TranscriptionModelId>,
    }

    impl Fixture {
        fn new() -> Self {
            let (start, started) = mpsc::channel();
            let (release, releases) = mpsc::channel::<(TranscriptionModelId, Result<(), String>)>();
            let (cancel, canceled) = mpsc::channel();
            let releases = Mutex::new(releases);
            let held = Mutex::new(Vec::new());
            let downloads =
                ModelDownloads::with_download(Arc::new(move |model, canceled, progress, _| {
                    progress.store(if model == A { 123 } else { 456 }, Ordering::Relaxed);
                    start.send(model).unwrap();
                    let deadline = Instant::now() + WAIT;
                    loop {
                        if canceled.load(Ordering::Acquire) {
                            cancel.send(model).unwrap();
                            return Err("canceled".into());
                        }
                        let mut held = lock(&held);
                        held.extend(lock(&releases).try_iter());
                        if let Some(index) = held.iter().position(|(m, _)| *m == model) {
                            return held.remove(index).1;
                        }
                        drop(held);
                        assert!(Instant::now() < deadline, "fixture never released");
                        thread::sleep(Duration::from_millis(1));
                    }
                }));
            Self {
                downloads: Arc::new(downloads),
                started,
                release,
                canceled,
            }
        }

        fn started(&self) -> TranscriptionModelId {
            self.started.recv_timeout(WAIT).expect("transfer started")
        }

        /// A caller waiting on `model` from another thread, with its cancel flag.
        fn waiter(
            &self,
            model: TranscriptionModelId,
        ) -> (Arc<AtomicBool>, thread::JoinHandle<Result<(), String>>) {
            let canceled = Arc::new(AtomicBool::new(false));
            let downloads = self.downloads.clone();
            let flag = canceled.clone();
            let worker = thread::spawn(move || {
                downloads.wait(model, &flag, &AtomicU64::new(0), &AtomicU8::new(0))
            });
            (canceled, worker)
        }

        fn settled(&self) -> Vec<DownloadStatus> {
            let deadline = Instant::now() + WAIT;
            loop {
                let status = self.downloads.status();
                if status.iter().all(|download| download.error.is_some()) {
                    return status;
                }
                assert!(Instant::now() < deadline, "transfers never finished");
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    #[test]
    fn models_download_at_once_and_a_replaced_choice_leaves_its_transfer_running() {
        let fixture = Fixture::new();
        let (choose_a, a) = fixture.waiter(A);
        assert_eq!(fixture.started(), A);
        // A newer choice replaces A; A keeps downloading beside B.
        choose_a.store(true, Ordering::Release);
        assert_eq!(
            a.join().unwrap().unwrap_err(),
            "Model preparation canceled."
        );
        let (_, b) = fixture.waiter(B);
        assert_eq!(fixture.started(), B);
        let status = fixture.downloads.status();
        assert_eq!(
            status
                .iter()
                .map(|download| (download.model, download.downloaded_bytes))
                .collect::<Vec<_>>(),
            [(A, 123), (B, 456)]
        );
        // Choosing A again joins its transfer instead of starting another.
        let (_, a_again) = fixture.waiter(A);
        fixture.release.send((B, Ok(()))).unwrap();
        assert_eq!(b.join().unwrap(), Ok(()));
        fixture.release.send((A, Ok(()))).unwrap();
        assert_eq!(a_again.join().unwrap(), Ok(()));
        assert!(fixture.started.try_recv().is_err(), "a duplicate transfer");
        assert!(fixture.settled().is_empty());
    }

    #[test]
    fn cancel_stops_only_that_model() {
        let fixture = Fixture::new();
        let (_, a) = fixture.waiter(A);
        assert_eq!(fixture.started(), A);
        let (_, b) = fixture.waiter(B);
        assert_eq!(fixture.started(), B);
        fixture.downloads.cancel(A);
        assert_eq!(a.join().unwrap().unwrap_err(), "Model download canceled.");
        assert_eq!(fixture.canceled.recv_timeout(WAIT).unwrap(), A);
        assert_eq!(
            fixture
                .downloads
                .status()
                .iter()
                .map(|download| download.model)
                .collect::<Vec<_>>(),
            [B]
        );
        fixture.release.send((B, Ok(()))).unwrap();
        assert_eq!(b.join().unwrap(), Ok(()));
        assert!(fixture.settled().is_empty());
    }

    #[test]
    fn an_unwatched_failure_stays_until_retried_or_dismissed() {
        let fixture = Fixture::new();
        let (detach, a) = fixture.waiter(A);
        assert_eq!(fixture.started(), A);
        detach.store(true, Ordering::Release);
        a.join().unwrap().unwrap_err();
        fixture
            .release
            .send((A, Err("fixture offline".into())))
            .unwrap();
        let failed = fixture.settled();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].error.as_deref(), Some("fixture offline"));
        // A retry starts a new transfer and reports its own result.
        let (_, retry) = fixture.waiter(A);
        assert_eq!(fixture.started(), A);
        fixture
            .release
            .send((A, Err("still offline".into())))
            .unwrap();
        assert_eq!(retry.join().unwrap().unwrap_err(), "still offline");
        assert!(fixture.downloads.status().is_empty(), "reported twice");
        // Cancel dismisses an unwatched failure.
        let (detach, a) = fixture.waiter(A);
        assert_eq!(fixture.started(), A);
        detach.store(true, Ordering::Release);
        a.join().unwrap().unwrap_err();
        fixture
            .release
            .send((A, Err("offline again".into())))
            .unwrap();
        assert_eq!(fixture.settled().len(), 1);
        fixture.downloads.cancel(A);
        assert!(fixture.downloads.status().is_empty());
    }

    #[test]
    fn a_panicking_transfer_fails_and_can_be_retried() {
        let attempts = AtomicU64::new(0);
        let downloads = ModelDownloads::with_download(Arc::new(move |_, _, _, _| {
            match attempts.fetch_add(1, Ordering::Relaxed) {
                0 => panic!("fixture download panic"),
                _ => Ok(()),
            }
        }));
        let wait = || {
            downloads.wait(
                A,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
                &AtomicU8::new(0),
            )
        };
        assert_eq!(wait().unwrap_err(), "Model download worker failed.");
        assert_eq!(wait(), Ok(()));
    }
}
