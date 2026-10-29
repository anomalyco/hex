//! The floating guide beside System Settings' Privacy & Security pane: it
//! opens the pane for one permission and offers Hex as a drag tile (or, for
//! a refused microphone, asks to switch Hex on), closing once it is granted.

use std::ffi::CStr;

use objc2::MainThreadMarker;

use crate::onboarding::PermissionKind;

unsafe extern "C" {
    fn hex_show_permission_guide(permission: i32);
    fn hex_permission_list_name(permission: i32) -> *const std::ffi::c_char;
}

const fn code(kind: PermissionKind) -> i32 {
    match kind {
        PermissionKind::Accessibility => 1,
        PermissionKind::Microphone => 2,
        PermissionKind::SystemAudio => 3,
        PermissionKind::ScreenRecording => 4,
    }
}

pub(crate) fn show(kind: PermissionKind) {
    if MainThreadMarker::new().is_some() {
        unsafe { hex_show_permission_guide(code(kind)) };
    }
}

/// What System Settings calls the list: macOS 27 renamed Accessibility.
pub(crate) fn list_name(kind: PermissionKind) -> &'static str {
    let name = unsafe { CStr::from_ptr(hex_permission_list_name(code(kind))) };
    name.to_str().unwrap_or("Accessibility")
}
