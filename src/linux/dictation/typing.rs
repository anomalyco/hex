//! The Linux listener's Labs › Live typing glue: when a capture's session
//! starts, the input-method session that shows its provisional words and
//! commits its final text. Split from [`super`] to keep it under the source
//! cap.

use std::sync::Arc;

use crate::dictation::DictationCapture;

/// A hotkey capture whose provisional words go into the focused text field.
pub(super) struct CaptureTyping {
    pub(super) session: crate::linux::live_typing::Session,
    /// Keeps the dictation stream publishing its text without the HUD plate.
    _demand: crate::dictation::live_text::TypingDemand,
}

/// Starts Live typing at the hold threshold (or the Toggle press), before
/// the stream opens so the engine sees the demand. It does not wait for a
/// stream: a CPU or non-streaming model still commits its final text through
/// the input method, with no preedit words. Short taps and chords never get
/// here, so they create no input method.
fn due(recording: bool, intentional: bool, toggle: bool) -> bool {
    recording && (toggle || intentional)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn begin(
    capture: &DictationCapture,
    toggle: bool,
    wayland: bool,
    live_typing: &mut Option<Arc<crate::linux::live_typing::LiveTyping>>,
    next_capture_id: &mut u64,
    typing: &mut Option<CaptureTyping>,
) {
    if typing.is_some()
        || !wayland
        || !crate::labs::live_typing()
        || !due(capture.is_recording(), capture.is_intentional(), toggle)
    {
        return;
    }
    if !live_typing.as_ref().is_none_or(|live| live.is_accepting()) {
        return;
    }
    let live = live_typing
        .get_or_insert_with(crate::linux::live_typing::LiveTyping::start)
        .clone();
    *next_capture_id += 1;
    let mut started = CaptureTyping {
        session: crate::linux::live_typing::Session::new(live, *next_capture_id),
        _demand: crate::dictation::live_text::typing_demand(),
    };
    started.session.start();
    *typing = Some(started);
}

/// Ends a capture's Live typing session without committing anything; the
/// session guard clears the preedit.
pub(super) fn cancel(typing: &mut Option<CaptureTyping>) {
    typing.take();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_typing_waits_for_a_recording_and_the_hold_or_toggle() {
        assert!(!due(false, false, true), "idle Toggle chunks start nothing");
        assert!(
            !due(true, false, false),
            "a Hold press waits for the threshold"
        );
        assert!(due(true, true, false));
        assert!(due(true, false, true), "Toggle starts at once");
    }
}
