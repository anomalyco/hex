//! Submitting a finished shortcut clip to the dictation worker, including a
//! recording the microphone cut short.

use color_eyre::Result;

use crate::desktop::events::{DictationPhase, EventLog, VoiceEvent, VoiceState, now_ms};
use crate::dictation::DictationClip;
use crate::dictation::context::ContextSnapshot;
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::feedback::{self, Tone};
use crate::parakeet::{DictationJobId, DictationWorker, TranscriptionTarget};

use super::{DictationSink, FinishResult};

/// Finishes a recording capture the microphone cut short. A kept clip becomes
/// a copy-only dictation; anything else keeps the previous failure, with its
/// error tone and Failed HUD.
pub(super) fn interrupted_dictation(
    salvaged: Option<(DictationClip, ContextSnapshot)>,
    raw: bool,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    if let Some((clip, context)) = salvaged
        && salvage_dictation(clip, raw, worker, context, device, events, indicator)?.is_some()
    {
        // The capture ended with a job, not a failure: the HUD keeps
        // transcribing until the copy completes.
        return super::emit_engine_state(events, false, worker, device);
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
/// microphone failed keeps the audio delivered before the loss. A normal one
/// is transcribed and processed like an ordinary dictation; a raw one is
/// transcribed as heard. Either way it is copied instead of pasted: the app
/// or call that disrupted the microphone may have taken focus. Paste Last
/// Dictation and History keep it like any other dictation.
///
/// Returns `None` for a near-silent clip, after saving its diagnostics: the
/// caller keeps the previous microphone-loss failure for it.
pub(super) fn salvage_dictation(
    clip: DictationClip,
    raw: bool,
    worker: &DictationWorker,
    context: ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<Option<FinishResult>> {
    if clip.is_near_silent() {
        crate::dictation::diagnostics::persist_near_silent(&clip.into_transcription_samples());
        return Ok(None);
    }
    let target = if raw {
        TranscriptionTarget::RawCopyOnlyPaste
    } else {
        TranscriptionTarget::CopyOnlyPaste
    };
    submit_dictation(
        clip,
        |clip| worker.transcribe(clip, target, context),
        true,
        device,
        events,
        indicator,
    )
    .map(Some)
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
