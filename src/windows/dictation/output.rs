//! The Windows listener's output stage: a transcribed job becomes a paste, a
//! clipboard copy when the shell is in front, History, and Paste Last's text.

use std::sync::Mutex;
use std::time::Instant;

use color_eyre::eyre::Result;

use crate::control::PrintedTranscript;
use crate::windows::paste::{Paste, WindowsPaster};

use super::{Job, JobResult, OutputKind};

#[allow(clippy::too_many_arguments)]
pub(super) fn process_dictation_job(
    mut job: Job,
    paster: &mut Result<WindowsPaster>,
    history: Option<&crate::history::History>,
    last_dictation: &Mutex<Option<String>>,
    mode_runtime: &std::sync::RwLock<crate::dictation::modes::ModeRuntime>,
    transformations: &crate::personal_commands::TransformationClient,
) -> JobResult {
    let started = Instant::now();
    let printed = job.print.is_some();
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
    let application = context.application.clone();
    let mut sent = false;
    let mut copied = false;
    let mut stop_phrase_only = false;
    let result = transcription.and_then(|raw_text| {
        // Labs › Stop phrase: the words that finished this capture never
        // reach the text or its processing.
        let raw_text =
            crate::dictation::stop_phrase::strip_finished(raw_text, job.stop_phrase.as_deref());
        if raw_text.is_empty() && job.stop_phrase.is_some() {
            // A capture that was only the phrase: nothing to paste, no
            // History, and nothing failed.
            stop_phrase_only = true;
            return Ok(String::new());
        }
        if raw_text.is_empty() {
            return Err("transcription was empty".into());
        }
        let processed = if job.flavour.raw() {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        } else {
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
        };
        let text = processed.text.clone();
        // A voice prompt that deliberately asked nothing ends quietly: no
        // failure, no paste, no last dictation, and no History.
        if processed.nothing {
            return Ok(text);
        }
        if text.trim().is_empty() {
            return Err("text replacements produced empty output".into());
        }
        // A printed dictation is never pasted, and Paste Last skips it.
        if !printed {
            let paster = paster.as_mut().map_err(|error| format!("{error:#}"))?;
            // Labs › YOLO mode, read as the output commits.
            let sends = job.by_shortcut
                && !job.flavour.raw()
                && crate::dictation::yolo_mode::sends(application.as_deref(), None);
            // Labs › Copy dictation only, read as the output commits:
            // dictations go to the clipboard unpasted; Paste Last still pastes.
            // A dictation YOLO mode sends is pasted anyway.
            let copy_only = crate::labs::copy_only() && !sends;
            let pasted = if copy_only {
                paster.copy_only(&text).map(|()| Paste::Copied)
            } else {
                paster.paste(&text, job.flavour.terminal_paste(), job.return_to)
            };
            sent = matches!(pasted, Ok(Paste::Sent)) && sends;
            let pasted = if sent {
                paster.press_return().map(|()| Paste::Sent)
            } else {
                pasted
            };
            copied = matches!(pasted, Ok(Paste::Copied));
            // A failed paste still keeps the text for Paste Last.
            *last_dictation
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(text.clone());
            pasted.map_err(|error| format!("{error:#}"))?;
        }
        if let Some(history) = history {
            let mut draft = crate::dictation::modes::history_draft(
                raw_text,
                &processed,
                application.clone(),
                job.audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
                job.recording.take(),
            );
            if printed {
                draft.kind = crate::history::HistoryKind::Printed;
            } else if sent {
                draft.kind = crate::history::HistoryKind::Send;
            } else if job.flavour.raw() {
                draft.kind = crate::history::HistoryKind::Raw;
            }
            if let Err(error) = history.record(draft) {
                tracing::warn!(%error, "could not retain Windows dictation history");
            }
        }
        Ok(text)
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
    } else {
        job_result(OutputKind::Dictation, result, latency_ms)
    };
    done.stop_phrase_only = stop_phrase_only;
    done.copied = copied;
    done
}

pub(super) fn process_paste_last(
    paster: &mut Result<WindowsPaster>,
    last_dictation: &Mutex<Option<String>>,
) -> JobResult {
    let started = Instant::now();
    let mut copied = false;
    let result = last_dictation
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .ok_or_else(|| "there is no completed dictation to paste".to_string())
        .and_then(|text| {
            let paste = paster
                .as_mut()
                .map_err(|error| format!("{error:#}"))?
                .paste(&text, false, None)
                .map_err(|error| format!("{error:#}"))?;
            copied = paste == Paste::Copied;
            Ok(text)
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
