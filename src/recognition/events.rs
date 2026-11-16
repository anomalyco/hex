//! What the dictation worker's events do to the HUD, the Caps Lock light, and
//! the activity log.

use color_eyre::Result;

use super::DictationSink;
use crate::desktop::events::{DictationPhase, EventLog};
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::feedback::{self, Tone};
use crate::parakeet::{DictationJobStage, PasteKind, TranscriptionTarget, WorkerEvent};

/// Reports one finished stage of the dictation worker's work.
pub(super) fn handle_dictation_event(
    event: WorkerEvent,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    match event {
        WorkerEvent::Completed {
            job_id,
            result: Ok(text),
            ..
        } if text.trim().is_empty() => {
            events.dictation(DictationPhase::Discarded, "")?;
            indicator.send(DictationIndicatorEvent::JobCompleted {
                job_id: job_id.value(),
            });
        }
        WorkerEvent::Completed {
            job_id,
            target,
            result: Ok(text),
            processing,
        } => {
            let phase = match target {
                TranscriptionTarget::Paste
                | TranscriptionTarget::CopyOnlyPaste
                | TranscriptionTarget::RawCopyOnlyPaste
                | TranscriptionTarget::RawPaste => DictationPhase::Pasted,
                TranscriptionTarget::Print | TranscriptionTarget::RawPrint => {
                    DictationPhase::Printed
                }
                TranscriptionTarget::Service => return Ok(()),
            };
            feedback::play(Tone::DictationDone);
            events.processed_dictation(phase, text, processing)?;
            indicator.send(DictationIndicatorEvent::JobCompleted {
                job_id: job_id.value(),
            });
        }
        WorkerEvent::Completed {
            job_id,
            result: Err(error),
            ..
        } => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.clone()), "")?;
            indicator.send(DictationIndicatorEvent::JobFailed {
                job_id: job_id.value(),
                message: Some(error),
            });
        }
        WorkerEvent::Stage { job_id, stage } => {
            indicator.send(match stage {
                DictationJobStage::Transcribing => DictationIndicatorEvent::Transcribing {
                    job_id: job_id.value(),
                },
                DictationJobStage::Processing => DictationIndicatorEvent::Processing {
                    job_id: job_id.value(),
                },
            });
        }
        WorkerEvent::ModelFailed(error) => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.clone()), "")?;
            indicator.send(DictationIndicatorEvent::Failed {
                message: Some(error),
            });
        }
        WorkerEvent::Pasted {
            kind,
            result: Ok(text),
        } => {
            let phase = match kind {
                PasteKind::LastTranscript => DictationPhase::Repasted,
                PasteKind::MeetingDelta => DictationPhase::MeetingPasted,
            };
            events.dictation(phase, text)?;
        }
        WorkerEvent::Pasted {
            result: Err(error), ..
        } => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
        }
    }
    Ok(())
}
