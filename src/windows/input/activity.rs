//! What the low-level hooks count as the user's own input between
//! dictations: a real key or click, never the shortcut's own chord, another
//! tool's injection, or one of Hex's synthetic events.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_XBUTTON1, VK_XBUTTON2};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::{CALLBACK_STATE, HEX_INJECTED};

/// The foreground window now, for comparing one reading with the next: a
/// switch between windows ends a paste continuation.
pub(crate) fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// Whether an event is the user's own input between dictations, which ends
/// a paste continuation: a real key or left, right, or middle click, never
/// a modifier (the shortcuts' own chords include one), another tool's
/// injection, or one of Hex's own synthetic events.
pub(super) fn ends_continuation(
    vk: u32,
    is_down: bool,
    modifier: bool,
    injected: bool,
    extra_info: usize,
) -> bool {
    is_down
        && !modifier
        && !injected
        && extra_info != HEX_INJECTED
        && !matches!(vk as u16, VK_XBUTTON1 | VK_XBUTTON2)
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
    fn real_input_ends_a_paste_continuation_but_shortcuts_and_injections_do_not() {
        // A typed key, and a left, right, or middle click.
        for vk in [0x41, 0x01, 0x02, 0x04] {
            assert!(ends_continuation(vk, true, false, false, 0), "{vk:#x}");
        }
        // A release inputs nothing, and a modifier is part of the shortcut's
        // own chord.
        assert!(!ends_continuation(0x41, false, false, false, 0));
        let control = u32::from(VK_LCONTROL);
        assert!(!ends_continuation(
            control,
            true,
            ModifierState::is_modifier(control),
            false,
            0
        ));
        // Another tool's injection, and Hex's own synthetic paste or chord.
        assert!(!ends_continuation(0x41, true, false, true, 0));
        assert!(!ends_continuation(0x41, true, false, false, HEX_INJECTED));
        // The auxiliary mouse buttons leave the dictation alone.
        for vk in [VK_XBUTTON1, VK_XBUTTON2] {
            assert!(!ends_continuation(u32::from(vk), true, false, false, 0));
        }
    }
}
