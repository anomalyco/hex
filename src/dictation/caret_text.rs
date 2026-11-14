//! Labs › Rewrite reads nearby text: the text before, inside, and after a
//! dictation's insertion point, as temporary context for one rewrite.
//!
//! The macOS reader lives in [`crate::accessibility`]; [`CaretText`] is plain
//! data every host's context snapshot can hold, and [`CaretTextRead`] is the
//! bounded one-read state machine `recognition::context_watch` starts.
//! Nothing here saves or logs the text.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::thread;

/// The text around one insertion point, as the mode's OpenCode rewrite sees
/// it for one capture: before the selection, the selection itself (which the
/// result replaces), and after it. Empty parts mean the app didn't name them.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct CaretText {
    pub before: String,
    pub selected: String,
    pub after: String,
}

impl std::fmt::Debug for CaretText {
    /// Lengths only: the text itself is never written to a log or a panic
    /// message, even through [`crate::dictation::context::ContextSnapshot`]'s
    /// own debug output.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CaretText")
            .field("before_chars", &self.before.chars().count())
            .field("selected_chars", &self.selected.chars().count())
            .field("after_chars", &self.after.chars().count())
            .finish()
    }
}

impl CaretText {
    /// Whether every part is empty, so the rewrite's prompt carries nothing.
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.selected.is_empty() && self.after.is_empty()
    }
}

/// One capture's read of the text around the cursor: at most one waiting
/// result and at most one running reader. `recognition::context_watch` starts
/// it as the capture becomes real and takes it when the capture finishes.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Default)]
pub(crate) struct CaretTextRead {
    read: Option<mpsc::Receiver<CaretText>>,
    /// A reader thread that has not finished yet: only one runs at a time.
    running: Arc<AtomicBool>,
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
impl CaretTextRead {
    /// Starts this capture's one read, if it has a pending capture and the
    /// lab is on. Off, nothing touches Accessibility and no thread starts.
    #[cfg(target_os = "macos")]
    pub(crate) fn start(&mut self, pending: bool) {
        if !pending || !crate::labs::cursor_context() {
            return;
        }
        let Some(sender) = self.begin(true) else {
            return;
        };
        let running = self.running.clone();
        running.store(true, Ordering::Release);
        let spawned = thread::Builder::new()
            .name("caret-text".into())
            .spawn(move || {
                struct Done(Arc<AtomicBool>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _done = Done(running);
                if let Some(caret) = from_focused_element() {
                    let _ = sender.send(caret);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the caret text reader");
            self.running.store(false, Ordering::Release);
            self.read = None;
        }
    }

    /// No Accessibility reader exists off macOS, so nothing starts there.
    #[cfg(not(target_os = "macos"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn start(&mut self, pending: bool) {
        let _ = pending;
    }

    /// Claims the one read slot. A read already waiting or a reader still
    /// running keeps this one from starting.
    fn begin(&mut self, reading: bool) -> Option<mpsc::Sender<CaretText>> {
        if !reading || self.read.is_some() || self.running.load(Ordering::Acquire) {
            return None;
        }
        let (sender, read) = mpsc::channel();
        self.read = Some(read);
        Some(sender)
    }

    /// This capture's text, if the read finished before its run.
    pub(crate) fn take(&mut self) -> Option<CaretText> {
        self.read
            .take()
            .and_then(|read| read.try_recv().ok())
            .filter(|caret| !caret.is_empty())
    }

    /// The capture ended without transcribing, or a new one started: a late
    /// read is discarded instead of reaching another dictation.
    pub(crate) fn cancel(&mut self) {
        self.read = None;
    }

    /// A read that answers at once, for the sequence tests.
    #[cfg(test)]
    fn read_now(&mut self, pending: bool, reading: bool, before: &str, after: &str) -> bool {
        if !pending {
            return false;
        }
        let Some(sender) = self.begin(reading) else {
            return false;
        };
        let caret = CaretText {
            before: before.into(),
            selected: String::new(),
            after: after.into(),
        };
        sender.send(caret).expect("the read's receiver is held");
        true
    }

    /// Marks a reader thread running or done, for the single-flight test.
    #[cfg(test)]
    fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }
}

/// The text around the focused text element's insertion point, when Labs ›
/// Rewrite reads nearby text is on. Only whether the read found
/// text and how long it took are logged, never the text.
#[cfg(target_os = "macos")]
fn from_focused_element() -> Option<CaretText> {
    let started = std::time::Instant::now();
    let caret = crate::dictation::context::frontmost_pid()
        .and_then(crate::accessibility::text_around_caret);
    tracing::debug!(
        found = caret.is_some(),
        read_ms = started.elapsed().as_millis(),
        "caret text read"
    );
    caret
}

/// The pending-capture and single-flight half of the reader
/// `recognition::context_watch` starts.
#[cfg(test)]
mod context_watch {
    use super::CaretTextRead;

    /// begin -> intend -> cancel -> intend -> finish: a cancelled capture's
    /// read never reaches the next dictation, and an intend with no pending
    /// capture starts nothing.
    #[test]
    fn a_cancelled_capture_leaves_no_stale_caret_text() {
        let mut caret = CaretTextRead::default();
        // begin: a press that is not a real capture yet.
        assert!(!caret.read_now(false, true, "Stale", ""));
        assert_eq!(caret.take(), None);
        // intend: the first real capture reads its field.
        assert!(caret.read_now(true, true, "We ship ", " on Friday."));
        // cancel: its read is dropped, and finish sees nothing stale.
        caret.cancel();
        assert_eq!(caret.take(), None);
        // intend without a pending capture, such as a loop pass after finish.
        assert!(!caret.read_now(false, true, "Stale", ""));
        assert_eq!(caret.take(), None);
        // The next capture's own read is the only one that reaches it.
        assert!(caret.read_now(true, true, "Meet me at ", " today."));
        let caret = caret.take().expect("the next capture's text");
        assert_eq!(caret.before, "Meet me at ");
        assert_eq!(caret.after, " today.");
    }

    #[test]
    fn one_caret_read_runs_at_a_time() {
        let mut caret = CaretTextRead::default();
        caret.set_running(true);
        assert!(
            !caret.read_now(true, true, "OpenCode", ""),
            "a running reader is not doubled"
        );
        caret.set_running(false);
        assert!(caret.read_now(true, true, "Qdrant", ""));
        assert!(
            !caret.read_now(true, true, "OpenCode", ""),
            "a waiting read is not replaced"
        );
        assert_eq!(
            caret.take().map(|caret| caret.before),
            Some("Qdrant".into())
        );
        assert!(
            !caret.read_now(true, false, "OpenCode", ""),
            "the lab being off reads nothing"
        );
        assert_eq!(caret.take(), None);
    }

    #[test]
    fn an_empty_read_is_no_context() {
        let mut caret = CaretTextRead::default();
        assert!(caret.read_now(true, true, "", ""));
        assert_eq!(caret.take(), None);
    }

    #[test]
    fn debug_names_only_lengths() {
        let caret = super::CaretText {
            before: "We ship on ".into(),
            selected: "Friday".into(),
            after: " morning.".into(),
        };
        let debug = format!("{caret:?}");
        assert!(
            !debug.contains("ship") && !debug.contains("Friday"),
            "{debug}"
        );
        assert!(debug.contains("before_chars: 11"), "{debug}");
    }
}
