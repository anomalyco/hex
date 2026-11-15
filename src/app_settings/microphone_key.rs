//! Who owns the microphone (dictation) key while Hex runs, and whether a
//! press this process took is still waiting for its release.
//!
//! macOS sends that key as a normal keyDown/keyUp for virtual key code 176
//! with SecondaryFn set, but its own Dictation shortcut is a symbolic hot key
//! the window server matches before the app and before Hex's annotated-session
//! key tap: only a session tap still sees the press
//! (`suppression::dictation_key`). That tap asks this type what each press is,
//! and remembers the presses it took so their releases are taken too, whatever
//! changed in between (a binding removed, a recording finished, the lab turned
//! off).
//!
//! The decision lives here, away from the tap, so it can be tested on every
//! OS: a press is Hex's only with Labs › Microphone key alone on, while nothing
//! pauses Hex, and only when a shortcut is being recorded or one of Hex's
//! shortcuts matches it. Everything else stays macOS's.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashMap;

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

/// The microphone key's presses this tap took, waiting for their releases.
#[derive(Debug, Default)]
pub(crate) struct MicrophoneKey {
    taken: HashMap<u16, Owner>,
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
    /// A release is Hex's only if this key took the press, so a press macOS
    /// kept keeps its release too, and one Hex took never leaks a stray key-up
    /// to the app.
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
            return match self.taken.remove(&code) {
                Some(Owner::Listener) => MicrophoneKeyTap::Listener { down: false },
                Some(Owner::Recorder) => MicrophoneKeyTap::Recorder { down: false },
                None => MicrophoneKeyTap::Pass,
            };
        }
        if code != MICROPHONE_KEY_CODE || !lab || paused {
            return MicrophoneKeyTap::Pass;
        }
        let owner = if capturing {
            Owner::Recorder
        } else if bound {
            Owner::Listener
        } else {
            return MicrophoneKeyTap::Pass;
        };
        self.taken.insert(code, owner);
        match owner {
            Owner::Listener => MicrophoneKeyTap::Listener { down: true },
            Owner::Recorder => MicrophoneKeyTap::Recorder { down: true },
        }
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

    /// A release after the lab turned off still belongs to the press Hex took,
    /// so the app never sees half a key.
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
}
