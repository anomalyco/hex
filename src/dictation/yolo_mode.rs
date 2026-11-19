//! Labs › YOLO mode: a shortcut dictation presses Return after its paste.
//! With apps chosen (`labs.yolo_mode_apps`), only a dictation whose
//! frontmost app was one of them sends. On
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

/// Read at output commit, only for a shortcut dictation's processed paste.
pub fn sends(application: Option<&str>, bundle_name: Option<&str>) -> bool {
    crate::labs::yolo_mode_apps().is_some_and(|apps| in_app(&apps, application, bundle_name))
}

/// No apps chosen sends anywhere; otherwise the frontmost app must match
/// one as a Mode's application does, and an unknown app never sends.
fn in_app(apps: &[String], application: Option<&str>, bundle_name: Option<&str>) -> bool {
    if apps.is_empty() {
        return true;
    }
    let context = crate::dictation::context::ContextSnapshot {
        application: application.map(str::to_owned),
        bundle_name: bundle_name.map(str::to_owned),
        ..Default::default()
    };
    apps.iter().any(|app| context.application_is(app))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(in_app(&[], None, None));
        assert!(in_app(&[], Some("Notes"), None));
        let apps = ["Slack".to_owned(), "Messages.app".to_owned()];
        assert!(in_app(&apps, Some("Slack"), None));
        assert!(in_app(&apps, Some("messages"), None));
        assert!(!in_app(&apps, Some("Notes"), None));
        assert!(!in_app(&apps, None, None), "an unknown app never sends");
    }

    /// The picker saves Finder's name (`Rex Beta.app`), while the running
    /// app calls itself `Rex`: either name sends.
    #[test]
    fn a_picked_app_sends_under_its_finder_name_or_its_own() {
        let picked = ["Rex Beta".to_owned()];
        assert!(in_app(&picked, Some("Rex"), Some("Rex Beta")));
        assert!(!in_app(&picked, Some("Rex"), None));
        let typed = ["Rex".to_owned()];
        assert!(in_app(&typed, Some("Rex"), Some("Rex Beta")));
        assert!(!in_app(&typed, Some("Rex Nightly"), Some("Rex Beta")));
    }
}
