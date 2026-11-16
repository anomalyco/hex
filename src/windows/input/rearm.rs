//! Re-arms Hex's keyboard hook so the dictation shortcut keeps working.
//!
//! Some apps (Tauri 2 ones among them, #1901) register raw keyboard input and
//! install their own `WH_KEYBOARD_LL` hook. While such a window holds the
//! foreground, Windows stops calling low-level keyboard hooks at all — Hex's
//! and the app's own — until a hook is installed again, whatever its position
//! in the chain. One side effect: a fresh install also puts Hex's hook ahead
//! of hooks other apps (a key remapper, say) installed after it.
//!
//! Out-of-context `EVENT_SYSTEM_FOREGROUND` callbacks arrive on the installing
//! thread inside `GetMessageW`, so the hook can be swapped during the message
//! loop: the new hook goes in first, then the old one is dropped, with nothing
//! pumped in between, so a held chord is neither duplicated nor lost. A
//! one-shot thread timer swaps once more shortly after, covering an app that
//! shows its window first and installs its hook only then.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, HHOOK, KillTimer, MSG, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_TIMER,
};

/// How long after a swap Hex installs one more hook.
const REARM_DELAY_MS: u32 = 1_500;

/// The keyboard hook Hex owns right now; the foreground callback replaces it.
static KEYBOARD_HOOK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The one-shot re-arm timer pending on the hook thread, if any.
static REARM_TIMER: AtomicUsize = AtomicUsize::new(0);

/// A re-arm failure has been logged; the next ones stay quiet.
static REARM_FAILED: AtomicBool = AtomicBool::new(false);

/// Hex's keyboard hook and the foreground watcher that re-arms it.
pub struct KeyboardHooks {
    foreground: HWINEVENTHOOK,
}

impl KeyboardHooks {
    /// Installs the keyboard hook and, if that works, the watcher.
    pub fn install() -> Self {
        let keyboard = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(super::keyboard_hook),
                ptr::null_mut(),
                0,
            )
        };
        KEYBOARD_HOOK.store(keyboard, Ordering::SeqCst);
        let mut foreground = ptr::null_mut();
        if !keyboard.is_null() {
            // Hex's own process registers raw keyboard input (winit's event
            // loop), so re-arming while a Hex window is in front would
            // recreate the silencing condition.
            foreground = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    ptr::null_mut(),
                    Some(foreground_changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            if foreground.is_null() {
                tracing::warn!(
                    "could not watch the foreground window to re-arm the Windows keyboard hook"
                );
            }
        }
        Self { foreground }
    }

    /// The keyboard hook installed now, or null if it could not be installed.
    pub fn keyboard(&self) -> HHOOK {
        KEYBOARD_HOOK.load(Ordering::SeqCst)
    }

    /// Handles the hook thread's messages: the re-arm timer swaps once more.
    pub fn on_message(message: &MSG) {
        if message.message != WM_TIMER {
            return;
        }
        let timer = REARM_TIMER.load(Ordering::SeqCst);
        if timer == 0 || message.wParam != timer {
            return;
        }
        REARM_TIMER.store(0, Ordering::SeqCst);
        unsafe {
            KillTimer(ptr::null_mut(), timer);
        }
        // The person may have switched to a Hex window meanwhile; re-arming
        // over it would recreate the silence this watcher skips Hex for.
        if !super::app_is_foreground() {
            swap(false);
        }
    }

    /// Stops the watcher and its timer, then drops the keyboard hook installed
    /// now: the foreground callback may have replaced the one `install` made.
    pub fn uninstall(self) {
        unsafe {
            if !self.foreground.is_null() {
                UnhookWinEvent(self.foreground);
            }
            let timer = REARM_TIMER.swap(0, Ordering::SeqCst);
            if timer != 0 {
                KillTimer(ptr::null_mut(), timer);
            }
            UnhookWindowsHookEx(KEYBOARD_HOOK.swap(ptr::null_mut(), Ordering::SeqCst));
        }
    }
}

/// Another window came to the front: install a fresh keyboard hook to re-arm
/// delivery for every low-level hook, then drop the old one.
unsafe extern "system" fn foreground_changed(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    swap(true);
}

/// Installs a fresh keyboard hook and drops the one it replaces. `schedule`
/// arms the follow-up swap; the timer's own swap never arms another.
fn swap(schedule: bool) {
    let previous = KEYBOARD_HOOK.load(Ordering::SeqCst);
    let keyboard = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(super::keyboard_hook),
            ptr::null_mut(),
            0,
        )
    };
    if keyboard.is_null() {
        if !REARM_FAILED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                error = %std::io::Error::last_os_error(),
                "could not re-arm the Windows keyboard hook; the shortcut may do nothing in the app now in front"
            );
        }
        return;
    }
    KEYBOARD_HOOK.store(keyboard, Ordering::SeqCst);
    REARM_FAILED.store(false, Ordering::Relaxed);
    unsafe { UnhookWindowsHookEx(previous) };
    tracing::debug!("re-armed the Windows keyboard hook");
    if schedule {
        let stale = REARM_TIMER.swap(0, Ordering::SeqCst);
        if stale != 0 {
            unsafe {
                KillTimer(ptr::null_mut(), stale);
            }
        }
        let timer = unsafe { SetTimer(ptr::null_mut(), 0, REARM_DELAY_MS, None) };
        REARM_TIMER.store(timer, Ordering::SeqCst);
    }
}
