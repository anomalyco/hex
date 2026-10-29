//! The Hex mark: a thin single-hexagon outline matching Round 3 of the icon
//! (`t573`, Hairline), built from one swappable [`HexGlyphSpec`].

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

/// Thin single-hexagon outline with a quiet breathing corona for `"Hex is ready"`.
pub(crate) fn hex_meter_glowing(p: Paint, key: &str, size: f32, now: f64, reduced: bool) -> El {
    let spec = ACTIVE_GLYPH;
    let w = spec.stroke_width(size);
    let pad = 12.0;
    let total = size + 2.0 * pad;
    let mut ring: Vec<Point> = spec
        .poly(size, 1.0)
        .into_iter()
        .map(|[x, y]| [x + pad, y + pad])
        .collect();
    ring.pop();

    let breath = if reduced {
        1.0
    } else {
        0.84 + 0.16 * (0.5 + 0.5 * (now * std::f64::consts::TAU / 4.8).sin() as f32)
    };
    let halo = if p.k.dark {
        rune_theme_opencode::scale("cobalt", true, 9).mix(p.text(), 0.45)
    } else {
        rune_theme_opencode::scale("cobalt", false, 8)
    };
    let outer_a = (if p.k.dark { 0.08 } else { 0.05 }) * breath;
    let inner_a = (if p.k.dark { 0.19 } else { 0.11 }) * breath;

    let shape = Shape::new()
        .stroke_with(
            ring.clone(),
            true,
            w * 5.2,
            LineCap::Round,
            halo.alpha(outer_a),
        )
        .stroke_with(
            ring.clone(),
            true,
            w * 2.5,
            LineCap::Round,
            halo.alpha(inner_a),
        )
        .stroke_with(ring, true, w, LineCap::Round, p.text());

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
