//! The Hex mark: a thin single-hexagon outline matching Round 3 of the icon
//! (`t573`, Hairline), built from one swappable [`HexGlyphSpec`], and the
//! full three-hexagon logo `"Hex is ready"` builds.

use rune::{El, LineCap, Shape, canvas};

use super::super::controls::Paint;

type Point = [f32; 2];

#[derive(Clone, Copy)]
pub(crate) struct HexGlyphSpec {
    pub(crate) vertices: [Point; 6],
    pub(crate) stroke_16: f32,
    pub(crate) stroke_max: f32,
    pub(crate) corner_radius: f32,
}

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

#[allow(dead_code)]
pub(crate) const SOFT_NUT: HexGlyphSpec = HexGlyphSpec {
    vertices: HAIRLINE.vertices,
    stroke_16: 1.5,
    stroke_max: 2.8,
    corner_radius: 0.14,
};

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

pub(crate) const ACTIVE_GLYPH: HexGlyphSpec = HAIRLINE;

impl HexGlyphSpec {
    pub(crate) fn stroke_width(self, size: f32) -> f32 {
        let t = ((size - 16.0) / 120.0).clamp(0.0, 1.0);
        self.stroke_16 + (self.stroke_max - self.stroke_16) * t
    }

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
                let pt = [
                    inv * inv * a[0] + 2.0 * inv * u * curr[0] + u * u * b[0],
                    inv * inv * a[1] + 2.0 * inv * u * curr[1] + u * u * b[1],
                ];
                pts.push(map(pt));
            }
        }
        pts.push(pts[0]);
        pts
    }
}

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

fn disc_points(centre: Point, radius: f32) -> Vec<Point> {
    (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
        })
        .collect()
}

pub(crate) fn hex_meter(p: Paint, key: &str, size: f32, progress: f32, closed: f32) -> El {
    canvas(meter_shape(p, size, progress, closed))
        .key(key)
        .size(size, size)
        .label(format!("{}%", (progress.clamp(0.0, 1.0) * 100.0).round()))
}

/// When each of the logo's hexagons lands on `"Hex is ready"`, outside in,
/// in seconds after the page appears; each sounds one rising note.
pub(crate) const BUILD_AT: [f64; 3] = [0.0, 0.15, 0.30];
/// When the last hexagon has settled.
pub(crate) const BUILD_SECS: f64 = 0.30 + SETTLE.0 * 1.5;
/// Each hexagon's spring: duration and bounce.
const SETTLE: (f64, f64) = (0.36, 0.12);
/// How long a hexagon takes to appear as it starts to settle.
const APPEAR: f64 = 0.08;

/// The app icon's three hexagons, in half the mark's size: the outline's and
/// middle line's centres, the filled centre's radius, and the lines' width.
const OUTER: f32 = 0.965;
const MIDDLE: f32 = 0.617;
const INNER: f32 = 0.305;
const LINE: f32 = 0.109;
/// How far beyond its place each hexagon starts, as a fraction of its size.
const TRAVEL: [f32; 3] = [0.12, 0.22, 0.45];

/// Progress of a critically-ish damped spring from 0 to 1 at `t` seconds.
fn settle(t: f64) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let (duration, bounce) = SETTLE;
    let omega = std::f64::consts::TAU / duration;
    let zeta = 1.0 - bounce;
    let damped = omega * (1.0 - zeta * zeta).sqrt();
    let decay = (-zeta * omega * t).exp();
    (1.0 - decay * ((damped * t).cos() + zeta * omega / damped * (damped * t).sin())) as f32
}

/// A pointy-top hexagon around `centre` with corners rounded by `corner`.
fn rounded_hex(centre: Point, radius: f32, corner: f32) -> Vec<Point> {
    let v: Vec<Point> = (0..6)
        .map(|i| {
            let a = (i as f32 * 60.0 - 90.0).to_radians();
            [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
        })
        .collect();
    let r = corner.min(radius * 0.4);
    let mut pts = Vec::with_capacity(6 * 7);
    for i in 0..6 {
        let (prev, curr, next) = (v[(i + 5) % 6], v[i], v[(i + 1) % 6]);
        let toward = |to: Point| {
            let d = [to[0] - curr[0], to[1] - curr[1]];
            let len = d[0].hypot(d[1]).max(1e-4);
            [curr[0] + d[0] / len * r, curr[1] + d[1] / len * r]
        };
        let (a, b) = (toward(prev), toward(next));
        for step in 0..=6 {
            let u = step as f32 / 6.0;
            let inv = 1.0 - u;
            pts.push([
                inv * inv * a[0] + 2.0 * inv * u * curr[0] + u * u * b[0],
                inv * inv * a[1] + 2.0 * inv * u * curr[1] + u * u * b[1],
            ]);
        }
    }
    pts
}

/// The Hex logo for `"Hex is ready"`, built outside in `age` seconds after
/// the page appears: the outline, the middle line, then the filled centre,
/// each springing in from just beyond its place. `handed`: the outline
/// arrives from the download pill already drawn. Reduced Motion shows it
/// built.
pub(crate) fn hex_logo(
    p: Paint,
    key: &str,
    size: f32,
    age: f64,
    reduced: bool,
    handed: bool,
) -> El {
    let pad = 12.0;
    let total = size + 2.0 * pad;
    let half = size * 0.5;
    let centre = [total * 0.5, total * 0.5];
    let line = LINE * half;
    let stage = |i: usize| -> (f32, f32) {
        if reduced || (i == 0 && handed) {
            return (1.0, 1.0);
        }
        let t = age - BUILD_AT[i];
        let shown = (t / APPEAR).clamp(0.0, 1.0) as f32;
        (1.0 + TRAVEL[i] * (1.0 - settle(t)), shown)
    };

    let (scale, shown) = stage(0);
    let outline = rounded_hex(centre, OUTER * half * scale, line * 0.9);
    let mut shape =
        Shape::new().stroke_with(outline, true, line, LineCap::Round, p.text().alpha(shown));
    let (scale, shown) = stage(1);
    if shown > 0.0 {
        let ring = rounded_hex(centre, MIDDLE * half * scale, line * 0.8);
        shape = shape.stroke_with(ring, true, line, LineCap::Round, p.text().alpha(shown));
    }
    let (scale, shown) = stage(2);
    if shown > 0.0 {
        let core = rounded_hex(centre, INNER * half * scale, line * 0.75);
        shape = shape.fill(core, p.text().alpha(shown));
    }
    canvas(shape).key(key).size(total, total).label("Hex")
}

fn meter_shape(p: Paint, size: f32, progress: f32, closed: f32) -> Shape {
    let spec = ACTIVE_GLYPH;
    let w = spec.stroke_width(size);
    let poly = spec.poly(size, 1.0);
    let mut ring = poly.clone();
    ring.pop();

    let progress = progress.clamp(0.0, 1.0);
    let closed = closed.clamp(0.0, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_stays_thin_from_menu_bar_to_hero() {
        for spec in [HAIRLINE, SOFT_NUT, FLAT_TOP] {
            assert!(spec.stroke_width(14.0) >= 1.1 && spec.stroke_width(14.0) <= 1.6);
            assert!(spec.stroke_width(136.0) <= 3.0);
        }
    }

    #[test]
    fn the_logo_builds_outside_in_and_settles() {
        assert_eq!(settle(-0.1), 0.0);
        let peak = (1..200)
            .map(|i| settle(i as f64 / 200.0))
            .fold(0.0, f32::max);
        assert!(
            peak > 1.0 && peak < 1.05,
            "a soft settle, not a bounce: {peak}"
        );
        assert!((settle(BUILD_SECS - BUILD_AT[2]) - 1.0).abs() < 0.01);
        assert!(
            BUILD_AT
                .windows(2)
                .all(|w| (0.12..=0.18).contains(&(w[1] - w[0])))
        );
        // A hexagon starting out never touches the one outside it.
        const {
            assert!(OUTER - LINE / 2.0 > MIDDLE * (1.0 + TRAVEL[1]) + LINE / 2.0);
            assert!(MIDDLE - LINE / 2.0 > INNER * (1.0 + TRAVEL[2]));
        }
    }

    #[test]
    fn sweep_covers_none_half_and_all_of_the_hexagon() {
        let poly = ACTIVE_GLYPH.poly(100.0, 1.0);
        assert!(sweep_poly(&poly, 0.0).0.is_empty());
        let (half, tip) = sweep_poly(&poly, 0.5);
        assert!(half.len() >= 3 && tip.is_some());
        let (full, tip) = sweep_poly(&poly, 1.0);
        assert_eq!(full.len(), 6);
        assert!(tip.is_none());
    }
}
