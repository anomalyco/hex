use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_rs::TranscriptionResult;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};

use crate::dictation::{DictationClip, strip_captains_log_protocol, strip_dictation_protocol};
use crate::paste::Paster;

pub struct Parakeet {
    model: ParakeetModel,
}

pub enum WorkerEvent {
    ModelFailed(String),
    Completed {
        target: TranscriptionTarget,
        result: Result<String, String>,
    },
    Repasted(Result<String, String>),
}

#[derive(Clone, Copy, Debug)]
pub enum TranscriptionTarget {
    Paste,
    Send,
    CaptainsLog,
}

enum WorkerJob {
    Transcribe {
        clip: DictationClip,
        target: TranscriptionTarget,
    },
    PasteLast,
}

pub struct DictationWorker {
    jobs: SyncSender<WorkerJob>,
    events: Receiver<WorkerEvent>,
    state: Arc<Mutex<WorkerState>>,
}

#[derive(Default)]
struct WorkerState {
    available: bool,
    pending: usize,
}

impl DictationWorker {
    pub fn start() -> Self {
        let (jobs, job_receiver) = mpsc::sync_channel::<WorkerJob>(1);
        let (event_sender, events) = mpsc::channel();
        let state = Arc::new(Mutex::new(WorkerState {
            available: true,
            pending: 0,
        }));
        let worker_state = state.clone();
        thread::spawn(move || {
            let mut parakeet = match Parakeet::load() {
                Ok(parakeet) => {
                    tracing::info!("Parakeet v2 loaded");
                    parakeet
                }
                Err(error) => {
                    let mut state = worker_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    state.available = false;
                    state.pending = 0;
                    let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                    return;
                }
            };
            let mut paster = match Paster::new() {
                Ok(paster) => paster,
                Err(error) => {
                    let mut state = worker_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    state.available = false;
                    state.pending = 0;
                    let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                    return;
                }
            };
            let mut last_transcript = None;
            while let Ok(job) = job_receiver.recv() {
                let event = match job {
                    WorkerJob::Transcribe { clip, target } => {
                        let total_started = Instant::now();
                        let prepare_started = Instant::now();
                        let samples = clip.into_parakeet_samples();
                        let result = (|| -> Result<String> {
                            let prepare = prepare_started.elapsed();
                            let inference_started = Instant::now();
                            let text = parakeet.transcribe(&samples)?;
                            let text = match target {
                                TranscriptionTarget::Paste | TranscriptionTarget::Send => {
                                    strip_dictation_protocol(&text)
                                }
                                TranscriptionTarget::CaptainsLog => {
                                    strip_captains_log_protocol(&text)
                                }
                            };
                            let inference = inference_started.elapsed();
                            let paste_started = Instant::now();
                            if !text.trim().is_empty() {
                                match target {
                                    TranscriptionTarget::Paste => paster.paste(&text)?,
                                    TranscriptionTarget::Send => {
                                        paster.paste(&text)?;
                                        crate::keyboard::post_enter()?;
                                    }
                                    TranscriptionTarget::CaptainsLog => append_daily_log(&text)?,
                                }
                                if !matches!(target, TranscriptionTarget::CaptainsLog) {
                                    last_transcript = Some(text.clone());
                                }
                            }
                            tracing::info!(
                                prepare_ms = prepare.as_millis(),
                                inference_ms = inference.as_millis(),
                                paste_ms = paste_started.elapsed().as_millis(),
                                total_ms = total_started.elapsed().as_millis(),
                                "dictation pipeline completed"
                            );
                            Ok(text)
                        })();
                        let result = result.map_err(|error| error.to_string());
                        WorkerEvent::Completed { target, result }
                    }
                    WorkerJob::PasteLast => {
                        let result = last_transcript
                            .as_deref()
                            .ok_or_else(|| "no previous transcript is available".to_string())
                            .and_then(|text| {
                                paster.paste(text).map_err(|error| error.to_string())?;
                                Ok(text.to_string())
                            });
                        WorkerEvent::Repasted(result)
                    }
                };
                let mut state = worker_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.pending = state.pending.saturating_sub(1);
                drop(state);
                if event_sender.send(event).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            events,
            state,
        }
    }

    pub fn transcribe(
        &self,
        clip: DictationClip,
        target: TranscriptionTarget,
    ) -> Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.available {
            return Err("Parakeet is unavailable");
        }
        self.jobs
            .try_send(WorkerJob::Transcribe { clip, target })
            .map(|()| state.pending += 1)
            .map_err(|error| match error {
                TrySendError::Full(_) => "dictation is already busy",
                TrySendError::Disconnected(_) => "Parakeet is unavailable",
            })
    }

    pub fn paste_last(&self) -> Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.available {
            return Err("dictation worker is unavailable");
        }
        self.jobs
            .try_send(WorkerJob::PasteLast)
            .map(|()| state.pending += 1)
            .map_err(|error| match error {
                TrySendError::Full(_) => "dictation is already busy",
                TrySendError::Disconnected(_) => "dictation worker is unavailable",
            })
    }

    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_busy(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            > 0
    }
}

fn append_daily_log(text: &str) -> Result<()> {
    let executable = std::env::var_os("VOICE_CONTROL_LOG_CLI")
        .map(Into::into)
        .or_else(|| dirs::home_dir().map(|home| home.join(".bun/bin/log")))
        .ok_or_else(|| eyre!("could not resolve the Organizer log CLI"))?;
    let output = Command::new(&executable)
        .args(["add", text])
        .output()
        .wrap_err_with(|| format!("could not invoke {}", executable.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(eyre!(
            "Organizer log CLI exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

impl Parakeet {
    pub fn load() -> Result<Self> {
        let model_path = dirs::data_dir()
            .ok_or_else(|| eyre!("macOS application support directory is unavailable"))?
            .join("voice-control/models/parakeet-tdt-0.6b-v2-int8");
        let model = ParakeetModel::load(&model_path, &Quantization::Int8).wrap_err_with(|| {
            format!("could not load Parakeet v2 from {}", model_path.display())
        })?;
        Ok(Self { model })
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        self.transcribe_segments(samples).map(|result| result.text)
    }

    pub fn transcribe_segments(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
        self.model
            .transcribe_with(
                samples,
                &ParakeetParams {
                    timestamp_granularity: Some(TimestampGranularity::Segment),
                    ..Default::default()
                },
            )
            .wrap_err("Parakeet transcription failed")
    }
}
