//! The capture capsule's keys on Windows.
//!
//! The capsule never takes focus (`rune_ui::capture_capsule`): its window keeps
//! `WS_EX_NOACTIVATE` and Rune hands an overlay the keyboard nowhere, so its
//! keys come from Hex's hook, as the macOS session tap hands them on in
//! `suppression::list_tap`. A key the capsule's grab takes is swallowed and
//! never reaches the app in front; everything else, Hex's own shortcuts and
//! Ctrl, Alt, or Windows chords first among them, goes on untouched.

use std::sync::{Mutex, MutexGuard};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardState, HKL, ToUnicodeEx, VK_BACK, VK_CAPITAL,
    VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NUMLOCK,
    VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SCROLL, VK_SHIFT, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::input::CallbackState;
use crate::capture_host::{CapsuleKey, capsule_key, list_open, suggesting};

/// `ToUnicodeEx`'s `wFlags` bit that translates without changing the thread's
/// dead-key state. Undocumented, but the toolkit-wide convention (Chromium, Qt
/// and GTK pass it): without it the call consumes a pending dead key, and the
/// app's next keystroke composes the wrong thing.
const DO_NOT_CHANGE_KEYBOARD_STATE: u32 = 0x4;

/// The keys the capsule took, by virtual key. Their releases stay with it, as
/// `suppression::list_tap` keeps them, so the app never hears an orphan release.
static TAKEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Offers one keyboard hook event to the capture capsule through the hook's own
/// state: true when the hook must swallow it, because the capsule took it.
pub(super) fn takes(
    state: &Mutex<Option<CallbackState>>,
    vk: u32,
    is_down: bool,
    scan: u32,
) -> bool {
    state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .is_some_and(|state| key(state, vk, is_down, scan))
}

/// `takes` with the state already in hand.
fn key(state: &CallbackState, vk: u32, is_down: bool, scan: u32) -> bool {
    if modifier(vk) {
        // Never swallowed: Hex's chords and the app in front need every one of
        // them. Shift alone is the capsule's (it marks the row under the
        // pointer), and still goes on, as macOS's `flagsChanged` does.
        if offered() && shift(vk) {
            capsule_key(CapsuleKey::Shift(is_down));
        }
        return false;
    }
    if locks(vk) {
        return false;
    }
    if !is_down {
        // A release goes with the press it belongs to, even after the list
        // closed: the app under it must never hear half a key.
        let mut taken = taken_keys();
        return match taken.iter().position(|held| *held == vk) {
            Some(at) => {
                taken.remove(at);
                true
            }
            None => false,
        };
    }
    if !offered() || command_held() || state.shortcut_owns(vk) {
        return false;
    }
    let key = named(vk, down(VK_SHIFT)).or_else(|| typed(vk, scan).map(CapsuleKey::Text));
    let Some(key) = key else {
        // A key that types nothing, such as a function key or a dead key: the
        // open list takes every key press (`capture_host::list_tap`'s
        // `ListTap::Swallow`), while the suggestions leave it to the app.
        if list_open() {
            remember(vk);
            return true;
        }
        return false;
    };
    let took = capsule_key(key);
    if took {
        remember(vk);
    }
    took
}

/// Records that the capsule took `vk`, so its release goes with it.
fn remember(vk: u32) {
    let mut taken = taken_keys();
    if !taken.contains(&vk) {
        taken.push(vk);
    }
}

/// Whether the capsule's list is up and takes keys at all: macOS's
/// `suppression::list_takes_keys`, before the chords the app owns are left out.
fn offered() -> bool {
    crate::app_settings::capture_on()
        && !crate::app_settings::shortcuts_suspended()
        && (list_open() || suggesting())
}

/// The named keys the open list uses, as `capture_host::list_key` reads them
/// from macOS's key codes; `None` for a key it would take as text instead.
fn named(vk: u32, shift: bool) -> Option<CapsuleKey> {
    Some(match vk as u16 {
        VK_ESCAPE => CapsuleKey::Escape,
        VK_RETURN => CapsuleKey::Return { shift },
        VK_TAB => CapsuleKey::Tab { shift },
        VK_BACK => CapsuleKey::Backspace,
        VK_UP => CapsuleKey::Up,
        VK_DOWN => CapsuleKey::Down,
        _ => return None,
    })
}

/// What `vk` types in the foreground window's layout, `None` for a dead key, a
/// control character, or a key that types nothing.
fn typed(vk: u32, scan: u32) -> Option<String> {
    let mut keys = [0_u8; 256];
    let mut buffer = [0_u16; 8];
    // SAFETY: `keys` is the 256-byte key-state array `GetKeyboardState` fills,
    // `buffer` is `ToUnicodeEx`'s UTF-16 out, and `layout()` is a layout handle.
    let length = unsafe {
        GetKeyboardState(keys.as_mut_ptr());
        // The hook thread's queue need not hold the modifiers the app's does:
        // take them from the async state the hook already trusts.
        for modifier in [VK_SHIFT, VK_CONTROL, VK_MENU] {
            keys[usize::from(modifier)] = if down(modifier) { 0x80 } else { 0 };
        }
        ToUnicodeEx(
            vk,
            scan,
            keys.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            DO_NOT_CHANGE_KEYBOARD_STATE,
            layout(),
        )
    };
    // Negative is a dead key, which the layout keeps for the app to compose.
    let length = usize::try_from(length).ok()?;
    let typed = String::from_utf16_lossy(&buffer[..length.min(buffer.len())]);
    (!typed.is_empty() && !typed.chars().any(char::is_control)).then_some(typed)
}

/// The layout of the window in front, which is where the key was going: this
/// hook's own thread has none of its own.
fn layout() -> HKL {
    // SAFETY: both read this session's window and layout state; a null handle
    // falls back to this thread's layout.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let layout = GetKeyboardLayout(thread);
        if layout.is_null() {
            GetKeyboardLayout(0)
        } else {
            layout
        }
    }
}

/// Whether `vk` is down now, from the async state the hook already trusts.
fn down(vk: u16) -> bool {
    // SAFETY: a plain reading of this session's key state.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

/// Ctrl, Alt, or Windows down: chords the app in front owns, and Hex's own
/// shortcuts with them, never the capsule.
fn command_held() -> bool {
    [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(down)
}

/// Any modifier: Ctrl, Alt, Windows, or Shift, either side. `Input` sends these
/// as key events, where macOS's tap hears them at `flagsChanged`.
fn modifier(vk: u32) -> bool {
    matches!(
        vk as u16,
        VK_SHIFT
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_CONTROL
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_MENU
            | VK_LMENU
            | VK_RMENU
            | VK_LWIN
            | VK_RWIN
    )
}

fn shift(vk: u32) -> bool {
    matches!(vk as u16, VK_SHIFT | VK_LSHIFT | VK_RSHIFT)
}

/// Caps Lock, Num Lock and Scroll Lock: the system's own toggles, never the
/// capsule's (macOS hears its locks and modifiers outside the list's keys).
fn locks(vk: u32) -> bool {
    matches!(vk as u16, VK_CAPITAL | VK_NUMLOCK | VK_SCROLL)
}

fn taken_keys() -> MutexGuard<'static, Vec<u32>> {
    TAKEN.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capsule_gets_the_keys_its_list_uses() {
        assert_eq!(named(u32::from(VK_ESCAPE), false), Some(CapsuleKey::Escape));
        assert_eq!(
            named(u32::from(VK_RETURN), true),
            Some(CapsuleKey::Return { shift: true })
        );
        assert_eq!(
            named(u32::from(VK_TAB), false),
            Some(CapsuleKey::Tab { shift: false })
        );
        assert_eq!(
            named(u32::from(VK_BACK), false),
            Some(CapsuleKey::Backspace)
        );
        assert_eq!(named(u32::from(VK_UP), false), Some(CapsuleKey::Up));
        assert_eq!(named(u32::from(VK_DOWN), false), Some(CapsuleKey::Down));
        // A letter is what it types; a function key is neither named nor text.
        assert_eq!(
            named(u32::from(b'H'), false),
            None,
            "a letter is text, not a named key"
        );
        assert_eq!(named(0x78, false), None);
    }

    #[test]
    fn the_modifiers_and_the_locks_stay_the_systems() {
        for vk in [
            VK_SHIFT,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_CONTROL,
            VK_LCONTROL,
            VK_RCONTROL,
            VK_MENU,
            VK_LMENU,
            VK_RMENU,
            VK_LWIN,
            VK_RWIN,
        ] {
            assert!(modifier(u32::from(vk)));
            assert!(!locks(u32::from(vk)));
        }
        // Only Shift is the capsule's, and only to mark a row with.
        for vk in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT] {
            assert!(shift(u32::from(vk)));
        }
        for vk in [VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU] {
            assert!(!shift(u32::from(vk)));
        }
        for vk in [VK_CAPITAL, VK_NUMLOCK, VK_SCROLL] {
            assert!(locks(u32::from(vk)));
            assert!(!modifier(u32::from(vk)));
        }
        assert!(!modifier(u32::from(VK_ESCAPE)) && !locks(u32::from(VK_ESCAPE)));
    }
}
