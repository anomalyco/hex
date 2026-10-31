//! A sparkle that flips on its vertical axis: four points with curved sides,
//! each side rising from its edge to a peak at the centre, drawn in the
//! theme's ink so the light rolls across each point as it turns. It rests
//! face-on, then turns half a turn with an ease in and out, which brings it
//! back to the same face, once per example of the agent card's copy, so the
//! copy changes as it turns edge-on. Rune turns shapes only in the plane, so
//! the turn is projected ahead of time into frames, each shown for its slot
//! of the loop through a clock-driven opacity cycle: the renderer flips
//! between them with no view rebuilds. Reduce Motion holds it face-on.

use std::f32::consts::PI;

use rune::{Color, Cycle, El, Shape, canvas, col};

use super::controls::Paint;

/// Seconds for one rest and one flip: one example of the card's copy.
pub(super) const PERIOD: f32 = 3.6;
/// The share of the loop it rests face-on. It turns edge-on 2.88 s in.
const REST: f32 = 0.6;
/// Frames in the flip, the face-on frame included: 60 a second.
const FRAMES: usize = 86;
/// How long each flip frame shows, as a share of the loop.
const STEP: f32 = (1.0 - REST) / FRAMES as f32;
/// How far the side points reach, against the vertical points' 1.
const REACH: f32 = 0.8;
/// How close each curved side passes to the centre: the smaller, the
/// thinner the points.
const PINCH: f32 = 0.08;
/// How far the peak stands out of the sparkle's plane.
const BEVEL: f32 = 0.25;
/// Points along each curved side.
const SAMPLES: usize = 14;
/// Rings of faces from the peak out to the edge.
const RINGS: usize = 6;
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

/// The sparkle turned `turn` radians about its vertical axis: its outline in
/// a middle shade, then the faces that look this way, shaded from `ground`
/// toward `ink` and painted back to front inside it. The outline gives the
/// tips one clean edge; the faces only shade what it covers.
fn faces(turn: f32, size: f32, ground: Color, ink: Color, dark: bool) -> Shape {
    let edge = boundary();
    let (sin, cos) = turn.sin_cos();
    let spin = |[x, y, z]: Point| [x * cos + z * sin, y, z * cos - x * sin];
    let mut faces: Vec<(f32, Vec<Point>, Point)> = Vec::new();
    for side in [1.0, -1.0] {
        let at = |s: usize, k: usize| {
            let r = k as f32 / RINGS as f32;
            let b = edge[s % edge.len()];
            [b[0] * r, b[1] * r, side * BEVEL * (1.0 - r).powf(1.5)]
        };
        for s in 0..edge.len() {
            for k in 0..RINGS {
                let face: Vec<Point> = if k == 0 {
                    vec![at(s, 0), at(s, 1), at(s + 1, 1)]
                } else {
                    vec![at(s, k), at(s, k + 1), at(s + 1, k + 1), at(s + 1, k)]
                };
                let view: Vec<Point> = face.into_iter().map(spin).collect();
                let normal = outward(&view);
                if normal[2] <= 0.0 {
                    continue;
                }
                let depth = view.iter().map(|v| v[2]).sum::<f32>() / view.len() as f32;
                faces.push((depth, view, normal));
            }
        }
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (scale, middle) = (size * 0.48, size / 2.0);
    let place = |v: &Point| [middle + v[0] * scale, middle + v[1] * scale];
    let outline: Vec<[f32; 2]> = edge.iter().map(|point| place(&spin(*point))).collect();
    let base = if dark { 0.55 } else { 0.7 };
    let mut shape = Shape::new().fill(outline, ground.mix(ink, base));
    for (_, view, normal) in faces {
        let light = shade(normal);
        let amount = if dark {
            0.3 + 0.7 * light
        } else {
            0.95 - 0.5 * light
        };
        let points: Vec<[f32; 2]> = view.iter().map(place).collect();
        shape = shape.fill(points, ground.mix(ink, amount.clamp(0.0, 1.0)));
    }
    shape
}

/// The sparkle's edge in its own plane: four tips joined by curves that bow
/// toward the centre, `SAMPLES` points per side, clockwise from the top
/// (y points down).
fn boundary() -> Vec<Point> {
    let tips: [Point; 4] = [
        [0.0, -1.0, 0.0],
        [REACH, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-REACH, 0.0, 0.0],
    ];
    let mut points = Vec::with_capacity(SAMPLES * 4);
    for i in 0..4 {
        let (a, b) = (tips[i], tips[(i + 1) % 4]);
        let toward = unit(add(a, b)).map(|c| c * PINCH);
        for j in 0..SAMPLES {
            let t = j as f32 / SAMPLES as f32;
            let (u, v, w) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            points.push([
                u * a[0] + v * toward[0] + w * b[0],
                u * a[1] + v * toward[1] + w * b[1],
                0.0,
            ]);
        }
    }
    points
}

/// The face's unit normal from its first point and its neighbours on
/// either side, pointing away from the sparkle's middle.
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
