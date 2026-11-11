//! Finishing a shortcut capture and submitting its clip to the dictation
//! worker, including a recording the microphone cut short.

use std::time::Duration;

use color_eyre::Result;

use crate::audio::CaptureInstant;
use crate::desktop::events::{DictationPhase, EventLog, VoiceEvent, VoiceState, now_ms};
use crate::dictation::audio::DictationAudio;
use crate::dictation::context::ContextSnapshot;
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::dictation::{DictationClip, Finish};
use crate::feedback::{self, Tone};
use crate::parakeet::{DictationJobId, DictationWorker, TranscriptionTarget};

use super::{DictationSink, FinishResult};

/// Finishes a recording capture the microphone cut short. A kept clip becomes
/// a copy-only dictation, or answers a print-only one; anything else keeps
/// the previous failure, with its error tone and Failed HUD, and tells a
/// printing command it failed.
pub(super) fn interrupted_dictation(
    salvaged: Option<(DictationClip, ContextSnapshot)>,
    mut print: Option<crate::control::PrintReply>,
    raw: bool,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    let target = if raw {
        TranscriptionTarget::RawCopyOnlyPaste
    } else {
        TranscriptionTarget::CopyOnlyPaste
    };
    let mut silent = false;
    if let Some((clip, context)) = salvaged {
        let reply = print.take();
        let submit = |clip| match reply {
            Some(reply) => worker.transcribe_printed(clip, reply, raw, context),
            None => worker.transcribe(clip, target, context),
        };
        if salvage_dictation(clip, submit, device, events, indicator)?.is_some() {
            // The capture ended with a job, not a failure: the HUD keeps
            // transcribing until the copy completes.
            return super::emit_engine_state(events, false, worker, device);
        }
        silent = true;
    }
    if let Some(print) = print {
        // A silent clip has no words to print; every other end failed.
        print.send(if silent {
            crate::control::PrintedTranscript::NoSpeech
        } else {
            crate::control::PrintedTranscript::Failed("the microphone was interrupted".into())
        });
    }
    super::fail_capture(
        "Microphone capture was interrupted; reconnecting.".into(),
        worker,
        device,
        events,
        indicator,
    )
}

/// A shortcut dictation or `hex dictation start` control capture whose
/// microphone failed keeps the audio delivered before the loss and hands it
/// to `submit`: a normal one is transcribed and processed like an ordinary
/// dictation, a raw one as heard, and either is copied instead of pasted
/// because the app or call that disrupted the microphone may have taken
/// focus. A print-only one answers the command that started it.
///
/// Returns `None` for a near-silent clip, after saving its diagnostics: the
/// caller keeps the previous microphone-loss failure for it.
pub(super) fn salvage_dictation(
    clip: DictationClip,
    submit: impl FnOnce(DictationClip) -> Result<DictationJobId, &'static str>,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<Option<FinishResult>> {
    if clip.is_near_silent() {
        crate::dictation::diagnostics::persist_near_silent(&clip.into_transcription_samples());
        return Ok(None);
    }
    submit_dictation(clip, submit, true, device, events, indicator).map(Some)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_dictation(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    worker: &DictationWorker,
    target: TranscriptionTarget,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<FinishResult> {
    finish_dictation_with(
        capture,
        ended_at,
        tail,
        |clip, salvaged| {
            let target = match target {
                TranscriptionTarget::Paste if salvaged => TranscriptionTarget::CopyOnlyPaste,
                TranscriptionTarget::RawPaste if salvaged => TranscriptionTarget::RawCopyOnlyPaste,
                target => target,
            };
            worker.transcribe(clip, target, context.clone())
        },
        device,
        events,
        indicator,
    )
}

/// Finishes the capture and hands its clip to `submit`, which is told
/// whether the microphone cut the capture short.
pub(super) fn finish_dictation_with(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    submit: impl FnOnce(DictationClip, bool) -> Result<DictationJobId, &'static str>,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<FinishResult> {
    let clip = match capture.finish_with_tail(ended_at, tail)? {
        Finish::Transcribe(clip) if !clip.is_near_silent() => clip,
        // The microphone failed first and the audio owner kept what it
        // heard. Claim it now, under this capture's own target and context:
        // a press after this finish makes its loss notification stale.
        Finish::Discard if let Some(clip) = capture.take_kept_hotkey() => {
            return submit_dictation(
                clip,
                |clip| submit(clip, true),
                true,
                device,
                events,
                indicator,
            );
        }
        finish => {
            events.dictation(DictationPhase::Discarded, "")?;
            indicator.send(DictationIndicatorEvent::Discarded);
            return Ok(match finish {
                Finish::Discard => FinishResult::Discarded,
                Finish::Transcribe(clip) => {
                    crate::dictation::diagnostics::persist_near_silent(
                        &clip.into_transcription_samples(),
                    );
                    FinishResult::Silent
                }
            });
        }
    };
    // The audio owner already announced a release with a tail.
    if tail.is_zero() {
        feedback::play(Tone::DictationStop);
    }
    submit_dictation(
        clip,
        |clip| submit(clip, false),
        false,
        device,
        events,
        indicator,
    )
}

/// Reports the capture as transcribing and hands its clip to `submit`.
pub(super) fn submit_dictation(
    clip: DictationClip,
    submit: impl FnOnce(DictationClip) -> Result<DictationJobId, &'static str>,
    salvaged: bool,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<FinishResult> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Transcribing,
        device: device.into(),
    })?;
    if salvaged {
        events.salvaged_dictation()?;
    } else {
        events.dictation(DictationPhase::Transcribing, "")?;
    }
    match submit(clip) {
        Ok(job_id) => {
            indicator.send(DictationIndicatorEvent::Submitted {
                job_id: job_id.value(),
            });
            Ok(FinishResult::Submitted)
        }
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            indicator.send(DictationIndicatorEvent::Failed {
                message: Some(error.into()),
            });
            Ok(FinishResult::Rejected(error.into()))
        }
    }
}
