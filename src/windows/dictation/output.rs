//! The Windows listener's output stage: a transcribed job becomes a paste, a
//! clipboard copy when the shell is in front or Copy dictation only is on,
//! History, and Paste Last's text, driven by `crate::dictation::modes::output`.

use std::sync::Mutex;
use std::time::Instant;

use color_eyre::eyre::Result;

use crate::control::PrintedTranscript;
use crate::dictation::modes::output::{
    DeliveryInput, DeliveryOutcome, EMPTY_PROCESSED_ERROR, EMPTY_TRANSCRIPT_ERROR, InsertPlan,
    InsertResult, OutputAdapter, OutputTarget, PreparedTranscript, deliver, prepare_transcript,
    resolve_paste_last,
};
use crate::dictation::paste_last::LastDictation;
use crate::windows::paste::{CaptureWindow, Paste, WindowsPaster};

use super::{Job, JobResult, OutputKind};

struct WindowsOutputAdapter<'a> {
    paster: &'a mut Result<WindowsPaster>,
    terminal_paste: bool,
    return_to: Option<CaptureWindow>,
    last_dictation: &'a Mutex<Option<LastDictation>>,
}

impl OutputAdapter for WindowsOutputAdapter<'_> {
    fn insert(&mut self, text: &str, plan: InsertPlan) -> Result<InsertResult, String> {
        let paster = self.paster.as_mut().map_err(|error| format!("{error:#}"))?;
        match plan {
            InsertPlan::CopyOnly => {
                paster
                    .copy_only(text)
                    .map_err(|error| format!("{error:#}"))?;
                Ok(InsertResult {
                    sent: false,
                    copied: true,
                })
            }
            InsertPlan::Paste { send, standalone } => {
                let pasted = paster
                    .paste(text, self.terminal_paste, self.return_to, standalone)
                    .map_err(|error| format!("{error:#}"))?;
                if pasted == Paste::Sent && send {
                    paster
                        .press_return()
                        .map_err(|error| format!("{error:#}"))?;
                    Ok(InsertResult {
                        sent: true,
                        copied: false,
                    })
                } else {
                    Ok(InsertResult {
                        sent: false,
                        copied: pasted == Paste::Copied,
                    })
                }
            }
        }
    }

    fn remember_last(&mut self, last: LastDictation, _recorded_in_history: bool) {
        *self
            .last_dictation
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(last);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_dictation_job(
    mut job: Job,
    paster: &mut Result<WindowsPaster>,
    history: Option<&crate::history::History>,
    last_dictation: &Mutex<Option<LastDictation>>,
    mode_runtime: &std::sync::RwLock<crate::dictation::modes::ModeRuntime>,
    transformations: &crate::personal_commands::TransformationClient,
) -> JobResult {
    let started = Instant::now();
    let target = OutputTarget::from_flavour(job.flavour, job.print.is_some());
    let inference_started = Instant::now();
    let transcription = job.transcript.wait().map(|transcribed| {
        tracing::info!(
            source = transcribed.source.label(),
            finish_ms = transcribed.finish_ms,
            audio_ms = job.audio_ms,
            "transcribed Windows dictation"
        );
        transcribed.text
    });
    let inference_ms = inference_started.elapsed().as_millis() as u64;
    let context = job.context.finish();
    let mut copied = false;
    let mut stop_phrase_only = false;
    let result = transcription.and_then(|raw_text| {
        let raw_text = match prepare_transcript(raw_text, job.stop_phrase.as_deref(), target) {
            PreparedTranscript::StopPhraseOnly => {
                stop_phrase_only = true;
                return Ok(String::new());
            }
            PreparedTranscript::Empty => {
                if let Some(print) = job.print.take() {
                    print.send(PrintedTranscript::NoSpeech);
                }
                return Err(EMPTY_TRANSCRIPT_ERROR.into());
            }
            PreparedTranscript::Ready(text) => text,
        };
        let processed = if target.processes() {
            let mode_runtime = mode_runtime
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            mode_runtime.process(
                &raw_text,
                &job.language,
                &context,
                transformations,
                crate::dictation::processor::Source::Dictation,
            )
        } else {
            crate::dictation::processor::Processed::unchanged(&raw_text)
        };
        let input = DeliveryInput {
            target,
            raw_text,
            text: processed.text,
            nothing: processed.nothing,
            application: context.application,
            bundle_name: None,
            processing: processed.observation,
            audio_ms: job.audio_ms,
            inference_ms,
            total_started: started,
            recording: job.recording.take(),
            allow_yolo: job.by_shortcut,
        };
        let mut adapter = WindowsOutputAdapter {
            paster,
            terminal_paste: job.flavour.terminal_paste(),
            return_to: job.return_to,
            last_dictation,
        };
        match deliver(input, history, &mut adapter)? {
            DeliveryOutcome::VoicePromptNothing => Ok(String::new()),
            DeliveryOutcome::EmptyOutput => Err(EMPTY_PROCESSED_ERROR.into()),
            DeliveryOutcome::Delivered {
                text,
                copied: was_copied,
                ..
            } => {
                copied = was_copied;
                Ok(text)
            }
        }
    });
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    tracing::info!(
        audio_ms = job.audio_ms,
        latency_ms,
        "completed Windows dictation job"
    );
    let mut done = if let Some(print) = job.print.take() {
        print.send(match &result {
            Ok(text) if text.trim().is_empty() => PrintedTranscript::NoSpeech,
            Ok(text) => PrintedTranscript::Text(text.clone()),
            Err(error) => PrintedTranscript::Failed(error.clone()),
        });
        job_result(OutputKind::Printed, result, latency_ms)
    } else if target.is_print() {
        job_result(OutputKind::Printed, result, latency_ms)
    } else {
        job_result(OutputKind::Dictation, result, latency_ms)
    };
    done.stop_phrase_only = stop_phrase_only;
    done.copied = copied;
    done
}

pub(super) fn process_paste_last(
    paster: &mut Result<WindowsPaster>,
    last_dictation: &Mutex<Option<LastDictation>>,
) -> JobResult {
    let started = Instant::now();
    let mut copied = false;
    let last = last_dictation
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let result = resolve_paste_last(last.as_ref(), None)
        .map_err(str::to_owned)
        .and_then(|last| {
            let paste = paster
                .as_mut()
                .map_err(|error| format!("{error:#}"))?
                .paste(last.text(), false, None, last.standalone())
                .map_err(|error| format!("{error:#}"))?;
            copied = paste == Paste::Copied;
            Ok(last.text().to_owned())
        });
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    tracing::info!(latency_ms, "completed Windows Paste Last job");
    let mut done = job_result(OutputKind::Repaste, result, latency_ms);
    done.copied = copied;
    done
}

fn job_result(kind: OutputKind, result: Result<String, String>, latency_ms: u32) -> JobResult {
    match result {
        Ok(text) => JobResult {
            kind,
            text: Some(text),
            latency_ms,
            result: Ok(()),
            stop_phrase_only: false,
            copied: false,
        },
        Err(error) => JobResult {
            kind,
            text: None,
            latency_ms,
            result: Err(error),
            stop_phrase_only: false,
            copied: false,
        },
    }
}
