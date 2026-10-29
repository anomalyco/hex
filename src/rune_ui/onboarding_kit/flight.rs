//! The site hero's word flight (`site/src/shout/main.js`, `launch`,
//! `updateWords`, `settleWord`, `updateLanding`), ported number for number:
//! a word leaves the mouth small, arcs out on a bent quadratic path, grows
//! then shrinks into its typeset slot with a few points of overshoot,
//! hands off to the typeset word over 0.12 s, and cools from hot.
//!
//! The site steps each word per frame; here every pose is a function of
//! the clock, so a frame can be drawn from any time. Drawing follows the
//! site's WebGPU renderer (`gpu.js`), the one its hero runs: ten copies
//! smeared along the velocity over the shutter, under an ember glow of two
//! blurred copies, since Rune text has no shadow.

use rune::{Anim, Color, El, Spring, TextStyle, el, text};

use super::super::controls::Paint;

/// How long the flying word and its typeset self cross-fade.
pub(crate) const HANDOFF: f64 = 0.12;
/// The WebGPU renderer's shutter, 0.85 of a 60 Hz frame.
const SHUTTER: f32 = 0.85 / 60.0;
/// The WebGPU renderer's ten samples along the velocity.
const TAPS: usize = 10;
/// The glow layers' strength in the word atlas's green channel, 170 / 255.
const GLOW_INK: f32 = 0.667;
/// The caret's glide, `1 - exp(-dt * 45)` per frame.
const CARET_RATE: f64 = 45.0;
/// A calm landing nudges the card: 18 pt/s into a spring of 7 Hz, ζ 0.4.
const NUDGE: f32 = 18.0;
const NUDGE_HZ: f32 = 7.0;
const NUDGE_ZETA: f32 = 0.4;
/// Any other card springs back at 9.2 Hz, ζ 0.72.
const HIT_HZ: f32 = 9.2;
const HIT_ZETA: f32 = 0.72;
/// The velocity a word starts with, before it has a previous frame.
const LAUNCH_SPEED: f32 = 1000.0;
/// Velocity and the angle it sets come from frame to frame.
const FRAME: f64 = 1.0 / 60.0;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The site's seeded `rnd(a, b)`, steady per word.
fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 78.233 + 1.7).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

/// One word's flight from `from` to its slot centre `to`, launched at
/// `start` at intensity `i` (0 calm to 1 furious; the hero opens calm,
/// under 0.22). `index` alternates the side it fans out on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flight {
    from: [f32; 2],
    to: [f32; 2],
    start: f64,
    i: f32,
    /// The sprite's size, `lerp(17, 66, i^1.2)`.
    size: f32,
    /// The typeset size it shrinks to.
    land: f32,
    bend: f32,
    dur: f64,
    back: f32,
}

/// Where a flying word is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    /// The size it's drawn at, and the size its text is set at before
    /// scaling, as the site scales one sprite per word.
    pub(crate) size: f32,
    pub(crate) sprite: f32,
    pub(crate) angle: f32,
    pub(crate) heat: f32,
    pub(crate) alpha: f32,
}

/// A landed word's typeset self: `fade` in from the handoff, `heat`
/// cooling from 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Landed {
    pub(crate) fade: f32,
    pub(crate) heat: f32,
}

impl Flight {
    pub(crate) fn new(
        from: [f32; 2],
        to: [f32; 2],
        start: f64,
        i: f32,
        land: f32,
        index: usize,
    ) -> Self {
        // `fanSide` starts at 1 and flips before each launch.
        let side = if index.is_multiple_of(2) { -1.0 } else { 1.0 };
        let from = [from[0], from[1] + 4.0];
        let dist = (to[0] - from[0]).hypot(to[1] - from[1]);
        Self {
            from,
            to,
            start,
            i,
            size: lerp(17.0, 66.0, i.powf(1.2)),
            land,
            bend: side * lerp(0.12, 0.3, i) * rnd(index, 0.5, 1.0),
            dur: f64::from((dist / lerp(800.0, 2600.0, i)).clamp(0.16, 0.9) * 1.25),
            back: (27.0 * lerp(2.0, 4.0, i) / (4.0 * dist.max(60.0))).cbrt(),
        }
    }

    pub(crate) fn start(&self) -> f64 {
        self.start
    }

    /// When it reaches its slot.
    pub(crate) fn arrives(&self) -> f64 {
        self.start + self.dur
    }

    /// Where it lands.
    pub(crate) fn to(&self) -> [f32; 2] {
        self.to
    }

    pub(crate) fn intensity(&self) -> f32 {
        self.i
    }

    /// The direction it's heading as it lands, a unit vector.
    pub(crate) fn heading(&self) -> [f32; 2] {
        let [x0, y0] = self.point(self.dur - FRAME);
        let [x1, y1] = self.to;
        let (dx, dy) = (x1 - x0, y1 - y0);
        let d = dx.hypot(dy).max(1e-3);
        [dx / d, dy / d]
    }

    /// Ease-out-back: no speed at arrival, a few points past it first.
    fn ease(&self, u: f32) -> f32 {
        let x1 = 1.0 - u;
        1.0 - (self.back + 1.0) * x1 * x1 * x1 + self.back * x1 * x1
    }

    fn point(&self, age: f64) -> [f32; 2] {
        let u = (age / self.dur).clamp(0.0, 1.0) as f32;
        let s = self.ease(u);
        let [sx, sy] = self.from;
        let [gx, gy] = self.to;
        let (dx, dy) = (gx - sx, gy - sy);
        let cx = sx + dx * 0.5 - dy * self.bend;
        let cy = sy + dy * 0.5 + dx * self.bend;
        let a = (1.0 - s) * (1.0 - s);
        let b = 2.0 * (1.0 - s) * s;
        let c = s * s;
        [a * sx + b * cx + c * gx, a * sy + b * cy + c * gy]
    }

    fn velocity(&self, age: f64) -> [f32; 2] {
        if age < FRAME * 0.5 {
            let (dx, dy) = (self.to[0] - self.from[0], self.to[1] - self.from[1]);
            let d = dx.hypot(dy).max(1e-3);
            return [dx / d * LAUNCH_SPEED, dy / d * LAUNCH_SPEED];
        }
        let [x0, y0] = self.point(age - FRAME);
        let [x1, y1] = self.point(age);
        [(x1 - x0) / FRAME as f32, (y1 - y0) / FRAME as f32]
    }

    /// The word in flight at `now`, or `None` before launch and once its
    /// handoff is over. Under Reduce Motion it never flies.
    pub(crate) fn pose(&self, now: f64, reduced: bool) -> Option<Pose> {
        let age = now - self.start;
        if reduced || age < 0.0 || age > self.dur + HANDOFF {
            return None;
        }
        let slot = self.land / self.size;
        if age >= self.dur {
            // Pinned to its slot while the typeset word fades in under it.
            let k = ((age - self.dur) / HANDOFF).clamp(0.0, 1.0) as f32;
            return Some(Pose {
                x: self.to[0],
                y: self.to[1],
                vx: 0.0,
                vy: 0.0,
                size: self.land,
                sprite: self.size,
                angle: 0.0,
                heat: self.heat0().max(0.9),
                alpha: 1.0 - k,
            });
        }
        let u = (age / self.dur) as f32;
        let [x, y] = self.point(age);
        let [vx, vy] = self.velocity(age);
        let grow = (age as f32 * 10.0).min(1.0);
        let mut ang = vy.atan2(vx);
        if ang > std::f32::consts::FRAC_PI_2 {
            ang -= std::f32::consts::PI;
        }
        if ang < -std::f32::consts::FRAC_PI_2 {
            ang += std::f32::consts::PI;
        }
        let e = smooth(0.15, 0.85, u);
        Some(Pose {
            x,
            y,
            vx,
            vy,
            size: self.size * lerp(0.3, 1.0, grow) * lerp(1.0, slot, e),
            sprite: self.size,
            angle: lerp(ang * 0.25, 0.0, e),
            heat: lerp(self.heat0(), self.heat0().max(0.9), e),
            alpha: 1.0,
        })
    }

    fn heat0(&self) -> f32 {
        lerp(0.1, 1.0, self.i)
    }

    /// The typeset word at `now`, once the flight has arrived.
    pub(crate) fn landed(&self, now: f64, reduced: bool) -> Option<Landed> {
        let since = now - if reduced { self.start } else { self.arrives() };
        if since < 0.0 {
            return None;
        }
        if reduced {
            return Some(Landed {
                fade: 1.0,
                heat: 0.0,
            });
        }
        let fade = (since / HANDOFF).clamp(0.0, 1.0) as f32;
        let cool = lerp(0.3, 0.6, self.i);
        let h = if fade >= 1.0 {
            (1.0 - ((since - HANDOFF).max(0.0) as f32) / cool).max(0.0)
        } else {
            1.0
        };
        Some(Landed { fade, heat: h * h })
    }

    /// How a landing at `now` nudges the card: a velocity kick along the
    /// word's last heading into a damped spring, summed over landings.
    pub(crate) fn nudge(&self, now: f64) -> [f32; 2] {
        self.nudge_by(now, NUDGE)
    }

    /// [`Flight::nudge`] for a card hit at the word's intensity, as
    /// `hitCard` pushes one that isn't the OpenCode window: gently while
    /// calm, hard past 0.36.
    pub(crate) fn hit(&self, now: f64) -> [f32; 2] {
        let push = if self.i < 0.36 {
            lerp(22.0, 48.0, self.i / 0.36)
        } else {
            lerp(75.0, 220.0, self.i)
        };
        self.spring(now, push, HIT_HZ, HIT_ZETA)
    }

    /// How brightly the card flashes from this landing at `now`: the
    /// site's `lerp(0.28, 0.72, i)`, fading at 6.5 a second.
    pub(crate) fn flash(&self, now: f64) -> f32 {
        let t = (now - self.arrives()) as f32;
        if t < 0.0 {
            return 0.0;
        }
        let start = if self.i < 0.36 {
            0.14
        } else {
            lerp(0.28, 0.72, self.i)
        };
        (start - t * 6.5).max(0.0)
    }

    fn nudge_by(&self, now: f64, push: f32) -> [f32; 2] {
        self.spring(now, push, NUDGE_HZ, NUDGE_ZETA)
    }

    fn spring(&self, now: f64, push: f32, hz: f32, zeta: f32) -> [f32; 2] {
        let t = (now - self.arrives()) as f32;
        if t < 0.0 {
            return [0.0, 0.0];
        }
        let [ux, uy] = self.heading();
        let w = std::f32::consts::TAU * hz;
        let wd = w * (1.0 - zeta * zeta).sqrt();
        let k = push * (-zeta * w * t).exp() * (wd * t).sin() / wd;
        [ux * k, uy * k]
    }
}

/// Where the caret is at `now` as it glides toward each landed word's end:
/// `targets` are `(when, x)` in landing order, starting at `x0`.
pub(crate) fn caret_x(x0: f32, targets: &[(f64, f32)], now: f64) -> f32 {
    let mut x = x0;
    let mut goal = x0;
    let mut t = f64::NEG_INFINITY;
    for &(at, to) in targets {
        if at > now {
            break;
        }
        if t.is_finite() {
            x = goal + (x - goal) * (-(at - t) * CARET_RATE).exp() as f32;
        }
        goal = to;
        t = at;
    }
    if t.is_finite() {
        x = goal + (x - goal) * (-(now - t) * CARET_RATE).exp() as f32;
    }
    x
}

/// The site's ember ramp: deep red, red, orange, near white.
pub(crate) fn ember(h: f32) -> Color {
    const STOPS: [[f32; 3]; 4] = [
        [0.35, 0.02, 0.01],
        [0.95, 0.16, 0.06],
        [1.0, 0.55, 0.22],
        [1.0, 0.95, 0.88],
    ];
    let h = h.clamp(0.0, 1.0) * 3.0;
    let i = (h.floor() as usize).min(2);
    let f = h - i as f32;
    let c = |k: usize| lerp(STOPS[i][k], STOPS[i + 1][k], f);
    srgb(c(0), c(1), c(2), 1.0)
}

/// A colour from sRGB components in 0..1, as the site writes them.
pub(crate) fn srgb(r: f32, g: f32, b: f32, a: f32) -> Color {
    let d = rune::color::srgb_decode;
    Color {
        r: d(r),
        g: d(g),
        b: d(b),
        a,
    }
}

/// The ink a word is drawn in at `heat`: the site's near white warming
/// toward cream in dark; in light, the theme's ink warming toward the
/// ember, as cream wouldn't show on white.
pub(crate) fn ink(p: Paint, heat: f32) -> Color {
    if p.k.dark {
        p.k.c.text_base.mix(srgb(1.0, 0.97, 0.88, 1.0), heat)
    } else {
        p.k.c.text_base.mix(ember(0.5), heat * 0.75)
    }
}

/// A landed word's glow at `heat`, `rgba(255, 120 + 90(1 - h), 60)`.
fn landed_glow(p: Paint, heat: f32) -> Color {
    let g = (120.0 + 90.0 * (1.0 - heat)) / 255.0;
    let strength = if p.k.dark { 0.9 } else { 0.55 };
    srgb(1.0, g, 60.0 / 255.0, strength * heat)
}

/// Positioned text, its box centred on `(x, y)` in the overlay's points.
fn word_at(
    key: String,
    word: &str,
    font: TextStyle,
    color: Color,
    x: f32,
    y: f32,
    angle: f32,
) -> El {
    let (w, h) = rune::text::measure(word, &[], font, None);
    text(word.to_string())
        .key(key)
        .font(font)
        .color(color)
        .absolute(x - w / 2.0, y - h / 2.0)
        .rotate(angle)
        .motion(Spring::new(0.016, 0.0))
        .offset_motion(Anim::INSTANT)
        .rotate_motion(Anim::INSTANT)
        .opacity_motion(Anim::INSTANT)
        .blur_motion(Anim::INSTANT)
}

/// A word in flight: ten taps smeared along its velocity over the
/// shutter under its ember glow, set once at the sprite's size and scaled
/// and turned as one layer.
pub(crate) fn flying(p: Paint, key: &str, word: &str, weight: f32, pose: Pose) -> El {
    let font = TextStyle::new(pose.sprite, weight).lh(pose.sprite * 1.3);
    let (w, h) = rune::text::measure(word, &[], font, None);
    let k = pose.size / pose.sprite;
    // A shift along the velocity in the stage, in the turned, scaled sprite.
    let (sin, cos) = pose.angle.sin_cos();
    let local = |s: f32| {
        let (dx, dy) = (-pose.vx * s, -pose.vy * s);
        [(dx * cos + dy * sin) / k, (-dx * sin + dy * cos) / k]
    };
    let mut sprite = el()
        .key(key.to_string())
        .absolute(pose.x - w / 2.0, pose.y - h / 2.0)
        .size(w, h)
        .scale(k)
        .rotate(pose.angle)
        .opacity(pose.alpha);
    if pose.heat > 0.05 {
        // The WebGPU renderer's glow: the word blurred twice in its atlas,
        // at 0.15 and 0.34 of its size, tinted and added at 0.28 + 1.15 h.
        let h = pose.heat;
        let glow = ember(0.38 + h * 0.45).alpha((GLOW_INK * (0.28 + h * 1.15)).min(1.0));
        for (n, sigma) in [0.153, 0.34].into_iter().enumerate() {
            sprite = sprite.child(
                text(word.to_string())
                    .key(format!("glow{n}"))
                    .font(font)
                    .color(glow)
                    .absolute(0.0, 0.0)
                    .blur(sigma * pose.sprite),
            );
        }
    }
    // Over-composited, so ten at rest add up to nearly solid ink, as the
    // shader's average does.
    let tap = 0.25;
    for n in 0..TAPS {
        let [dx, dy] = local(((n as f32 + 0.5) / TAPS as f32 - 0.5) * SHUTTER);
        sprite = sprite.child(
            text(word.to_string())
                .key(format!("tap{n}"))
                .font(font)
                .color(ink(p, pose.heat))
                .absolute(dx, dy)
                .opacity(tap),
        );
    }
    sprite
}

/// The faint shimmer on landed word `index`'s glow at `now`.
pub(crate) fn flicker(now: f64, index: usize) -> f32 {
    0.85 + 0.15 * (now as f32 * 37.0 + index as f32 * 1.7).sin()
}

/// A typeset word in its slot, `at` its box's centre, glowing while hot.
/// `flick` is its glow's shimmer (see [`flicker`]).
pub(crate) fn typeset(
    p: Paint,
    key: &str,
    word: &str,
    font: TextStyle,
    at: [f32; 2],
    state: Landed,
    flick: f32,
) -> Vec<El> {
    let [x, y] = at;
    let mut out = Vec::with_capacity(2);
    if state.heat > 0.02 {
        let glow = landed_glow(p, state.heat);
        out.push(
            word_at(
                format!("{key}-glow"),
                word,
                font,
                glow.alpha(glow.a * flick),
                x,
                y,
                0.0,
            )
            .blur((1.5 + 9.0 * state.heat) / 2.0)
            .opacity(state.fade),
        );
    }
    out.push(
        word_at(key.to_string(), word, font, ink(p, state.heat), x, y, 0.0).opacity(state.fade),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight() -> Flight {
        Flight::new([0.0, 300.0], [200.0, 0.0], 1.0, 0.12, 13.0, 0)
    }

    #[test]
    fn a_word_leaves_small_and_lands_at_its_slot_size() {
        let f = flight();
        let first = f.pose(1.0, false).unwrap();
        assert!((first.size - f.size * 0.3).abs() < 1e-3);
        let pinned = f.pose(f.arrives() + 0.01, false).unwrap();
        assert_eq!((pinned.x, pinned.y, pinned.size), (200.0, 0.0, 13.0));
        assert!(f.pose(f.arrives() + HANDOFF + 0.01, false).is_none());
    }

    #[test]
    fn a_calm_word_overshoots_a_little_then_stops() {
        let f = flight();
        let mut past = false;
        let mut t = f.start();
        while t < f.arrives() {
            let p = f.pose(t, false).unwrap();
            past |= p.y < -0.5;
            t += 1.0 / 240.0;
        }
        assert!(past, "ease-out-back carries it past the slot");
    }

    #[test]
    fn landed_words_cool_from_hot() {
        let f = flight();
        let at = f.arrives();
        assert_eq!(f.landed(at - 0.01, false), None);
        assert_eq!(f.landed(at + HANDOFF, false).unwrap().heat, 1.0);
        assert_eq!(f.landed(at + 2.0, false).unwrap().heat, 0.0);
    }

    #[test]
    fn the_caret_glides_toward_each_landing() {
        let targets = [(1.0, 50.0), (1.5, 90.0)];
        assert_eq!(caret_x(10.0, &targets, 0.5), 10.0);
        let mid = caret_x(10.0, &targets, 1.02);
        assert!(mid > 10.0 && mid < 50.0);
        assert!((caret_x(10.0, &targets, 3.0) - 90.0).abs() < 0.01);
    }
}
