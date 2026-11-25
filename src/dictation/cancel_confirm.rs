//! Labs › Confirm long cancels (#2837): once a dictation has recorded for
//! [`LONG_RECORDING`], its cancel shortcut asks first. The first press keeps
//! recording and shows a notice beside the HUD; a second press within
//! [`CONFIRM_WINDOW`] discards the recording as a cancel always has. A press
//! after the window asks again. With the lab off, or before the threshold,
//! every press cancels at once, as before.
//!
//! Each OS's shortcut machine owns one [`CancelConfirm`]: it says when a
//! capture starts and ends, and asks [`CancelConfirm::press`] before it acts
//! on the cancel shortcut. Hex's own cancels (lost input, a suspended tap,
//! `hex dictation cancel`) never ask.

use std::time::{Duration, Instant};

/// How long a dictation records before its cancel shortcut asks first.
pub(crate) const LONG_RECORDING: Duration = Duration::from_secs(10);

/// How long a first press waits for the second that cancels.
pub(crate) const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

/// The notice beside the HUD while a cancel waits for its second press.
pub(crate) const NOTICE: &str = "Press again to discard this dictation";

/// One shortcut machine's capture start and waiting first press.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CancelConfirm {
    /// When the current capture started, if one records.
    since: Option<Instant>,
    /// When a first press asked, if it still may be confirmed.
    asked_at: Option<Instant>,
}

impl CancelConfirm {
    /// A capture started at `now`.
    pub(crate) fn started(&mut self, now: Instant) {
        *self = Self {
            since: Some(now),
            asked_at: None,
        };
    }

    /// The capture ended, however it ended.
    pub(crate) fn ended(&mut self) {
        *self = Self::default();
    }

    /// The cancel shortcut was pressed at `now`: whether it cancels now.
    /// `false` means it asked instead; the caller keeps recording and shows
    /// the notice ([`ask`]). With `lab` off it always cancels.
    pub(crate) fn press(&mut self, lab: bool, now: Instant) -> bool {
        let long = self
            .since
            .is_some_and(|since| now.saturating_duration_since(since) >= LONG_RECORDING);
        if !lab || !long {
            return true;
        }
        let confirmed = self
            .asked_at
            .is_some_and(|asked| now.saturating_duration_since(asked) <= CONFIRM_WINDOW);
        if confirmed {
            self.asked_at = None;
            return true;
        }
        self.asked_at = Some(now);
        false
    }

    /// [`Self::press`] with the notice: a first press shows it, and the
    /// press that confirms takes it down.
    pub(crate) fn press_with_notice(&mut self, lab: bool, now: Instant) -> bool {
        let asked = self.asked_at.is_some();
        let cancels = self.press(lab, now);
        if !cancels {
            ask();
        } else if asked {
            confirmed();
        }
        cancels
    }
}

/// Shows the notice for a first press beside the HUD, for
/// [`CONFIRM_WINDOW`].
fn ask() {
    tracing::info!("dictation cancel asked for a second press");
    crate::rune_ui::live_text::show_notice(
        crate::desktop::i18n::tr(NOTICE).to_owned(),
        CONFIRM_WINDOW,
    );
}

/// Takes the notice down once the second press cancels.
fn confirmed() {
    crate::rune_ui::live_text::dismiss_failure();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    #[test]
    fn with_the_lab_off_every_press_cancels() {
        let start = Instant::now();
        let mut confirm = CancelConfirm::default();
        confirm.started(start);
        assert!(confirm.press(false, at(start, 60.0)));
    }

    #[test]
    fn a_short_dictation_cancels_at_once() {
        let start = Instant::now();
        let mut confirm = CancelConfirm::default();
        confirm.started(start);
        assert!(confirm.press(true, at(start, 9.5)));
    }

    #[test]
    fn a_long_dictation_asks_then_a_second_press_cancels() {
        let start = Instant::now();
        let mut confirm = CancelConfirm::default();
        confirm.started(start);
        assert!(
            !confirm.press(true, at(start, 30.0)),
            "the first press asks"
        );
        assert!(confirm.press(true, at(start, 32.0)), "the second cancels");
    }

    #[test]
    fn a_press_after_the_window_asks_again() {
        let start = Instant::now();
        let mut confirm = CancelConfirm::default();
        confirm.started(start);
        assert!(!confirm.press(true, at(start, 30.0)));
        assert!(
            !confirm.press(true, at(start, 34.0)),
            "too late: it asks again"
        );
        assert!(confirm.press(true, at(start, 35.0)));
    }

    #[test]
    fn a_new_capture_forgets_the_last_ones_question() {
        let start = Instant::now();
        let mut confirm = CancelConfirm::default();
        confirm.started(start);
        assert!(!confirm.press(true, at(start, 30.0)));
        confirm.ended();
        confirm.started(at(start, 31.0));
        assert!(confirm.press(true, at(start, 32.0)), "the new one is short");
    }

    #[test]
    fn an_unknown_start_cancels_at_once() {
        let mut confirm = CancelConfirm::default();
        assert!(confirm.press(true, Instant::now()));
    }
}
