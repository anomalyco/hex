//! The Hex mark as a progress meter: the core fills first, then the inner
//! ring's six bars, then the outer ring's, each bar sweeping clockwise. When
//! the download finishes, the gaps between bars close and the meter becomes
//! the mark itself.

use rune::{Color, El, Shape, canvas};

use super::super::controls::Paint;

/// The geometry's half extent: points run from -`HALF` to `HALF`.
const HALF: f32 = 62.0;
const CORE: f32 = 16.0;
const RINGS: [f32; 2] = [31.0, 50.0];
const BAR: f32 = 11.0;
/// The gap between neighbouring bars while the meter is open.
const GAP: f32 = 6.0;
/// The core's share of the whole download.
const CORE_SHARE: f32 = 0.12;

type Point = [f32; 2];

/// One piece of the mark and the slice of progress it fills over.
struct Part {
    points: Vec<Point>,
    /// The direction it fills along.
    toward: Point,
    from: f32,
    to: f32,
}

impl Part {
    fn fill(&self, progress: f32) -> f32 {
        ((progress - self.from) / (self.to - self.from)).clamp(0.0, 1.0)
    }
}

fn corners(radius: f32) -> [Point; 6] {
    std::array::from_fn(|k| {
        let angle = (-90.0 + 60.0 * k as f32).to_radians();
        [radius * angle.cos(), radius * angle.sin()]
    })
}

/// Every part, in the order it fills, with the gaps `gap` wide.
fn parts(gap: f32) -> Vec<Part> {
    let mut parts = vec![Part {
        points: corners(CORE).to_vec(),
        toward: [0.0, -1.0],
        from: 0.0,
        to: CORE_SHARE,
    }];
    let inner = (1.0 - CORE_SHARE) * RINGS[0] / (RINGS[0] + RINGS[1]);
    let shares = [
        (CORE_SHARE, inner),
        (CORE_SHARE + inner, 1.0 - CORE_SHARE - inner),
    ];
    let half = BAR / 2.0 / (std::f32::consts::PI / 6.0).cos();
    for (ring, (start, share)) in RINGS.into_iter().zip(shares) {
        let (outer, inner) = (corners(ring + half), corners(ring - half));
        for k in 0..6 {
            let j = (k + 1) % 6;
            let (dx, dy) = (outer[j][0] - outer[k][0], outer[j][1] - outer[k][1]);
            let length = dx.hypot(dy);
            let toward = [dx / length, dy / length];
            let nudge = |p: Point, sign: f32| {
                [
                    p[0] + toward[0] * gap / 2.0 * sign,
                    p[1] + toward[1] * gap / 2.0 * sign,
                ]
            };
            parts.push(Part {
                points: vec![
                    nudge(outer[k], 1.0),
                    nudge(outer[j], -1.0),
                    nudge(inner[j], -1.0),
                    nudge(inner[k], 1.0),
                ],
                toward,
                from: start + share / 6.0 * k as f32,
                to: start + share / 6.0 * (k + 1) as f32,
            });
        }
    }
    parts
}

/// The part of `points` behind a line across `toward`, `fraction` of the
/// way along it.
fn cut(points: &[Point], toward: Point, fraction: f32) -> Vec<Point> {
    let along: Vec<f32> = points
        .iter()
        .map(|p| p[0] * toward[0] + p[1] * toward[1])
        .collect();
    let (low, high) = along
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    let line = low + (high - low) * fraction;
    let mut out = Vec::with_capacity(points.len() + 2);
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let (a, b) = (points[i], points[j]);
        if along[i] <= line {
            out.push(a);
        }
        if (along[i] <= line) != (along[j] <= line) {
            let u = (line - along[i]) / (along[j] - along[i]);
            out.push([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]);
        }
    }
    out
}

fn shifted(points: Vec<Point>) -> Vec<Point> {
    points
        .into_iter()
        .map(|[x, y]| [x + HALF, y + HALF])
        .collect()
}

/// The meter `size` points square at `progress` (0 to 1). `closed` (0 to
/// 1) narrows the gaps until the bars join into whole rings.
pub(crate) fn hex_meter(p: Paint, key: &str, size: f32, progress: f32, closed: f32) -> El {
    let base = p.k.c.border_base;
    let lit = p.text();
    canvas(shape(base, lit, progress, closed))
        .key(key)
        .size(size, size)
        .label(format!("{}%", (progress * 100.0).round()))
}

fn shape(base: Color, lit: Color, progress: f32, closed: f32) -> Shape {
    let gap = GAP * (1.0 - closed.clamp(0.0, 1.0));
    let mut shape = Shape::new().view_box(2.0 * HALF, 2.0 * HALF);
    for part in parts(gap) {
        let fill = part.fill(progress);
        if fill < 1.0 {
            shape = shape.fill(shifted(part.points.clone()), base);
        }
        if fill > 0.0 {
            let points = if fill >= 1.0 {
                part.points
            } else {
                cut(&part.points, part.toward, fill)
            };
            if points.len() >= 3 {
                shape = shape.fill(shifted(points), lit);
            }
        }
    }
    shape
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fills(progress: f32) -> Vec<f32> {
        parts(GAP).iter().map(|part| part.fill(progress)).collect()
    }

    #[test]
    fn the_core_then_the_inner_ring_then_the_outer_ring_fill() {
        let early = fills(CORE_SHARE / 2.0);
        assert!(early[0] > 0.0 && early[0] < 1.0);
        assert!(early[1..].iter().all(|f| *f == 0.0));

        let middle = fills(0.4);
        assert_eq!(middle[0], 1.0);
        assert!(middle[1..7].iter().any(|f| *f > 0.0));
        assert!(middle[7..].iter().all(|f| *f == 0.0));

        assert!(fills(1.0).iter().all(|f| *f == 1.0));
        assert!(fills(0.0).iter().all(|f| *f == 0.0));
    }

    #[test]
    fn the_parts_cover_the_whole_download_without_overlap() {
        let parts = parts(GAP);
        assert_eq!(parts.len(), 13);
        assert_eq!(parts[0].from, 0.0);
        for pair in parts.windows(2) {
            assert!((pair[0].to - pair[1].from).abs() < 1e-5);
        }
        assert!((parts.last().unwrap().to - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_half_filled_bar_is_cut_across_its_length() {
        let bar = &parts(GAP)[1];
        let half = cut(&bar.points, bar.toward, 0.5);
        assert!(half.len() >= 3);
        let along = |p: &Point| p[0] * bar.toward[0] + p[1] * bar.toward[1];
        let full: Vec<f32> = bar.points.iter().map(along).collect();
        let mid = (full.iter().cloned().fold(f32::MAX, f32::min)
            + full.iter().cloned().fold(f32::MIN, f32::max))
            / 2.0;
        assert!(half.iter().all(|p| along(p) <= mid + 1e-3));
    }
}
