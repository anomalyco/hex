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

/// Labs › Signature: a normal dictation carries the saved line once at its
/// end, before the paste, History, and Paste Last read the same text. A raw,
/// printed, or API transcript stays unsigned.
pub(super) fn sign_output(
    target: TranscriptionTarget,
    completed: &mut CompletedTranscript,
) -> bool {
    if !matches!(
        target,
        TranscriptionTarget::Paste | TranscriptionTarget::CopyOnlyPaste
    ) {
        return false;
    }
    let Some(text) = crate::dictation::signature::append(
        &completed.text,
        &crate::labs::signature_text(),
        crate::labs::signature(),
    ) else {
        return false;
    };
    completed.text = text;
    true
}

pub(super) fn recover_output(
    history: Option<&History>,
    last: &mut Option<LastDictation>,
    kept: LastDictation,
) {
    if let Some(history) = history {
        history.recover_output(kept);
        *last = None;
    } else {
        *last = Some(kept);
    }
}

/// Labs › YOLO mode, after a `Send` paste landed at `pasted_at`: presses
/// Return unless Escape came after the release. Returns whether it sent; the
/// text stays pasted either way.
pub(super) fn send_unless_escaped(
    gate: crate::dictation::yolo_mode::SendGate,
    pasted_at: Instant,
    paste: &mut impl FnMut(&str, PasteMode) -> Result<String>,
) -> bool {
    if !gate.wait(pasted_at) {
        tracing::info!("YOLO mode pasted without Return: Escape after the release");
        return false;
    }
    match paste("", PasteMode::Return) {
        Ok(_) => {
            tracing::info!(
                after_submit_ms = gate.submitted_at().elapsed().as_millis(),
                "YOLO mode pressed Return"
            );
            true
        }
        Err(error) => {
            tracing::warn!(%error, "YOLO mode could not press Return");
            false
        }
    }
}

/// Paste Last: reinsert the remembered dictation, standing alone when it
/// carries the signature line so the next dictation never joins it.
pub(super) fn paste_last(
    last: Option<&LastDictation>,
    recovered: Option<LastDictation>,
    paste: &mut impl FnMut(&str, PasteMode) -> Result<String>,
) -> Result<String, String> {
    let last = last
        .cloned()
        .or(recovered)
        .ok_or_else(|| "no previous transcript is available".to_string())?;
    let mode = if last.standalone() {
        PasteMode::Standalone
    } else {
        PasteMode::Continue
    };
    paste(last.text(), mode).map_err(|error| error.to_string())?;
    Ok(last.text().to_owned())
}
