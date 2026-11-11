//! Labs › Pause from menu bar (macOS): the runtime-only pause behind the
//! menu bar's Pause Hex row. While paused every capture shortcut is inert
//! and the event tap passes key events through unmodified, exactly as while
//! a shortcut binding is recorded ([`crate::app_settings`]'s
//! `hotkey_capture_active`). A resume waits for a neutral keyboard, so a key
//! held on the way to Resume can never start a capture.
//!
//! Nothing here is persisted: a restart starts unpaused.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// How long the keyboard must read as neutral after Resume before shortcuts
/// go live again: 100 ms, the same window stale-key recovery uses in
/// `src/suppression.rs`. That module is macOS-only, so the value is repeated
/// here rather than shared.
const NEUTRAL_DURATION: Duration = Duration::from_millis(100);

/// How long a pending resume may wait for a neutral keyboard before it logs
/// one warning naming the key still held.
const SLOW_RESUME: Duration = Duration::from_secs(5);

/// Whether Hex's shortcuts run, are paused, or wait for a neutral keyboard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Running,
    Paused,
    /// Resume was chosen; shortcuts stay suspended until the keyboard is
    /// neutral for [`NEUTRAL_DURATION`].
    Resuming,
}

impl State {
    /// This state's byte in [`STATE`].
    const fn code(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Paused => 1,
            Self::Resuming => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Paused,
            2 => Self::Resuming,
            _ => Self::Running,
        }
    }
}

/// What a pause or resume request did, for the one log line it writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change {
    Paused,
    Resuming,
    Unchanged,
}

/// The pause state and its neutral-keyboard requirement. The clock and the
/// keyboard are passed in, so the rules stay pure and testable.
#[derive(Debug)]
pub struct Machine {
    state: State,
    neutral_since: Option<Instant>,
    resuming_since: Option<Instant>,
}

impl Machine {
    pub const fn new() -> Self {
        Self {
            state: State::Running,
            neutral_since: None,
            resuming_since: None,
        }
    }

    pub const fn state(&self) -> State {
        self.state
    }

    /// Whether shortcuts stay suspended: paused, or resuming. The menu's
    /// Pause Hex / Resume Hex title reads the lock-free [`STATE`] mirror
    /// instead; this is the machine's own answer for its tests.
    #[cfg(test)]
    pub const fn suspended(&self) -> bool {
        !matches!(self.state, State::Running)
    }

    /// Pauses Hex's shortcuts, reporting whether this call changed anything.
    pub fn pause(&mut self) -> bool {
        let changed = self.state != State::Paused;
        self.state = State::Paused;
        self.neutral_since = None;
        self.resuming_since = None;
        changed
    }

    /// Starts a resume at `now`, reporting whether this call started one.
    /// Shortcuts stay suspended until the keyboard has been neutral for
    /// [`NEUTRAL_DURATION`].
    pub fn resume(&mut self, now: Instant) -> bool {
        if self.state != State::Paused {
            return false;
        }
        self.state = State::Resuming;
        self.resuming_since = Some(now);
        self.neutral_since = None;
        true
    }

    /// The menu row's action: pause, start a resume, or leave a pending
    /// resume waiting.
    pub fn toggle(&mut self, now: Instant) -> Change {
        match self.state {
            State::Running => {
                self.pause();
                Change::Paused
            }
            State::Paused => {
                self.resume(now);
                Change::Resuming
            }
            State::Resuming => Change::Unchanged,
        }
    }

    /// Ends a pause or a pending resume at once, for the lab turning off.
    pub fn clear(&mut self) {
        self.state = State::Running;
        self.neutral_since = None;
        self.resuming_since = None;
    }

    /// Ends a pause or a pending resume when the lab is off; while it stays
    /// on, the pause stands.
    pub fn reconcile(&mut self, lab_on: bool) {
        if !lab_on {
            self.clear();
        }
    }

    /// Advances a pending resume with the keyboard as it is at `now`: a held
    /// key restarts the neutral window, and only a full neutral window goes
    /// live. Reports whether this call took the shortcuts live.
    pub fn settle(&mut self, neutral: bool, now: Instant) -> bool {
        if self.state != State::Resuming {
            return false;
        }
        if !neutral {
            self.neutral_since = None;
            return false;
        }
        let Some(since) = self.neutral_since else {
            self.neutral_since = Some(now);
            return false;
        };
        if now.duration_since(since) < NEUTRAL_DURATION {
            return false;
        }
        self.clear();
        true
    }

    /// How long a pending resume has waited at `now`, or `None` when nothing
    /// waits.
    pub fn resuming_for(&self, now: Instant) -> Option<Duration> {
        if self.state != State::Resuming {
            return None;
        }
        self.resuming_since
            .map(|since| now.saturating_duration_since(since))
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

/// The process's pause state: one per Hex, and a restart starts unpaused.
static MACHINE: Mutex<Machine> = Mutex::new(Machine::new());

/// A lock-free mirror of [`MACHINE`]'s state, so the tap thread and the HID
/// threads ask whether shortcuts are suspended without waiting on its lock.
static STATE: AtomicU8 = AtomicU8::new(State::Running.code());

/// Whether the one slow-resume warning has been logged for this resume.
static SLOW_RESUME_WARNED: AtomicBool = AtomicBool::new(false);

fn with<T>(change: impl FnOnce(&mut Machine) -> T) -> T {
    let mut machine = MACHINE.lock().unwrap_or_else(|error| error.into_inner());
    let result = change(&mut machine);
    STATE.store(machine.state().code(), Ordering::Release);
    result
}

fn state() -> State {
    State::from_code(STATE.load(Ordering::Acquire))
}

/// The menu row's action: pause, resume, or leave a pending resume alone.
pub fn toggle() {
    match with(|machine| machine.toggle(Instant::now())) {
        Change::Paused => log_paused(),
        Change::Resuming => log_resuming(),
        // Choosing Resume Hex again while it waits leaves it waiting, so the
        // row cannot re-pause the resume it says is under way.
        Change::Unchanged => {}
    }
}

fn log_paused() {
    SLOW_RESUME_WARNED.store(false, Ordering::Release);
    tracing::info!("paused every shortcut from the menu bar");
}

fn log_resuming() {
    SLOW_RESUME_WARNED.store(false, Ordering::Release);
    tracing::info!("resume requested; waiting for a neutral keyboard");
}

/// Drops a pause or a pending resume; the lab being off leaves nothing to
/// resume from.
pub fn reconcile(lab_on: bool) {
    with(|machine| machine.reconcile(lab_on));
}

/// Whether every capture shortcut stays suspended: paused, or resuming.
/// Lock-free, because the taps ask it for every key they pass through.
pub fn suspended() -> bool {
    state() != State::Running
}

/// Advances a pending resume with the keyboard as it is now, and reports
/// whether shortcuts stay suspended after it. The listener calls this on
/// every pass; nothing else needs to. Only a pending resume reads the
/// keyboard and takes the machine's lock, once, so a running or paused Hex
/// pays neither.
pub fn tick() -> bool {
    if state() == State::Resuming {
        settle_once();
    }
    suspended() || crate::app_settings::hotkey_capture_active()
}

/// One settle pass: reads the keyboard, settles the machine under its lock
/// once, logs going live, and warns once when the wait drags on.
fn settle_once() {
    let neutral = keyboard_neutral();
    let now = Instant::now();
    let (live, waited) = with(|machine| {
        let live = machine.settle(neutral, now);
        (live, machine.resuming_for(now))
    });
    if live {
        SLOW_RESUME_WARNED.store(false, Ordering::Release);
        tracing::info!("shortcuts live again");
        return;
    }
    let Some(waited) = waited else {
        return;
    };
    if waited < SLOW_RESUME || SLOW_RESUME_WARNED.swap(true, Ordering::AcqRel) {
        return;
    }
    tracing::warn!(
        waited_ms = waited.as_millis() as u64,
        held_key_code = ?held_key_code(),
        "resume is still waiting for a neutral keyboard"
    );
}

/// Whether no key or modifier is held now. Linux and Windows have no Pause
/// lab, so their shortcut listeners never reach a resume.
fn keyboard_neutral() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::suppression::keyboard_is_neutral()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// The first key code still held while a resume waits, for its warning.
fn held_key_code() -> Option<u16> {
    #[cfg(target_os = "macos")]
    {
        crate::suppression::held_key_code()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn pause_suspends_and_resume_only_follows_a_neutral_keyboard() {
        let base = Instant::now();
        let mut machine = Machine::new();
        assert_eq!(machine.state(), State::Running);
        assert!(!machine.suspended());

        assert!(machine.pause());
        assert!(machine.suspended());
        // Pausing again changes nothing.
        assert!(!machine.pause());
        assert!(machine.suspended());

        // A key is held when Resume is chosen: the window never opens while
        // it is down, however long Hex waits.
        assert!(machine.resume(at(base, 0)));
        assert!(machine.suspended());
        machine.settle(false, at(base, 0));
        machine.settle(false, at(base, 5_000));
        assert_eq!(machine.state(), State::Resuming);

        // Neutral for less than the window is not enough.
        machine.settle(true, at(base, 5_000));
        machine.settle(true, at(base, 5_050));
        assert!(machine.suspended());

        // Held again mid-window restarts it.
        machine.settle(false, at(base, 5_060));
        machine.settle(true, at(base, 5_100));
        machine.settle(true, at(base, 5_150));
        assert!(machine.suspended());

        // A full neutral window goes live.
        assert!(machine.settle(true, at(base, 5_200)));
        assert_eq!(machine.state(), State::Running);
        assert!(!machine.suspended());
    }

    #[test]
    fn a_neutral_keyboard_past_the_window_resumes() {
        let base = Instant::now();
        let mut machine = Machine::new();
        machine.pause();
        machine.resume(base);
        machine.settle(true, base);
        assert!(machine.settle(true, at(base, NEUTRAL_DURATION.as_millis() as u64)));
        assert!(!machine.suspended());
    }

    #[test]
    fn choosing_resume_while_a_resume_waits_leaves_it_waiting() {
        let base = Instant::now();
        let mut machine = Machine::new();
        assert_eq!(machine.toggle(base), Change::Paused);
        assert!(machine.suspended());

        // The row reads Resume Hex from here on, and choosing it resumes.
        assert_eq!(machine.toggle(at(base, 100)), Change::Resuming);
        assert_eq!(machine.state(), State::Resuming);

        // Choosing Resume Hex again while it waits neither re-pauses nor
        // restarts the wait.
        assert_eq!(machine.toggle(at(base, 200)), Change::Unchanged);
        assert_eq!(machine.state(), State::Resuming);
        assert!(machine.suspended());
        assert_eq!(
            machine.resuming_for(at(base, 300)),
            Some(Duration::from_millis(200))
        );

        machine.settle(true, at(base, 300));
        assert!(machine.settle(true, at(base, 400)));
        assert_eq!(machine.state(), State::Running);
        assert_eq!(machine.toggle(at(base, 500)), Change::Paused);
    }

    #[test]
    fn a_pending_resume_reports_how_long_it_has_waited() {
        let base = Instant::now();
        let mut machine = Machine::new();
        machine.pause();
        machine.resume(base);
        assert_eq!(
            machine.resuming_for(at(base, SLOW_RESUME.as_millis() as u64)),
            Some(SLOW_RESUME)
        );
        // Going live ends the wait.
        machine.settle(true, at(base, 5_000));
        machine.settle(true, at(base, 5_100));
        assert_eq!(machine.resuming_for(at(base, 6_000)), None);
        // A running Hex reports no wait at all.
        assert_eq!(
            Machine::new().resuming_for(at(base, SLOW_RESUME.as_millis() as u64)),
            None
        );
    }

    #[test]
    fn turning_the_lab_off_resumes_at_once() {
        let base = Instant::now();
        let mut machine = Machine::new();
        machine.pause();
        // The lab turning off clears the pause without waiting for a
        // keyboard.
        machine.reconcile(false);
        machine.settle(true, base);
        assert_eq!(machine.state(), State::Running);

        // A pending resume is dropped too.
        machine.pause();
        machine.resume(base);
        machine.reconcile(false);
        assert!(!machine.suspended());
        assert_eq!(machine.state(), State::Running);

        // The lab staying on keeps the pause.
        machine.pause();
        machine.reconcile(true);
        assert!(machine.suspended());
        assert_eq!(machine.state(), State::Paused);
    }

    #[test]
    fn settling_leaves_a_running_hex_alone() {
        let base = Instant::now();
        let mut machine = Machine::new();
        machine.settle(false, base);
        machine.settle(true, at(base, 1_000));
        assert_eq!(machine.state(), State::Running);
    }

    #[test]
    fn a_resume_ignored_while_running_does_not_suspend() {
        let mut machine = Machine::new();
        assert!(!machine.resume(Instant::now()));
        machine.settle(false, Instant::now());
        assert_eq!(machine.state(), State::Running);
    }
}
