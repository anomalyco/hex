//! Throwing a meeting capsule into a corner: the drag's release velocity,
//! the corner that throw carries it toward, and the spring that carries the
//! window from where it was let go into that corner.

use std::collections::VecDeque;

use rune::Spring;

use super::corner_placement;
use crate::app_settings::MeetingIndicatorCorner;

/// The kit-motion `GLIDE` token: travel to a new place, no overshoot.
pub(super) const GLIDE: Spring = Spring::new(0.32, 0.0);
/// How far back the release velocity reads the pointer, in seconds.
const VELOCITY_WINDOW: f64 = 0.08;
/// The fastest throw counted, in points per second.
const MAX_SPEED: f32 = 6000.0;
/// UIScrollView's normal deceleration per millisecond: a throw picks the
/// corner nearest where the capsule would coast to rest.
const DECELERATION: f32 = 0.998;
/// Within this many points of the corner (and ten times it in points per
/// second) the glide is done.
const SETTLED: f32 = 0.5;

/// One drag's pointer history and the glide after it.
#[derive(Debug, Default)]
pub(super) struct Throw {
    /// The window's offset from its corner when the drag began: a press
    /// mid-glide catches the capsule where it is.
    grab: (f32, f32),
    /// `(clock, x, y)` offsets from the corner over the last
    /// [`VELOCITY_WINDOW`].
    samples: VecDeque<(f64, f32, f32)>,
    glide: Option<Glide>,
}

impl Throw {
    /// A press: stops a glide in flight where it is.
    pub(super) fn press(&mut self, now: f64) {
        self.grab = self
            .glide
            .take()
            .and_then(|glide| glide.at(now))
            .unwrap_or_default();
        self.samples.clear();
        self.samples.push_back((now, self.grab.0, self.grab.1));
    }

    /// The window's offset from its corner after pointer travel (dx, dy),
    /// recorded for the release velocity.
    pub(super) fn track(&mut self, now: f64, dx: f32, dy: f32) -> (f32, f32) {
        let offset = (self.grab.0 + dx, self.grab.1 + dy);
        self.samples.push_back((now, offset.0, offset.1));
        while self.samples.len() > 2
            && self
                .samples
                .front()
                .is_some_and(|(at, ..)| now - at > VELOCITY_WINDOW)
        {
            self.samples.pop_front();
        }
        offset
    }

    /// The pointer's velocity over the last [`VELOCITY_WINDOW`], in points
    /// per second. A pointer held still before release throws nothing.
    pub(super) fn velocity(&self, now: f64) -> (f32, f32) {
        let recent = || {
            self.samples
                .iter()
                .filter(move |(at, ..)| now - at <= VELOCITY_WINDOW)
        };
        let (Some(first), Some(last)) = (recent().next(), recent().next_back()) else {
            return (0.0, 0.0);
        };
        let dt = (last.0 - first.0) as f32;
        if dt < 0.008 {
            return (0.0, 0.0);
        }
        let velocity = ((last.1 - first.1) / dt, (last.2 - first.2) / dt);
        let speed = velocity.0.hypot(velocity.1);
        if speed > MAX_SPEED {
            let scale = MAX_SPEED / speed;
            (velocity.0 * scale, velocity.1 * scale)
        } else {
            velocity
        }
    }

    /// Ends the drag with no glide.
    pub(super) fn stop(&mut self) {
        self.grab = (0.0, 0.0);
        self.samples.clear();
        self.glide = None;
    }

    /// Glides from `landing` into its corner, starting `now`. Returns the
    /// first offset, or `None` when there is nothing to travel.
    pub(super) fn glide(&mut self, landing: Landing, now: f64) -> Option<(f32, f32)> {
        self.stop();
        let glide = Glide::new(landing.offset, landing.velocity, now);
        let first = glide.at(now)?;
        self.glide = Some(glide);
        Some(first)
    }

    /// Where the glide holds the window `now`, or `None` once it is in its
    /// corner (or there is no glide).
    pub(super) fn step(&mut self, now: f64) -> Option<(f32, f32)> {
        let offset = self.glide.as_ref()?.at(now);
        if offset.is_none() {
            self.glide = None;
        }
        offset
    }

    pub(super) fn gliding(&self) -> bool {
        self.glide.is_some()
    }
}

/// Where a released capsule goes: its corner, and its offset and velocity
/// from that corner as it is let go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Landing {
    pub(super) corner: MeetingIndicatorCorner,
    pub(super) offset: (f32, f32),
    pub(super) velocity: (f32, f32),
}

/// The corner nearest where a capsule window of `size` let go at `offset`
/// from `from` with `velocity` would coast to rest, on a `screen`.
pub(super) fn land(
    from: MeetingIndicatorCorner,
    offset: (f32, f32),
    velocity: (f32, f32),
    screen: (f32, f32),
    size: (f32, f32),
) -> Landing {
    let coast = DECELERATION / (1.0 - DECELERATION) / 1000.0;
    let corner = nearest_corner(
        from,
        offset.0 + velocity.0 * coast,
        offset.1 + velocity.1 * coast,
        screen,
        size,
    );
    let (fx, fy) = rest_origin(from, screen, size);
    let (tx, ty) = rest_origin(corner, screen, size);
    Landing {
        corner,
        offset: (offset.0 + fx - tx, offset.1 + fy - ty),
        velocity,
    }
}

/// The top-left of a capsule window of `size` points resting in `corner`
/// on a screen of `screen` points. A drag's (dx, dy) adds to it in every
/// corner.
pub(super) fn rest_origin(
    corner: MeetingIndicatorCorner,
    screen: (f32, f32),
    size: (f32, f32),
) -> (f32, f32) {
    let (_, margin, _, inset) = corner_placement(corner, 0.0, 0.0);
    let x = if corner.is_right() {
        screen.0 - inset - size.0
    } else {
        inset
    };
    let y = if corner.is_top() {
        margin
    } else {
        screen.1 - margin - size.1
    };
    (x, y)
}

/// The corner nearest the centre of a capsule window of `size` points after
/// a drag of (dx, dy) points from `from`, on a screen of `screen` points.
pub(super) fn nearest_corner(
    from: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
    screen: (f32, f32),
    size: (f32, f32),
) -> MeetingIndicatorCorner {
    let (left, top) = rest_origin(from, screen, size);
    let x = left + dx + size.0 / 2.0;
    let y = top + dy + size.1 / 2.0;
    match (y < screen.1 / 2.0, x >= screen.0 / 2.0) {
        (true, true) => MeetingIndicatorCorner::TopRight,
        (true, false) => MeetingIndicatorCorner::TopLeft,
        (false, true) => MeetingIndicatorCorner::BottomRight,
        (false, false) => MeetingIndicatorCorner::BottomLeft,
    }
}

/// The window springing from an offset to rest in its corner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glide {
    from: (f32, f32),
    velocity: (f32, f32),
    start: f64,
    duration: f32,
}

impl Glide {
    fn new(from: (f32, f32), velocity: (f32, f32), start: f64) -> Self {
        let duration = GLIDE
            .settling_duration_from(from.0, velocity.0, 0.0, SETTLED)
            .max(GLIDE.settling_duration_from(from.1, velocity.1, 0.0, SETTLED));
        Self {
            from,
            velocity,
            start,
            duration,
        }
    }

    /// The offset `now`, or `None` once settled.
    fn at(&self, now: f64) -> Option<(f32, f32)> {
        let t = ((now - self.start) as f32).max(0.0);
        if t >= self.duration {
            return None;
        }
        Some((
            GLIDE.sample(self.from.0, self.velocity.0, 0.0, t).0,
            GLIDE.sample(self.from.1, self.velocity.1, 0.0, t).0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MeetingIndicatorCorner::*;

    const SCREEN: (f32, f32) = (1512.0, 982.0);
    const SIZE: (f32, f32) = (260.0, 180.0);

    #[test]
    fn velocity_reads_the_last_moments_of_the_drag() {
        let mut throw = Throw::default();
        throw.press(0.0);
        for i in 1..=10 {
            throw.track(i as f64 * 0.016, -(i as f32) * 16.0, 0.0);
        }
        let (vx, vy) = throw.velocity(0.16);
        assert!((vx + 1000.0).abs() < 1.0, "{vx}");
        assert_eq!(vy, 0.0);
        // Held still before release: no throw.
        assert_eq!(throw.velocity(0.4), (0.0, 0.0));
        // A flick is capped.
        let mut flick = Throw::default();
        flick.press(0.0);
        flick.track(0.01, -500.0, 0.0);
        let (vx, _) = flick.velocity(0.01);
        assert!((vx + MAX_SPEED).abs() < 0.1, "{vx}");
    }

    #[test]
    fn a_throw_picks_the_corner_it_heads_for() {
        // Let go near where it started, but thrown left and down.
        let landing = land(TopRight, (-100.0, 40.0), (-2000.0, 0.0), SCREEN, SIZE);
        assert_eq!(landing.corner, TopLeft);
        let landing = land(TopRight, (-100.0, 40.0), (0.0, 1500.0), SCREEN, SIZE);
        assert_eq!(landing.corner, BottomRight);
        // Dropped without a throw, the nearest corner.
        let landing = land(TopRight, (-100.0, 40.0), (0.0, 0.0), SCREEN, SIZE);
        assert_eq!(
            landing,
            Landing {
                corner: TopRight,
                offset: (-100.0, 40.0),
                velocity: (0.0, 0.0),
            }
        );
        // Dragged past the middle but thrown back, it returns.
        let landing = land(TopRight, (-700.0, 0.0), (2500.0, 0.0), SCREEN, SIZE);
        assert_eq!(landing.corner, TopRight);
    }

    #[test]
    fn the_window_lands_in_the_new_corner_where_it_was_let_go() {
        let landing = land(TopRight, (-1100.0, 0.0), (-800.0, 0.0), SCREEN, SIZE);
        assert_eq!(landing.corner, TopLeft);
        let before = rest_origin(TopRight, SCREEN, SIZE);
        let after = rest_origin(TopLeft, SCREEN, SIZE);
        assert!((before.0 - 1100.0 - (after.0 + landing.offset.0)).abs() < 0.01);
        assert!((before.1 - (after.1 + landing.offset.1)).abs() < 0.01);
    }

    #[test]
    fn the_glide_springs_into_the_corner_and_stops() {
        let mut throw = Throw::default();
        let landing = Landing {
            corner: TopLeft,
            offset: (120.0, 30.0),
            velocity: (-900.0, 0.0),
        };
        let first = throw.glide(landing, 1.0).unwrap();
        assert_eq!(first, (120.0, 30.0));
        let mid = throw.step(1.1).unwrap();
        assert!(mid.0 < 120.0 && mid.0 > -1.0, "{mid:?}");
        assert!(mid.1 < 30.0 && mid.1 > 0.0, "{mid:?}");
        // No overshoot past the corner on the way in.
        let mut t = 1.0;
        while let Some((x, y)) = throw.step(t) {
            assert!(x >= -SETTLED && y >= -SETTLED, "{x} {y} at {t}");
            t += 1.0 / 120.0;
        }
        assert!(t - 1.0 < 1.0, "settles within a second, took {t}");
        assert!(!throw.gliding());
    }

    #[test]
    fn a_press_catches_a_gliding_capsule_where_it_is() {
        let mut throw = Throw::default();
        throw.glide(
            Landing {
                corner: TopLeft,
                offset: (200.0, 0.0),
                velocity: (0.0, 0.0),
            },
            0.0,
        );
        let held = throw.step(0.05).unwrap();
        throw.press(0.05);
        assert!(!throw.gliding());
        assert_eq!(throw.track(0.06, 10.0, 0.0), (held.0 + 10.0, held.1));
    }

    #[test]
    fn nothing_to_travel_is_no_glide() {
        let mut throw = Throw::default();
        let landing = Landing {
            corner: TopRight,
            offset: (0.0, 0.0),
            velocity: (0.0, 0.0),
        };
        assert_eq!(throw.glide(landing, 0.0), None);
        assert!(!throw.gliding());
    }
}
