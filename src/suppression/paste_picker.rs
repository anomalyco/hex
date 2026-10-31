//! The session tap's side of Labs › Paste picker: what happens to keys,
//! modifier changes, and clicks while the list shows.

use std::sync::Mutex;

use super::{
    CGEventGetFlags, CGEventGetIntegerValueField, CGEventGetLocation, CgPoint, EVENT_FLAGS_CHANGED,
    EVENT_KEY_DOWN, EVENT_KEY_UP, EVENT_LEFT_MOUSE_DOWN, EVENT_OTHER_MOUSE_DOWN,
    EVENT_RIGHT_MOUSE_DOWN, EventRef, KEYBOARD_EVENT_AUTOREPEAT, KEYBOARD_EVENT_KEYCODE,
};

/// What the tap does with an event once the picker has seen it.
pub(super) enum Swallow {
    /// Pass it on.
    No,
    /// Drop it before the listener sees it.
    Only,
    /// Let the listener see it, then drop it.
    AfterListener,
}

/// Hands the open Paste picker (`labs.paste_picker`) its keys, modifier
/// releases, and outside clicks on the session tap's thread, keeping every
/// key press and its matching release away from the focused app.
pub(super) fn tap(event_type: u32, event: EventRef) -> Swallow {
    use crate::paste_picker::{
        PickerKeyDisposition, is_open, tap_flags, tap_key_down, tap_key_up, tap_mouse_down,
    };
    static TAKEN: Mutex<u128> = Mutex::new(0);
    if event_type == EVENT_FLAGS_CHANGED {
        if is_open() && crate::labs::paste_picker() && !crate::app_settings::hotkey_capture_active()
        {
            // SAFETY: CoreGraphics supplied a valid event.
            tap_flags(unsafe { CGEventGetFlags(event) });
        }
        return Swallow::No;
    }
    if matches!(
        event_type,
        EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN
    ) {
        if is_open() {
            // SAFETY: CoreGraphics supplied a valid mouse event.
            let CgPoint { x, y } = unsafe { CGEventGetLocation(event) };
            tap_mouse_down(x, y);
        }
        return Swallow::No;
    }
    if !matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP) {
        return Swallow::No;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) } as u16;
    if event_type == EVENT_KEY_UP {
        tap_key_up(code);
        let Some(bit) = 1u128.checked_shl(u32::from(code)) else {
            return Swallow::No;
        };
        let mut taken = TAKEN.lock().unwrap_or_else(|error| error.into_inner());
        let ours = *taken & bit != 0;
        *taken &= !bit;
        return if ours {
            Swallow::AfterListener
        } else {
            Swallow::No
        };
    }
    if !is_open() || !crate::labs::paste_picker() || crate::app_settings::hotkey_capture_active() {
        return Swallow::No;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let flags = unsafe { CGEventGetFlags(event) };
    let autorepeat = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_AUTOREPEAT) } != 0;
    if tap_key_down(code, flags, autorepeat) == PickerKeyDisposition::Swallow {
        if let Some(bit) = 1u128.checked_shl(u32::from(code)) {
            *TAKEN.lock().unwrap_or_else(|error| error.into_inner()) |= bit;
        }
        Swallow::Only
    } else {
        Swallow::No
    }
}
