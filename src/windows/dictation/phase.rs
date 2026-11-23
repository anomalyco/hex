//! Explicit capture-lifecycle phases for the Windows dictation listener.
//!
//! Correlated per-capture state (`flavour`, `by_shortcut`, `return_to`,
//! `feedback_started`, `start_tone_played`, `stop_phrase`, and the post-release
//! `Finishing` payload) lives inside the variant that owns it, so ending or
//! cancelling a capture structurally drops every field instead of leaving stale
//! state for a later capture.

use crate::control::PrintReply;
use crate::dictation::{DictationFlavour, ReleaseWait};
use crate::windows::context::PendingContext;
use crate::windows::paste::CaptureWindow;

/// A hotkey press accepted while the microphone is not delivering yet;
/// recording starts at the first fresh audio chunk.
pub(super) struct PendingCapture {
    pub(super) flavour: DictationFlavour,
    pub(super) by_shortcut: bool,
    pub(super) return_to: Option<CaptureWindow>,
}

/// An active dictation capture collecting audio from the microphone.
pub(super) struct ActiveCapture {
    pub(super) flavour: DictationFlavour,
    pub(super) by_shortcut: bool,
    /// The window in front when this capture started, where its paste returns
    /// if the desktop or a taskbar is in front instead.
    pub(super) return_to: Option<CaptureWindow>,
    pub(super) feedback_started: bool,
    /// Whether this capture's start tone already played on its press.
    pub(super) start_tone_played: bool,
    /// Labs › Stop phrase: this capture's detector, fresh for every capture.
    pub(super) stop_phrase: crate::dictation::stop_phrase::Detector,
}

/// A release whose capture is still collecting its post-release tail.
pub(super) struct Finishing {
    pub(super) context: PendingContext,
    pub(super) release: ReleaseWait,
    pub(super) print: Option<PrintReply>,
    pub(super) flavour: DictationFlavour,
    /// A shortcut, not a `hex dictation` command, started the capture.
    pub(super) by_shortcut: bool,
    /// Labs › Stop phrase: the words that ended this capture, when they did.
    pub(super) stop_phrase: Option<String>,
    /// The window in front when the capture started.
    pub(super) return_to: Option<CaptureWindow>,
}

/// The shortcut/command capture's lifecycle on the Windows listener loop.
#[derive(Default)]
pub(super) enum CapturePhase {
    #[default]
    Idle,
    Pending(PendingCapture),
    Recording(ActiveCapture),
    Finishing(Finishing),
}

impl CapturePhase {
    pub(super) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub(super) fn is_pending(&self) -> bool {
        matches!(self, Self::Pending(_))
    }

    pub(super) fn is_recording(&self) -> bool {
        matches!(self, Self::Recording(_))
    }

    pub(super) fn is_finishing(&self) -> bool {
        matches!(self, Self::Finishing(_))
    }

    /// Pending or recording: the listener has an unfinished dictation capture.
    pub(super) fn is_dictating(&self) -> bool {
        matches!(self, Self::Pending(_) | Self::Recording(_))
    }

    /// Any non-idle phase (pending, recording, or collecting its release tail).
    pub(super) fn is_active(&self) -> bool {
        !self.is_idle()
    }

    pub(super) fn begin_recording(
        &mut self,
        flavour: DictationFlavour,
        by_shortcut: bool,
        return_to: Option<CaptureWindow>,
        feedback_started: bool,
        start_tone_played: bool,
    ) {
        *self = Self::Recording(ActiveCapture {
            flavour,
            by_shortcut,
            return_to,
            feedback_started,
            start_tone_played,
            stop_phrase: crate::dictation::stop_phrase::Detector::new(),
        });
    }

    pub(super) fn begin_pending(
        &mut self,
        flavour: DictationFlavour,
        by_shortcut: bool,
        return_to: Option<CaptureWindow>,
    ) {
        *self = Self::Pending(PendingCapture {
            flavour,
            by_shortcut,
            return_to,
        });
    }

    /// Promotes a `Pending` press to `Recording` at the first fresh audio
    /// chunk. Returns `Some(feedback_started)` when promoted, or `None` if not
    /// pending.
    pub(super) fn promote_pending(&mut self, toggle_mode: bool) -> Option<bool> {
        let Self::Pending(pending) = std::mem::replace(self, Self::Idle) else {
            return None;
        };
        let feedback_started = toggle_mode;
        *self = Self::Recording(ActiveCapture {
            flavour: pending.flavour,
            by_shortcut: pending.by_shortcut,
            return_to: pending.return_to,
            feedback_started,
            start_tone_played: feedback_started,
            stop_phrase: crate::dictation::stop_phrase::Detector::new(),
        });
        Some(feedback_started)
    }

    /// Clears a `Pending` capture back to `Idle`, dropping its `return_to`.
    pub(super) fn take_pending(&mut self) -> bool {
        if self.is_pending() {
            *self = Self::Idle;
            true
        } else {
            false
        }
    }

    /// Moves an active `Recording` out of the phase, leaving `Idle` behind so
    /// `return_to` and `start_tone_played` cannot linger.
    pub(super) fn take_recording(&mut self) -> Option<ActiveCapture> {
        match std::mem::replace(self, Self::Idle) {
            Self::Recording(active) => Some(active),
            other => {
                *self = other;
                None
            }
        }
    }

    /// Re-arms a tentative `Recording` whose stream failed before the hold
    /// threshold, preserving `flavour`, `by_shortcut`, and `return_to` while
    /// resetting tone/feedback state.
    pub(super) fn rearm_recording(&mut self) {
        if let Self::Recording(active) = std::mem::replace(self, Self::Idle) {
            *self = Self::Pending(PendingCapture {
                flavour: active.flavour,
                by_shortcut: active.by_shortcut,
                return_to: active.return_to,
            });
        }
    }

    pub(super) fn recording_mut(&mut self) -> Option<&mut ActiveCapture> {
        match self {
            Self::Recording(active) => Some(active),
            _ => None,
        }
    }

    pub(super) fn finishing(&self) -> Option<&Finishing> {
        match self {
            Self::Finishing(finishing) => Some(finishing),
            _ => None,
        }
    }

    pub(super) fn finishing_mut(&mut self) -> Option<&mut Finishing> {
        match self {
            Self::Finishing(finishing) => Some(finishing),
            _ => None,
        }
    }

    pub(super) fn enter_finishing(&mut self, finishing: Finishing) {
        *self = Self::Finishing(finishing);
    }

    pub(super) fn take_finishing(&mut self) -> Option<Finishing> {
        match std::mem::replace(self, Self::Idle) {
            Self::Finishing(finishing) => Some(finishing),
            other => {
                *self = other;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CaptureInstant;
    use crate::dictation::DictationCapture;

    #[test]
    fn cancel_and_finish_clear_return_to_and_start_tone() {
        let mut phase = CapturePhase::default();
        let window = Some(CaptureWindow::for_test(42, 7));

        // Cancel drops the active capture completely.
        phase.begin_recording(DictationFlavour::Raw, true, window, true, true);
        let cancelled = phase.take_recording().expect("recording");
        assert_eq!(cancelled.return_to, window);
        assert!(cancelled.start_tone_played);
        assert!(phase.is_idle());
        assert!(phase.take_recording().is_none());

        // Finish moves return_to into Finishing and leaves Idle once taken.
        phase.begin_recording(DictationFlavour::Normal, true, window, false, true);
        let active = phase.take_recording().expect("recording");
        let capture = DictationCapture::new(16_000);
        phase.enter_finishing(Finishing {
            context: PendingContext::default(),
            release: ReleaseWait::new(&capture, CaptureInstant::ZERO),
            print: None,
            flavour: active.flavour,
            by_shortcut: active.by_shortcut,
            stop_phrase: None,
            return_to: active.return_to,
        });
        assert!(phase.is_finishing());
        assert!(!phase.is_recording());
        let finished = phase.take_finishing().expect("finishing");
        assert_eq!(finished.return_to, window);
        assert!(phase.is_idle());

        // A later capture with no foreground window sees None, never the old window.
        phase.begin_recording(DictationFlavour::Normal, true, None, false, false);
        let next = phase.take_recording().expect("recording");
        assert_eq!(next.return_to, None);
        assert!(!next.start_tone_played);
    }

    #[test]
    fn rearming_tentative_recording_preserves_window_and_clears_tone() {
        let mut phase = CapturePhase::default();
        let window = Some(CaptureWindow::for_test(99, 12));
        phase.begin_recording(DictationFlavour::TerminalPaste, true, window, true, true);
        phase.rearm_recording();
        assert!(phase.is_pending());
        assert!(!phase.is_recording());

        // Promoting on the first fresh chunk starts cleanly without the old tone.
        assert_eq!(phase.promote_pending(false), Some(false));
        let active = phase.take_recording().expect("recording");
        assert_eq!(active.flavour, DictationFlavour::TerminalPaste);
        assert!(active.by_shortcut);
        assert_eq!(active.return_to, window);
        assert!(!active.feedback_started);
        assert!(!active.start_tone_played);

        // Discarding a pending press clears it back to Idle.
        phase.begin_pending(DictationFlavour::Normal, true, window);
        assert!(phase.take_pending());
        assert!(phase.is_idle());
        assert!(!phase.take_pending());
    }
}
