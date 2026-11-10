//! Labs › Pause from menu bar (macOS): the runtime-only pause behind the
//! menu bar's Pause Hex row. While paused every capture shortcut is inert
//! and the event tap passes key events through unmodified, exactly as while
//! a shortcut binding is recorded ([`crate::app_settings`]'s
//! `hotkey_capture_active`). A resume waits for a neutral keyboard, so a key
//! held on the way to Resume can never start a capture.
//!
//! Nothing here is persisted: a restart starts unpaused.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the keyboard must read as neutral after Resume before shortcuts
/// go live again; the same requirement stale-key recovery makes.
const NEUTRAL_DURATION: Duration = Duration::from_millis(100);

/// Whether Hex's shortcuts run, are paused, or wait for a neutral keyboard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Running,
    Paused,
    /// Resume was chosen; shortcuts stay suspended until the keyboard is
    /// neutral for [`NEUTRAL_DURATION`].
    Resuming,
}

/// The pause state and its neutral-keyboard requirement. The clock and the
/// keyboard are passed in, so the rules stay pure and testable.
#[derive(Debug)]
pub struct Machine {
    state: State,
    neutral_since: Option<Instant>,
}

impl Machine {
    pub const fn new() -> Self {
        Self {
            state: State::Running,
            neutral_since: None,
        }
    }

    pub const fn state(&self) -> State {
        self.state
    }

    /// Whether Hex is paused, for the menu's Pause Hex / Resume Hex title.
    /// A pending resume already reads as resumed.
    pub const fn paused(&self) -> bool {
        matches!(self.state(), State::Paused)
    }

    /// Whether shortcuts stay suspended: paused, or resuming.
    pub const fn suspended(&self) -> bool {
        !matches!(self.state(), State::Running)
    }

    /// Pauses Hex's shortcuts. An active capture is cancelled by the
    /// listener's own suspension handling.
    pub fn pause(&mut self) {
        self.state = State::Paused;
        self.neutral_since = None;
    }

    /// Starts a resume. Shortcuts stay suspended until the keyboard has been
    /// neutral for [`NEUTRAL_DURATION`].
    pub fn resume(&mut self) {
        if self.state == State::Paused {
            self.state = State::Resuming;
        }
        self.neutral_since = None;
    }

    /// Ends a pause or a pending resume at once, for the lab turning off.
    pub fn clear(&mut self) {
        self.state = State::Running;
        self.neutral_since = None;
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
    /// live.
    pub fn settle(&mut self, neutral: bool, now: Instant) {
        if self.state != State::Resuming {
            return;
        }
        if !neutral {
            self.neutral_since = None;
            return;
        }
        let Some(since) = self.neutral_since else {
            self.neutral_since = Some(now);
            return;
        };
        if now.duration_since(since) >= NEUTRAL_DURATION {
            self.clear();
        }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

/// The process's pause state: one per Hex, and a restart starts unpaused.
static MACHINE: Mutex<Machine> = Mutex::new(Machine::new());

fn with<T>(change: impl FnOnce(&mut Machine) -> T) -> T {
    let mut machine = MACHINE.lock().unwrap_or_else(|error| error.into_inner());
    change(&mut machine)
}

/// Pauses Hex's shortcuts.
pub fn pause() {
    with(Machine::pause);
}

/// Starts a resume, which goes live once the keyboard is neutral.
pub fn resume() {
    with(Machine::resume);
}

/// Drops a pause or a pending resume; the lab being off leaves nothing to
/// resume from.
pub fn reconcile(lab_on: bool) {
    with(|machine| machine.reconcile(lab_on));
}

/// Whether Hex is paused, for the menu's Pause Hex / Resume Hex title.
pub fn paused() -> bool {
    with(|machine| machine.paused())
}

/// Whether every capture shortcut stays suspended: paused, or resuming.
pub fn suspended() -> bool {
    with(|machine| machine.suspended())
}

fn state() -> State {
    with(|machine| machine.state())
}

/// Advances a pending resume with the keyboard as it is now, and reports
/// whether shortcuts stay suspended after it. The listener calls this on
/// every pass; nothing else needs to. Only a pending resume reads the
/// keyboard, so a running or paused Hex pays no key scan here.
pub fn tick() -> bool {
    let neutral = state() == State::Resuming && keyboard_neutral();
    let now = Instant::now();
    let suspended = with(|machine| {
        machine.settle(neutral, now);
        machine.suspended()
    });
    suspended || crate::app_settings::hotkey_capture_active()
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

        machine.pause();
        assert!(machine.paused());
        assert!(machine.suspended());

        // A key is held when Resume is chosen: the window never opens while
        // it is down, however long Hex waits.
        machine.resume();
        assert!(!machine.paused());
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
        machine.settle(true, at(base, 5_200));
        assert_eq!(machine.state(), State::Running);
        assert!(!machine.suspended());
        assert!(!machine.paused());
    }

    #[test]
    fn a_neutral_keyboard_past_the_window_resumes() {
        let base = Instant::now();
        let mut machine = Machine::new();
        machine.pause();
        machine.resume();
        machine.settle(true, base);
        machine.settle(true, at(base, NEUTRAL_DURATION.as_millis() as u64));
        assert!(!machine.suspended());
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
        machine.resume();
        machine.reconcile(false);
        assert!(!machine.suspended());
        assert_eq!(machine.state(), State::Running);

        // The lab staying on keeps the pause.
        machine.pause();
        machine.reconcile(true);
        assert!(machine.paused());
        assert!(machine.suspended());
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
        machine.resume();
        machine.settle(false, Instant::now());
        assert_eq!(machine.state(), State::Running);
    }
}
