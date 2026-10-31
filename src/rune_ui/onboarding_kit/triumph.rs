//! `"With Hex"`'s payoff, timed from the scene's cheers ([`Cheer`]): your
//! spoken line goes out, tapbacks rain on it, replies cascade in, status
//! pills and renames pile on, and confetti and fireworks go off over the
//! stage. Every part is optional per scene.

use rune::{Color, El, LineCap, Shape, canvas};

use super::super::controls::Paint;
use super::scene_script::{Cheer, Effect, Script, Speaker};
use super::typing::{Glyph, Timing, renamed};

/// The least time the last cheer holds before the score.
const HOLD: f64 = 1.75;
/// A first reply that types first shows its dots from this long after the
/// send.
const DOTS_FROM: f64 = 0.4;
/// How long a changed pill flares.
const PILL_FLARE: f64 = 0.6;
/// Effects called together go off this far apart.
const EFFECT_STEP: f64 = 0.3;
/// An untimed tapback lands this long after the last, the first this long
/// after its command.
const TAP_STEP: f64 = 0.2;
const TAP_FIRST: f64 = 0.3;

/// One line of the payoff.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Said {
    Them(u8, &'static str),
    You(&'static str),
    /// A celebratory system pill.
    Pill(String),
}

impl Said {
    pub(crate) fn text(&self) -> &str {
        match self {
            Said::Them(_, text) | Said::You(text) => text,
            Said::Pill(text) => text,
        }
    }
}

/// `"With Hex"`'s beats, in seconds after the shortcut goes down.
#[derive(Clone, Debug)]
pub(crate) struct HexBeats {
    /// Your line leaving the field.
    pub(crate) send: f64,
    /// The boss typing before a first reply.
    pub(crate) dots: Option<(f64, f64)>,
    /// Tapbacks on your line.
    pub(crate) taps: Vec<(f64, Glyph)>,
    pub(crate) lines: Vec<(f64, Said)>,
    pub(crate) renames: Vec<(f64, &'static str)>,
    /// The pill under the header changing, or going.
    pub(crate) pills: Vec<(f64, Option<&'static str>)>,
    pub(crate) effects: Vec<(f64, Effect)>,
    /// Everything has landed and been read: the score.
    pub(crate) done: f64,
}

impl HexBeats {
    /// The scene's cheers laid out after your line goes out at `send`.
    pub(crate) fn plan(script: &'static Script<'static>, send: f64, t: &Timing) -> Self {
        let mut b = HexBeats {
            send,
            dots: None,
            taps: Vec::new(),
            lines: Vec::new(),
            renames: Vec::new(),
            pills: Vec::new(),
            effects: Vec::new(),
            done: send,
        };
        let mut at = send;
        let mut replied = false;
        let mut fx = f64::NEG_INFINITY;
        for cheer in &script.with.cheers {
            match *cheer {
                Cheer::Tap(ref taps) => {
                    let mut last = at + TAP_FIRST - TAP_STEP;
                    for &(glyph, after) in taps {
                        last = after.map_or(last + TAP_STEP, |after| at + after);
                        b.taps.push((last, glyph));
                    }
                }
                Cheer::Says { who, text, gap } => {
                    let gap = gap.unwrap_or(if replied { t.cheer } else { t.hex_reply });
                    if !replied && gap > DOTS_FROM + 0.3 && who != Speaker::You {
                        b.dots = Some((at + DOTS_FROM, at + gap));
                    }
                    replied = true;
                    at += gap;
                    b.lines.push((
                        at,
                        match who {
                            Speaker::Them(i) => Said::Them(i, text),
                            Speaker::You => Said::You(text),
                        },
                    ));
                }
                Cheer::Status(copy) => {
                    at += t.cheer;
                    b.lines.push((at, Said::Pill(copy.to_string())));
                }
                Cheer::Rename(name) => {
                    at += t.cheer;
                    b.renames.push((at, name));
                    b.lines.push((at, Said::Pill(renamed(script.boss(), name))));
                }
                Cheer::Pill(copy) => b.pills.push((at, copy)),
                Cheer::Effect(effect) => {
                    fx = at.max(fx + EFFECT_STEP);
                    b.effects.push((fx, effect));
                }
                Cheer::Wait(secs) => at += secs,
            }
        }
        let last = b.taps.iter().map(|&(at, _)| at).fold(at, f64::max);
        let read = b.lines.last().map_or(0.0, |(_, said)| t.read(said.text()));
        b.done = last + read.max(HOLD);
        b
    }

    /// The pill at `t`, from `pill`, and how hard it flares from changing.
    pub(crate) fn pill(&self, t: f64, pill: Option<&'static str>) -> (Option<&'static str>, f32) {
        match self.pills.iter().take_while(|&&(at, _)| at <= t).last() {
            Some(&(at, copy)) => {
                let since = ((t - at) / PILL_FLARE) as f32;
                (copy, (1.0 - since).clamp(0.0, 1.0).powi(2))
            }
            None => (pill, 0.0),
        }
    }

    /// The thread's name at `t`, from `title`.
    pub(crate) fn title(&self, t: f64, title: &'static str) -> &'static str {
        self.renames
            .iter()
            .take_while(|&&(at, _)| at <= t)
            .last()
            .map_or(title, |&(_, name)| name)
    }
}

/// The longest the payoff runs from the shortcut, and how many tapbacks
/// rain on your line.
#[cfg(test)]
pub(crate) const MAX_SECS: f64 = 8.0;
#[cfg(test)]
pub(crate) const MAX_TAPS: usize = 4;

/// What a scene's payoff breaks, sent `send` seconds after the shortcut.
#[cfg(test)]
pub(crate) fn check(script: &'static Script<'static>, send: f64, t: &Timing) -> Vec<String> {
    let beats = HexBeats::plan(script, send, t);
    let mut problems = Vec::new();
    if beats.done > MAX_SECS {
        problems.push(format!(
            "\"With Hex\" runs {:.1} s from the shortcut; keep it under {MAX_SECS} s",
            beats.done
        ));
    }
    if beats.taps.len() > MAX_TAPS {
        problems.push(format!(
            "{} tapbacks on your spoken line; it holds {MAX_TAPS}",
            beats.taps.len()
        ));
    }
    problems
}

/// How long confetti and fireworks run.
pub(crate) const CONFETTI_LIFE: f32 = 2.4;
const FIREWORKS_LIFE: f32 = 2.0;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 12.9898 + 4.1).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

/// The colours a celebration throws: OpenCode theme scales.
const HUES: [&str; 7] = ["cobalt", "amber", "mint", "coral", "lilac", "yuzu", "cyan"];

fn hue(p: Paint, i: usize) -> Color {
    rune_theme_opencode::scale(HUES[i % HUES.len()], p.k.dark, 9)
}

/// Where something thrown at `v` from `from` is after `t`, with `drag`
/// slowing it and `gravity` pulling it down.
fn thrown(from: [f32; 2], v: [f32; 2], t: f32, drag: f32, gravity: f32) -> [f32; 2] {
    let decay = (-drag * t).exp();
    let reach = (1.0 - decay) / drag;
    let terminal = gravity / drag;
    [
        from[0] + v[0] * reach,
        from[1] + terminal * t + (v[1] - terminal) * reach,
    ]
}

const BITS: usize = 110;

/// Confetti from two cannons low on either side, fluttering down, `t`
/// seconds after it fires; under Reduce Motion a still scatter that fades.
fn confetti(mut shape: Shape, p: Paint, size: [f32; 2], t: f32, reduced: bool) -> Shape {
    if !(0.0..CONFETTI_LIFE).contains(&t) {
        return shape;
    }
    let fade = if reduced {
        (t / 0.25).min(1.0) * (1.0 - ((t - 1.4) / 0.6).clamp(0.0, 1.0))
    } else {
        1.0 - ((t - (CONFETTI_LIFE - 0.6)) / 0.6).clamp(0.0, 1.0)
    };
    let pose = if reduced { 0.9 } else { t };
    for i in 0..BITS {
        let seed = i * 11 + 3;
        let left = i % 2 == 0;
        let from = [if left { 24.0 } else { size[0] - 24.0 }, size[1] * 0.62];
        let angle = rnd(seed, 0.95, 1.45);
        let speed = rnd(seed + 1, 420.0, 820.0);
        let v = [
            angle.cos() * speed * if left { 1.0 } else { -1.0 },
            -angle.sin() * speed,
        ];
        let [mut x, y] = thrown(from, v, pose, 2.4, 560.0);
        let flutter = rnd(seed + 2, 0.0, std::f32::consts::TAU);
        x += 9.0 * (pose * rnd(seed + 3, 4.0, 7.0) + flutter).sin() * pose.min(1.0);
        let spin = rnd(seed + 4, -9.0, 9.0) * pose + flutter;
        let flip = (pose * rnd(seed + 5, 6.0, 12.0) + flutter).cos();
        let (w, h) = (rnd(seed + 6, 3.0, 5.0), rnd(seed + 7, 6.0, 9.0));
        let (c, s) = (spin.cos(), spin.sin());
        let corner = |dx: f32, dy: f32| {
            let dx = dx * w * 0.5 * flip;
            let dy = dy * h * 0.5;
            [x + dx * c - dy * s, y + dx * s + dy * c]
        };
        let color = hue(p, i).alpha(fade);
        shape = shape.fill(
            vec![
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ],
            color,
        );
    }
    shape
}

/// Three shells rising from below the stage and bursting over the card, `t`
/// seconds after the first goes up. Reduce Motion shows none.
fn fireworks(mut shape: Shape, p: Paint, size: [f32; 2], t: f32) -> Shape {
    const RISE: f32 = 0.42;
    const SPARKS: usize = 32;
    let shells = [
        (0.0, [0.26, 0.24], 1),
        (0.3, [0.72, 0.16], 3),
        (0.58, [0.5, 0.34], 5),
    ];
    for (n, &(delay, [bx, by], tone)) in shells.iter().enumerate() {
        let t = t - delay;
        if t < 0.0 {
            continue;
        }
        let burst = [bx * size[0], by * size[1]];
        let color = hue(p, tone);
        if t < RISE {
            let u = t / RISE;
            let eased = 1.0 - (1.0 - u) * (1.0 - u);
            let head = [burst[0], lerp(size[1], burst[1], eased)];
            let tail = [burst[0], (head[1] + 34.0 * (1.0 - u * 0.6)).min(size[1])];
            shape = shape.line(tail, head, 2.0, LineCap::Round, color.alpha(0.85));
            continue;
        }
        let t = t - RISE;
        if t < 0.18 {
            let u = t / 0.18;
            let r = 6.0 + 22.0 * u;
            shape = shape.stroke_with(
                (0..24)
                    .map(|k| {
                        let a = k as f32 / 24.0 * std::f32::consts::TAU;
                        [burst[0] + r * a.cos(), burst[1] + r * a.sin()]
                    })
                    .collect::<Vec<_>>(),
                true,
                2.0 * (1.0 - u),
                LineCap::Round,
                p.k.c.white.alpha(0.7 * (1.0 - u)),
            );
        }
        for k in 0..SPARKS {
            let seed = n * 97 + k * 7;
            let life = rnd(seed, 0.95, 1.3);
            if t >= life {
                continue;
            }
            let a = k as f32 / SPARKS as f32 * std::f32::consts::TAU + rnd(seed + 1, -0.08, 0.08);
            let speed = rnd(seed + 2, 240.0, 330.0);
            let v = [a.cos() * speed, a.sin() * speed];
            let at = thrown(burst, v, t, 3.2, 170.0);
            let behind = thrown(burst, v, (t - 0.07).max(0.0), 3.2, 170.0);
            let left = 1.0 - t / life;
            let spark = if k % 3 == 0 { hue(p, tone + 2) } else { color };
            shape = shape.line(
                behind,
                at,
                1.8 * left + 0.4,
                LineCap::Round,
                spark.alpha(left.sqrt()),
            );
        }
    }
    shape
}

/// Every effect `beats` sets off, `t` seconds after the shortcut, over a
/// stage of `size`; `None` once they're all done.
pub(crate) fn effects(
    p: Paint,
    beats: &HexBeats,
    t: f64,
    size: [f32; 2],
    reduced: bool,
) -> Option<El> {
    let mut shape = Shape::new();
    let mut any = false;
    for &(at, effect) in &beats.effects {
        let since = (t - at) as f32;
        match effect {
            Effect::Confetti if (0.0..CONFETTI_LIFE).contains(&since) => {
                any = true;
                shape = confetti(shape, p, size, since, reduced);
            }
            Effect::Fireworks if !reduced && (0.0..FIREWORKS_LIFE).contains(&since) => {
                any = true;
                shape = fireworks(shape, p, size, since);
            }
            _ => {}
        }
    }
    any.then(|| {
        canvas(shape)
            .key("celebration")
            .absolute(0.0, 0.0)
            .size(size[0], size[1])
    })
}

#[cfg(test)]
mod tests {
    use super::super::scene_script::{SCENES, bundled};
    use super::*;

    fn leaked(name: &str) -> &'static Script<'static> {
        Box::leak(Box::new(bundled(name)))
    }

    #[test]
    fn every_payoff_fits_in_eight_seconds() {
        for (name, _) in SCENES {
            let beats = HexBeats::plan(leaked(name), 2.2, &Timing::default());
            assert!(beats.done <= 8.0, "{name}: {}", beats.done);
            assert!(beats.lines.windows(2).all(|w| w[0].0 <= w[1].0));
        }
    }

    #[test]
    fn the_cheers_cascade_and_the_effects_follow() {
        let t = Timing::default();
        let beats = HexBeats::plan(leaked("A-incontinence.scene"), 2.0, &t);
        let replies: Vec<f64> = beats
            .lines
            .iter()
            .filter(|(_, said)| matches!(said, Said::Them(..)))
            .map(|&(at, _)| at)
            .collect();
        assert!(replies.len() >= 3);
        assert!((replies[1] - replies[0] - t.cheer).abs() < 1e-9);
        assert!(beats.dots.is_some_and(|(_, until)| until == replies[0]));
        assert!(beats.taps.iter().all(|&(at, _)| at > beats.send));
        assert_eq!(beats.title(beats.done, "Launch Room"), "Bow Before You");
        let [(confetti, Effect::Confetti), (fireworks, Effect::Fireworks)] = beats.effects[..]
        else {
            panic!("{:?}", beats.effects);
        };
        assert!(fireworks > confetti);
    }

    #[test]
    fn confetti_slows_and_falls() {
        let a = thrown([0.0, 0.0], [100.0, -100.0], 0.5, 2.4, 560.0);
        let b = thrown([0.0, 0.0], [100.0, -100.0], 1.5, 2.4, 560.0);
        assert!(b[1] > a[1], "it falls");
        assert!(b[0] > a[0]);
    }
}
