//! What the low-level hooks count as the user's own input between
//! dictations: a real key or click, never the shortcut's own chord or one of
//! Hex's synthetic events.

use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::{CALLBACK_STATE, HEX_INJECTED};

/// The foreground window now, for comparing one reading with the next: a
/// switch between windows ends a paste continuation.
pub(crate) fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// Whether an event is the user's own input between dictations, which ends
/// a paste continuation: a press that is not a modifier (the shortcuts' own
/// chords include one) and not one of Hex's own synthetic events. Input
/// another program injects is the user's too: a remapper, keyboard sharing,
/// or the touch keyboard typed what is in front of them, so it ends the
/// continuation as macOS's tap does.
pub(super) fn ends_continuation(is_down: bool, modifier: bool, extra_info: usize) -> bool {
    is_down && !modifier && extra_info != HEX_INJECTED
}

/// The user typed or clicked: the next paste no longer continues the last
/// one.
pub(super) fn invalidate() {
    let guard = CALLBACK_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(state) = guard.as_ref() {
        state.activity.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::input::modifiers::ModifierState;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_LCONTROL;

    #[test]
    fn real_input_ends_a_paste_continuation_but_hexs_own_events_do_not() {
        // A real press ends it, whatever key or mouse button sent it: macOS
        // counts any mouse-down, auxiliary buttons included.
        assert!(ends_continuation(true, false, 0));
        // A release inputs nothing, and a modifier is part of the shortcut's
        // own chord.
        assert!(!ends_continuation(false, false, 0));
        let control = u32::from(VK_LCONTROL);
        assert!(!ends_continuation(
            true,
            ModifierState::is_modifier(control),
            0
        ));
        // Hex's own synthetic paste or chord. Another program's injected key
        // carries no marker, so it ends the continuation above.
        assert!(!ends_continuation(true, false, HEX_INJECTED));
    }
}
