//! Whether a press may act, the instant its edge applies to, and the refusal
//! a control gets while it can't start a capture.

use crate::audio::CaptureInstant;
use crate::control::ControlReply;
use crate::dictation::audio::DictationAudio;

/// Whether Hex takes Escape from the app in front: for a capture (drawing,
/// its press, the capsule's picker) and a held dictation it cancels, never a
/// hands-free one, and never while a programmatic dictation records: the
/// listener ignores every key then, so Escape stays with its owner app
/// rather than being swallowed.
pub(super) fn claims_escape(
    programmatic: bool,
    capture_escape: bool,
    held_dictation: bool,
) -> bool {
    !programmatic && (capture_escape || held_dictation)
}

/// Whether a Caps Lock press may act. An Annotate capture records the
/// narration itself, so a Caps Lock press must not start a dictation over it;
/// a press that finishes an already-running Caps Lock capture still works.
pub(super) fn caps_lock_can_press(caps_recording: bool, annotate_recording: bool) -> bool {
    caps_recording || !annotate_recording
}

/// Why a control can't start a capture now, if it can't.
pub(super) fn capture_refusal(other_recording: bool, suspended: bool) -> Option<ControlReply> {
    if other_recording {
        Some(ControlReply::error(
            "busy",
            "another recording is in progress; finish it first",
        ))
    } else if suspended {
        Some(ControlReply::error(
            "busy",
            "Hex is recording a shortcut in Settings or reopening the microphone; try again",
        ))
    } else {
        None
    }
}

/// A `hex paste-last` (or the menu bar's Paste Last Dictation) waits for
/// every capture and shortcut gesture to end: pasting cancels the audio
/// owner's capture, which would strand the machine that started it.
pub(super) fn paste_last_refusal(capture_active: bool) -> Option<ControlReply> {
    capture_active.then(|| {
        ControlReply::error(
            "busy",
            "a recording or shortcut is in progress; finish it first",
        )
    })
}

/// The instant a shortcut edge applies to. A capture pressed while the
/// microphone was released has no authoritative timeline yet, so its edge
/// uses wall-clock time instead.
pub(super) fn capture_boundary(input: &DictationAudio, pressed_cold: bool) -> CaptureInstant {
    if pressed_cold {
        CaptureInstant::now()
    } else {
        input.captured_through()
    }
}

/// Whether a press sees a released microphone now. While the microphone is
/// being opened again the previous state stands: the stream's timeline starts
/// at its open, so a press must not anchor to the closed one's zero instant.
pub(super) fn release_for_press(
    release_microphone: bool,
    previous: bool,
    input_open: bool,
) -> bool {
    release_microphone || (previous && !input_open)
}

/// What a press, and the capture it starts, meet of the warm microphone.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Release {
    /// Whether a press now meets a released microphone: the setting is on or
    /// the person is away.
    pub(super) press: bool,
    /// Whether the capture recording now was pressed while released. The
    /// latch clears once the stream is back, but a capture pressed cold keeps
    /// its wall-clock edge and still owes its start tone.
    pub(super) capture: bool,
}

impl Release {
    /// The latches a listener starts with.
    pub(super) fn new(released: bool) -> Self {
        Self {
            press: released,
            capture: released,
        }
    }

    /// Records the capture starting now: it is cold exactly when a press
    /// meets a released microphone.
    pub(super) fn begin(&mut self) {
        self.capture = self.press;
    }

    /// This pass's press latch, keeping the recording capture's own record.
    fn observed(self, released: bool, input_open: bool) -> Self {
        Self {
            press: release_for_press(released, self.press, input_open),
            capture: self.capture,
        }
    }
}

/// The warm microphone's release, kept in step with the setting and with the
/// person being away (the display asleep, the session locked, or the Mac
/// asleep). The owner is told as either changes.
pub(super) struct WarmRelease {
    /// What the owner was last told to release.
    forwarded: bool,
    /// What the owner was last told about being away. It starts with neither,
    /// so the first pass tells it.
    forward_away: bool,
}

impl WarmRelease {
    /// The policy a listener starts with and the latches it acts on.
    pub(super) fn start() -> (Self, Release) {
        let away = crate::audio::system_idle::released();
        let released = crate::app_settings::microphone_policy().release_while_idle || away;
        (
            Self {
                forwarded: released,
                forward_away: false,
            },
            Release::new(released),
        )
    }

    /// Brings the owner up to date and returns the latches this pass acts on.
    pub(super) fn reconcile(&mut self, input: &DictationAudio, release: Release) -> Release {
        let away = crate::audio::system_idle::released();
        let released = crate::app_settings::microphone_policy().release_while_idle || away;
        if released != self.forwarded {
            input.set_release_while_idle(released);
            self.forwarded = released;
        }
        if away != self.forward_away {
            input.set_away(away);
            self.forward_away = away;
        }
        release.observed(released, input.is_open())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A capture pressed while the microphone was released keeps that record
    /// once the stream is back: its own tone and edge still depend on it,
    /// while the next press takes the stream's timeline again.
    #[test]
    fn a_capture_keeps_the_cold_press_it_started_from() {
        let mut release = Release::new(true);
        release.begin();
        assert!(release.press && release.capture);

        // The stream is back: the next press is warm...
        release = release.observed(false, true);
        assert!(!release.press);
        // ...but the capture started cold still owes its tone.
        assert!(release.capture);

        // A press started warm never owed one.
        release.begin();
        assert!(!release.press && !release.capture);
    }
}
