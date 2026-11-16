//! Who owns the microphone (dictation) key while Hex runs, and whether a
//! press this process took is still waiting for its release.
//!
//! macOS sends that key as a normal keyDown/keyUp for virtual key code 176
//! with SecondaryFn set. Hex reads it at a session tap
//! (`suppression::dictation_key`), because macOS's own Dictation shortcut is
//! matched before Hex's annotated-session key tap and the app. That tap asks
//! this type what each press is, and remembers the one press it took so the
//! release is taken too while the tap is still there, even when the binding or
//! the recording changed in between. Turning the lab off removes the tap and
//! forgets what it took.
//!
//! The decision lives here, away from the tap, so it can be tested on every
//! OS: a press is Hex's only with Labs › Microphone key alone on, while nothing
//! pauses Hex, and only when a shortcut is being recorded or one of Hex's
//! shortcuts matches it. Everything else stays macOS's.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::hotkeys::MICROPHONE_KEY_CODE;

/// What the microphone key is for at this press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MicrophoneKeyTap {
    /// Not Hex's: macOS's, untouched by the tap.
    Pass,
    /// Hex's shortcut: fed to the listener, which matches it like any key.
    Listener { down: bool },
    /// Hex is recording a shortcut: fed to the recorder in the window.
    Recorder { down: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner {
    Listener,
    Recorder,
}

/// The microphone key's press this tap took, waiting for its release.
#[derive(Debug, Default)]
pub(crate) struct MicrophoneKey {
    taken: Option<Owner>,
}

impl MicrophoneKey {
    /// What the session tap does with one key event.
    ///
    /// `bound` says one of Hex's shortcuts matches this press (the tap asks
    /// `RuntimeHotkey::matches_key_press`, so modifiers count and a key that
    /// only matches with others held is not Hex's alone), `capturing` is
    /// `hotkey_capture_active`, and `paused` is Labs › Pause from menu bar,
    /// which suspends every shortcut. Recording takes the key before any
    /// binding: the chord being recorded replaces the old one.
    ///
    /// A release is Hex's only if this key took the press. A press that passes
    /// through forgets anything left behind, so a release that never arrived
    /// cannot claim a later press's release.
    pub(crate) fn take(
        &mut self,
        code: u16,
        down: bool,
        lab: bool,
        paused: bool,
        capturing: bool,
        bound: bool,
    ) -> MicrophoneKeyTap {
        if !down {
            return match self.taken.take() {
                Some(Owner::Listener) => MicrophoneKeyTap::Listener { down: false },
                Some(Owner::Recorder) => MicrophoneKeyTap::Recorder { down: false },
                None => MicrophoneKeyTap::Pass,
            };
        }
        if code != MICROPHONE_KEY_CODE || !lab || paused {
            self.taken = None;
            return MicrophoneKeyTap::Pass;
        }
        let owner = if capturing {
            Owner::Recorder
        } else if bound {
            Owner::Listener
        } else {
            self.taken = None;
            return MicrophoneKeyTap::Pass;
        };
        self.taken = Some(owner);
        match owner {
            Owner::Listener => MicrophoneKeyTap::Listener { down: true },
            Owner::Recorder => MicrophoneKeyTap::Recorder { down: true },
        }
    }

    /// Whether a press this tap took belongs to the listener: its release would
    /// be lost if the tap went away, which ends the capture it started.
    pub(crate) fn holds_listener_press(&self) -> bool {
        matches!(self.taken, Some(Owner::Listener))
    }

    /// Forget the press this tap took: its events end here, so its release is
    /// the app's again (or was never delivered).
    pub(crate) fn clear(&mut self) {
        self.taken = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mic key is Hex's only with its lab on and a reason to take it.
    #[test]
    fn the_microphone_key_is_hexs_only_while_its_lab_wants_it() {
        let mut key = MicrophoneKey::default();
        let mic = MICROPHONE_KEY_CODE;
        // The lab off leaves the key to macOS, whatever is still saved.
        assert_eq!(
            key.take(mic, true, false, false, false, true),
            MicrophoneKeyTap::Pass
        );
        // On, with nothing bound and no recording, macOS keeps it.
        assert_eq!(
            key.take(mic, true, true, false, false, false),
            MicrophoneKeyTap::Pass
        );
        // Pause from menu bar leaves it alone like every other shortcut.
        assert_eq!(
            key.take(mic, true, true, true, true, true),
            MicrophoneKeyTap::Pass
        );
        // Only the microphone key code is ever taken.
        assert_eq!(
            key.take(49, true, true, false, true, true),
            MicrophoneKeyTap::Pass
        );
        // A bound shortcut takes the press for the listener...
        assert_eq!(
            key.take(mic, true, true, false, false, true),
            MicrophoneKeyTap::Listener { down: true }
        );
        // ...and its release too, even though nothing is bound by then.
        assert_eq!(
            key.take(mic, false, true, false, false, false),
            MicrophoneKeyTap::Listener { down: false }
        );
        // A second release was never taken.
        assert_eq!(
            key.take(mic, false, true, false, false, true),
            MicrophoneKeyTap::Pass
        );
    }

    /// Recording takes the key before a binding, and its release follows even
    /// after the recording stops.
    #[test]
    fn a_recorded_press_keeps_its_release() {
        let mut key = MicrophoneKey::default();
        let mic = MICROPHONE_KEY_CODE;
        assert_eq!(
            key.take(mic, true, true, false, true, true),
            MicrophoneKeyTap::Recorder { down: true }
        );
        assert_eq!(
            key.take(mic, true, true, false, true, true),
            MicrophoneKeyTap::Recorder { down: true },
            "a repeat is the recording's too"
        );
        assert_eq!(
            key.take(mic, false, true, true, false, false),
            MicrophoneKeyTap::Recorder { down: false }
        );
        assert_eq!(
            key.take(mic, false, false, false, false, false),
            MicrophoneKeyTap::Pass,
            "and the next press is macOS's again"
        );
    }

    /// A press Hex took keeps its release while the tap that took it is still
    /// there, even if the lab turns off between the two.
    #[test]
    fn a_taken_press_is_released_by_the_tap_that_took_it() {
        let mut key = MicrophoneKey::default();
        let mic = MICROPHONE_KEY_CODE;
        assert_eq!(
            key.take(mic, true, true, false, false, true),
            MicrophoneKeyTap::Listener { down: true }
        );
        assert_eq!(
            key.take(mic, false, false, false, false, false),
            MicrophoneKeyTap::Listener { down: false }
        );
    }

    /// A press whose release never arrived must not claim a later press's
    /// release: the next press passes through and clears it.
    #[test]
    fn a_pass_through_press_forgets_a_press_whose_release_never_came() {
        let mut key = MicrophoneKey::default();
        let mic = MICROPHONE_KEY_CODE;
        assert_eq!(
            key.take(mic, true, true, false, false, true),
            MicrophoneKeyTap::Listener { down: true }
        );
        assert_eq!(
            key.take(mic, true, false, false, false, false),
            MicrophoneKeyTap::Pass
        );
        assert_eq!(
            key.take(mic, false, false, false, false, false),
            MicrophoneKeyTap::Pass
        );
        assert!(!key.holds_listener_press());
    }

    /// A tap that goes away forgets its press, so the app keeps the release.
    #[test]
    fn a_cleared_press_is_the_apps_again() {
        let mut key = MicrophoneKey::default();
        let mic = MICROPHONE_KEY_CODE;
        assert_eq!(
            key.take(mic, true, true, false, false, true),
            MicrophoneKeyTap::Listener { down: true }
        );
        assert!(key.holds_listener_press());
        key.clear();
        assert!(!key.holds_listener_press());
        assert_eq!(
            key.take(mic, false, true, false, false, true),
            MicrophoneKeyTap::Pass
        );
    }
}
