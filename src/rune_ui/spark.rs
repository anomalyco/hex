//! A spark that turns on its vertical axis: two long points up and down and
//! four shorter ones around its middle, the outline curving in between them,
//! each face shaded in the theme's ink. Rune turns shapes only in the plane, so the spark is
//! projected ahead of time into one frame per step of a turn, and each frame
//! shows for its slot of the loop through a clock-driven opacity cycle: the
//! renderer flips between them with no view rebuilds. The spark repeats
//! every quarter turn, so one quarter is all the frames there are. Reduce
//! Motion holds the first.

use std::f32::consts::{FRAC_PI_2, TAU};

use rune::{Color, Cycle, El, Shape, canvas, col};

use super::controls::Paint;

/// Points around the spark's middle; it repeats every `1 / ARMS` of a turn.
const ARMS: usize = 4;
/// Frames in one repeat.
const FRAMES: usize = 72;
/// Seconds for one repeat, so a whole turn takes `ARMS` times as long.
const REPEAT: f32 = 3.0;
/// How far the middle points reach, against the vertical points' 1.
const REACH: f32 = 0.66;
/// How far the valleys between points sit from the centre: the smaller,
/// the thinner the points.
const WAIST: f32 = 0.17;
/// How far each eighth's ridge point stands from the centre.
const RIDGE: f32 = 0.24;
/// How far the camera looks down onto the spark, in radians.
const PITCH: f32 = 0.12;
/// Each frame is shown for its slot of the repeat and hidden for the rest.
/// A slot runs a hair past its end so rounding at the boundary never leaves
/// a moment with no frame; the next frame paints over the overlap.
const SLOT: [(f32, f32); 4] = [
    (0.0, 1.0),
    (1.0 / FRAMES as f32 + 1e-3, 1.0),
    (1.0 / FRAMES as f32 + 1e-3, 0.0),
    (1.0, 0.0),
];

/// The spark, `size` points square.
pub(super) fn spark(p: Paint, key: &str, size: f32) -> El {
    let ink = p.text();
    let ground = p.k.c.bg_base;
    let frames = (0..FRAMES).map(|frame| {
        let turn = frame as f32 / FRAMES as f32 * TAU / ARMS as f32;
        canvas(faces(turn, size, ground, ink, p.k.dark))
            .key(format!("{key}-frame{frame}"))
            .absolute(0.0, 0.0)
            .size(size, size)
            .cycle(Cycle {
                period: REPEAT,
                phase: frame as f32 / FRAMES as f32,
                stops: &SLOT,
                still: if frame == 0 { 1.0 } else { 0.0 },
                fps: FRAMES as f32 / REPEAT,
            })
    });
    col().key(key).size(size, size).children(frames)
}

type Point = [f32; 3];

/// The spark turned by `turn` radians, as opaque faces shaded from
/// `ground` toward `ink`, painted back to front.
fn faces(turn: f32, size: f32, ground: Color, ink: Color, dark: bool) -> Shape {
    // y points down. Between every two neighbouring points the outline
    // sinks to a valley near the centre, and each eighth of the spark is a
    // fan around a ridge point that stands a little proud of the valleys.
    let tips = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
    let arm = |k: usize| {
        let angle = turn + k as f32 * FRAC_PI_2;
        [REACH * angle.cos(), 0.0, REACH * angle.sin()]
    };
    let valley = |a: Point, b: Point| unit(add(unit(a), unit(b))).map(|c| c * WAIST);
    let mut triangles: Vec<[Point; 3]> = Vec::new();
    for tip in tips {
        for k in 0..ARMS {
            let (a, b) = (arm(k), arm((k + 1) % ARMS));
            let ridge = unit(add(add(unit(tip), unit(a)), unit(b))).map(|c| c * RIDGE);
            let outline = [tip, valley(tip, a), a, valley(a, b), b, valley(b, tip)];
            for i in 0..outline.len() {
                triangles.push([outline[i], outline[(i + 1) % outline.len()], ridge]);
            }
        }
    }
    let mut seen: Vec<(f32, [Point; 3], Point)> = triangles
        .into_iter()
        .filter_map(|triangle| {
            let view = triangle.map(look);
            let normal = outward(&view);
            (normal[2] > 0.0).then(|| ((view[0][2] + view[1][2] + view[2][2]) / 3.0, view, normal))
        })
        .collect();
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    let scale = size * 0.47;
    let centre = [size / 2.0, size / 2.0];
    let mut shape = Shape::new();
    for (_, view, normal) in seen {
        let light = shade(normal);
        let amount = if dark {
            0.12 + 0.82 * light
        } else {
            0.78 - 0.64 * light
        };
        let points = view.map(|v| [centre[0] + v[0] * scale, centre[1] + v[1] * scale]);
        shape = shape.fill(
            spread(points).to_vec(),
            ground.mix(ink, amount.clamp(0.0, 1.0)),
        );
    }
    shape
}

/// The triangle grown a quarter point from its centroid, so neighbouring
/// faces overlap instead of showing the card through their antialiased
/// shared edges.
fn spread(points: [[f32; 2]; 3]) -> [[f32; 2]; 3] {
    let centre = [
        (points[0][0] + points[1][0] + points[2][0]) / 3.0,
        (points[0][1] + points[1][1] + points[2][1]) / 3.0,
    ];
    points.map(|[x, y]| {
        let (dx, dy) = (x - centre[0], y - centre[1]);
        let length = (dx * dx + dy * dy).sqrt().max(1e-6);
        [x + dx / length * 0.25, y + dy / length * 0.25]
    })
}

/// A model point as the tilted camera sees it: x right, y down, z toward
/// the viewer.
fn look([x, y, z]: Point) -> Point {
    let (sin, cos) = PITCH.sin_cos();
    [x, y * cos + z * sin, z * cos - y * sin]
}

/// The face's unit normal, pointing away from the spark's centre.
fn outward(points: &[Point; 3]) -> Point {
    let sub = |a: Point, b: Point| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (u, v) = (sub(points[1], points[0]), sub(points[2], points[0]));
    let mut n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let centre = add(add(points[0], points[1]), points[2]);
    if dot(n, centre) < 0.0 {
        n = n.map(|c| -c);
    }
    unit(n)
}

/// How lit a face turned to `normal` looks, 0 to 1: soft light from the
/// top left and a small sharp glint where it catches it.
fn shade(normal: Point) -> f32 {
    let key = unit([-0.5, -0.7, 0.6]);
    let half = unit([key[0], key[1], key[2] + 1.0]);
    let diffuse = dot(normal, key).max(0.0);
    let glint = dot(normal, half).max(0.0).powi(24);
    (0.08 + 0.6 * diffuse + 0.5 * glint).min(1.0)
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
    /// the spark never blinks out.
    #[test]
    fn a_frame_always_shows() {
        let boundaries =
            (0..FRAMES * 3).map(|slot| f64::from(REPEAT) * slot as f64 / FRAMES as f64);
        let between = (0..997).map(|step| f64::from(step) * 0.0071);
        for t in boundaries.chain(between) {
            let shown = (0..FRAMES)
                .filter(|frame| {
                    let cycle = Cycle {
                        period: REPEAT,
                        phase: *frame as f32 / FRAMES as f32,
                        stops: &SLOT,
                        still: 0.0,
                        fps: 30.0,
                    };
                    cycle.sample(t) > 0.999
                })
                .count();
            assert!((1..=2).contains(&shown), "{shown} frames at {t}");
        }
    }
}
