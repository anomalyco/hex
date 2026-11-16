//! Labs › YOLO mode: a shortcut dictation held for at
//! least the lab's chosen time (`labs.yolo_mode_seconds`, 2 s by
//! default) presses Return after its paste, so a long message sends itself.
//! With apps chosen (`labs.yolo_mode_apps`), only a dictation whose
//! frontmost app was one of them sends. Shorter ones paste as usual. On
//! macOS, Escape after the release and before Return goes skips the Return:
//! the text still pastes, unsent.

use std::time::Duration;
#[cfg(target_os = "macos")]
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

/// How long the paste gets to land before Return goes after it: apps that
/// read the clipboard asynchronously (Electron, browsers) could otherwise
/// see Return first and send an empty message.
pub const SETTLE: Duration = Duration::from_millis(100);

/// How long after a dictation is submitted (its release plus the 200 ms
/// release tail) Return waits, so an Escape has time to stop it. Pastes land
/// 0.25–0.5 s after submission and Return went 100 ms later, so it delays a
/// send by at most about 0.25 s.
#[cfg(target_os = "macos")]
pub const ESCAPE_WINDOW: Duration = Duration::from_millis(500);

/// Escape presses the listener has seen. A send compares the count with its
/// submission's to tell whether one came after its release.
#[cfg(target_os = "macos")]
static ESCAPES: AtomicU64 = AtomicU64::new(0);

/// Counts one Escape press seen by the shortcut listener.
#[cfg(target_os = "macos")]
pub fn note_escape() {
    ESCAPES.fetch_add(1, Ordering::AcqRel);
}

/// Whether a submitted dictation may still press Return: no Escape since it
/// was submitted.
#[derive(Clone, Copy, Debug)]
#[cfg(target_os = "macos")]
pub struct SendGate {
    escapes: &'static AtomicU64,
    seen: u64,
    submitted_at: Instant,
}

#[cfg(target_os = "macos")]
impl SendGate {
    /// The gate of a dictation submitted now.
    pub fn open() -> Self {
        Self::on(&ESCAPES, Instant::now())
    }

    pub(crate) fn on(escapes: &'static AtomicU64, submitted_at: Instant) -> Self {
        Self {
            escapes,
            seen: escapes.load(Ordering::Acquire),
            submitted_at,
        }
    }

    pub fn submitted_at(self) -> Instant {
        self.submitted_at
    }

    /// Waits until Return may go, at least `SETTLE` after the paste landed
    /// at `pasted_at` and no sooner than `ESCAPE_WINDOW` after submission,
    /// then answers whether it still goes: no Escape came since submission.
    pub fn wait(self, pasted_at: Instant) -> bool {
        let at = (self.submitted_at + ESCAPE_WINDOW).max(pasted_at + SETTLE);
        std::thread::sleep(at.saturating_duration_since(Instant::now()));
        self.escapes.load(Ordering::Acquire) == self.seen
    }
}

/// Whether a shortcut dictation held `held_ms`, captured with `application`
/// frontmost, presses Return after its paste: the lab is on, the hold
/// reached its chosen time, and the app is one it sends in. Callers pass
/// only hotkey dictations that paste processed text; Dictate without
/// processing, printing stops, and `hex dictation` or local API captures
/// never send.
pub fn sends(held_ms: u64, application: Option<&str>) -> bool {
    crate::labs::yolo_mode().is_some_and(|minimum| {
        long_enough(held_ms, minimum) && in_app(&crate::labs::yolo_mode_apps(), application)
    })
}

fn long_enough(held_ms: u64, minimum: Duration) -> bool {
    u128::from(held_ms) >= minimum.as_millis()
}

/// No apps chosen sends anywhere; otherwise the frontmost app must match
/// one as a Mode's application does, and an unknown app never sends.
fn in_app(apps: &[String], application: Option<&str>) -> bool {
    if apps.is_empty() {
        return true;
    }
    let context = crate::dictation::context::ContextSnapshot {
        application: application.map(str::to_owned),
        ..Default::default()
    };
    apps.iter().any(|app| context.application_is(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_reaching_the_chosen_time_is_long_enough() {
        let two = Duration::from_secs(2);
        assert!(!long_enough(0, two));
        assert!(!long_enough(1_999, two));
        assert!(long_enough(2_000, two));
        assert!(long_enough(60_000, two));
        assert!(!long_enough(4_999, Duration::from_secs(5)));
        assert!(long_enough(1_000, Duration::from_secs(1)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn escape_after_submission_closes_the_gate() {
        static ESCAPES: AtomicU64 = AtomicU64::new(0);
        let past = Instant::now() - ESCAPE_WINDOW - SETTLE;
        ESCAPES.fetch_add(1, Ordering::AcqRel);
        let gate = SendGate::on(&ESCAPES, past);
        assert!(gate.wait(past), "an Escape before submission doesn't count");
        ESCAPES.fetch_add(1, Ordering::AcqRel);
        assert!(!gate.wait(past));
        assert!(SendGate::on(&ESCAPES, past).wait(past));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn return_waits_for_the_window_and_the_settle() {
        static ESCAPES: AtomicU64 = AtomicU64::new(0);
        let started = Instant::now();
        let gate = SendGate::on(
            &ESCAPES,
            started - ESCAPE_WINDOW + Duration::from_millis(30),
        );
        assert!(gate.wait(started - SETTLE));
        assert!(started.elapsed() >= Duration::from_millis(30));
        let started = Instant::now();
        assert!(gate.wait(started - SETTLE + Duration::from_millis(30)));
        assert!(started.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn chosen_apps_limit_where_it_sends() {
        assert!(in_app(&[], None));
        assert!(in_app(&[], Some("Notes")));
        let apps = ["Slack".to_owned(), "Messages.app".to_owned()];
        assert!(in_app(&apps, Some("Slack")));
        assert!(in_app(&apps, Some("messages")));
        assert!(!in_app(&apps, Some("Notes")));
        assert!(!in_app(&apps, None), "an unknown app never sends");
    }
}
