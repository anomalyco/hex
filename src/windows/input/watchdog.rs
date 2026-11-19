//! Checks that the session's input still reaches Hex's low-level hooks.
//!
//! Windows silently removes a low-level hook whose callback takes longer than
//! `LowLevelHooksTimeout`, and an app that registers raw keyboard input can
//! stop every keyboard hook from being called (see `rearm`). Either way the
//! listener keeps its last state and hears nothing: no shortcut, no Escape,
//! and no release for a chord that was already down. A timer on the hook
//! thread compares the session's last input (`GetLastInputInfo`) with what
//! the hooks last saw and installs fresh keyboard and mouse hooks when input
//! keeps arriving without reaching them. On the same tick it finishes a
//! modifier-only chord whose release never arrived.
//!
//! Both hooks are on this thread, and either one stamps the shared tick. A
//! mouse move the mouse hook still sees therefore cannot look like silence;
//! it can keep a dead keyboard hook from being noticed only until the move
//! is older than the silence window. When the mouse hook is removed too,
//! neither hook stamps and the watchdog re-arms both.
//!
//! The hook callbacks only record state, including pending log lines: the
//! log file is a mutex-guarded write, and blocking a hook is what gets it
//! removed. This tick writes those lines after dropping any lock it took.
//! The back-off and pending lines are process-wide; `install` resets them
//! for a restarted listener.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

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
/// The longest wait between silent re-arms, reached by doubling.
const MAX_WAIT_MS: u32 = 60_000;

/// The repeating watchdog timer on the hook thread, or zero before it starts
/// (and after it stops).
static TIMER: AtomicUsize = AtomicUsize::new(0);

/// When either low-level hook last saw an event, in the tick count both the
/// hooks and `GetLastInputInfo` use.
static LAST_EVENT_MS: AtomicU32 = AtomicU32::new(0);

/// Bumped by every hook stamp, so the tick can reset its back-off as soon as
/// a hook sees input again.
static EVENT_GENERATION: AtomicU32 = AtomicU32::new(0);

/// The generation the last tick saw.
static SEEN_GENERATION: AtomicU32 = AtomicU32::new(0);

/// When the watchdog last tried a silent re-arm, or zero for none yet.
static LAST_REARM_MS: AtomicU32 = AtomicU32::new(0);

/// How long after the last silent re-arm the next one may run: doubles from
/// the base silence up to `MAX_WAIT_MS`.
static NEXT_WAIT_MS: AtomicU32 = AtomicU32::new(HOOK_SILENCE_MS);

/// Re-arms in the current silent streak, for its start and end lines.
static STREAK_REARMS: AtomicU32 = AtomicU32::new(0);

/// A pending "dictation cancelled by the cancel key" line: Toggle's cancel
/// key, the only mode with one.
static CANCEL_LOG: AtomicBool = AtomicBool::new(false);

/// A pending "Escape ignored while recording" line: Toggle's cancel key when
/// it is Escape and its modifiers did not match.
static ESCAPE_IGNORED_LOG: AtomicBool = AtomicBool::new(false);

/// Starts the watchdog on the hook thread and resets its back-off. A failure
/// leaves the hooks as they were, without the check.
pub(super) fn install() {
    CANCEL_LOG.store(false, Ordering::Relaxed);
    ESCAPE_IGNORED_LOG.store(false, Ordering::Relaxed);
    LAST_REARM_MS.store(0, Ordering::Relaxed);
    NEXT_WAIT_MS.store(HOOK_SILENCE_MS, Ordering::Relaxed);
    STREAK_REARMS.store(0, Ordering::Relaxed);
    SEEN_GENERATION.store(EVENT_GENERATION.load(Ordering::Relaxed), Ordering::Relaxed);
    let timer = unsafe { SetTimer(std::ptr::null_mut(), 0, TICK_MS, None) };
    if timer != 0 {
        TIMER.store(timer, Ordering::SeqCst);
        return;
    }
    tracing::warn!(
        error = %std::io::Error::last_os_error(),
        "could not start the Windows keyboard hook watchdog"
    );
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
/// injected input counts, since it proves delivery. The mouse hook is
/// installed and re-armed with the keyboard hook, so a move it sees is
/// another sign the hooks still run.
pub(super) fn stamp() {
    LAST_EVENT_MS.store(unsafe { GetTickCount() }, Ordering::Relaxed);
    EVENT_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Records a toggle cancel for the tick to write. No log file here: the hook
/// callback must stay short.
pub(super) fn note_cancelled() {
    CANCEL_LOG.store(true, Ordering::Relaxed);
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
    let generation = EVENT_GENERATION.load(Ordering::Relaxed);
    if generation != SEEN_GENERATION.swap(generation, Ordering::Relaxed) {
        finish_streak();
        LAST_REARM_MS.store(0, Ordering::Relaxed);
        NEXT_WAIT_MS.store(HOOK_SILENCE_MS, Ordering::Relaxed);
    }
    if let Some(last_input) = session_last_input() {
        let last_rearm = LAST_REARM_MS.load(Ordering::Relaxed);
        let wait = NEXT_WAIT_MS.load(Ordering::Relaxed);
        if should_rearm(
            now,
            last_input,
            LAST_EVENT_MS.load(Ordering::Relaxed),
            last_rearm,
            wait,
            super::app_is_foreground(),
        ) {
            // Back off whether or not the install works: a hook that stays
            // silent (a lock screen or elevated window, say) must not spin.
            LAST_REARM_MS.store(now, Ordering::Relaxed);
            NEXT_WAIT_MS.store(next_wait(wait), Ordering::Relaxed);
            if rearm::rearm_after_silence() && STREAK_REARMS.fetch_add(1, Ordering::Relaxed) == 0 {
                tracing::info!(
                    "input was reaching the session but not Hex's low-level hooks; re-armed them"
                );
            }
        }
    }
    heal_stale_chord(now);
    flush_logs();
}

/// One line when a silent streak that re-armed more than once ends, with its
/// count. A single re-arm already logged when it happened.
fn finish_streak() {
    let rearms = STREAK_REARMS.swap(0, Ordering::Relaxed);
    if rearms > 1 {
        tracing::info!(rearms, "input is reaching Hex's low-level hooks again");
    }
}

/// Writes the lines the hook path only recorded, with no callback state held.
fn flush_logs() {
    if CANCEL_LOG.swap(false, Ordering::Relaxed) {
        tracing::info!("dictation cancelled by the cancel key");
    }
    if ESCAPE_IGNORED_LOG.swap(false, Ordering::Relaxed) {
        tracing::info!(
            reason = "the cancel key's modifiers are not down",
            "Escape ignored while recording"
        );
    }
}

/// An Escape that reached `process_button`'s Escape branch: it ends no
/// dictation, not even as a chord, and reaches the app (#786). Records one
/// the cancel key would have taken had its modifiers matched, so a capture
/// that ignores Escape leaves a trace the next tick writes. The cancel key
/// exists only in Toggle mode.
pub(super) fn escape_reached_app(state: &CallbackState, is_down: bool) {
    if is_down
        && state.cancel_vk == Some(u32::from(VK_ESCAPE))
        && state.capture.wants_cancel()
        && !state
            .cancel
            .as_ref()
            .is_some_and(|binding| state.holds(binding))
    {
        ESCAPE_IGNORED_LOG.store(true, Ordering::Relaxed);
    }
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
/// `HOOK_SILENCE_MS`, and the back-off since the last silent re-arm
/// (`last_rearm_ms` zero for none, `wait_ms` doubling) has passed. Hex in
/// front is skipped: re-arming over its own raw keyboard input would
/// recreate the silence. Tick counts wrap every 49.7 days, so every distance
/// is a wrapping subtraction; `hook_last_ms` of zero (nothing seen since the
/// process started) is simply old.
fn should_rearm(
    now_ms: u32,
    session_last_ms: u32,
    hook_last_ms: u32,
    last_rearm_ms: u32,
    wait_ms: u32,
    hex_foreground: bool,
) -> bool {
    !hex_foreground
        && now_ms.wrapping_sub(session_last_ms) <= SESSION_INPUT_MS
        && now_ms.wrapping_sub(hook_last_ms) >= HOOK_SILENCE_MS
        && (last_rearm_ms == 0 || now_ms.wrapping_sub(last_rearm_ms) >= wait_ms)
}

/// The next silent re-arm waits twice as long, up to `MAX_WAIT_MS`.
fn next_wait(wait_ms: u32) -> u32 {
    wait_ms.saturating_mul(2).min(MAX_WAIT_MS)
}

/// Finishes a modifier-only normal-dictation chord whose release never
/// arrived: its keys are up while the capture still records. Toggle mode
/// locks on that release, Hold mode finishes, as the release would have; the
/// release is stamped at the tick, so a Hold capture keeps the audio between
/// the real release and this check (up to `TICK_MS` extra). Only a keyless
/// chord takes this path; a chord with a key has its own release handling.
///
/// The caller writes the log line after this returns and the guard is gone.
fn heal_stale_chord(now_ms: u32) {
    let healed = {
        let mut guard = CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        heal(state, now_ms, key_down)
    };
    if healed {
        tracing::info!("the shortcut's release never arrived; resynchronized");
    }
}

/// [`heal_stale_chord`] with the key read passed in, as the hook's own
/// `process_button` takes it: a test cannot make keys physical. Returns
/// whether it released the capture, so the caller can log outside the lock.
pub(super) fn heal(state: &mut CallbackState, now_ms: u32, key_held: impl Fn(u16) -> bool) -> bool {
    if super::CAPTURE_INHIBIT.load(Ordering::Relaxed)
        || !state.capture.active
        || !state.capture.flavour.normal()
        || state.binding.key.is_some()
    {
        return false;
    }
    // Read the modifiers as the hook would have, including AltGr's
    // synthesized Left Ctrl, since a release can land on another desktop.
    let read = ModifierState::read(&key_held);
    let altgr_only = altgr_only();
    state.raw_modifiers = read;
    state.modifiers = read.without_altgr(altgr_only);
    if state.holds(&state.binding) {
        return false;
    }
    // Mirror the release path's bookkeeping: the swallowed menu modifier is
    // gone with the release, and the sides read as up must not pair with a
    // fresh press in `SideDownTimes::together`.
    state.mask_release = false;
    state.side_down.forget_released(&key_held);
    if let Some(action) = state.capture.release(state.double_tap_enabled, now_ms) {
        state.dirty = state.holds_any_required(&state.binding);
        send_event(state, action, now_ms);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_judged_from_the_session_and_the_hooks() {
        let now = 10_000;
        // Input is arriving and the hooks see it.
        assert!(!should_rearm(now, now - 500, now - 100, 0, 3_000, false));
        // No session input: the hooks simply have nothing to deliver.
        assert!(!should_rearm(
            now,
            now - 2_000,
            now - 10_000,
            0,
            3_000,
            false
        ));
        // The session sees input the hooks have not seen for a while.
        assert!(should_rearm(now, now - 500, now - 3_000, 0, 3_000, false));
        // A slow callback gets the first tick.
        assert!(!should_rearm(now, now - 500, now - 2_000, 0, 3_000, false));
        // Hex in front: re-arming would silence the app's own raw input.
        assert!(!should_rearm(now, now - 500, now - 3_000, 0, 3_000, true));
    }

    #[test]
    fn a_silent_streak_waits_longer_after_each_rearm() {
        let now = 100_000;
        let session = now - 500;
        let hook = now - 10_000;
        // The first silent re-arm runs at the base silence.
        assert!(should_rearm(now, session, hook, 0, HOOK_SILENCE_MS, false));
        // Then no sooner than the doubled wait.
        assert!(!should_rearm(now, session, hook, now - 5_000, 6_000, false));
        assert!(should_rearm(now, session, hook, now - 6_000, 6_000, false));

        assert_eq!(next_wait(HOOK_SILENCE_MS), 6_000);
        assert_eq!(next_wait(6_000), 12_000);
        assert_eq!(next_wait(48_000), MAX_WAIT_MS);
        assert_eq!(next_wait(MAX_WAIT_MS), MAX_WAIT_MS);
    }

    #[test]
    fn distances_survive_the_tick_count_wrapping() {
        let now = 100;
        // The hooks' last event was just before the wrap: 4,101 ms ago.
        assert!(should_rearm(
            now,
            u32::MAX - 20,
            u32::MAX - 4_000,
            0,
            3_000,
            false
        ));
        // The last silent re-arm was just before the wrap: 200 ms ago.
        assert!(!should_rearm(
            now,
            50,
            u32::MAX - 10_000,
            u32::MAX - 100,
            3_000,
            false
        ));
        // An old one clears the wait across the wrap.
        assert!(should_rearm(
            now,
            50,
            u32::MAX - 10_000,
            u32::MAX - 4_000,
            3_000,
            false
        ));
    }
}
