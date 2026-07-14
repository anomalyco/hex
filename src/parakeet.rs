use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};

use crate::dictation::{DictationClip, dictation_control_suffix};
use crate::paste::Paster;

pub struct Parakeet {
    model: ParakeetModel,
}

pub enum WorkerEvent {
    ModelFailed(String),
    Completed(Result<String, String>),
    Repasted(Result<String, String>),
}

enum WorkerJob {
    Transcribe { clip: DictationClip, send: bool },
    PasteLast,
}

pub struct DictationWorker {
    jobs: SyncSender<WorkerJob>,
    events: Receiver<WorkerEvent>,
}

impl DictationWorker {
    pub fn start() -> Self {
        let (jobs, job_receiver) = mpsc::sync_channel::<WorkerJob>(1);
        let (event_sender, events) = mpsc::channel();
        thread::spawn(move || {
            let mut parakeet = match Parakeet::load() {
                Ok(parakeet) => {
                    tracing::info!("Parakeet v2 loaded");
                    parakeet
                }
                Err(error) => {
                    let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                    return;
                }
            };
            let mut paster = match Paster::new() {
                Ok(paster) => paster,
                Err(error) => {
                    let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                    return;
                }
            };
            let mut last_transcript = None;
            while let Ok(job) = job_receiver.recv() {
                let event = match job {
                    WorkerJob::Transcribe { clip, send } => {
                        let total_started = Instant::now();
                        let prepare_started = Instant::now();
                        let samples = clip.into_parakeet_samples();
                        let result = (|| -> Result<String> {
                            let prepare = prepare_started.elapsed();
                            let inference_started = Instant::now();
                            let text = strip_dictation_control(&parakeet.transcribe(&samples)?);
                            let inference = inference_started.elapsed();
                            let paste_started = Instant::now();
                            if !text.trim().is_empty() {
                                paster.paste(&text)?;
                                if send {
                                    crate::keyboard::post_enter()?;
                                }
                                last_transcript = Some(text.clone());
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
                        WorkerEvent::Completed(result)
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
                if event_sender.send(event).is_err() {
                    break;
                }
            }
        });
        Self { jobs, events }
    }

    pub fn transcribe(&self, clip: DictationClip, send: bool) -> Result<(), &'static str> {
        self.jobs
            .try_send(WorkerJob::Transcribe { clip, send })
            .map_err(|error| match error {
                TrySendError::Full(_) => "dictation is already busy",
                TrySendError::Disconnected(_) => "Parakeet is unavailable",
            })
    }

    pub fn paste_last(&self) -> Result<(), &'static str> {
        self.jobs
            .try_send(WorkerJob::PasteLast)
            .map_err(|error| match error {
                TrySendError::Full(_) => "dictation is already busy",
                TrySendError::Disconnected(_) => "dictation worker is unavailable",
            })
    }

    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }
}

fn strip_dictation_control(text: &str) -> String {
    let mut text = text.trim();
    while let Some((_, prefix_end)) = dictation_control_suffix(text) {
        text = text[..prefix_end].trim_end_matches(|character: char| {
            character.is_whitespace() || character.is_ascii_punctuation()
        });
    }
    text.to_string()
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
        self.model
            .transcribe_with(
                samples,
                &ParakeetParams {
                    timestamp_granularity: Some(TimestampGranularity::Segment),
                    ..Default::default()
                },
            )
            .map(|result| result.text)
            .wrap_err("Parakeet transcription failed")
    }
}

#[cfg(test)]
mod tests {
    use super::strip_dictation_control;

    #[test]
    fn removes_voice_dictation_control_suffix() {
        assert_eq!(
            strip_dictation_control("Hello from voice control. Dictate send."),
            "Hello from voice control"
        );
        assert_eq!(strip_dictation_control("Dictate stop."), "");
        assert_eq!(
            strip_dictation_control("How are we doing here? Dictates stop."),
            "How are we doing here"
        );
        assert_eq!(
            strip_dictation_control("Testing it. Dictate send. Dictate send."),
            "Testing it"
        );
        assert_eq!(
            strip_dictation_control("Keep dictate here."),
            "Keep dictate here."
        );
    }
}
