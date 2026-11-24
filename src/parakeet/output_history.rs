//! macOS insertion and Paste Last adapter over the shared finished-dictation
//! output policy (`crate::dictation::modes::output`).
use super::*;
use crate::dictation::modes::output::{
    DeliveryInput, DeliveryOutcome, InsertPlan, InsertResult, OutputAdapter, deliver,
    resolve_paste_last,
};

pub(super) fn deliver_completed(
    target: TranscriptionTarget,
    completed: CompletedTranscript,
    paste: &mut impl FnMut(&str, PasteMode) -> Result<String>,
    last_transcript: &mut Option<LastDictation>,
    history: Option<&History>,
) -> Result<String, String> {
    let paste_started = Instant::now();
    let timings = completed.timings;
    let return_to = completed.return_to;
    let input = DeliveryInput {
        target,
        raw_text: completed.raw,
        text: completed.text,
        nothing: completed.nothing,
        application: completed.application,
        bundle_name: completed.bundle_name,
        processing: completed.processing,
        audio_ms: timings.audio_ms,
        inference_ms: timings.inference_ms as u64,
        total_started: timings.total_started,
        recording: completed.recording,
        allow_yolo: timings.send_gate.is_some(),
    };
    let mut adapter = MacOutputAdapter {
        target,
        return_to,
        send_gate: timings.send_gate,
        paste,
        last_transcript,
        history,
    };
    let outcome = deliver(input, history, &mut adapter)?;
    let text = match outcome {
        DeliveryOutcome::VoicePromptNothing | DeliveryOutcome::EmptyOutput => String::new(),
        DeliveryOutcome::Delivered { text, .. } => text,
    };
    tracing::info!(
        audio_ms = timings.audio_ms,
        queue_ms = timings.queue_ms,
        prepare_ms = timings.prepare_ms,
        inference_ms = timings.inference_ms,
        paste_ms = paste_started.elapsed().as_millis(),
        total_ms = timings.total_started.elapsed().as_millis(),
        "dictation pipeline completed"
    );
    Ok(text)
}

struct MacOutputAdapter<'a, F> {
    target: TranscriptionTarget,
    return_to: Option<i32>,
    send_gate: Option<crate::dictation::yolo_mode::SendGate>,
    paste: &'a mut F,
    last_transcript: &'a mut Option<LastDictation>,
    history: Option<&'a History>,
}

impl<F: FnMut(&str, PasteMode) -> Result<String>> OutputAdapter for MacOutputAdapter<'_, F> {
    fn clipboard_text(&mut self) -> Option<String> {
        crate::dictation::clipboard_phrase::system_text()
    }

    fn insert(&mut self, text: &str, plan: InsertPlan) -> Result<InsertResult, String> {
        let (mode, send) = match plan {
            InsertPlan::CopyOnly => (PasteMode::CopyOnly, false),
            InsertPlan::Paste {
                send,
                standalone: true,
            } => (PasteMode::Standalone, send),
            InsertPlan::Paste {
                send: true,
                standalone: false,
            } => (PasteMode::Send, true),
            InsertPlan::Paste {
                send: false,
                standalone: false,
            } => (PasteMode::Continue, false),
        };
        // A posting refusal means no keystroke can land, so say so before
        // another app is brought forward (macOS 27.3's keystroke grant).
        if !matches!(mode, PasteMode::CopyOnly) {
            crate::paste::ensure_keystroke_access().map_err(|error| error.to_string())?;
        }
        // Labs › Return to app: the app the capture started in comes back
        // first. When it can't, nothing is pasted; the shared delivery policy
        // keeps the text for Paste last dictation and skips History.
        if !matches!(mode, PasteMode::CopyOnly)
            && let Err(error) = crate::paste::return_to_app::bring_to_front(self.return_to)
        {
            tracing::warn!(
                %error,
                "a dictation found another app in front and couldn't return to its own"
            );
            return Err(error.to_string());
        }
        let pasted = (self.paste)(text, mode).map_err(|error| error.to_string())?;
        let sent = if send {
            self.send_gate
                .take()
                .is_some_and(|gate| send_unless_escaped(gate, Instant::now(), self.paste))
        } else {
            false
        };
        #[cfg(target_os = "macos")]
        correction_watch::watch_paste(&pasted, self.target, mode, sent);
        Ok(InsertResult {
            sent,
            copied: matches!(mode, PasteMode::CopyOnly),
        })
    }

    fn remember_last(&mut self, last: LastDictation, recorded_in_history: bool) {
        if recorded_in_history {
            *self.last_transcript = Some(last);
        } else {
            recover_output(self.history, self.last_transcript, last);
        }
    }
}

#[cfg(test)]
pub(super) fn record_history(
    history: &History,
    target: TranscriptionTarget,
    sent: bool,
    completed: &mut CompletedTranscript,
) -> Option<u64> {
    let mut input = DeliveryInput {
        target,
        raw_text: completed.raw.clone(),
        text: completed.text.clone(),
        nothing: completed.nothing,
        application: completed.application.clone(),
        bundle_name: completed.bundle_name.clone(),
        processing: completed.processing.clone(),
        audio_ms: completed.timings.audio_ms,
        inference_ms: completed.timings.inference_ms as u64,
        total_started: completed.timings.total_started,
        recording: completed.recording.take(),
        allow_yolo: completed.timings.send_gate.is_some(),
    };
    crate::dictation::modes::output::record_history(history, target, sent, &mut input)
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
    let last = resolve_paste_last(last, recovered).map_err(str::to_owned)?;
    let mode = if last.standalone() {
        PasteMode::Standalone
    } else {
        PasteMode::Continue
    };
    paste(last.text(), mode).map_err(|error| error.to_string())?;
    Ok(last.text().to_owned())
}
