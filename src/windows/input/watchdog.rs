//! Checks that the session's input still reaches Hex's low-level hooks.
//!
//! Windows silently removes a low-level hook whose callback takes longer than
//! `LowLevelHooksTimeout`, and an app that registers raw keyboard input can
//! stop every keyboard hook from being called (see `rearm`). Either way the
//! listener keeps its last state and hears nothing: no shortcut, no Escape,
//! and no release for a chord that was already down. A timer on the hook
//! thread compares the session's last input (`GetLastInputInfo`) with what
//! the hooks last saw and installs a fresh keyboard hook when input keeps
//! arriving without reaching them. On the same tick it finishes a
//! modifier-only chord whose release never arrived.
//!
//! Both low-level hooks are on this thread and the mouse hook is always
//! installed (the listener refuses to start without it), so either one
//! stamps the shared last-event tick. A mouse move therefore never looks
//! like silence, and it can keep a dead keyboard hook from being noticed
//! only until the move is older than the silence window.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, MSG, SetTimer, WM_TIMER};

use super::modifiers::{ModifierState, altgr_only, key_down};
use super::rearm;
use super::{CALLBACK_STATE, CallbackState, send_event};

/// How often the hook thread checks the hooks.
const TICK_MS: u32 = 1_000;
/// The session saw input this recently.
const SESSION_INPUT_MS: u32 = 1_000;
/// This long without either hook stamping a single event means input is not
/// reaching them: the first tick gives a slow callback room to finish.
const HOOK_SILENCE_MS: u32 = 3_000;
/// Log the first silent re-arm, then every 60th with the running count.
const LOG_EVERY: u32 = 60;

/// The repeating watchdog timer on the hook thread, or zero before it starts
/// (and after it stops).
static TIMER: AtomicUsize = AtomicUsize::new(0);

/// When either low-level hook last saw an event, in the tick count both the
/// hooks and `GetLastInputInfo` use.
static LAST_EVENT_MS: AtomicU32 = AtomicU32::new(0);

/// Silent re-arms since the listener started, for the rate-limited log.
static SILENT_REARMS: AtomicU32 = AtomicU32::new(0);

/// When an ignored Escape last logged, so a held key's auto-repeat writes one
/// line, not one per repeat.
static ESCAPE_IGNORED_MS: AtomicU32 = AtomicU32::new(0);

/// Starts the watchdog on the hook thread. A failure leaves the hooks as
/// they were, without the check.
pub(super) fn install() {
    let timer = unsafe { SetTimer(std::ptr::null_mut(), 0, TICK_MS, None) };
    if timer == 0 {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "could not start the Windows keyboard hook watchdog"
        );
        return;
    }
    TIMER.store(timer, Ordering::SeqCst);
}

/// Stops the watchdog before the hooks it watches go.
pub(super) fn uninstall() {
    let timer = TIMER.swap(0, Ordering::SeqCst);
    if timer != 0 {
        unsafe {
            KillTimer(std::ptr::null_mut(), timer);
        }
    }
}

/// Stamps that a hook saw an event, before it looks at the event at all:
/// injected input counts, since it proves delivery. The mouse hook is always
/// installed beside the keyboard hook (the listener refuses to start without
/// it), so a move it sees is another sign the hooks still run.
pub(super) fn stamp() {
    LAST_EVENT_MS.store(unsafe { GetTickCount() }, Ordering::Relaxed);
}

/// Handles the watchdog's timer message. `false` for any other message or
/// timer, which the caller's one-shot re-arm may own.
pub(super) fn on_message(message: &MSG) -> bool {
    let timer = TIMER.load(Ordering::SeqCst);
    if message.message != WM_TIMER || timer == 0 || message.wParam != timer {
        return false;
    }
    tick();
    true
}

fn tick() {
    let now = unsafe { GetTickCount() };
    if let Some(last_input) = session_last_input() {
        let hook_last = LAST_EVENT_MS.load(Ordering::Relaxed);
        if should_rearm(now, last_input, hook_last, super::app_is_foreground())
            && rearm::rearm_after_silence()
        {
            // Re-arming over Hex's own raw keyboard input would recreate the
            // silence, which is why a Hex window in front is skipped above.
            let count = SILENT_REARMS.fetch_add(1, Ordering::Relaxed) + 1;
            if logs_rearm(count) {
                tracing::info!(
                    rearms = count,
                    "input was reaching the session but not the Windows keyboard hook; re-armed it"
                );
            }
        }
    }
    heal_stale_chord(now);
}

/// An Escape that reached `process_button`'s Escape branch: it ends no
/// dictation, not even as a chord, and reaches the app (#786). Logs one the
/// cancel shortcut would have taken had its modifiers matched, so a capture
/// that ignores Escape leaves a trace.
pub(super) fn escape_reached_app(state: &CallbackState, is_down: bool) {
    if is_down
        && state.cancel_vk == Some(u32::from(VK_ESCAPE))
        && state.capture.wants_cancel()
        && !state
            .cancel
            .as_ref()
            .is_some_and(|binding| state.holds(binding))
    {
        escape_ignored("the cancel shortcut's modifiers are not down");
    }
}

/// Logs an Escape that reached the app because the cancel shortcut did not
/// match, at most once a second while the key repeats.
fn escape_ignored(reason: &str) {
    let now = unsafe { GetTickCount() };
    let last = ESCAPE_IGNORED_MS.load(Ordering::Relaxed);
    if now.wrapping_sub(last) < TICK_MS {
        return;
    }
    ESCAPE_IGNORED_MS.store(now, Ordering::Relaxed);
    tracing::info!(reason, "Escape ignored while recording");
}

/// The session's last input event, keyboard or mouse, in tick counts.
fn session_last_input() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a live LASTINPUTINFO with its size set.
    (unsafe { GetLastInputInfo(&mut info) } != 0).then_some(info.dwTime)
}

/// Whether input is reaching the session but not the hooks: the session saw
/// input within `SESSION_INPUT_MS`, neither hook has stamped anything for
/// `HOOK_SILENCE_MS`, and Hex is not in front. Tick counts wrap every 49.7
/// days, so every distance is a wrapping subtraction. `hook_last` of zero
/// (nothing seen since the listener started) is simply old.
fn should_rearm(
    now_ms: u32,
    session_last_ms: u32,
    hook_last_ms: u32,
    hex_foreground: bool,
) -> bool {
    !hex_foreground
        && now_ms.wrapping_sub(session_last_ms) <= SESSION_INPUT_MS
        && now_ms.wrapping_sub(hook_last_ms) >= HOOK_SILENCE_MS
}

/// One info line for the first silent re-arm, then every 60th with the count.
/// A hook left silent (input on another desktop, say) would otherwise log
/// every tick.
fn logs_rearm(count: u32) -> bool {
    count == 1 || count.is_multiple_of(LOG_EVERY)
}

/// Finishes a modifier-only normal-dictation chord whose release never
/// arrived: its keys are up while the capture still records. Toggle mode
/// locks on that release, Hold mode finishes, exactly as the release would
/// have. Only a keyless chord takes this path; a chord with a key has its
/// own release handling.
fn heal_stale_chord(now_ms: u32) {
    let mut guard = CALLBACK_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    heal(state, now_ms, key_down);
}

/// [`heal_stale_chord`] with the key read passed in, as the hook's own
/// `process_button` takes it: a test cannot make keys physical.
pub(super) fn heal(state: &mut CallbackState, now_ms: u32, key_held: impl Fn(u16) -> bool) {
    if !state.capture.active || !state.capture.flavour.normal() || state.binding.key.is_some() {
        return;
    }
    // Read the modifiers as the hook would have, including AltGr's
    // synthesized Left Ctrl, since a release can land on another desktop.
    let read = ModifierState::read(key_held);
    let altgr_only = altgr_only();
    state.raw_modifiers = read;
    state.modifiers = read.without_altgr(altgr_only);
    if state.holds(&state.binding) {
        return;
    }
    if let Some(action) = state.capture.release(state.double_tap_enabled, now_ms) {
        state.dirty = state.holds_any_required(&state.binding);
        send_event(state, action, now_ms);
    }
    tracing::info!("the shortcut's release never arrived; resynchronized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_judged_from_the_session_and_the_hooks() {
        let now = 10_000;
        // Input is arriving and the hooks see it.
        assert!(!should_rearm(now, now - 500, now - 100, false));
        // No session input: the hooks simply have nothing to deliver.
        assert!(!should_rearm(now, now - 2_000, now - 10_000, false));
        // The session sees input the hooks have not seen for a while.
        assert!(should_rearm(now, now - 500, now - 3_000, false));
        // A slow callback gets the first tick.
        assert!(!should_rearm(now, now - 500, now - 2_000, false));
        // Hex in front: re-arming would silence the app's own raw input.
        assert!(!should_rearm(now, now - 500, now - 3_000, true));
    }

    #[test]
    fn distances_survive_the_tick_count_wrapping() {
        let now = 100;
        // The hooks' last event was just before the wrap: 4,101 ms ago.
        assert!(should_rearm(now, u32::MAX - 20, u32::MAX - 4_000, false));
        // The session's last input was just before the wrap: 151 ms ago,
        // and the hooks saw this side's input 40 ms ago.
        assert!(!should_rearm(now, u32::MAX - 50, 60, false));
        // Both are recent across the wrap.
        assert!(!should_rearm(now, u32::MAX - 20, u32::MAX - 20, false));
    }

    #[test]
    fn only_the_first_rearm_and_its_counts_are_logged() {
        assert!(logs_rearm(1));
        for count in 2..LOG_EVERY {
            assert!(!logs_rearm(count), "{count}");
        }
        assert!(logs_rearm(LOG_EVERY));
        assert!(!logs_rearm(LOG_EVERY + 1));
        assert!(logs_rearm(LOG_EVERY * 2));
    }
}
