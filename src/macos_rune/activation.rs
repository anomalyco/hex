//! Switching to Hex with ⌘Tab activates it but sends no reopen Apple event,
//! so a closed window stayed closed and only the Dock icon brought it back
//! (#1298). Hex watches its own activation instead.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2::sel;

use super::MenuTarget;

/// Calls the target's `applicationDidBecomeActive:` whenever Hex activates.
pub(super) fn observe_activation(target: &MenuTarget) {
    // SAFETY: the target outlives the observer (kept in PUMP for the app's
    // life), and the selector matches its method.
    unsafe {
        let _: () = msg_send![
            &*objc2_foundation::NSNotificationCenter::defaultCenter(),
            addObserver: target,
            selector: sel!(applicationDidBecomeActive:),
            name: objc2_app_kit::NSApplicationDidBecomeActiveNotification,
            object: std::ptr::null::<AnyObject>(),
        ];
    }
}

/// Whether Hex becoming active should open its window: only while it is in
/// the Dock and the app switcher (a regular app), nothing of Hex's took the
/// keyboard as it activated (an overlay), and the window isn't already open.
pub(super) fn reopen_on_activation(
    regular: bool,
    has_key_window: bool,
    window_visible: bool,
) -> bool {
    regular && !has_key_window && !window_visible
}
