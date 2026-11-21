//! A shot's rectangle on the display while it flies: from the area it
//! was taken from into its card, and from its card into the editor and
//! back. Four springs move its left, top, width, and height together, and
//! a fifth, its lift, grows its corner and shadow as it leaves the screen
//! it was part of. The overlay's poll steps it on the frame clock, and
//! retargeting keeps every spring's velocity, so a new shot, a close, or a
//! moved destination mid-flight never jumps.

use crate::rune_hud::spring::{Spec, Spring};

/// The way there: critically damped, so neither size nor place overshoots.
pub(crate) const FLY: Spec = Spec::new(0.5, 0.0);
/// Its corner and shadow, a little ahead of its size, so it reads as
/// lifting off the screen before it shrinks away from it.
pub(crate) const LIFT: Spec = Spec::new(0.32, 0.0);
/// Within this many points of its target, and slower than [`REST_V`]
/// points a second, it lands.
const REST: f32 = 0.2;
const REST_V: f32 = 4.0;
/// The most time one step covers, so a stalled frame slows the flight
/// rather than skipping it.
const MAX_STEP: f64 = 1.0 / 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Flight {
    rect: [Spring; 4],
    lift: Spring,
}

impl Flight {
    /// At rest at `rect`, `lift` off the screen (0 is flat on it, 1 a
    /// card).
    pub(crate) fn at(rect: [f32; 4], lift: f32) -> Self {
        Self {
            rect: rect.map(Spring::at),
            lift: Spring::at(lift),
        }
    }

    pub(crate) fn rect(&self) -> [f32; 4] {
        let [x, y, w, h] = self.rect.map(|s| s.x);
        [x, y, w.max(1.0), h.max(1.0)]
    }

    pub(crate) fn lift(&self) -> f32 {
        self.lift.x.clamp(0.0, 1.0)
    }

    /// Where it's heading.
    pub(crate) fn target(&self) -> [f32; 4] {
        self.rect.map(|s| s.target)
    }

    /// How much of the way from `from` to its target it has come, by its
    /// size and place together: 0 at `from`, 1 there.
    pub(crate) fn progress(&self, from: [f32; 4]) -> f32 {
        let to = self.target();
        let now = self.rect();
        let (mut gone, mut whole) = (0.0, 0.0);
        for i in 0..4 {
            gone += (now[i] - from[i]) * (to[i] - from[i]);
            whole += (to[i] - from[i]).powi(2);
        }
        if whole <= f32::EPSILON {
            1.0
        } else {
            (gone / whole).clamp(0.0, 1.0)
        }
    }

    /// Moves `dt` seconds toward `to`, `lift` off the screen; true while
    /// it's still moving.
    pub(crate) fn step(&mut self, to: [f32; 4], lift: f32, dt: f64) -> bool {
        let dt = dt.clamp(0.0, MAX_STEP) as f32;
        for (spring, to) in self.rect.iter_mut().zip(to) {
            spring.step_to(to, FLY, dt);
        }
        self.lift.step_to(lift, LIFT, dt);
        if self.resting() {
            self.land();
            false
        } else {
            true
        }
    }

    /// At `to` now, with no velocity: Reduce Motion, or a still.
    pub(crate) fn jump(&mut self, to: [f32; 4], lift: f32) {
        *self = Self::at(to, lift);
    }

    /// Whether it's at its target, still.
    pub(crate) fn resting(&self) -> bool {
        let near = |s: &Spring, rest: f32| (s.x - s.target).abs() <= rest && s.v.abs() <= REST_V;
        self.rect.iter().all(|s| near(s, REST)) && near(&self.lift, 0.005)
    }

    fn land(&mut self) {
        for spring in &mut self.rect {
            spring.jump(spring.target);
        }
        self.lift.jump(self.lift.target);
    }
}

/// `a` to `b` by `t`.
pub(crate) fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `shadow` as it shows `lift` off the screen: none flat on it, all of it
/// as a card.
pub(crate) fn lifted(mut shadow: rune::Shadow, lift: f32) -> rune::Shadow {
    shadow.color.a *= lift.clamp(0.0, 1.0);
    shadow
}

/// Seconds since the last step, and now remembered as it.
pub(crate) fn tick(last: &mut Option<f64>, now: f64) -> f64 {
    let dt = last.map_or(0.0, |at| now - at);
    *last = Some(now);
    dt.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every frame at 120 Hz from `flight` toward `to` until it lands.
    fn trace(mut flight: Flight, to: [f32; 4], lift: f32) -> Vec<[f32; 4]> {
        let mut frames = vec![flight.rect()];
        for _ in 0..240 {
            let moving = flight.step(to, lift, 1.0 / 120.0);
            frames.push(flight.rect());
            if !moving {
                break;
            }
        }
        frames
    }

    #[test]
    fn a_shot_lands_on_its_card_without_overshooting_or_jumping() {
        // A 900×600 area in the middle of a laptop display, into a card in
        // its corner.
        let from = [270.0, 150.0, 900.0, 600.0];
        let to = [1184.0, 724.0, 240.0, 160.0];
        let frames = trace(Flight::at(from, 0.0), to, 1.0);
        assert_eq!(frames[0], from, "the first frame is exactly the area");
        assert_eq!(*frames.last().unwrap(), to, "it lands exactly on the card");
        assert!(frames.len() < 120, "it lands within a second");
        for pair in frames.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            // Never past the card: no overshoot in place or size.
            assert!(b[0] <= to[0] + 0.01 && b[1] <= to[1] + 0.01);
            assert!(b[2] >= to[2] - 0.01 && b[3] >= to[3] - 0.01);
            // No frame moves more than a smooth flight does at 120 Hz.
            let step = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            assert!(step < 60.0, "a jump of {step} points");
            // Its shape stays the shot's: width and height shrink together.
            let ratio = |r: [f32; 4]| r[2] / r[3];
            assert!((ratio(b) - 1.5).abs() < 0.02, "squashed to {}", ratio(b));
        }
    }

    #[test]
    fn retargeting_mid_flight_keeps_its_place_and_way() {
        let from = [0.0, 0.0, 1440.0, 900.0];
        let to = [1184.0, 734.0, 240.0, 150.0];
        let mut flight = Flight::at(from, 0.0);
        for _ in 0..12 {
            flight.step(to, 1.0, 1.0 / 120.0);
        }
        let before = flight.rect();
        let mut kept = flight;
        kept.step(to, 1.0, 1.0 / 120.0);
        // A second shot pushes this card up a slot.
        let above = [1184.0, 572.0, 240.0, 150.0];
        flight.step(above, 1.0, 1.0 / 120.0);
        let after = flight.rect();
        assert!(after[0] > before[0], "still heading right, as it was");
        // The frame after the turn is the frame it would have drawn, give
        // or take the new pull: its velocity carried over.
        for i in 0..4 {
            assert!(
                (after[i] - kept.rect()[i]).abs() < 1.0,
                "{after:?} vs {:?}",
                kept.rect()
            );
        }
        assert_eq!(flight.target(), above);
    }

    #[test]
    fn its_corner_and_shadow_arrive_before_its_size() {
        let from = [0.0, 0.0, 1000.0, 600.0];
        let mut flight = Flight::at(from, 0.0);
        let to = [1000.0, 700.0, 200.0, 120.0];
        for _ in 0..18 {
            flight.step(to, 1.0, 1.0 / 120.0);
        }
        let size = flight.progress(from);
        assert!(flight.lift() > size, "lift {} size {size}", flight.lift());
    }

    #[test]
    fn the_same_flight_at_60_and_120_hz() {
        let from = [100.0, 100.0, 800.0, 500.0];
        let to = [1100.0, 700.0, 240.0, 150.0];
        let run = |hz: u32| {
            let mut flight = Flight::at(from, 0.0);
            for _ in 0..hz / 4 {
                flight.step(to, 1.0, 1.0 / f64::from(hz));
            }
            flight.rect()
        };
        let (a, b) = (run(60), run(120));
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.05, "{a:?} vs {b:?}");
        }
    }
}
