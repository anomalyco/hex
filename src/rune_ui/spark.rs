//! A sparkle that flips on its vertical axis: four points with curved sides,
//! bevelled so each point has a lit half and a shaded half, drawn in the
//! theme's ink. It rests face-on, then turns half a turn with an ease in and
//! out, which brings it back to the same face. Rune turns shapes only in the
//! plane, so the turn is projected ahead of time into frames, each shown for
//! its slot of the loop through a clock-driven opacity cycle: the renderer
//! flips between them with no view rebuilds. Reduce Motion holds it face-on.

use std::f32::consts::PI;

use rune::{Color, Cycle, El, Shape, canvas, col};

use super::controls::Paint;

/// Seconds for one rest and one flip.
const PERIOD: f32 = 5.0;
/// The share of the loop it rests face-on.
const REST: f32 = 0.62;
/// Frames in the flip, the face-on frame included.
const FRAMES: usize = 57;
/// How long each flip frame shows, as a share of the loop.
const STEP: f32 = (1.0 - REST) / FRAMES as f32;
/// How far the side points reach, against the vertical points' 1.
const REACH: f32 = 0.8;
/// How close each curved side passes to the centre: the smaller, the
/// thinner the points.
const PINCH: f32 = 0.05;
/// How far the bevel's centre stands out of the sparkle's plane.
const BEVEL: f32 = 0.22;
/// Points along each curved side.
const SAMPLES: usize = 16;
/// The face-on frame shows through the rest and one flip step; a hair more
/// so rounding at a boundary never leaves a moment with no frame.
const RESTING: [(f32, f32); 4] = [
    (0.0, 1.0),
    (REST + STEP + 1e-3, 1.0),
    (REST + STEP + 1e-3, 0.0),
    (1.0, 0.0),
];
/// Every other frame shows for one flip step.
const TURNING: [(f32, f32); 4] = [
    (0.0, 1.0),
    (STEP + 1e-3, 1.0),
    (STEP + 1e-3, 0.0),
    (1.0, 0.0),
];

/// The sparkle, `size` points square.
pub(super) fn spark(p: Paint, key: &str, size: f32) -> El {
    let ink = p.text();
    let ground = p.k.c.bg_base;
    let frames = (0..FRAMES).map(|frame| {
        let turn = PI * ease(frame as f32 / FRAMES as f32);
        canvas(faces(turn, size, ground, ink, p.k.dark))
            .key(format!("{key}-frame{frame}"))
            .absolute(0.0, 0.0)
            .size(size, size)
            .cycle(cycle(frame))
    });
    col().key(key).size(size, size).children(frames)
}

/// When `frame` shows: the face-on frame from the last flip step through the
/// rest, every other frame for its own step of the flip.
fn cycle(frame: usize) -> Cycle {
    let (phase, stops) = if frame == 0 {
        (1.0 - STEP, &RESTING)
    } else {
        (REST + (frame - 1) as f32 * STEP, &TURNING)
    };
    Cycle {
        period: PERIOD,
        phase,
        stops,
        still: if frame == 0 { 1.0 } else { 0.0 },
        fps: 1.0 / (STEP * PERIOD),
    }
}

/// Slow out of the rest, quick through the middle, slow into the next.
fn ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

type Point = [f32; 3];

/// The sparkle turned `turn` radians about its vertical axis, as opaque
/// bevel faces shaded from `ground` toward `ink`, painted back to front.
fn faces(turn: f32, size: f32, ground: Color, ink: Color, dark: bool) -> Shape {
    // y points down; the tips go clockwise from the top.
    let tips: [Point; 4] = [
        [0.0, -1.0, 0.0],
        [REACH, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-REACH, 0.0, 0.0],
    ];
    let (sin, cos) = turn.sin_cos();
    let spin = |[x, y, z]: Point| [x * cos + z * sin, y, z * cos - x * sin];
    let mut halves: Vec<(f32, Vec<Point>, Point)> = Vec::new();
    for side in [1.0, -1.0] {
        let centre = [0.0, 0.0, BEVEL * side];
        for i in 0..4 {
            let curve = side_curve(tips[i], tips[(i + 1) % 4]);
            let (first, second) = curve.split_at(SAMPLES / 2);
            let mut lit: Vec<Point> = vec![centre];
            lit.extend_from_slice(first);
            lit.push(second[0]);
            let mut shaded: Vec<Point> = vec![centre];
            shaded.extend_from_slice(second);
            shaded.push(tips[(i + 1) % 4]);
            for half in [lit, shaded] {
                let view: Vec<Point> = half.iter().map(|point| spin(*point)).collect();
                let normal = outward(&view);
                if normal[2] <= 0.0 {
                    continue;
                }
                let depth = view.iter().map(|v| v[2]).sum::<f32>() / view.len() as f32;
                halves.push((depth, view, normal));
            }
        }
    }
    halves.sort_by(|a, b| a.0.total_cmp(&b.0));
    let scale = size * 0.48;
    let middle = size / 2.0;
    let mut shape = Shape::new();
    for (_, view, normal) in halves {
        let light = shade(normal);
        let amount = if dark {
            0.28 + 0.72 * light
        } else {
            0.98 - 0.5 * light
        };
        let points: Vec<[f32; 2]> = view
            .iter()
            .map(|v| [middle + v[0] * scale, middle + v[1] * scale])
            .collect();
        shape = shape.fill(spread(points), ground.mix(ink, amount.clamp(0.0, 1.0)));
    }
    shape
}

/// The curved side from tip `a` to tip `b`, bowing in past the centre, as
/// `SAMPLES` points starting at `a` (the end, `b`, is the next side's start).
fn side_curve(a: Point, b: Point) -> Vec<Point> {
    let toward = unit(add(a, b)).map(|c| c * PINCH);
    (0..SAMPLES)
        .map(|j| {
            let t = j as f32 / SAMPLES as f32;
            let (u, v, w) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            [
                u * a[0] + v * toward[0] + w * b[0],
                u * a[1] + v * toward[1] + w * b[1],
                0.0,
            ]
        })
        .collect()
}

/// The polygon grown a quarter point from its centroid, so neighbouring
/// faces overlap instead of showing the card through their antialiased
/// shared edges.
fn spread(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let count = points.len() as f32;
    let centre = points.iter().fold([0.0, 0.0], |a, p| {
        [a[0] + p[0] / count, a[1] + p[1] / count]
    });
    points
        .into_iter()
        .map(|[x, y]| {
            let (dx, dy) = (x - centre[0], y - centre[1]);
            let length = (dx * dx + dy * dy).sqrt().max(1e-6);
            [x + dx / length * 0.25, y + dy / length * 0.25]
        })
        .collect()
}

/// The face's unit normal from its centre point and the two ends of its
/// edge, pointing away from the sparkle's middle.
fn outward(points: &[Point]) -> Point {
    let sub = |a: Point, b: Point| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (u, v) = (
        sub(points[1], points[0]),
        sub(points[points.len() - 1], points[0]),
    );
    let mut n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let centre = points.iter().fold([0.0; 3], |a, p| add(a, *p));
    if dot(n, centre) < 0.0 {
        n = n.map(|c| -c);
    }
    unit(n)
}

/// How lit a face turned to `normal` looks, 0 to 1: light from the top left
/// and a glint where the face swings through its reflection.
fn shade(normal: Point) -> f32 {
    let key = unit([-0.55, -0.6, 0.58]);
    let half = unit([key[0], key[1], key[2] + 1.0]);
    let diffuse = dot(normal, key).max(0.0);
    let glint = dot(normal, half).max(0.0).powi(30);
    (0.15 + 0.75 * diffuse + 0.35 * glint).min(1.0)
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(v: Point) -> Point {
    let length = dot(v, v).sqrt().max(1e-6);
    v.map(|c| c / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Some frame shows at every moment, the slot boundaries included, so
    /// the sparkle never blinks out.
    #[test]
    fn a_frame_always_shows() {
        let boundaries = (0..FRAMES * 2)
            .map(|slot| f64::from(PERIOD) * (f64::from(REST) + slot as f64 * f64::from(STEP)));
        let between = (0..997).map(|step| f64::from(step) * 0.0171);
        for t in boundaries.chain(between) {
            let shown = (0..FRAMES)
                .filter(|frame| cycle(*frame).sample(t) > 0.999)
                .count();
            assert!((1..=2).contains(&shown), "{shown} frames at {t}");
        }
    }
}
