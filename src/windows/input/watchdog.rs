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
//! The hook callbacks never write logs: they record pending lines as
//! atomics. This tick writes them, and the heal's line, after dropping the
//! hook state lock, so no log write ever runs inside a hook callback or
//! under `CALLBACK_STATE`. The tick still runs on the hook thread, so a
//! stalled log writer can delay pending hook calls; it writes at most a few
//! rare lines (streak start/end, a cancel, an ignored Escape, a heal).
//! The back-off and pending lines are process-wide; `install` resets them
//! for a restarted listener.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    KillTimer, MSG, SPI_GETKEYBOARDDELAY, SetTimer, SystemParametersInfoW, WM_TIMER,
};

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
/// The heal's extra room past Windows' auto-repeat delay.
const HEAL_MARGIN_MS: u32 = 250;
/// The heal's gate when the keyboard repeat delay cannot be read.
const HEAL_GATE_FALLBACK_MS: u32 = 1_000;

/// The repeating watchdog timer on the hook thread, or zero before it starts
/// (and after it stops).
static TIMER: AtomicUsize = AtomicUsize::new(0);

/// When either low-level hook last saw an event, in the tick count both the
/// hooks and `GetLastInputInfo` use.
static LAST_EVENT_MS: AtomicU32 = AtomicU32::new(0);

/// When the keyboard hook last saw an event. Separate from the shared tick:
/// a mouse move proves nothing about a keyboard hook another hook silenced.
static LAST_KEYBOARD_EVENT_MS: AtomicU32 = AtomicU32::new(0);

/// Bumped by every keyboard event, so the back-off resets only when the
/// keyboard hook itself sees input.
static KEYBOARD_GENERATION: AtomicU32 = AtomicU32::new(0);

/// The silent re-arm streak and its wait, touched only on the hook thread.
static BACKOFF: Mutex<Backoff> = Mutex::new(Backoff::new());

/// A pending "dictation cancelled by the cancel key" line: Toggle's cancel
/// key, the only mode with one.
static CANCEL_LOG: AtomicBool = AtomicBool::new(false);

/// A pending "Escape ignored while recording" line: Toggle's cancel key when
/// it is Escape and its modifiers did not match.
static ESCAPE_IGNORED_LOG: AtomicBool = AtomicBool::new(false);

/// The silent re-arm streak: how long to wait since the last attempt, how
/// many attempts succeeded, and the keyboard generation the last tick saw.
struct Backoff {
    last_rearm_ms: u32,
    wait_ms: u32,
    streak: u32,
    seen_keyboard_generation: u32,
}

impl Backoff {
    /// No re-arm yet, waiting the base silence, having seen no keyboard
    /// event.
    const fn new() -> Self {
        Self {
            last_rearm_ms: 0,
            wait_ms: HOOK_SILENCE_MS,
            streak: 0,
            seen_keyboard_generation: 0,
        }
    }

    /// Runs one tick: a keyboard event ends a streak and resets the wait; a
    /// mouse-only stamp does neither. Otherwise the base re-arm decision
    /// applies, including the back-off since the last attempt.
    fn on_tick(
        &mut self,
        now_ms: u32,
        keyboard_generation: u32,
        session_last_ms: Option<u32>,
        hook_last_ms: u32,
        hex_foreground: bool,
    ) -> Decision {
        if keyboard_generation != self.seen_keyboard_generation {
            self.seen_keyboard_generation = keyboard_generation;
            self.last_rearm_ms = 0;
            self.wait_ms = HOOK_SILENCE_MS;
            let rearms = std::mem::take(&mut self.streak);
            if rearms > 1 {
                return Decision::StreakEnded { rearms };
            }
        }
        let Some(session_last_ms) = session_last_ms else {
            return Decision::Idle;
        };
        if !should_rearm(
            now_ms,
            session_last_ms,
            hook_last_ms,
            self.last_rearm_ms,
            self.wait_ms,
            hex_foreground,
        ) {
            return Decision::Idle;
        }
        // Back off whether or not the install works: a hook that stays
        // silent (a lock screen or elevated window, say) must not spin.
        self.last_rearm_ms = now_ms;
        self.wait_ms = next_wait(self.wait_ms);
        Decision::Rearm {
            first: self.streak == 0,
        }
    }

    /// A silent re-arm was installed, so it counts toward its streak.
    fn rearmed(&mut self) {
        self.streak += 1;
    }
}

/// What the tick must do after [`Backoff::on_tick`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Idle,
    /// Attempt a re-arm; `first` starts a streak, so it logs once.
    Rearm {
        first: bool,
    },
    /// The keyboard hook saw input again after a longer silent streak.
    StreakEnded {
        rearms: u32,
    },
}

/// Starts the watchdog on the hook thread and resets its back-off. A failure
/// leaves the hooks as they were, without the check.
pub(super) fn install() {
    CANCEL_LOG.store(false, Ordering::Relaxed);
    ESCAPE_IGNORED_LOG.store(false, Ordering::Relaxed);
    *BACKOFF.lock().unwrap_or_else(|error| error.into_inner()) = Backoff {
        seen_keyboard_generation: KEYBOARD_GENERATION.load(Ordering::Relaxed),
        ..Backoff::new()
    };
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
/// another sign the hooks still run, though not the keyboard hook.
pub(super) fn stamp() {
    LAST_EVENT_MS.store(unsafe { GetTickCount() }, Ordering::Relaxed);
}

/// [`stamp`] for the keyboard hook, which also resets the back-off and gates
/// the heal.
pub(super) fn stamp_keyboard() {
    let now = unsafe { GetTickCount() };
    LAST_EVENT_MS.store(now, Ordering::Relaxed);
    LAST_KEYBOARD_EVENT_MS.store(now, Ordering::Relaxed);
    KEYBOARD_GENERATION.fetch_add(1, Ordering::Relaxed);
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
    let keyboard_generation = KEYBOARD_GENERATION.load(Ordering::Relaxed);
    let session = session_last_input();
    let hook_last = LAST_EVENT_MS.load(Ordering::Relaxed);
    let foreground = super::app_is_foreground();
    let decision = {
        let mut backoff = BACKOFF.lock().unwrap_or_else(|error| error.into_inner());
        backoff.on_tick(now, keyboard_generation, session, hook_last, foreground)
    };
    match decision {
        Decision::Idle => {}
        Decision::Rearm { first } => {
            if rearm::rearm_after_silence() {
                if first {
                    tracing::info!(
                        "input was reaching the session but not Hex's low-level hooks; re-armed them"
                    );
                }
                BACKOFF
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .rearmed();
            }
        }
        Decision::StreakEnded { rearms } => {
            tracing::info!(rearms, "input is reaching Hex's keyboard hook again");
        }
    }
    heal_stale_chord(now, keyboard_quiet_threshold());
    flush_logs();
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

/// Windows' auto-repeat delay, from `SPI_GETKEYBOARDDELAY` 0..3, plus the
/// heal's margin. A repeated key stamps faster than this, so a quiet keyboard
/// hook really has no key held down as far as it can see.
fn keyboard_delay_ms(level: u32) -> u32 {
    250 + level.min(3) * 250 + HEAL_MARGIN_MS
}

/// The heal's gate: how long the keyboard hook must have been quiet.
fn keyboard_quiet_threshold() -> u32 {
    let mut level = 0u32;
    // SAFETY: `level` is a live u32 the call writes when it succeeds.
    let read = unsafe {
        SystemParametersInfoW(SPI_GETKEYBOARDDELAY, 0, (&mut level as *mut u32).cast(), 0)
    };
    if read == 0 {
        HEAL_GATE_FALLBACK_MS
    } else {
        keyboard_delay_ms(level)
    }
}

/// Whether the keyboard hook has been quiet longer than `threshold_ms`: a
/// key state that reads up is then a missing release, not a held key whose
/// repeats another hook in the chain hides from `GetAsyncKeyState`.
fn keyboard_quiet(now_ms: u32, keyboard_last_ms: u32, threshold_ms: u32) -> bool {
    now_ms.wrapping_sub(keyboard_last_ms) > threshold_ms
}

/// Finishes a modifier-only normal-dictation chord whose release never
/// arrived: its keys are up while the capture still records. Toggle mode
/// locks on that release, Hold mode finishes, as the release would have; the
/// release is stamped at the tick, so a Hold capture keeps the audio between
/// the real release and this check (up to `TICK_MS` extra). Only a keyless
/// normal-dictation chord takes this path; another shortcut's keyless
/// capture with a lost release still needs the re-arm and its next press.
///
/// The caller writes the log line after this returns and the guard is gone.
fn heal_stale_chord(now_ms: u32, quiet_ms: u32) {
    if !keyboard_quiet(
        now_ms,
        LAST_KEYBOARD_EVENT_MS.load(Ordering::Relaxed),
        quiet_ms,
    ) {
        return;
    }
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
    // Nothing is written back until the chord really is up.
    let raw = ModifierState::read(&key_held);
    let masked = raw.without_altgr(altgr_only());
    if masked.holds(&state.binding, &raw) {
        return false;
    }
    state.raw_modifiers = raw;
    state.modifiers = masked;
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
    fn a_silent_streak_backs_off_and_only_a_keyboard_event_resets_it() {
        let mut backoff = Backoff::new();
        let tick = |backoff: &mut Backoff, at: u32, keyboard_generation: u32| {
            backoff.on_tick(at, keyboard_generation, Some(at - 500), at - 10_000, false)
        };
        let start = 100_000;
        // The first silent tick re-arms at the base silence.
        assert_eq!(
            tick(&mut backoff, start, 0),
            Decision::Rearm { first: true }
        );
        backoff.rearmed();
        // A mouse-only stamp leaves the keyboard generation alone, so the
        // wait stays doubled and the streak does not end.
        assert_eq!(tick(&mut backoff, start + 1_000, 0), Decision::Idle);
        assert_eq!(tick(&mut backoff, start + 5_000, 0), Decision::Idle);
        assert_eq!(
            tick(&mut backoff, start + 6_000, 0),
            Decision::Rearm { first: false }
        );
        backoff.rearmed();
        // Still silent, the second wait has doubled again.
        assert_eq!(tick(&mut backoff, start + 17_000, 0), Decision::Idle);
        assert_eq!(
            tick(&mut backoff, start + 18_000, 0),
            Decision::Rearm { first: false }
        );
        backoff.rearmed();
        // A keyboard event ends the streak with its count and resets the
        // wait: the next silent tick re-arms at the base silence again.
        assert_eq!(
            tick(&mut backoff, start + 19_000, 1),
            Decision::StreakEnded { rearms: 3 }
        );
        assert_eq!(
            tick(&mut backoff, start + 22_000, 1),
            Decision::Rearm { first: true }
        );
    }

    #[test]
    fn a_single_rearm_streak_ends_quietly() {
        let mut backoff = Backoff::new();
        assert_eq!(
            backoff.on_tick(10_000, 0, Some(9_500), 0, false),
            Decision::Rearm { first: true }
        );
        backoff.rearmed();
        assert_eq!(
            backoff.on_tick(11_000, 1, Some(10_500), 10_900, false),
            Decision::Idle,
            "one re-arm already logged when it happened"
        );
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

    #[test]
    fn a_recent_keyboard_event_blocks_the_heal() {
        let now = 10_000;
        let threshold = 500;
        // The key state reads up, but the keyboard hook stamped 100 ms ago:
        // another hook is hiding a held modifier whose repeats still arrive.
        assert!(!keyboard_quiet(now, now - 100, threshold));
        assert!(!keyboard_quiet(now, now - threshold, threshold));
        assert!(keyboard_quiet(now, now - threshold - 1, threshold));
        // Across the tick count's wrap.
        assert!(keyboard_quiet(100, u32::MAX - 500, threshold));
    }

    #[test]
    fn the_repeat_delay_maps_and_clamps() {
        assert_eq!(keyboard_delay_ms(0), 500);
        assert_eq!(keyboard_delay_ms(1), 750);
        assert_eq!(keyboard_delay_ms(2), 1_000);
        assert_eq!(keyboard_delay_ms(3), 1_250);
        assert_eq!(keyboard_delay_ms(9), 1_250);
    }

    #[test]
    fn the_next_wait_doubles_to_the_cap() {
        assert_eq!(next_wait(HOOK_SILENCE_MS), 6_000);
        assert_eq!(next_wait(6_000), 12_000);
        assert_eq!(next_wait(48_000), MAX_WAIT_MS);
        assert_eq!(next_wait(MAX_WAIT_MS), MAX_WAIT_MS);
    }
}
