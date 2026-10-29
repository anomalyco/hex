//! The Hex mark: a thin single-hexagon outline matching Round 3 of the icon
//! (`t573`, Hairline), built from one swappable [`HexGlyphSpec`] so trying
//! Soft nut or Flat top later changes a single constant.
//!
//! As a loader ([`hex_loader`]), the single outline traces clockwise from its
//! top apex while a delicate braided calibration wave tunes inside it, with no
//! heavy glow. At small sizes ([`hex_meter`]), it draws the clean perimeter
//! sweep alone so 16–20 pt marks stay razor-sharp.

use rune::{El, LineCap, Shape, canvas};

use super::super::controls::Paint;

type Point = [f32; 2];

/// One candidate Hex ring in canonical `[-1, 1]` coordinates, starting at the
/// top where progress begins and running clockwise.
#[derive(Clone, Copy)]
pub(crate) struct HexGlyphSpec {
    pub(crate) vertices: [Point; 6],
    /// Stroke width at 16 pt, and its ceiling at display sizes.
    pub(crate) stroke_16: f32,
    pub(crate) stroke_max: f32,
    /// Corner rounding in canonical units (`0.0` keeps crisp hairline corners).
    pub(crate) corner_radius: f32,
}

/// Pointy-top hairline hexagon (`t573` option 1 · Hairline): `(8,0)-(15,4)-(15,12)-(8,16)-(1,12)-(1,4)`.
pub(crate) const HAIRLINE: HexGlyphSpec = HexGlyphSpec {
    vertices: [
        [0.0, -0.88],
        [0.7621, -0.44],
        [0.7621, 0.44],
        [0.0, 0.88],
        [-0.7621, 0.44],
        [-0.7621, -0.44],
    ],
    stroke_16: 1.2,
    stroke_max: 2.2,
    corner_radius: 0.0,
};

/// Pointy-top rounded ring (`t573` option 2 · Soft nut), kept beside
/// [`HAIRLINE`] so swapping the onboarding mark is a one-line change.
#[allow(dead_code)]
pub(crate) const SOFT_NUT: HexGlyphSpec = HexGlyphSpec {
    vertices: HAIRLINE.vertices,
    stroke_16: 1.5,
    stroke_max: 2.8,
    corner_radius: 0.14,
};

/// Flat-top hexagon (`t573` option 3 · Flat top).
#[allow(dead_code)]
pub(crate) const FLAT_TOP: HexGlyphSpec = HexGlyphSpec {
    vertices: [
        [-0.44, -0.7621],
        [0.44, -0.7621],
        [0.88, 0.0],
        [0.44, 0.7621],
        [-0.44, 0.7621],
        [-0.88, 0.0],
    ],
    stroke_16: 1.35,
    stroke_max: 2.4,
    corner_radius: 0.06,
};

/// The active onboarding mark across every screen, pill, and icon.
pub(crate) const ACTIVE_GLYPH: HexGlyphSpec = HAIRLINE;

impl HexGlyphSpec {
    pub(crate) fn stroke_width(self, size: f32) -> f32 {
        let t = ((size - 16.0) / 108.0).clamp(0.0, 1.0);
        self.stroke_16 + (self.stroke_max - self.stroke_16) * t
    }

    /// Closed perimeter points in `[0..size, 0..size]`, scaled by `scale`
    /// around the centre (the last point repeats the first for partial sweeps).
    pub(crate) fn poly(self, size: f32, scale: f32) -> Vec<Point> {
        let half = size * 0.5;
        let map = |[x, y]: Point| [half + x * half * scale, half + y * half * scale];
        let v = self.vertices;
        if self.corner_radius <= 0.001 {
            let mut pts: Vec<Point> = v.into_iter().map(map).collect();
            pts.push(pts[0]);
            return pts;
        }
        let r = self.corner_radius.min(0.35);
        let mut pts = Vec::with_capacity(32);
        for i in 0..6 {
            let prev = v[(i + 5) % 6];
            let curr = v[i];
            let next = v[(i + 1) % 6];
            let to_prev = [prev[0] - curr[0], prev[1] - curr[1]];
            let to_next = [next[0] - curr[0], next[1] - curr[1]];
            let lp = to_prev[0].hypot(to_prev[1]).max(1e-4);
            let ln = to_next[0].hypot(to_next[1]).max(1e-4);
            let a = [curr[0] + to_prev[0] / lp * r, curr[1] + to_prev[1] / lp * r];
            let b = [curr[0] + to_next[0] / ln * r, curr[1] + to_next[1] / ln * r];
            for step in 0..=4 {
                let u = step as f32 / 4.0;
                let inv = 1.0 - u;
                let p = [
                    inv * inv * a[0] + 2.0 * inv * u * curr[0] + u * u * b[0],
                    inv * inv * a[1] + 2.0 * inv * u * curr[1] + u * u * b[1],
                ];
                pts.push(map(p));
            }
        }
        pts.push(pts[0]);
        pts
    }
}

/// Prefix of a closed polyline `poly` covering `fraction` (`0..=1`) of its
/// arc length, plus its tip point.
fn sweep_poly(poly: &[Point], fraction: f32) -> (Vec<Point>, Option<Point>) {
    let f = fraction.clamp(0.0, 1.0);
    if f <= 0.0 || poly.len() < 2 {
        return (Vec::new(), None);
    }
    if f >= 1.0 {
        let mut closed = poly.to_vec();
        closed.pop();
        return (closed, None);
    }
    let segs: Vec<f32> = poly
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .collect();
    let total: f32 = segs.iter().sum();
    let target = total * f;
    let mut acc = 0.0;
    let mut out = vec![poly[0]];
    for (i, &len) in segs.iter().enumerate() {
        if acc + len >= target {
            let u = if len > 1e-5 {
                ((target - acc) / len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let a = poly[i];
            let b = poly[i + 1];
            let tip = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
            out.push(tip);
            return (out, Some(tip));
        }
        acc += len;
        out.push(poly[i + 1]);
    }
    let last = *out.last().unwrap_or(&poly[0]);
    (out, Some(last))
}

/// Regular polygon approximation of a small crisp disc, drawn as a filled
/// polygon so translucent strokes never double-blend.
pub(crate) fn disc_points(centre: Point, radius: f32) -> Vec<Point> {
    (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
        })
        .collect()
}

/// Interior acoustic wave (`t573` `glyphs.py` `wave` / `braid`), mapped to
/// `[0..size, 0..size]`.
pub(crate) fn wave_pts(size: f32, amp: f32, phase: f32, sign: f32, taper: bool) -> Vec<Point> {
    const STEPS: usize = 44;
    let x0 = size * 0.28;
    let x1 = size * 0.72;
    let mid_y = size * 0.5;
    let max_a = size * 0.125 * amp;
    (0..=STEPS)
        .map(|i| {
            let u = i as f32 / STEPS as f32;
            let env = if taper {
                (std::f32::consts::PI * u).sin().powf(0.9)
            } else {
                1.0
            };
            let x = x0 + (x1 - x0) * u;
            let y = mid_y - sign * max_a * env * (std::f32::consts::TAU * u + phase).sin();
            [x, y]
        })
        .collect()
}

/// Draws the thin single-hexagon mark or compact progress ring at `size`.
pub(crate) fn hex_meter(p: Paint, key: &str, size: f32, progress: f32, closed: f32) -> El {
    canvas(meter_shape(p, size, progress, closed))
        .key(key)
        .size(size, size)
        .label(format!("{}%", (progress.clamp(0.0, 1.0) * 100.0).round()))
}

fn meter_shape(p: Paint, size: f32, progress: f32, closed: f32) -> Shape {
    let spec = ACTIVE_GLYPH;
    let w = spec.stroke_width(size);
    let poly = spec.poly(size, 1.0);
    let mut ring = poly.clone();
    ring.pop();

    let progress = progress.clamp(0.0, 1.0);
    let closed = closed.clamp(0.0, 1.0);
    // Opaque colours prevent joint overdraw on multi-segment polylines.
    let track = p.k.c.bg_deep.mix(p.k.c.border_base, 0.9);
    let lit = p.text();

    let mut shape = Shape::new();
    if progress < 1.0 {
        shape = shape.stroke_with(ring.clone(), true, w, LineCap::Round, track);
    }
    if progress >= 1.0 && closed >= 1.0 {
        shape = shape.stroke_with(ring, true, w, LineCap::Round, lit);
    } else if progress > 0.002 {
        let (arc, tip) = sweep_poly(&poly, progress);
        let closed_loop = progress >= 1.0;
        shape = shape.stroke_with(arc, closed_loop, w, LineCap::Round, lit);
        if let Some(pt) = tip.filter(|_| size >= 36.0) {
            shape = shape.fill(disc_points(pt, w * 1.15), lit);
        }
    }
    shape
}

/// The centerpiece loader on `"Setting up Hex"` and `"Almost ready"`: the
/// thin single-hexagon outline sweeping clockwise around a delicate braided
/// calibration wave that tunes into phase as the model arrives.
pub(crate) fn hex_loader(
    p: Paint,
    key: &str,
    size: f32,
    progress: f32,
    closed: f32,
    now: f64,
    reduced: bool,
) -> El {
    let spec = ACTIVE_GLYPH;
    let w = spec.stroke_width(size);
    let progress = progress.clamp(0.0, 1.0);
    let closed = closed.clamp(0.0, 1.0);
    let open = 1.0 - closed;
    let bg = p.k.c.bg_deep;

    let mut shape = Shape::new();

    // Subtle inner geometric echo hexagon + 6 precision corner ticks while downloading.
    if open > 0.01 {
        let echo_scale = 0.76 + 0.24 * closed;
        let mut echo = spec.poly(size, echo_scale);
        echo.pop();
        let echo_ink = bg.mix(p.k.c.border_weak, 0.55 * open);
        shape = shape.stroke_with(echo, true, 1.0, LineCap::Round, echo_ink);

        let half = size * 0.5;
        let tick_ink = bg.mix(p.k.c.border_base, 0.75 * open);
        for v in spec.vertices {
            let r0 = 0.88 - 0.07 * open;
            let r1 = 0.88 - 0.02 * open;
            shape = shape.line(
                [
                    half + v[0] * half * (r0 / 0.88),
                    half + v[1] * half * (r0 / 0.88),
                ],
                [
                    half + v[0] * half * (r1 / 0.88),
                    half + v[1] * half * (r1 / 0.88),
                ],
                1.0,
                LineCap::Round,
                tick_ink,
            );
        }
    }

    // Outer perimeter track and clockwise active stroke.
    let poly = spec.poly(size, 1.0);
    let mut ring = poly.clone();
    ring.pop();
    let track = bg.mix(p.k.c.border_base, 0.85);
    let lit = p.text();

    if progress < 1.0 {
        shape = shape.stroke_with(ring.clone(), true, w, LineCap::Round, track);
    }
    if progress >= 1.0 {
        shape = shape.stroke_with(ring, true, w, LineCap::Round, lit);
    } else if progress > 0.002 {
        let (arc, tip) = sweep_poly(&poly, progress);
        shape = shape.stroke_with(arc, false, w, LineCap::Round, lit);
        if let Some(pt) = tip {
            shape = shape.fill(disc_points(pt, w * 1.35), lit);
        }
    }

    // Interior calibration wave: wakes from a flat horizon as bytes arrive,
    // then settles into a calm, balanced wave trace when Ready.
    let phase = if reduced { 0.35 } else { -(now as f32) * 2.4 };
    let amp = (0.14 + 0.72 * progress) * (1.0 - 0.38 * closed);
    let primary_ink = bg.mix(p.text(), 0.35 + 0.55 * progress);
    let secondary_ink = bg.mix(p.muted(), (0.28 + 0.36 * progress) * open);
    if open > 0.04 {
        shape = shape.stroke(
            wave_pts(size, amp * 0.82, phase + 0.9, -1.0, true),
            1.25,
            secondary_ink,
        );
    }
    shape = shape.stroke(wave_pts(size, amp, phase, 1.0, true), 1.5, primary_ink);

    canvas(shape)
        .key(key)
        .size(size, size)
        .label(format!("{}%", (progress * 100.0).round()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_covers_none_half_and_all_of_the_hexagon() {
        let poly = ACTIVE_GLYPH.poly(100.0, 1.0);
        assert!(sweep_poly(&poly, 0.0).0.is_empty());

        let (half, tip) = sweep_poly(&poly, 0.5);
        assert!(half.len() >= 4);
        let end = tip.expect("an unfinished sweep reports its leading tip");
        // Halfway clockwise from the top apex lands at the bottom apex.
        assert!((end[0] - 50.0).abs() < 0.5, "{end:?}");
        assert!(end[1] > 90.0, "{end:?}");

        let (full, tip_done) = sweep_poly(&poly, 1.0);
        assert_eq!(full.len(), 6);
        assert!(tip_done.is_none());
    }

    #[test]
    fn stroke_stays_thin_from_menu_bar_to_hero() {
        let small = ACTIVE_GLYPH.stroke_width(16.0);
        let large = ACTIVE_GLYPH.stroke_width(124.0);
        assert!((1.1..=1.4).contains(&small), "{small}");
        assert!((2.0..=2.4).contains(&large), "{large}");
    }
}
