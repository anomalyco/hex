//! The Windows dictation shortcut's explicit capture state machine.
//!
//! Tracks the legal phases of one physical or command-driven capture (idle,
//! armed first tap of double-tap-only, held and recording, or locked
//! hands-free) so illegal combinations such as `locked && active` or stale
//! `pressed_at_ms` cannot be represented.

use super::{DOUBLE_TAP_WINDOW_MS, DictationFlavour, HotkeyAction};

/// The capture's legal states: idle (optionally holding the last release for a
/// double tap), armed on the silent first tap of double-tap-only, physically
/// held and recording, or locked hands-free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Capture {
    Idle {
        /// The shortcut that last owned a capture (or `Normal` initially); a
        /// tap of another shortcut never counts toward its double tap.
        flavour: DictationFlavour,
        last_release_ms: Option<u32>,
    },
    /// Double-tap-only: the first tap is down and waits for the second.
    Armed { flavour: DictationFlavour },
    /// Physically held and recording.
    Held {
        flavour: DictationFlavour,
        /// When this held press began, from the hook's tick count, for Labs ›
        /// Hold through other keys.
        pressed_at_ms: u32,
        /// Releasing this press locks hands-free instead of finishing (Toggle
        /// mode, or the second tap of a double tap).
        locks_on_release: bool,
    },
    /// Hands-free recording (double-tap lock, Toggle release, or a command).
    Locked { flavour: DictationFlavour },
}

pub(super) struct CaptureState {
    pub(super) phase: Capture,
    /// Every press acts as the second tap of a double-tap: releasing it
    /// locks the capture, and the next press finishes it.
    pub(super) toggle: bool,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl CaptureState {
    pub(super) fn new(toggle: bool) -> Self {
        Self {
            phase: Capture::Idle {
                flavour: DictationFlavour::Normal,
                last_release_ms: None,
            },
            toggle,
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        matches!(self.phase, Capture::Idle { .. })
    }

    pub(super) fn is_down(&self) -> bool {
        matches!(self.phase, Capture::Armed { .. } | Capture::Held { .. })
    }

    #[cfg(test)]
    pub(super) fn is_recording(&self) -> bool {
        matches!(self.phase, Capture::Held { .. } | Capture::Locked { .. })
    }

    pub(super) fn is_locked(&self) -> bool {
        matches!(self.phase, Capture::Locked { .. })
    }

    pub(super) fn flavour(&self) -> DictationFlavour {
        match self.phase {
            Capture::Idle { flavour, .. }
            | Capture::Armed { flavour }
            | Capture::Held { flavour, .. }
            | Capture::Locked { flavour } => flavour,
        }
    }

    /// Whether another shortcut owns a capture in progress.
    pub(super) fn owned_by_other(&self, flavour: DictationFlavour) -> bool {
        self.flavour() != flavour && !self.is_idle()
    }

    /// Hands an idle capture to one shortcut; a tap of the other one never
    /// counts toward its double tap.
    pub(super) fn claim(&mut self, flavour: DictationFlavour) {
        if let Capture::Idle {
            flavour: current,
            last_release_ms,
        } = &mut self.phase
            && *current != flavour
        {
            *current = flavour;
            *last_release_ms = None;
        }
    }

    pub(super) fn press(
        &mut self,
        double_tap_enabled: bool,
        double_tap_only: bool,
        timestamp_ms: u32,
    ) -> Option<HotkeyAction> {
        match self.phase {
            Capture::Locked { flavour } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: None,
                };
                Some(HotkeyAction::Finish)
            }
            Capture::Armed { .. } | Capture::Held { .. } => None,
            Capture::Idle {
                flavour,
                last_release_ms,
            } => {
                let double_tapped = last_release_ms.is_some_and(|released| {
                    timestamp_ms.wrapping_sub(released) < DOUBLE_TAP_WINDOW_MS
                });
                let locks_on_release = self.toggle || (double_tap_enabled && double_tapped);
                if double_tap_only && !locks_on_release {
                    // Double-tap-only: the first tap arms the window silently;
                    // recording waits for the second tap.
                    self.phase = Capture::Armed { flavour };
                    return None;
                }
                self.phase = Capture::Held {
                    flavour,
                    pressed_at_ms: timestamp_ms,
                    locks_on_release,
                };
                Some(HotkeyAction::Start)
            }
        }
    }

    pub(super) fn release(
        &mut self,
        double_tap_enabled: bool,
        timestamp_ms: u32,
    ) -> Option<HotkeyAction> {
        match self.phase {
            Capture::Idle { .. } | Capture::Locked { .. } => None,
            Capture::Armed { flavour } => {
                // Releasing an armed-but-silent first tap emits nothing.
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: double_tap_enabled.then_some(timestamp_ms),
                };
                None
            }
            Capture::Held {
                flavour,
                locks_on_release: true,
                ..
            } => {
                self.phase = Capture::Locked { flavour };
                None
            }
            Capture::Held {
                flavour,
                locks_on_release: false,
                ..
            } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: double_tap_enabled.then_some(timestamp_ms),
                };
                Some(HotkeyAction::Finish)
            }
        }
    }

    /// Whether Toggle's cancel shortcut is Hex's now: only while a toggled
    /// capture records, so the key stays with every app otherwise.
    pub(super) fn wants_cancel(&self) -> bool {
        self.toggle && matches!(self.phase, Capture::Held { .. } | Capture::Locked { .. })
    }

    pub(super) fn cancel(&mut self) -> Option<HotkeyAction> {
        match self.phase {
            Capture::Idle { .. } => None,
            Capture::Armed { flavour }
            | Capture::Held { flavour, .. }
            | Capture::Locked { flavour } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: None,
                };
                Some(HotkeyAction::Cancel)
            }
        }
    }
}
