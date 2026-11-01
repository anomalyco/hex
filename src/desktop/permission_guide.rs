//! The floating guide beside System Settings' Privacy & Security pane: it
//! opens the pane for one permission, clears retired Hex identities' stale TCC
//! rows in the background, and offers Hex as a drag tile (or, for a refused
//! microphone, asks to switch Hex on), closing once it is granted.

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;

use crate::desktop::onboarding::{
    LEGACY_MAC_BUNDLE_IDS, PermissionKind, SettingsFocus, TccResetOutcome, classify_tccutil_reset,
    default_mac_bundle_id, legacy_tcc_services, own_tcc_services,
};

unsafe extern "C" {
    fn hex_show_permission_guide(permission: i32);
    fn hex_permission_list_name(permission: i32) -> *const std::ffi::c_char;
    fn hex_permission_settings_state() -> i32;
    fn hex_prompt_accessibility_trust() -> bool;
    fn hex_running_app_bundle_id(buf: *mut u8, cap: usize) -> usize;
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
    clear_legacy_entries_async(kind);
    if MainThreadMarker::new().is_some() {
        unsafe { hex_show_permission_guide(code(kind)) };
    }
}

/// What System Settings calls the list: macOS 27 renamed Accessibility.
pub(crate) fn list_name(kind: PermissionKind) -> &'static str {
    let name = unsafe { CStr::from_ptr(hex_permission_list_name(code(kind))) };
    name.to_str().unwrap_or("Accessibility")
}

/// Whether System Settings has an open window, whether it is frontmost, and
/// whether Hex is active. Main thread only.
pub(crate) fn settings_focus() -> SettingsFocus {
    if MainThreadMarker::new().is_none() {
        return SettingsFocus::default();
    }
    let bits = unsafe { hex_permission_settings_state() };
    SettingsFocus {
        settings_open: bits & 1 != 0,
        settings_frontmost: bits & 2 != 0,
        hex_active: bits & 4 != 0,
    }
}

/// Registers the running binary in TCC's Accessibility list and triggers
/// macOS's prompt when untrusted.
pub(crate) fn prompt_accessibility_trust() -> bool {
    unsafe { hex_prompt_accessibility_trust() }
}

/// The running `.app` bundle's `CFBundleIdentifier`, or the build's default
/// Hex identifier (`anomaly.hex` / `anomaly.hex.test`) outside a bundle.
pub(crate) fn running_bundle_id() -> String {
    let mut buf = [0u8; 256];
    let len = unsafe { hex_running_app_bundle_id(buf.as_mut_ptr(), buf.len()) };
    if len > 0
        && let Ok(id) = std::str::from_utf8(&buf[..len])
    {
        return id.to_owned();
    }
    default_mac_bundle_id(option_env!("HEX_TEST_BUILD")).to_owned()
}

fn tccutil_allowed() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        !std::env::args_os()
            .any(|arg| arg == "--snap" || arg == "--video" || arg == "--list-scenes")
    }
}

fn reset_bundle_service(service: &str, bundle_id: &str) {
    if !tccutil_allowed() {
        return;
    }
    let output = crate::desktop::background_process::command("/usr/bin/tccutil")
        .args(["reset", service, bundle_id])
        .output();
    match output {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            match classify_tccutil_reset(output.status.code(), &stderr) {
                TccResetOutcome::Cleared => {
                    tracing::info!(service, bundle_id, "cleared stale TCC permission entry");
                }
                TccResetOutcome::NothingToClear => {
                    tracing::debug!(service, bundle_id, "no TCC entry on disk to clear");
                }
                TccResetOutcome::Failed => {
                    tracing::warn!(
                        service,
                        bundle_id,
                        code = ?output.status.code(),
                        stderr = stderr.trim(),
                        "tccutil reset did not succeed"
                    );
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, service, bundle_id, "could not run tccutil reset");
        }
    }
}

/// Clears retired Hex bundle IDs (`com.kitlangton.hex2`, `com.kitlangton.Hex`,
/// `com.kitlangton.Hex.debug`) once per process for `kind`'s TCC services off
/// the UI thread, so System Settings never lists duplicate Hex rows.
pub(crate) fn clear_legacy_entries_async(kind: PermissionKind) {
    static ACCESSIBILITY_CLEARED: AtomicBool = AtomicBool::new(false);
    static SCREEN_AUDIO_CLEARED: AtomicBool = AtomicBool::new(false);

    let services = legacy_tcc_services(kind);
    if services.is_empty() || !tccutil_allowed() {
        return;
    }
    let guard = match kind {
        PermissionKind::Accessibility => &ACCESSIBILITY_CLEARED,
        PermissionKind::SystemAudio | PermissionKind::ScreenRecording => &SCREEN_AUDIO_CLEARED,
        PermissionKind::Microphone => return,
    };
    if guard.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("hex-tcc-legacy".into())
        .spawn(move || {
            for service in services {
                for bundle_id in LEGACY_MAC_BUNDLE_IDS {
                    reset_bundle_service(service, bundle_id);
                }
            }
        })
    {
        guard.store(false, Ordering::Relaxed);
        tracing::warn!(%error, "could not start legacy TCC cleanup");
    }
}

/// Resets the running bundle ID plus retired Hex bundle IDs for `kind` off the
/// UI thread, then calls `done` so the main thread can re-prompt and reopen
/// System Settings.
pub(crate) fn reset_and_reopen_async(
    kind: PermissionKind,
    done: impl FnOnce(PermissionKind) + Send + 'static,
) {
    let own_services = own_tcc_services(kind);
    let legacy_services = legacy_tcc_services(kind);
    if own_services.is_empty() {
        done(kind);
        return;
    }
    let bundle_id = running_bundle_id();
    if let Err(error) = std::thread::Builder::new()
        .name("hex-tcc-reset".into())
        .spawn(move || {
            for service in own_services {
                reset_bundle_service(service, &bundle_id);
            }
            for service in legacy_services {
                for legacy_id in LEGACY_MAC_BUNDLE_IDS {
                    if legacy_id != bundle_id {
                        reset_bundle_service(service, legacy_id);
                    }
                }
            }
            done(kind);
        })
    {
        tracing::warn!(%error, ?kind, "could not start TCC permission reset");
    }
}
