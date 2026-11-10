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

/// A recording shortcut whose microphone failed keeps the audio delivered
/// before the loss. It is transcribed and processed like an ordinary
/// dictation, but copied instead of pasted: the app or call that disrupted
/// the microphone may have taken focus. Paste Last Dictation and History keep
/// it like any other dictation.
pub(super) fn salvage_dictation(
    clip: DictationClip,
    worker: &DictationWorker,
    context: ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<FinishResult> {
    if clip.is_near_silent() {
        crate::dictation::diagnostics::persist_near_silent(&clip.into_transcription_samples());
        events.discarded_dictation(Some("microphone-lost"))?;
        indicator.send(DictationIndicatorEvent::Discarded);
        return Ok(FinishResult::Silent);
    }
    submit_dictation(
        clip,
        |clip| worker.transcribe(clip, TranscriptionTarget::CopyOnlyPaste, context),
        true,
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
