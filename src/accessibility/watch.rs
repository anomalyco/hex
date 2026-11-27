//! One app's windows, read again and again: what the meeting speaker
//! reader looks at while a call records.
//!
//! The app is found by its bundle id and found again when it quits and
//! comes back. Each read is one bounded [`tree::snapshot`] of every window
//! the app lists, in front or not. A Chromium or Electron app shows its page
//! only once an assistive client asks, so the app is asked once each time it
//! is found; nothing else is ever written to it.
//!
//! It only reads: it never clicks, focuses, raises or types. A snapshot
//! holds other people's names and on-screen text, so nothing here logs one.

use super::tree::{self, Budgets, Snapshot};
use super::{CfObject, foreign_application, live};

/// The windows of the app with one bundle id.
pub(crate) struct AppWindows {
    bundle_id: String,
    budgets: Budgets,
    app: Option<App>,
}

/// The running app, once found.
struct App {
    pid: i32,
    element: CfObject,
    attributes: live::Attributes,
    /// Whether the app has yet to take the request for its web tree.
    web_tree_owed: bool,
}

impl AppWindows {
    /// The windows of a Chromium or Electron app, which is asked for its web
    /// accessibility tree. Each read stops at `budgets`.
    pub(crate) fn of_web_app(bundle_id: &str, budgets: Budgets) -> Self {
        Self {
            bundle_id: bundle_id.to_owned(),
            budgets,
            app: None,
        }
    }

    /// The app's windows now. Nothing at all while Hex may not use
    /// Accessibility or the app is not running.
    pub(crate) fn snapshot(&mut self) -> Snapshot {
        if !live::trusted() {
            self.app = None;
            return Snapshot::default();
        }
        if self.app.as_ref().is_some_and(|app| !running(app.pid)) {
            self.app = None;
        }
        if self.app.is_none() {
            self.app = self.find();
        }
        let Some(app) = &mut self.app else {
            return Snapshot::default();
        };
        if app.web_tree_owed {
            app.web_tree_owed = !live::request_web_accessibility(&app.element);
        }
        tree::snapshot(live::windows(&app.element, &app.attributes), self.budgets)
    }

    fn find(&self) -> Option<App> {
        // AppKit returns the lookup autoreleased, and this runs on a plain
        // thread with no pool of its own to drain it.
        let (pid, _) = objc2::rc::autoreleasepool(|_| {
            crate::microphone_activity::running_application_by_bundle_id(&self.bundle_id)
        })?;
        Some(App {
            pid,
            element: foreign_application(pid)?,
            attributes: live::Attributes::new()?,
            web_tree_owed: true,
        })
    }
}

/// Whether a process with this id is still there.
fn running(pid: i32) -> bool {
    // SAFETY: signal 0 sends nothing; it only asks whether the process is there.
    let answered = unsafe { libc::kill(pid, 0) } == 0;
    answered || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Before anything is asked of another app: an app that is not running
    /// reads as no windows, whether or not this test process may use
    /// Accessibility, and is looked for again at the next read.
    #[test]
    fn an_app_that_is_not_running_reads_as_no_windows() {
        let budgets = Budgets {
            max_nodes: 10,
            max_depth: 2,
            deadline: Duration::from_millis(50),
        };
        let mut windows = AppWindows::of_web_app("invalid.hex.no-such-call-app", budgets);
        for _ in 0..2 {
            assert_eq!(windows.snapshot(), Snapshot::default());
            assert!(windows.app.is_none());
        }
    }

    #[test]
    fn a_process_that_has_gone_is_no_longer_running() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        assert!(running(pid));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!running(pid));
    }
}
