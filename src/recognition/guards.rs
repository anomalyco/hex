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

/// The instant a shortcut edge applies to. A released microphone has no
/// authoritative timeline yet, so the edge uses wall-clock time instead.
pub(super) fn capture_boundary(input: &DictationAudio, release_while_idle: bool) -> CaptureInstant {
    if release_while_idle {
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

/// Brings the owner's release-while-idle up to date, and returns the flag it
/// now holds with the one a press acts on. The warm microphone is released
/// while the setting asks or the person demonstrably cannot dictate (the
/// display asleep, the session locked, or the Mac asleep).
pub(super) fn reconcile_release(
    input: &DictationAudio,
    previous: bool,
    forwarded: bool,
) -> (bool, bool) {
    let release = crate::app_settings::microphone_policy().release_while_idle
        || crate::audio::system_idle::released();
    if release != forwarded {
        input.set_release_while_idle(release);
    }
    (
        release,
        release_for_press(release, previous, input.is_open()),
    )
}
