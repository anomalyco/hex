//! The ready panel's stay: how long it waits before it closes, held while
//! the pointer is inside, and drawn as a rounded-rect ring tracing its ✕
//! box that drains as the time runs out. One countdown idiom with the
//! call-ended capsule's ring and Annotate's Undo.

use std::time::{Duration, Instant};

use rune::{El, LineCap, Shape, Tween, canvas, el};

use super::super::controls::Paint;
use super::Capsule;
use super::state::{ReadyHover, State};
use rune::Tx;

/// The ring's box: the ✕'s 28 pt hit box.
pub(super) const RING_SIZE: f32 = 28.0;
/// Stroke width of the ring.
const RING_STROKE: f32 = 1.5;
/// From the box's edge to the stroke's centreline: the stroke lies on the
/// box's own edge.
const RING_INSET: f32 = RING_STROKE / 2.0;
/// The ✕ box's corners, concentric with the panel's across its inset.
const BOX_RADIUS: f32 =
    super::super::controls::CAPSULE.inner(super::super::controls::CAPSULE.button_inset);
/// A stay this close to its end counts as over when its timer fires.
const SLACK: Duration = Duration::from_millis(250);

/// What is left of the stay, and the drawn drain.
pub(super) struct Stay {
    total: Duration,
    /// What was left when it last paused or resumed.
    left: Duration,
    /// When it resumed; `None` while held.
    since: Option<Instant>,
    line: Tween<[f32; 2]>,
}

impl Stay {
    pub(super) fn new(total: Duration) -> Self {
        let mut stay = Self {
            total,
            left: total,
            since: None,
            line: held(0.0),
        };
        stay.resume();
        stay
    }

    pub(super) fn running(&self) -> bool {
        self.since.is_some()
    }

    /// What is left now.
    pub(super) fn left(&self) -> Duration {
        self.since
            .map_or(self.left, |since| self.left.saturating_sub(since.elapsed()))
    }

    pub(super) fn nearly_over(&self) -> bool {
        self.left() <= SLACK
    }

    /// How much of the ring has drained, 0 to 1.
    fn past(&self, left: Duration) -> f32 {
        if self.total.is_zero() {
            return 1.0;
        }
        (1.0 - left.as_secs_f32() / self.total.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub(super) fn pause(&mut self) {
        if self.since.is_some() {
            self.left = self.left();
            self.since = None;
            self.line = held(self.past(self.left));
        }
    }

    pub(super) fn resume(&mut self) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
            let past = self.past(self.left);
            self.line = Tween::linear([past, 1.0], [1.0, 1.0], self.left.max(SLACK));
        }
    }
}

/// A drain that stays where it is.
fn held(past: f32) -> Tween<[f32; 2]> {
    Tween::linear([past, 1.0], [past, 1.0], Duration::from_secs(1))
}

/// The ring round the ✕: a faint track and the part of the stay left.
pub(super) fn ring(p: Paint, stay: &Stay) -> El {
    let path = outline();
    let layer = |ink| {
        canvas(
            Shape::new()
                .stroke_with(path.clone(), false, RING_STROKE, LineCap::Round, ink)
                .view_box(RING_SIZE, RING_SIZE),
        )
        .absolute(0.0, 0.0)
        .size(RING_SIZE, RING_SIZE)
    };
    el().key("stay")
        .absolute(0.0, 0.0)
        .size(RING_SIZE, RING_SIZE)
        .inert()
        .a11y_hidden()
        .child(layer(p.k.c.border_weak).key("track"))
        .child(layer(p.muted()).key("line").live_trim(&stay.line))
}

/// The ring's centreline round the ✕ box, from the top centre clockwise
/// and back.
fn outline() -> Vec<[f32; 2]> {
    super::call_ended::rounded_rect(RING_SIZE, RING_INSET, BOX_RADIUS - RING_INSET)
}

/// The ready panel's closing, timed by its stay.
impl Capsule {
    /// Handles a panel timeout: a hovered Ready panel waits until the pointer
    /// leaves; everything else closes at once.
    pub(super) fn expire_panel(&mut self) {
        if let State::Ready { hover, stay, .. } = &mut self.state {
            if *hover != ReadyHover::Idle {
                *hover = ReadyHover::ExpiredWhileHovered;
                return;
            }
            // A hover held the stay: its ring still has time, and the frame
            // hook closes the panel when it runs out.
            if !stay.nearly_over() {
                return;
            }
        }
        self.close();
    }

    /// Wakes the frame hook when a running stay will be over.
    pub(super) fn wake_for_stay(&self, tx: &mut Tx) {
        if let State::Ready {
            hover: ReadyHover::Idle,
            stay,
            ..
        } = &self.state
            && stay.running()
        {
            tx.wake_after(stay.left());
        }
    }

    /// Closes a ready panel whose stay ran out with the pointer outside.
    pub(super) fn stay_over(&mut self) -> bool {
        let over = matches!(
            &self.state,
            State::Ready { hover: ReadyHover::Idle, stay, .. } if stay.running() && stay.left().is_zero()
        );
        if over {
            self.close();
        }
        over
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_stay_keeps_what_was_left_and_runs_on_from_there() {
        let mut stay = Stay::new(Duration::from_secs(15));
        assert!(stay.running());
        stay.pause();
        let left = stay.left();
        assert!(!stay.running());
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(stay.left(), left, "held, it doesn't run down");
        stay.resume();
        assert!(stay.running());
        assert!(stay.left() <= left);
        assert!(!stay.nearly_over());
    }

    #[test]
    fn the_ring_stays_inside_the_close_box() {
        assert!(outline().iter().all(|[x, y]| {
            (RING_INSET - 0.01..=RING_SIZE - RING_INSET + 0.01).contains(x)
                && (RING_INSET - 0.01..=RING_SIZE - RING_INSET + 0.01).contains(y)
        }));
    }
}
