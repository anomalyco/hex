//! Adopt ordinary listener commands into the physical shortcut's lifecycle,
//! and let Labs › Stop phrase finish a hands-free capture as its press would.

use std::sync::Mutex;

use windows_sys::Win32::System::SystemInformation::GetTickCount;

use super::{
    CALLBACK_STATE, Capture, CaptureState, DictationFlavour, HotkeyAction, WindowsHotkeyMonitor,
    send_event,
};

/// Labs › Stop phrase: the words the finish request carries, set just before
/// its event and taken by the listener that handles it. It rides beside the
/// hook's state because a `String` can't live in the `Copy` events themselves;
/// only a `HotkeyAction::Finish` takes it, so no other action queued ahead of
/// the finish can steal its words, and no other action carries them.
static STOP_PHRASE: Mutex<Option<String>> = Mutex::new(None);

impl CaptureState {
    /// A CLI/tray start is hands-free, like a completed double tap. Only
    /// ordinary dictations enter this state; SDK captures keep their owner.
    /// If a physical shortcut hold is already down, keep its release edge
    /// instead of orphaning it behind a lock.
    pub(super) fn start_command(&mut self, flavour: DictationFlavour) {
        match self.phase {
            Capture::Held { .. } => {}
            Capture::Idle { .. } | Capture::Armed { .. } | Capture::Locked { .. } => {
                self.phase = Capture::Locked { flavour };
            }
        }
    }

    /// Labs › Stop phrase: only a locked capture (a Toggle press released, or
    /// a completed double tap) is hands-free, so its job strips the words. A
    /// held press reads them as speech and records on, as does an armed first
    /// tap or an idle shortcut.
    pub(super) fn finish_hands_free(&mut self) -> bool {
        match self.phase {
            Capture::Locked { flavour } => {
                self.phase = Capture::Idle {
                    flavour,
                    last_release_ms: None,
                };
                true
            }
            Capture::Idle { .. } | Capture::Armed { .. } | Capture::Held { .. } => false,
        }
    }
}

impl WindowsHotkeyMonitor {
    /// Adopt a command the listener accepted, without generating another
    /// shortcut event. Physical input then finishes/cancels the same capture.
    pub(crate) fn command_capture(&self, recording: Option<DictationFlavour>) {
        let mut guard = CALLBACK_STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = guard.as_mut() {
            match recording {
                Some(flavour) => state.capture.start_command(flavour),
                None => {
                    state.capture.cancel();
                }
            }
        }
    }

    /// Labs › Stop phrase: a hands-free capture ends now, exactly as the
    /// shortcut's finishing press ends it. Anything else records on, and
    /// returns false.
    pub(crate) fn finish_hands_free(&self, phrase: String) -> bool {
        let mut guard = CALLBACK_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(state) = guard.as_mut() else {
            return false;
        };
        if !state.capture.finish_hands_free() {
            return false;
        }
        let time = unsafe { GetTickCount() };
        *STOP_PHRASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(phrase);
        send_event(state, HotkeyAction::Finish, time);
        true
    }

    /// The words the finish being handled carries, when Labs › Stop phrase
    /// ended it. Only that finish takes them, and taking them clears them.
    pub(crate) fn take_stop_phrase(&self) -> Option<String> {
        STOP_PHRASE.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
