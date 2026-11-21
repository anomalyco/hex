//! What ordered output keeps after insertion or recovery.
use super::*;

/// Record one successfully pasted result. History failures must never fail
/// the paste that already happened.
pub(super) fn record_history(
    history: &History,
    target: TranscriptionTarget,
    sent: bool,
    completed: &mut CompletedTranscript,
) -> Option<u64> {
    let kind = match target {
        TranscriptionTarget::Paste if sent => HistoryKind::Send,
        TranscriptionTarget::Paste | TranscriptionTarget::CopyOnlyPaste => HistoryKind::Dictation,
        TranscriptionTarget::RawPaste | TranscriptionTarget::RawCopyOnlyPaste => HistoryKind::Raw,
        TranscriptionTarget::Print | TranscriptionTarget::RawPrint => HistoryKind::Printed,
        TranscriptionTarget::Service => return None,
    };
    let draft = HistoryDraft {
        kind,
        raw_text: completed.raw.clone(),
        final_text: completed.text.clone(),
        application: completed.application.clone(),
        processing: completed.processing.clone(),
        audio_ms: completed.timings.audio_ms,
        inference_ms: completed.timings.inference_ms as u64,
        total_ms: completed.timings.total_started.elapsed().as_millis() as u64,
        recording: completed.recording.take(),
        model: crate::history::dictation_model(),
    };
    match history.record(draft) {
        Ok(id) => id,
        Err(error) => {
            tracing::warn!(%error, "could not record dictation history");
            None
        }
    }
}

pub(super) fn recover_output(history: Option<&History>, last: &mut Option<String>, text: String) {
    if let Some(history) = history {
        history.recover_output(text);
        *last = None;
    } else {
        *last = Some(text);
    }
}
