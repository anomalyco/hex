use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crate::transcription_models::{TranscriptionModelId, definition};

type Download =
    dyn Fn(TranscriptionModelId, &AtomicBool, &AtomicU64) -> Result<(), String> + Send + Sync;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DownloadStatus {
    pub model: TranscriptionModelId,
    pub downloaded_bytes: u64,
}

struct Task {
    model: TranscriptionModelId,
    canceled: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    worker: JoinHandle<Result<(), String>>,
    restart: bool,
}

/// One transfer per catalog model, independent of the selected language.
/// Finished downloads do not load a model or change the active selection.
pub(crate) struct ModelDownloads {
    download: Arc<Download>,
    tasks: Vec<Task>,
}

impl Default for ModelDownloads {
    fn default() -> Self {
        Self::with_download(Arc::new(|model, canceled, progress| {
            crate::transcription_models::download_with_progress(
                definition(model),
                canceled,
                progress,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        }))
    }
}

impl ModelDownloads {
    pub(crate) fn with_download(download: Arc<Download>) -> Self {
        Self {
            download,
            tasks: Vec::new(),
        }
    }

    pub fn start(&mut self, model: TranscriptionModelId) -> Result<(), String> {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.model == model) {
            task.restart = task.canceled.load(Ordering::Acquire);
            return Ok(());
        }
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let download = self.download.clone();
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        let worker = thread::Builder::new()
            .name("model-download".into())
            .spawn(move || download(model, &worker_canceled, &worker_progress))
            .map_err(|error| format!("Could not start model download: {error}"))?;
        self.tasks.push(Task {
            model,
            canceled,
            progress,
            worker,
            restart: false,
        });
        Ok(())
    }

    pub fn cancel(&mut self, model: TranscriptionModelId) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.model == model) {
            task.restart = false;
            task.canceled.store(true, Ordering::Release);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn is_idle(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn contains(&self, model: TranscriptionModelId) -> bool {
        self.tasks.iter().any(|task| {
            task.model == model && (task.restart || !task.canceled.load(Ordering::Acquire))
        })
    }

    pub fn status(&self) -> Vec<DownloadStatus> {
        self.tasks
            .iter()
            .filter(|task| task.restart || !task.canceled.load(Ordering::Acquire))
            .map(|task| DownloadStatus {
                model: task.model,
                downloaded_bytes: if task.restart {
                    0
                } else {
                    task.progress.load(Ordering::Relaxed)
                },
            })
            .collect()
    }

    pub fn poll(&mut self) -> Vec<(TranscriptionModelId, Result<(), String>)> {
        let mut results = Vec::new();
        let mut index = 0;
        while index < self.tasks.len() {
            if !self.tasks[index].worker.is_finished() {
                index += 1;
                continue;
            }
            let task = self.tasks.remove(index);
            let result = task
                .worker
                .join()
                .unwrap_or_else(|_| Err("Model download worker failed.".into()));
            if task.restart {
                if let Err(error) = self.start(task.model) {
                    results.push((task.model, Err(error)));
                }
            } else if !task.canceled.load(Ordering::Acquire) {
                results.push((task.model, result));
            }
        }
        results
    }
}

impl Drop for ModelDownloads {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.canceled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};

    #[test]
    fn models_download_concurrently_with_independent_progress_and_cancellation() {
        let (started, starts) = mpsc::channel();
        let (release_a, releases_a) = mpsc::channel();
        let (release_b, releases_b) = mpsc::channel();
        let releases_a = Mutex::new(releases_a);
        let releases_b = Mutex::new(releases_b);
        let a = TranscriptionModelId::ParakeetV2;
        let b = TranscriptionModelId::ParakeetV3;
        let mut downloads =
            ModelDownloads::with_download(Arc::new(move |model, canceled, progress| {
                progress.store(if model == a { 123 } else { 456 }, Ordering::Relaxed);
                started.send(model).unwrap();
                if model == a {
                    releases_a.lock().unwrap().recv().unwrap();
                    assert!(canceled.load(Ordering::Acquire));
                } else {
                    releases_b.lock().unwrap().recv().unwrap();
                    assert!(!canceled.load(Ordering::Acquire));
                }
                Ok(())
            }));
        downloads.start(a).unwrap();
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), a);
        downloads.start(b).unwrap();
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), b);
        downloads.start(a).unwrap();
        assert!(starts.try_recv().is_err(), "duplicate transfer");
        assert_eq!(
            downloads.status(),
            vec![
                DownloadStatus {
                    model: a,
                    downloaded_bytes: 123
                },
                DownloadStatus {
                    model: b,
                    downloaded_bytes: 456
                },
            ]
        );
        downloads.cancel(a);
        assert!(!downloads.contains(a));
        assert!(downloads.contains(b));
        release_a.send(()).unwrap();
        release_b.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut completed = Vec::new();
        while !downloads.tasks.is_empty() {
            completed.extend(downloads.poll());
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(completed, vec![(b, Ok(()))]);
    }

    #[test]
    fn failed_or_panicked_downloads_allow_retry() {
        let attempts = AtomicU64::new(0);
        let mut downloads = ModelDownloads::with_download(Arc::new(move |_, _, _| match attempts
            .fetch_add(1, Ordering::Relaxed)
        {
            0 => Err("fixture offline".into()),
            1 => panic!("fixture download panic"),
            _ => Ok(()),
        }));
        let model = TranscriptionModelId::ParakeetV2;
        for expected in [
            Err("fixture offline".into()),
            Err("Model download worker failed.".into()),
            Ok(()),
        ] {
            downloads.start(model).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let results = downloads.poll();
                if !results.is_empty() {
                    assert_eq!(results, vec![(model, expected)]);
                    assert!(!downloads.contains(model));
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
        }
    }

    #[test]
    fn retry_waits_for_the_canceled_transfer_to_exit() {
        let (started, starts) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let releases = Mutex::new(releases);
        let model = TranscriptionModelId::ParakeetV2;
        let mut downloads = ModelDownloads::with_download(Arc::new(move |_, _, _| {
            started.send(()).unwrap();
            releases.lock().unwrap().recv().unwrap();
            Ok(())
        }));
        downloads.start(model).unwrap();
        starts.recv_timeout(Duration::from_secs(3)).unwrap();
        downloads.cancel(model);
        downloads.start(model).unwrap();
        assert!(downloads.contains(model));
        assert!(starts.try_recv().is_err());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(downloads.poll().is_empty(), "canceled result escaped");
            if starts.try_recv().is_ok() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        release.send(()).unwrap();
        loop {
            let result = downloads.poll();
            if !result.is_empty() {
                assert_eq!(result, vec![(model, Ok(()))]);
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
}
