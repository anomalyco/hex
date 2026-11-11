//! Labs › Keep mic open after dictating: with Release microphone
//! while idle on, the microphone a capture opens stays open for the lab's
//! window after the capture ends, so the next dictation starts at once. One
//! type decides it for every OS's listener.

use std::time::{Duration, Instant};

/// The window a capture's microphone keeps after it ends, from
/// [`crate::labs::microphone_linger`].
pub(crate) fn window() -> Option<Duration> {
    crate::labs::microphone_linger()
}

/// A released microphone's post-capture window: armed when a capture ends,
/// cleared when one starts, and closed only once it passes with nothing
/// recording.
///
/// Off (`window()` is `None`), [`Self::may_close`] answers as the plain
/// release-while-idle check always did, so nothing changes.
#[derive(Debug, Default)]
pub(crate) struct MicrophoneLinger {
    /// When the window ends; `None` before the first idle pass and while a
    /// capture records.
    deadline: Option<Instant>,
}

impl MicrophoneLinger {
    /// Whether the released microphone may close now, one pass at a time.
    /// `busy` is whether anything still wants the microphone: a capture, a
    /// client's dictation, a release tail, or a capture sink.
    ///
    /// The first idle pass after a capture arms the window, a busy pass
    /// clears it, and only a later idle pass past the window closes. A lab
    /// turned off closes at the next idle pass, as before.
    pub(crate) fn may_close(&mut self, now: Instant, busy: bool) -> bool {
        if busy {
            self.deadline = None;
            return false;
        }
        self.may_close_when_idle(now, window())
    }

    /// [`Self::may_close`] with `window` given, so a test drives it without
    /// this process's settings.
    fn may_close_when_idle(&mut self, now: Instant, window: Option<Duration>) -> bool {
        let Some(window) = window else {
            self.deadline = None;
            return true;
        };
        let deadline = *self.deadline.get_or_insert(now + window);
        if now >= deadline {
            self.deadline = None;
            return true;
        }
        false
    }

    /// A capture started, its stream went away (a lingering stream that heard
    /// only digital silence closes now), or a capture sink took the
    /// microphone: no earlier window keeps it.
    ///
    /// Windows has no digital-silence reopen, so nothing there calls this
    /// outside tests.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(crate) fn clear(&mut self) {
        self.deadline = None;
    }

    /// Whether a window is running, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) fn armed(&self) -> bool {
        self.deadline.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(60);

    fn later(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn the_microphone_closes_only_after_the_window() {
        let mut linger = MicrophoneLinger::default();
        let start = Instant::now();
        assert!(!linger.may_close_when_idle(start, Some(WINDOW)));
        assert!(linger.armed());
        assert!(!linger.may_close_when_idle(later(start, 59), Some(WINDOW)));
        assert!(linger.may_close_when_idle(later(start, 60), Some(WINDOW)));
        assert!(!linger.armed(), "a closed window is not armed");
    }

    #[test]
    fn a_new_capture_clears_the_window_and_its_end_starts_a_fresh_one() {
        let mut linger = MicrophoneLinger::default();
        let start = Instant::now();
        assert!(!linger.may_close_when_idle(start, Some(WINDOW)));
        // A capture records inside the window: nothing may close.
        assert!(!linger.may_close(later(start, 30), true));
        assert!(!linger.armed(), "a busy pass clears the window");
        // Its own end starts a fresh window rather than the old deadline.
        let ended = later(start, 90);
        assert!(!linger.may_close_when_idle(ended, Some(WINDOW)));
        assert!(!linger.may_close_when_idle(later(start, 120), Some(WINDOW)));
        assert!(linger.may_close_when_idle(later(ended, 60), Some(WINDOW)));
    }

    #[test]
    fn no_window_keeps_the_plain_release_while_idle_close() {
        let mut linger = MicrophoneLinger::default();
        let now = Instant::now();
        // Armed by an earlier pass with the lab on.
        assert!(!linger.may_close_when_idle(now, Some(WINDOW)));
        // Turning the lab off releases it at the next idle pass.
        assert!(linger.may_close_when_idle(now, None));
        assert!(!linger.armed());
        // And it stays exactly the old check: idle closes at once, a capture
        // never closes.
        assert!(linger.may_close_when_idle(now, None));
        assert!(!linger.may_close(now, true));
    }

    #[test]
    fn a_cleared_window_waits_for_the_next_capture_to_end() {
        let mut linger = MicrophoneLinger::default();
        let now = Instant::now();
        assert!(!linger.may_close_when_idle(now, Some(WINDOW)));
        linger.clear();
        assert!(!linger.may_close_when_idle(later(now, 30), Some(WINDOW)));
        assert!(linger.may_close_when_idle(later(now, 90), Some(WINDOW)));
    }
}
