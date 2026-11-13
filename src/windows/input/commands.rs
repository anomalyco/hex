//! Adopt ordinary listener commands into the physical shortcut's lifecycle.

use super::{CALLBACK_STATE, CaptureState, DictationFlavour, WindowsHotkeyMonitor};

impl CaptureState {
    /// A CLI/tray start is hands-free, like a completed double tap. Only
    /// ordinary dictations enter this state; SDK captures keep their owner.
    pub(super) fn start_command(&mut self, flavour: DictationFlavour) {
        *self = Self {
            locked: true,
            recording: true,
            toggle: self.toggle,
            flavour,
            ..Self::default()
        };
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
}
