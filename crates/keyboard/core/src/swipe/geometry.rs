//! Geometry in key pitches: distances, resampling, shapes and the two location passes
//! (`SwipeDecoder`'s static helpers, in the same operation order, so every cost comes out
//! bit for bit as Swift's).

/// A point or a step, in key pitches (`SwipeDecoder.Vec`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Swift's `min(x, y)`: `y < x ? y : x` (so `x` on a tie or a NaN), not `f64::min`.
#[inline(always)]
pub(crate) fn min(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

/// Swift's `max(x, y)`: `y >= x ? y : x` (so `x` on a NaN), not `f64::max`.
#[inline(always)]
pub(crate) fn max(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

/// A stable sort by Swift's `<` (`areInIncreasingOrder`): Swift's `sort` is stable, and so
/// is `sort_by`; the comparator answers as the predicate does, never `partial_cmp`.
pub(crate) fn sort_by_less<T>(items: &mut [T], less: impl Fn(&T, &T) -> bool) {
    items.sort_by(|a, b| {
        if less(a, b) {
            std::cmp::Ordering::Less
        } else if less(b, a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}

#[inline(always)]
pub(crate) fn distance(a: Point, b: Point) -> f64 {
    ((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)).sqrt()
}

#[inline(always)]
pub(crate) fn squared(a: Point, b: Point) -> f64 {
    (a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)
}

/// The keys within `radius` of `point`, nearest first, at most `limit`, with their squared
/// distances; the nearest two however far.
pub(crate) fn nearest(
    centres: &[Point],
    point: Point,
    radius: f64,
    limit: usize,
) -> Vec<(usize, f64)> {
    let mut sorted: Vec<(usize, f64)> = centres
        .iter()
        .enumerate()
        .map(|(i, &c)| (i, squared(c, point)))
        .collect();
    sort_by_less(&mut sorted, |a, b| a.1 < b.1);
    let found: Vec<(usize, f64)> = sorted
        .iter()
        .take(limit)
        .copied()
        .filter(|e| e.1 <= radius * radius)
        .collect();
    if found.len() < 2 {
        return sorted.iter().take(limit.min(2)).copied().collect();
    }
    found
}

pub(crate) fn distance_to_polyline(point: Point, line: &[Point]) -> f64 {
    let mut best = f64::INFINITY;
    for i in 1..line.len() {
        let (a, b) = (line[i - 1], line[i]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let span = dx * dx + dy * dy;
        let mut t = if span > 0.0 {
            ((point.x - a.x) * dx + (point.y - a.y) * dy) / span
        } else {
            0.0
        };
        t = min(1.0, max(0.0, t));
        best = min(best, squared(point, Point::new(a.x + t * dx, a.y + t * dy)));
    }
    best.sqrt()
}

/// `out.len()` points of `line` (`length` long) evenly spaced along it into `out`, first and
/// last included.
pub(crate) fn resample(line: &[Point], length: f64, out: &mut [Point]) {
    let count = line.len();
    let samples = out.len();
    out[0] = line[0];
    out[samples - 1] = line[count - 1];
    if samples <= 2 {
        return;
    }
    let step = length / (samples - 1) as f64;
    let mut segment = 0;
    let mut walked = 0.0;
    let mut span = distance(line[0], line[1.min(count - 1)]);
    for (i, slot) in out.iter_mut().enumerate().take(samples - 1).skip(1) {
        let target = step * i as f64;
        while walked + span < target && segment + 2 < count {
            walked += span;
            segment += 1;
            span = distance(line[segment], line[segment + 1]);
        }
        let (a, b) = (line[segment], line[(segment + 1).min(count - 1)]);
        let t = if span > 0.0 {
            min(1.0, max(0.0, (target - walked) / span))
        } else {
            0.0
        };
        *slot = Point::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
    }
}

/// The points moved to their centroid and scaled so their bounding box's longer side is 1.
pub(crate) fn normalized(points: &[Point]) -> Vec<Point> {
    let (centre, scale) = frame(points);
    points
        .iter()
        .map(|v| Point::new((v.x - centre.x) * scale, (v.y - centre.y) * scale))
        .collect()
}

#[inline(always)]
fn frame(points: &[Point]) -> (Point, f64) {
    let (mut sx, mut sy) = (0.0, 0.0);
    let (mut min_x, mut max_x) = (f64::INFINITY, -f64::INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, -f64::INFINITY);
    for v in points {
        sx += v.x;
        sy += v.y;
        min_x = min(min_x, v.x);
        max_x = max(max_x, v.x);
        min_y = min(min_y, v.y);
        max_y = max(max_y, v.y);
    }
    let side = max(max_x - min_x, max_y - min_y);
    let n = points.len() as f64;
    (
        Point::new(sx / n, sy / n),
        if side > 1e-6 { 1.0 / side } else { 1.0 },
    )
}

/// Mean squared distance between the gesture's normalized shape and the template's.
pub(crate) fn shape_distance(shape: &[Point], template: &[Point]) -> f64 {
    let (centre, scale) = frame(template);
    let mut total = 0.0;
    for (i, &s) in shape.iter().enumerate() {
        let t = Point::new(
            (template[i].x - centre.x) * scale,
            (template[i].y - centre.y) * scale,
        );
        total += squared(s, t);
    }
    total / shape.len() as f64
}

pub(crate) fn mean_squared_distance(a: &[Point], b: &[Point]) -> f64 {
    let mut total = 0.0;
    for (i, &p) in a.iter().enumerate() {
        total += squared(p, b[i]);
    }
    total / a.len() as f64
}

/// Dynamic time warping of two equally long point lists within a band, symmetric steps (a
/// diagonal step counts twice), as a mean squared distance. `rows` are two rows of `a.len()`.
pub(crate) fn warped_distance(
    a: &[Point],
    b: &[Point],
    band: usize,
    rows: &mut (Vec<f64>, Vec<f64>),
) -> f64 {
    let n = a.len();
    for j in 0..n {
        rows.0[j] = f64::INFINITY;
    }
    for i in 0..n {
        for j in 0..n {
            rows.1[j] = f64::INFINITY;
        }
        let (lo, hi) = (i.saturating_sub(band), (n - 1).min(i + band));
        for j in lo..=hi {
            let c = squared(a[i], b[j]);
            let mut best = if i == 0 && j == 0 {
                2.0 * c
            } else {
                f64::INFINITY
            };
            if i > 0 && j > 0 {
                best = min(best, rows.0[j - 1] + 2.0 * c);
            }
            if i > 0 {
                best = min(best, rows.0[j] + c);
            }
            if j > 0 {
                best = min(best, rows.1[j - 1] + c);
            }
            rows.1[j] = best;
        }
        std::mem::swap(&mut rows.0, &mut rows.1);
    }
    rows.0[n - 1] / (2 * n) as f64
}
