//! The Undo chip's countdown: the chip's own outline drains clockwise from
//! the top and is gone the moment the Undo window ends.
//!
//! The ring is one line along the outline, trimmed to the time left by a
//! render-time [`Tween`] (`live_trim`), so it drains every frame with no
//! producer ticking and no view rebuilds. The look's other layers (a bright
//! head, a glow, a fade into the track) are the same line trimmed to spans
//! that travel with its moving end. The last moment fades it out, so
//! nothing pops at the end. While the pointer holds the countdown every
//! layer stands still, and a new ring takes over from what was left once
//! it leaves. Under Reduce Motion it drains as the plain line alone.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use rune::color::{srgb_decode, srgb_encode};
use rune::{Color, El, LineCap, Shape, Tween, canvas, el};

use super::super::controls::Paint;
use super::flow::{Pres, undo_s};
use super::surface::{CHIP_RADIUS, HINT_GAP, UNDO_LABEL, medium};
use super::tuning::{self, RingLook};

const STROKE: f32 = 1.5;
/// How strongly the track shows the whole outline, under the line.
const TRACK: f32 = 0.09;
/// The line fades out over the window's last this long.
const FADE_S: f64 = 0.18;
/// Points along each rounded corner.
const CORNER_STEPS: usize = 16;

/// One layer of the ring: the outline from `from` to `to` past the moving
/// end (fractions of the way round, or of what is left when `along`;
/// `None` is the top centre, where the line ends), in `ink`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    from: f32,
    to: Option<f32>,
    along: bool,
    ink: Ink,
    width: f32,
    /// Gaussian blur, in points; a blurred layer shows through its own
    /// opacity, `amount`.
    blur: f32,
    cap: LineCap,
}

impl Layer {
    /// Where this layer runs with the moving end `at` of the way round.
    /// Linear in `at`, so one linear tween carries it to the end.
    fn span(&self, at: f32) -> [f32; 2] {
        let scale = if self.along { 1.0 - at } else { 1.0 };
        [
            at + self.from * scale,
            self.to.map_or(1.0, |to| at + to * scale),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ink {
    /// The line's own ink, `amount` of the way from the ground.
    Line(f32),
    /// The brightest text ink, `amount` of the way from the ground.
    Bright(f32),
}

/// The plain line: from the moving end to the top centre.
const LINE: Layer = Layer {
    from: 0.0,
    to: None,
    along: false,
    ink: Ink::Line(1.0),
    width: STROKE,
    blur: 0.0,
    cap: LineCap::Round,
};

/// The bands [`RingLook::Fade`] dims the line in, how far along what is
/// left they reach, and how faint the moving end gets.
const FADE_BANDS: usize = 24;
const FADE_REACH: f32 = 0.6;
const FADE_FLOOR: f32 = 0.22;

fn layers(look: RingLook, reduced: bool) -> Vec<Layer> {
    if reduced {
        return vec![LINE];
    }
    match look {
        RingLook::Comet => vec![
            LINE,
            Layer {
                to: Some(0.03),
                ink: Ink::Bright(0.9),
                ..LINE
            },
            Layer {
                from: -0.006,
                to: Some(0.012),
                ink: Ink::Bright(0.22),
                width: 2.5,
                blur: 1.4,
                ..LINE
            },
        ],
        // A glow the moving end leaves behind it, over the outline it just
        // gave up, brightest where it is.
        RingLook::Glow => [(0.03, 0.2), (0.08, 0.12), (0.16, 0.07), (0.28, 0.04)]
            .into_iter()
            .map(|(reach, amount)| Layer {
                from: -reach,
                to: Some(0.0),
                ink: Ink::Bright(amount),
                width: 3.0,
                blur: 1.8,
                ..LINE
            })
            .chain([
                LINE,
                Layer {
                    to: Some(0.015),
                    ink: Ink::Bright(0.9),
                    ..LINE
                },
            ])
            .collect(),
        RingLook::Fade => (0..FADE_BANDS)
            .map(|k| {
                let t = k as f32 / FADE_BANDS as f32;
                let eased = t * t * (3.0 - 2.0 * t);
                Layer {
                    from: t * FADE_REACH,
                    along: true,
                    ink: Ink::Line(FADE_FLOOR + (1.0 - FADE_FLOOR) * eased),
                    cap: if k == 0 { LineCap::Round } else { LineCap::Butt },
                    ..LINE
                }
            })
            .collect(),
    }
}

/// The live ring for one Undo window.
pub(crate) struct Ring {
    /// When the window ends, on the flow's clock.
    end: f64,
    /// Held by the pointer, with this long left.
    held: Option<f64>,
    layers: Vec<(Layer, Tween<[f32; 2]>)>,
    fade: Tween<f32>,
}

impl Ring {
    fn new(end: f64, held: Option<f64>, now: f64, reduced: bool) -> Self {
        let window = undo_s();
        let left = held.unwrap_or(end - now).clamp(0.0, window);
        // How far round the moving end is now, and when it's back at the top.
        let past = (1.0 - left / window) as f32;
        let track = |from, to, over: f64| match held {
            Some(_) => Tween::linear(from, from, Duration::ZERO),
            None => Tween::linear(from, to, Duration::from_secs_f64(over)),
        };
        let layers = layers(tuning::get().ring, reduced)
            .into_iter()
            .map(|layer| (layer, track(layer.span(past), layer.span(1.0), left)))
            .collect();
        // Opacity holds at 1 (clamped) until the last FADE_S, then fades.
        let full = (left / FADE_S) as f32;
        let fade = match held {
            Some(_) => Tween::linear(full, full, Duration::ZERO),
            None => Tween::linear(full, 0.0, Duration::from_secs_f64(left)),
        };
        Self {
            end,
            held,
            layers,
            fade,
        }
    }
}

/// Keeps `ring` on the window `pres` counts down: built when one begins,
/// kept while it runs, dropped when Undo has none.
pub(crate) fn sync(ring: &mut Option<Ring>, pres: &Pres, now: f64, reduced: bool) {
    let (end, held) = match pres {
        Pres::Done(done) if done.undo => (done.countdown, done.held),
        _ => (None, None),
    };
    match end {
        None => *ring = None,
        Some(end)
            if ring
                .as_ref()
                .is_some_and(|ring| (ring.end, ring.held) == (end, held)) => {}
        Some(end) => *ring = Some(Ring::new(end, held, now, reduced)),
    }
}

/// The Undo chip `size` big on `ground` (what shows behind it), with its
/// countdown when there is one.
pub(crate) fn chip(p: Paint, base: El, size: [f32; 2], ground: Color, ring: Option<&Ring>) -> El {
    let k = p.k;
    let ink = p.muted();
    let mut chip = base
        .radius(CHIP_RADIUS)
        .gap(HINT_GAP)
        .hover(k.c.hover)
        .press(k.c.pressed);
    if let Some(ring) = ring {
        chip = chip.child(border(p, size, ground, ring));
    }
    chip.child(super::label(UNDO_LABEL, medium(), ink))
        .role("button")
        .label(UNDO_LABEL)
        .hit()
}

/// The faint track and the ring's layers along the chip's own outline.
/// Every stroke is opaque, mixed over `ground`, so a line never shows its
/// own joints; only blurred layers show through an opacity.
fn border(p: Paint, [w, h]: [f32; 2], ground: Color, ring: &Ring) -> El {
    let path = outline(w, h, STROKE * 0.5);
    let layer = |shape: Shape| canvas(shape.view_box(w, h)).absolute(0.0, 0.0).size(w, h);
    let track = Shape::new().stroke_with(
        path.clone(),
        false,
        STROKE,
        LineCap::Round,
        tint(ground, p.text(), TRACK),
    );
    // A glow on a light ground is a shadow; it stays fainter there.
    let glow = if ground.luminance() > 0.5 { 0.5 } else { 1.0 };
    let mut lines = el().absolute(0.0, 0.0).size(w, h).live_opacity(&ring.fade);
    for (spec, span) in &ring.layers {
        let (ink, amount) = match spec.ink {
            Ink::Line(a) => (p.muted(), a),
            Ink::Bright(a) => (p.text(), a),
        };
        let color = if spec.blur > 0.0 {
            ink
        } else {
            tint(ground, ink, amount)
        };
        let mut line = layer(Shape::new().stroke_with(path.clone(), false, spec.width, spec.cap, color))
            .live_trim(span);
        if spec.blur > 0.0 {
            line = line.blur(spec.blur).opacity(amount * glow);
        }
        lines = lines.child(line);
    }
    el().absolute(0.0, 0.0)
        .size(w, h)
        .child(layer(track))
        .child(lines)
}

/// `ink` `amount` of the way over `ground`, as an alpha would show it on
/// screen (in sRGB, not linear light), opaque.
fn tint(ground: Color, ink: Color, amount: f32) -> Color {
    let m = |a: f32, b: f32| srgb_decode(srgb_encode(a) + (srgb_encode(b) - srgb_encode(a)) * amount);
    Color {
        r: m(ground.r, ink.r),
        g: m(ground.g, ink.g),
        b: m(ground.b, ink.b),
        a: 1.0,
    }
}

/// A `w` × `h` rounded rectangle's outline `inset` in from its edge, with
/// the chip's corners, from the top centre clockwise and back.
fn outline(w: f32, h: f32, inset: f32) -> Vec<[f32; 2]> {
    let (x0, y0, x1, y1) = (inset, inset, w - inset, h - inset);
    let r = (CHIP_RADIUS - inset).min((y1 - y0) * 0.5).min((x1 - x0) * 0.5).max(0.01);
    let mut points = vec![[w * 0.5, y0]];
    let corners = [
        (x1 - r, y0 + r, -FRAC_PI_2),
        (x1 - r, y1 - r, 0.0),
        (x0 + r, y1 - r, FRAC_PI_2),
        (x0 + r, y0 + r, PI),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=CORNER_STEPS {
            let a = start + FRAC_PI_2 * i as f32 / CORNER_STEPS as f32;
            let point = [cx + r * a.cos(), cy + r * a.sin()];
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
    }
    points.push([w * 0.5, y0]);
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(points: &[[f32; 2]]) -> f32 {
        points
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum()
    }

    #[test]
    fn the_outline_starts_at_the_top_centre_and_goes_clockwise() {
        let (w, h) = (60.0, 28.0);
        let path = outline(w, h, 0.0);
        assert_eq!(path.first(), Some(&[30.0, 0.0]));
        assert_eq!(path.last(), Some(&[30.0, 0.0]));
        // A quarter of the way round is on the right-hand side, halfway the
        // bottom centre.
        let total = length(&path);
        let at = |f: f32| {
            let mut left = f * total;
            for w in path.windows(2) {
                let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                if d >= left {
                    let k = left / d;
                    return [w[0][0] + (w[1][0] - w[0][0]) * k, w[0][1] + (w[1][1] - w[0][1]) * k];
                }
                left -= d;
            }
            path[path.len() - 1]
        };
        assert!(at(0.25)[0] > 50.0);
        let bottom = at(0.5);
        assert!((bottom[0] - 30.0).abs() < 0.05 && (bottom[1] - 28.0).abs() < 0.05);
        // The corners follow the chip's radius, within a hair.
        let ideal = 2.0 * (w - 2.0 * CHIP_RADIUS) + 2.0 * (h - 2.0 * CHIP_RADIUS) + 2.0 * PI * CHIP_RADIUS;
        assert!((total - ideal).abs() < 0.05, "{total} vs {ideal}");
    }

    #[test]
    fn a_ring_follows_its_window_and_rebuilds_only_for_a_new_one() {
        use super::super::flow::{Done, Lead};
        let counting = |end: Option<f64>| {
            Pres::Done(Done {
                undo: true,
                countdown: end,
                ..Done::plain(Lead::Check, "Sent to HOME")
            })
        };
        let mut ring = None;
        sync(&mut ring, &counting(Some(5.0)), 0.0, false);
        assert!(ring.as_ref().is_some_and(|r| r.layers.iter().any(|(l, _)| *l == LINE)));
        // The same window a second on keeps the ring it had.
        sync(&mut ring, &counting(Some(5.0)), 1.0, false);
        assert_eq!(ring.as_ref().map(|r| r.end), Some(5.0));
        sync(&mut ring, &counting(Some(9.0)), 4.0, false);
        assert_eq!(ring.as_ref().map(|r| r.end), Some(9.0));
        sync(&mut ring, &counting(None), 6.0, false);
        assert!(ring.is_none());
    }

    #[test]
    fn every_look_keeps_one_line_to_the_top_and_reduce_motion_keeps_only_it() {
        for look in RingLook::ALL {
            let all = layers(look, false);
            assert!(all.iter().any(|l| l.from == 0.0 && l.to.is_none()), "{look:?}");
            assert_eq!(layers(look, true), vec![LINE]);
        }
    }

    #[test]
    fn a_held_ring_stands_still_where_it_was() {
        use super::super::flow::{Done, Lead};
        let held = Pres::Done(Done {
            undo: true,
            countdown: Some(5.0),
            held: Some(3.5),
            ..Done::plain(Lead::Check, "Sending to SEER")
        });
        let mut ring = None;
        sync(&mut ring, &held, 1.5, false);
        assert_eq!(ring.as_ref().and_then(|r| r.held), Some(3.5));
        // Let go, it drains again from a new ring on the moved window.
        let going = Pres::Done(Done {
            undo: true,
            countdown: Some(7.5),
            ..Done::plain(Lead::Check, "Sending to SEER")
        });
        sync(&mut ring, &going, 4.0, false);
        let ring = ring.expect("a ring again");
        assert_eq!((ring.end, ring.held), (7.5, None));
    }
}
