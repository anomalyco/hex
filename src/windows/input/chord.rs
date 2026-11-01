//! The settings pane's chord recorder: it polls the key state for the next
//! shortcut the user presses.

use std::sync::atomic::Ordering;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_REMOVE,
    PeekMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::{
    CAPTURE_NAMED_KEYS, MOUSE_KEYS, ModifierState, NUMPAD_DOWN, NUMPAD_ENTER, WindowsHotkey,
    key_down, track_numpad,
};
use crate::app_settings::{HotkeyModifiers, ModifierSide};

/// The capture's own keyboard hook, on the thread that polls: it tells
/// numpad keys from the navigation keys they send with Num Lock off
/// (`NUMPAD_DOWN`), whether or not the listener's hook runs (#815). It only
/// watches; every key goes on to the next hook. The thread must `pump`.
pub(crate) struct NumpadWatch(HHOOK);

impl NumpadWatch {
    pub fn install() -> Option<Self> {
        // SAFETY: a low-level hook owned by this thread, removed on drop.
        let hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(numpad_hook), std::ptr::null_mut(), 0)
        };
        if hook.is_null() {
            tracing::warn!(
                error = %std::io::Error::last_os_error(),
                "the shortcut recorder could not watch numpad keys"
            );
            return None;
        }
        Some(Self(hook))
    }

    /// Delivers the hook's pending key events.
    pub fn pump(&self) {
        let mut message = MSG::default();
        // SAFETY: this thread's message queue.
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for NumpadWatch {
    fn drop(&mut self) {
        // SAFETY: the hook this watch installed.
        unsafe { UnhookWindowsHookEx(self.0) };
        NUMPAD_DOWN.store(0, Ordering::Relaxed);
    }
}

unsafe extern "system" fn numpad_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam as u32;
        let is_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        if is_down || matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            // SAFETY: a keyboard hook's lparam is a KBDLLHOOKSTRUCT.
            track_numpad(unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) }, is_down);
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// One step of the settings-pane chord capture. The UI polls this on a
/// timer instead of listening to window keystrokes because dictation
/// bindings can be modifier-only chords (for example Ctrl+Win), which
/// never produce a keystroke event.
pub(crate) enum ChordPoll {
    Pending,
    Cancelled,
    Captured(WindowsHotkey),
    /// A key no shortcut can use went down: the recorder says so, once a
    /// press, and starts the chord over.
    Unsupported,
}

/// Records the next chord the user presses: modifiers plus a regular key
/// complete immediately; a modifier-only chord completes when every
/// accumulated modifier key is released, keeping the sides
/// `HotkeyModifiers::recorded_sides` keeps (both Alts stay both); Escape
/// cancels.
#[derive(Default)]
pub(crate) struct ChordCapture {
    mouse_armed: bool,
    /// An unsupported key is down and was reported.
    unsupported: bool,
    /// Every sided modifier key seen down since the chord began.
    seen: ModifierState,
}

impl ChordCapture {
    pub(super) fn arm_mouse(&mut self, held: bool) -> bool {
        self.mouse_armed |= !held;
        self.mouse_armed
    }

    pub fn poll(&mut self) -> ChordPoll {
        if key_down(VK_ESCAPE) {
            return ChordPoll::Cancelled;
        }
        if !self.mouse_armed {
            // Do not bind the click that opened the recorder.
            if !self.arm_mouse(MOUSE_KEYS.iter().any(|&(vk, _)| key_down(vk))) {
                return ChordPoll::Pending;
            }
        }
        let held = ModifierState::current();
        if let Some(key) = pressed_capture_key() {
            let any = |down: bool| down.then_some(ModifierSide::Either);
            return ChordPoll::Captured(WindowsHotkey {
                control: any(held.left_control || held.right_control),
                windows: any(held.left_windows || held.right_windows),
                alt: any(held.left_alt || held.right_alt),
                shift: any(held.left_shift || held.right_shift),
                key: Some(key),
            });
        }
        if unsupported_key_down() {
            // A key the bindings cannot express (media keys, ...) is being
            // pressed: forget the chord in progress so releasing the
            // modifiers cannot record a wrong modifier-only binding.
            let reported = self.unsupported;
            *self = Self {
                mouse_armed: self.mouse_armed,
                unsupported: true,
                ..Self::default()
            };
            return if reported {
                ChordPoll::Pending
            } else {
                ChordPoll::Unsupported
            };
        }
        self.unsupported = false;
        self.seen.add(&held);
        if self.seen.any() && !held.any() {
            return ChordPoll::Captured(self.held());
        }
        ChordPoll::Pending
    }

    /// The modifier keys a release would record so far.
    pub fn held(&self) -> WindowsHotkey {
        let seen = &self.seen;
        let side = |left: bool, right: bool| match (left, right) {
            (true, true) => Some(ModifierSide::Both),
            (true, false) => Some(ModifierSide::Left),
            (false, true) => Some(ModifierSide::Right),
            (false, false) => None,
        };
        let modifiers = HotkeyModifiers {
            control: side(seen.left_control, seen.right_control),
            option: side(seen.left_alt, seen.right_alt),
            shift: side(seen.left_shift, seen.right_shift),
            command: side(seen.left_windows, seen.right_windows),
            function: false,
        }
        .recorded_sides();
        WindowsHotkey {
            control: modifiers.control,
            windows: modifiers.command,
            alt: modifiers.option,
            shift: modifiers.shift,
            key: None,
        }
    }
}

impl ModifierState {
    fn add(&mut self, other: &Self) {
        self.left_control |= other.left_control;
        self.right_control |= other.right_control;
        self.left_windows |= other.left_windows;
        self.right_windows |= other.right_windows;
        self.left_alt |= other.left_alt;
        self.right_alt |= other.right_alt;
        self.left_shift |= other.left_shift;
        self.right_shift |= other.right_shift;
    }

    fn any(&self) -> bool {
        self.left_control
            || self.right_control
            || self.left_windows
            || self.right_windows
            || self.left_alt
            || self.right_alt
            || self.left_shift
            || self.right_shift
    }
}

/// Whether any pressed key is one the capture cannot map to a binding
/// name: everything except mouse buttons, modifiers, lock/IME keys, and
/// the mappable set.
fn unsupported_key_down() -> bool {
    for vk in 0x08..=0xFE_u16 {
        match vk {
            // Modifiers (generic + left/right) and the Windows keys.
            0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C => continue,
            // Escape cancels the recorder; lock and IME state keys are inert.
            0x1B | 0x14 | 0x15..=0x1A | 0x90 => continue,
            _ => {}
        }
        if !key_down(vk) {
            continue;
        }
        let mappable = CAPTURE_NAMED_KEYS.iter().any(|(named, _)| *named == vk)
            || (0x41..=0x5A).contains(&vk)
            || (0x30..=0x39).contains(&vk)
            || (0x70..=0x87).contains(&vk)
            // Numpad 5 with Num Lock off; the hook names it.
            || vk == 0x0C;
        if !mappable {
            return true;
        }
    }
    false
}

fn pressed_capture_key() -> Option<String> {
    if let Some((_, name)) = MOUSE_KEYS.into_iter().find(|&(vk, _)| key_down(vk)) {
        return Some(name.into());
    }
    match NUMPAD_DOWN.load(Ordering::Relaxed) {
        0 => {}
        NUMPAD_ENTER => return Some("num enter".into()),
        numpad => {
            if let Some((_, name)) = CAPTURE_NAMED_KEYS
                .into_iter()
                .find(|&(vk, _)| u32::from(vk) == numpad)
            {
                return Some(name.into());
            }
        }
    }
    if let Some((_, name)) = CAPTURE_NAMED_KEYS.into_iter().find(|&(vk, _)| key_down(vk)) {
        return Some(name.to_string());
    }
    for vk in 0x41..=0x5A_u16 {
        // A-Z
        if key_down(vk) {
            return Some(char::from(vk as u8).to_ascii_lowercase().to_string());
        }
    }
    for vk in 0x30..=0x39_u16 {
        // 0-9
        if key_down(vk) {
            return Some(char::from(vk as u8).to_string());
        }
    }
    for vk in 0x70..=0x87_u16 {
        // F1-F24
        if key_down(vk) {
            return Some(format!("f{}", vk - 0x70 + 1));
        }
    }
    None
}
