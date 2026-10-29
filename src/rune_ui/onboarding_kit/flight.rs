//! The site hero's word flight (`site/src/shout/main.js`), adapted to OpenCode
//! theme tokens (`rune_theme_opencode::ember`) with crisp, restrained landing
//! sheen so typeset words read cleanly without muddy glow.

use rune::{Anim, Color, El, Spring, TextStyle, el, text};

use super::super::controls::Paint;

/// How long the flying word and its typeset self cross-fade.
pub(crate) const HANDOFF: f64 = 0.11;
/// Shutter smear across 10 velocity samples.
const SHUTTER: f32 = 0.72 / 60.0;
const TAPS: usize = 10;
const GLOW_INK: f32 = 0.42;
const CARET_RATE: f64 = 45.0;
const NUDGE: f32 = 14.0;
const NUDGE_HZ: f32 = 7.0;
const NUDGE_ZETA: f32 = 0.55;
const HIT_HZ: f32 = 8.5;
const HIT_ZETA: f32 = 0.52;
const LAUNCH_SPEED: f32 = 920.0;
const FRAME: f64 = 1.0 / 60.0;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 78.233 + 1.7).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

/// One word's flight from `from` to its slot centre `to`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flight {
    from: [f32; 2],
    to: [f32; 2],
    start: f64,
    i: f32,
    size: f32,
    land: f32,
    bend: f32,
    dur: f64,
    back: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) vx: f32,
    pub(crate) vy: f32,
    pub(crate) size: f32,
    pub(crate) sprite: f32,
    pub(crate) angle: f32,
    pub(crate) heat: f32,
    pub(crate) alpha: f32,
}

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
        let side = if index.is_multiple_of(2) { -1.0 } else { 1.0 };
        let from = [from[0], from[1] + 2.0];
        let dist = (to[0] - from[0]).hypot(to[1] - from[1]);
        Self {
            from,
            to,
            start,
            i,
            size: lerp(16.0, 52.0, i.powf(1.2)),
            land,
            bend: side * lerp(0.10, 0.24, i) * rnd(index, 0.55, 1.0),
            dur: f64::from((dist / lerp(760.0, 2200.0, i)).clamp(0.18, 0.85) * 1.2),
            back: (22.0 * lerp(1.6, 3.2, i) / (4.0 * dist.max(60.0))).cbrt(),
        }
    }

    pub(crate) fn start(&self) -> f64 {
        self.start
    }

    pub(crate) fn arrives(&self) -> f64 {
        self.start + self.dur
    }

    pub(crate) fn intensity(&self) -> f32 {
        self.i
    }

    pub(crate) fn to(&self) -> [f32; 2] {
        self.to
    }

    /// Unit direction of arrival at the slot.
    pub(crate) fn heading(&self) -> [f32; 2] {
        let [x0, y0] = self.point(self.dur - FRAME);
        let [x1, y1] = self.to;
        let (dx, dy) = (x1 - x0, y1 - y0);
        let d = dx.hypot(dy).max(1e-3);
        [dx / d, dy / d]
    }

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

    pub(crate) fn pose(&self, now: f64, reduced: bool) -> Option<Pose> {
        let age = now - self.start;
        if reduced || age < 0.0 || age > self.dur + HANDOFF {
            return None;
        }
        let slot = self.land / self.size;
        if age >= self.dur {
            let k = ((age - self.dur) / HANDOFF).clamp(0.0, 1.0) as f32;
            return Some(Pose {
                x: self.to[0],
                y: self.to[1],
                vx: 0.0,
                vy: 0.0,
                size: self.land,
                sprite: self.size,
                angle: 0.0,
                heat: self.heat0().max(0.75),
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
            angle: lerp(ang * 0.22, 0.0, e),
            heat: lerp(self.heat0(), self.heat0().max(0.75), e),
            alpha: 1.0,
        })
    }

    fn heat0(&self) -> f32 {
        lerp(0.08, 0.9, self.i)
    }

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
        let cool = lerp(0.24, 0.42, self.i);
        let h = if fade >= 1.0 {
            (1.0 - ((since - HANDOFF).max(0.0) as f32) / cool).max(0.0)
        } else {
            1.0
        };
        Some(Landed { fade, heat: h * h })
    }

    pub(crate) fn nudge(&self, now: f64) -> [f32; 2] {
        self.spring(now, NUDGE, NUDGE_HZ, NUDGE_ZETA)
    }

    pub(crate) fn hit(&self, now: f64) -> [f32; 2] {
        let push = if self.i < 0.36 {
            lerp(16.0, 34.0, self.i / 0.36)
        } else {
            lerp(48.0, 130.0, self.i)
        };
        self.spring(now, push, HIT_HZ, HIT_ZETA)
    }

    pub(crate) fn flash(&self, now: f64) -> f32 {
        let t = (now - self.arrives()) as f32;
        if t < 0.0 {
            return 0.0;
        }
        let start = if self.i < 0.36 {
            0.12
        } else {
            lerp(0.22, 0.55, self.i)
        };
        (start - t * 7.5).max(0.0)
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

/// Brand ember ramp derived strictly from OpenCode theme tokens (`ember` scale 5..12).
pub(crate) fn ember_tone(p: Paint, h: f32) -> Color {
    let dark = p.k.dark;
    let stops = [
        rune_theme_opencode::ember(dark, 6),
        rune_theme_opencode::ember(dark, 9),
        rune_theme_opencode::ember(dark, 11),
        if dark {
            p.k.c.text_strong
        } else {
            rune_theme_opencode::ember(false, 10)
        },
    ];
    let scaled = h.clamp(0.0, 1.0) * 3.0;
    let idx = (scaled.floor() as usize).min(2);
    let frac = scaled - idx as f32;
    stops[idx].mix(stops[idx + 1], frac)
}

pub(crate) fn ink(p: Paint, heat: f32) -> Color {
    if p.k.dark {
        let warm_peak =
            p.k.c
                .text_strong
                .mix(rune_theme_opencode::ember(true, 11), 0.28);
        p.k.c.text_base.mix(warm_peak, heat)
    } else {
        p.k.c
            .text_base
            .mix(rune_theme_opencode::ember(false, 9), heat * 0.65)
    }
}

fn landed_glow(p: Paint, heat: f32) -> Color {
    let base = rune_theme_opencode::ember(p.k.dark, 9);
    let strength = if p.k.dark { 0.34 } else { 0.20 };
    base.alpha(strength * heat)
}

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

pub(crate) fn flying(p: Paint, key: &str, word: &str, weight: f32, pose: Pose) -> El {
    let font = TextStyle::new(pose.sprite, weight).lh(pose.sprite * 1.3);
    let (w, h) = rune::text::measure(word, &[], font, None);
    let k = pose.size / pose.sprite;
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
    if pose.heat > 0.08 {
        let h_val = pose.heat;
        let glow =
            ember_tone(p, 0.35 + h_val * 0.4).alpha((GLOW_INK * (0.22 + h_val * 0.7)).min(0.65));
        sprite = sprite.child(
            text(word.to_string())
                .key("glow0")
                .font(font)
                .color(glow)
                .absolute(0.0, 0.0)
                .blur(0.14 * pose.sprite),
        );
    }
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

pub(crate) fn flicker(now: f64, index: usize) -> f32 {
    0.88 + 0.12 * (now as f32 * 32.0 + index as f32 * 1.7).sin()
}

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
    if state.heat > 0.05 {
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
            .blur(1.0 + 2.6 * state.heat)
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
            past |= p.y < -0.3;
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
