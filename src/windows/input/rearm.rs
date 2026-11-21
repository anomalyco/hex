//! Re-arms Hex's low-level hooks so the dictation shortcut keeps working.
//!
//! Some apps (Tauri 2 ones among them, #1901) register raw keyboard input and
//! install their own `WH_KEYBOARD_LL` hook. While such a window holds the
//! foreground, Windows stops calling low-level keyboard hooks at all — Hex's
//! and the app's own — until a hook is installed again, whatever its position
//! in the chain. Windows also drops a hook whose callback runs past
//! `LowLevelHooksTimeout`, and the mouse hook can go the same way. One side
//! effect: a fresh install also puts Hex's hook ahead of hooks other apps
//! (a key remapper, say) installed after it.
//!
//! Out-of-context `EVENT_SYSTEM_FOREGROUND` callbacks arrive on the installing
//! thread inside `GetMessageW`, so the hooks can be swapped during the message
//! loop: the new hooks go in first, then the old ones are dropped, with
//! nothing pumped in between, so a held chord is neither duplicated nor lost.
//! A one-shot thread timer swaps once more shortly after, covering an app that
//! shows its window first and installs its hook only then. A repeating timer
//! (`watchdog`) covers a hook Windows stopped calling with no foreground
//! change at all, and re-arms both hooks.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, HHOOK, KillTimer, MSG, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_TIMER,
};

/// How long after a swap Hex installs one more hook.
const REARM_DELAY_MS: u32 = 1_500;

/// The keyboard hook Hex owns right now; the foreground callback replaces it.
static KEYBOARD_HOOK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The mouse hook Hex owns right now, swapped with the keyboard hook.
static MOUSE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The one-shot re-arm timer pending on the hook thread, if any.
static REARM_TIMER: AtomicUsize = AtomicUsize::new(0);

/// A re-arm failure has been logged; the next ones stay quiet.
static REARM_FAILED: AtomicBool = AtomicBool::new(false);

/// Hex's keyboard and mouse hooks and the foreground watcher that re-arms
/// them.
pub struct KeyboardHooks {
    foreground: HWINEVENTHOOK,
}

impl KeyboardHooks {
    /// Installs both low-level hooks and, if they work, the watcher and the
    /// watchdog.
    pub fn install() -> Self {
        let keyboard = install_keyboard();
        let mouse = if keyboard.is_null() {
            ptr::null_mut()
        } else {
            install_mouse()
        };
        KEYBOARD_HOOK.store(keyboard, Ordering::SeqCst);
        MOUSE_HOOK.store(mouse, Ordering::SeqCst);
        let mut foreground = ptr::null_mut();
        if !keyboard.is_null() && !mouse.is_null() {
            super::watchdog::install();
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
                tracing::warn!("could not watch the foreground window to re-arm the Windows hooks");
            }
        }
        Self { foreground }
    }

    /// The keyboard hook installed now, or null if it could not be installed.
    pub fn keyboard(&self) -> HHOOK {
        KEYBOARD_HOOK.load(Ordering::SeqCst)
    }

    /// The mouse hook installed now, or null if it could not be installed.
    pub fn mouse(&self) -> HHOOK {
        MOUSE_HOOK.load(Ordering::SeqCst)
    }

    /// Handles the hook thread's messages: the re-arm timer swaps once more,
    /// and the watchdog checks the hooks every second.
    pub fn on_message(message: &MSG) {
        if message.message != WM_TIMER {
            return;
        }
        if super::watchdog::on_message(message) {
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

    /// Stops the watcher and its timer, then drops the hooks installed now:
    /// the foreground callback may have replaced the ones `install` made.
    pub fn uninstall(self) {
        super::watchdog::uninstall();
        unsafe {
            if !self.foreground.is_null() {
                UnhookWinEvent(self.foreground);
            }
            let timer = REARM_TIMER.swap(0, Ordering::SeqCst);
            if timer != 0 {
                KillTimer(ptr::null_mut(), timer);
            }
            UnhookWindowsHookEx(KEYBOARD_HOOK.swap(ptr::null_mut(), Ordering::SeqCst));
            UnhookWindowsHookEx(MOUSE_HOOK.swap(ptr::null_mut(), Ordering::SeqCst));
        }
    }
}

/// Another window came to the front: install fresh hooks to re-arm delivery
/// for every low-level hook, then drop the old ones.
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

/// Installs fresh hooks because the session's input stopped reaching the old
/// ones, which the watchdog logs. `false` if they could not be installed;
/// `swap` warns once about that.
pub(super) fn rearm_after_silence() -> bool {
    swap(false)
}

fn install_keyboard() -> HHOOK {
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(super::keyboard_hook),
            ptr::null_mut(),
            0,
        )
    }
}

fn install_mouse() -> HHOOK {
    unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(super::mouse_hook), ptr::null_mut(), 0) }
}

/// Installs fresh keyboard and mouse hooks and drops the ones they replace.
/// Both new hooks go in before either old one is dropped. `schedule` arms the
/// follow-up swap; the timer's own swap never arms another.
fn swap(schedule: bool) -> bool {
    let previous_keyboard = KEYBOARD_HOOK.load(Ordering::SeqCst);
    let previous_mouse = MOUSE_HOOK.load(Ordering::SeqCst);
    let keyboard = install_keyboard();
    if keyboard.is_null() {
        rearm_failed(std::io::Error::last_os_error());
        return false;
    }
    let mouse = install_mouse();
    if mouse.is_null() {
        let error = std::io::Error::last_os_error();
        // Keep the pair that works rather than half a re-arm.
        unsafe {
            UnhookWindowsHookEx(keyboard);
        }
        rearm_failed(error);
        return false;
    }
    KEYBOARD_HOOK.store(keyboard, Ordering::SeqCst);
    MOUSE_HOOK.store(mouse, Ordering::SeqCst);
    REARM_FAILED.store(false, Ordering::Relaxed);
    unsafe {
        UnhookWindowsHookEx(previous_keyboard);
        UnhookWindowsHookEx(previous_mouse);
    }
    // The watchdog logs the re-arms it decides on at info; a foreground
    // change can repeat often, so these swaps stay at debug.
    tracing::debug!("re-armed the Windows keyboard and mouse hooks");
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
    true
}

/// One warning for the first failed re-arm; later failures stay quiet.
fn rearm_failed(error: std::io::Error) {
    if !REARM_FAILED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            error = %error,
            "could not re-arm the Windows low-level hooks; the shortcuts may do nothing in the app now in front"
        );
    }
}
