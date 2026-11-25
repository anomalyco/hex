//! Labs › Middle mouse dictation (macOS): holding the middle mouse button
//! for half a second starts a hands-free dictation, and the next long press
//! finishes it. A shorter click does nothing, and every click, long or short,
//! still reaches the app in front: the gesture only watches the button.
//!
//! The event tap reports each middle-button press, drag, and release here
//! ([`press`], [`drag`], [`reset`]); a press that drags the pointer more than
//! [`DRAG_SLOP`] points (panning, autoscroll) is the app's and never counts.
//! The listener asks on every pass whether the held press just became a long one
//! ([`take_long_press`]), so the gesture fires while the button is still
//! down rather than on its release. The press-to-capture decision (finish a
//! hands-free capture, or lock the dictation shortcut's machine) lives with
//! the shortcut machines in `recognition::hotkeys`.

use std::time::Duration;

use crate::audio::CaptureInstant;

/// How long the middle button must stay down to toggle dictation.
pub(crate) const LONG_PRESS: Duration = Duration::from_millis(500);

/// `kCGMouseButtonCenter`: the middle button's CoreGraphics number.
pub(crate) const MIDDLE_BUTTON: u16 = 2;

/// How far, in points, the pointer may move from where the press went down
/// before the press counts as a drag (panning, autoscroll) and never toggles.
pub(crate) const DRAG_SLOP: f64 = 4.0;

/// A pointer position in global display points.
pub(crate) type Point = (f64, f64);

/// One middle-button press as far as the gesture cares: when it went down,
/// and whether it already counted as a long press.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LongPress {
    down_at: Option<CaptureInstant>,
    origin: Point,
    fired: bool,
}

impl LongPress {
    pub(crate) const IDLE: Self = Self {
        down_at: None,
        origin: (0.0, 0.0),
        fired: false,
    };

    /// The button went down at `at`, with the pointer at `origin`; a press
    /// already held is replaced, so a release the tap missed cannot leave an
    /// old press behind.
    pub(crate) fn press(&mut self, at: CaptureInstant, origin: Point) {
        *self = Self {
            down_at: Some(at),
            origin,
            fired: false,
        };
    }

    /// The held button dragged the pointer to `to`: past [`DRAG_SLOP`] from
    /// where it went down, the press is the app's drag and is cancelled.
    pub(crate) fn drag(&mut self, to: Point) {
        let (dx, dy) = (to.0 - self.origin.0, to.1 - self.origin.1);
        if self.down_at.is_some() && dx.hypot(dy) > DRAG_SLOP {
            self.cancel();
        }
    }

    /// Forgets the press in flight: it never becomes a long press.
    pub(crate) fn cancel(&mut self) {
        *self = Self::IDLE;
    }

    /// The button came up: a press shorter than [`LONG_PRESS`] did nothing.
    pub(crate) fn release(&mut self) {
        self.cancel();
    }

    /// Whether the press held at `now` just became a long press: true once
    /// per press, never for a released one.
    pub(crate) fn due(&mut self, now: CaptureInstant) -> bool {
        if !self.ripe(now) {
            return false;
        }
        self.fired = true;
        true
    }

    /// Held at least [`LONG_PRESS`] by `now` and not yet counted.
    fn ripe(&self, now: CaptureInstant) -> bool {
        !self.fired
            && self.down_at.is_some_and(|down_at| {
                now.checked_duration_since(down_at)
                    .is_some_and(|held| held >= LONG_PRESS)
            })
    }
}

#[cfg(target_os = "macos")]
static GESTURE: std::sync::Mutex<LongPress> = std::sync::Mutex::new(LongPress::IDLE);

#[cfg(target_os = "macos")]
fn gesture() -> std::sync::MutexGuard<'static, LongPress> {
    GESTURE.lock().unwrap_or_else(|error| error.into_inner())
}

/// The middle button went down, as the event tap saw it while the lab is on.
#[cfg(target_os = "macos")]
pub(crate) fn press(at: CaptureInstant, origin: Point) {
    gesture().press(at, origin);
}

/// The held middle button dragged the pointer to `to`.
#[cfg(target_os = "macos")]
pub(crate) fn drag(to: Point) {
    gesture().drag(to);
}

/// Forgets a press in flight: the button came up, the lab turned off, or
/// the tap lost events.
#[cfg(target_os = "macos")]
pub(crate) fn reset() {
    gesture().release();
}

/// Whether the held middle button just became a long press, for the
/// listener. `still_down` reads the button's live state, so a release the
/// tap has not reported yet never counts as a long press.
#[cfg(target_os = "macos")]
pub(crate) fn take_long_press(now: CaptureInstant, still_down: impl FnOnce() -> bool) -> bool {
    let mut gesture = gesture();
    if !gesture.ripe(now) {
        return false;
    }
    if !still_down() {
        gesture.release();
        return false;
    }
    gesture.due(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> CaptureInstant {
        CaptureInstant::from_nanos(value * 1_000_000)
    }

    #[test]
    fn a_press_becomes_long_once_after_half_a_second() {
        let mut press = LongPress::IDLE;
        press.press(ms(1_000), (0.0, 0.0));
        assert!(!press.due(ms(1_499)), "still a click");
        assert!(press.due(ms(1_500)));
        assert!(!press.due(ms(1_600)), "one toggle per press");
        press.release();
        press.press(ms(2_000), (0.0, 0.0));
        assert!(press.due(ms(2_700)), "the next press counts again");
    }

    #[test]
    fn a_short_click_never_becomes_long() {
        let mut press = LongPress::IDLE;
        press.press(ms(1_000), (0.0, 0.0));
        assert!(!press.due(ms(1_200)));
        press.release();
        assert!(!press.due(ms(5_000)));
    }

    #[test]
    fn a_new_press_replaces_one_whose_release_was_missed() {
        let mut press = LongPress::IDLE;
        press.press(ms(1_000), (0.0, 0.0));
        press.press(ms(3_000), (0.0, 0.0));
        assert!(!press.due(ms(3_100)), "timed from the new press");
        assert!(press.due(ms(3_500)));
    }

    #[test]
    fn a_dragged_press_never_becomes_long() {
        let mut press = LongPress::IDLE;
        press.press(ms(1_000), (100.0, 100.0));
        press.drag((102.0, 101.0));
        assert!(
            press.due(ms(1_500)),
            "a jitter within the slop is still a press"
        );

        press.press(ms(2_000), (100.0, 100.0));
        press.drag((103.0, 103.0));
        assert!(
            !press.due(ms(2_500)),
            "dragged past the slop: the app's pan"
        );
        assert!(!press.due(ms(5_000)));
        press.release();
        press.press(ms(6_000), (103.0, 103.0));
        assert!(press.due(ms(6_500)), "the next press counts again");
    }
}
