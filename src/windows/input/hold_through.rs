//! Keys and buttons that join a held modifier-only shortcut: another app's
//! chord ends the capture, unless the hold is long enough for Labs › Hold
//! through other keys.

use std::time::Duration;

use super::{Capture, CaptureState, HotkeyAction};

impl CaptureState {
    /// Another key joined a held modifier-only shortcut: it was a chord
    /// meant for another application. Only a recording capture reports
    /// it, since a bare cancel also cancels the newest unfinished job.
    pub(super) fn interrupt(&mut self) -> Option<HotkeyAction> {
        match self.phase {
            Capture::Idle { .. } | Capture::Locked { .. } => None,
            Capture::Armed { flavour } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: None,
                };
                None
            }
            Capture::Held { flavour, .. } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: None,
                };
                Some(HotkeyAction::Cancel)
            }
        }
    }

    /// Whether a key or button going down now joins a hold that keeps
    /// recording: Labs › Hold through other keys, in Hold mode,
    /// on a modifier-only shortcut a second or more into its press, which
    /// the release then finishes as usual. `hold_through_keys` is the lab's
    /// gate, read as the event arrives. Before the first second, and with
    /// the lab off, the joining key is a chord as before.
    pub(super) fn holds_through_joining_keys(
        &self,
        hold_through_keys: bool,
        timestamp_ms: u32,
    ) -> bool {
        match self.phase {
            Capture::Held { pressed_at_ms, .. } => crate::dictation::holds_through_joining_keys(
                hold_through_keys,
                self.toggle,
                Duration::from_millis(u64::from(timestamp_ms.wrapping_sub(pressed_at_ms))),
            ),
            Capture::Idle { .. } | Capture::Armed { .. } | Capture::Locked { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CaptureState, HotkeyAction};

    /// The hook's `!modifier_event` branch treats keys and mouse buttons
    /// alike, so one predicate covers both.
    #[test]
    fn a_long_hold_keeps_recording_through_a_joining_key_or_button() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        // Inside the first second the chord ends the capture, as before.
        assert!(!state.holds_through_joining_keys(true, 100));
        assert!(!state.holds_through_joining_keys(true, 1_099));
        // At one second and later the key reaches the app and recording goes on.
        assert!(state.holds_through_joining_keys(true, 1_100));
        assert!(state.holds_through_joining_keys(true, 9_000));
        assert!(
            !state.holds_through_joining_keys(false, 9_000),
            "off is unchanged"
        );
        // The joined key is still held as the shortcut lets go: it finishes.
        assert_eq!(state.release(true, 9_100), Some(HotkeyAction::Finish));
        assert!(
            !state.holds_through_joining_keys(true, 9_200),
            "nothing records once the shortcut is released"
        );
    }

    #[test]
    fn toggle_and_a_wrapping_tick_count_keep_the_second() {
        // Toggle's held press locks on release, so the lab leaves it alone.
        let mut toggle = CaptureState::new(true);
        assert_eq!(toggle.press(true, false, 100), Some(HotkeyAction::Start));
        assert!(!toggle.holds_through_joining_keys(true, 9_000));

        // A press just before the tick count wraps keeps its full second.
        let mut wrapped = CaptureState::default();
        assert_eq!(
            wrapped.press(true, false, u32::MAX - 500),
            Some(HotkeyAction::Start)
        );
        assert!(!wrapped.holds_through_joining_keys(true, 498));
        assert!(wrapped.holds_through_joining_keys(true, 499));
    }
}
