//! A faceted gem that turns on its vertical axis, drawn in the theme's ink
//! like cut glass. Rune turns shapes only in the plane, so the gem is
//! projected ahead of time into one frame per step of a turn, and each frame
//! shows for its slot of the loop through a clock-driven opacity cycle: the
//! renderer flips between them with no view rebuilds. The cut repeats every
//! eighth of a turn, so one eighth is all the frames there are. Reduce
//! Motion holds the first.

use rune::{Color, Cycle, El, Shape, canvas, col};

use super::controls::Paint;

/// Facets around the gem; the cut repeats every `1 / SIDES` of a turn.
const SIDES: usize = 8;
/// Frames in one repeat of the cut.
const FRAMES: usize = 48;
/// Seconds for one repeat, so a whole turn takes `SIDES` times as long.
const REPEAT: f32 = 2.0;
/// How far the camera looks down onto the table, in radians.
const PITCH: f32 = 0.38;
/// Each frame is shown for its slot of the repeat and hidden for the rest.
/// A slot runs a hair past its end so rounding at the boundary never leaves
/// a moment with no frame; the next frame paints over the overlap.
const SLOT: [(f32, f32); 4] = [
    (0.0, 1.0),
    (1.0 / FRAMES as f32 + 1e-3, 1.0),
    (1.0 / FRAMES as f32 + 1e-3, 0.0),
    (1.0, 0.0),
];

/// The gem, `size` points square.
pub(super) fn gem(p: Paint, key: &str, size: f32) -> El {
    let ink = p.text();
    let ground = p.k.c.bg_base;
    let frames = (0..FRAMES).map(|frame| {
        let turn = frame as f32 / FRAMES as f32 * std::f32::consts::TAU / SIDES as f32;
        canvas(cut(turn, size, ground, ink, p.k.dark))
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

/// The gem turned by `turn` radians, as opaque facets shaded from `ground`
/// toward `ink`.
fn cut(turn: f32, size: f32, ground: Color, ink: Color, dark: bool) -> Shape {
    let ring = |radius: f32, height: f32, offset: f32| -> Vec<Point> {
        (0..SIDES)
            .map(|side| {
                let angle = turn + (side as f32 + offset) * std::f32::consts::TAU / SIDES as f32;
                [radius * angle.cos(), height, radius * angle.sin()]
            })
            .collect()
    };
    // y points down: the table on top, the girdle at its widest, the culet
    // at the bottom point.
    let table = ring(0.56, -0.62, 0.5);
    let crown = ring(1.0, -0.24, 0.0);
    let bezel = ring(1.0, -0.24, 0.5);
    let girdle = ring(1.0, -0.14, 0.0);
    let between = ring(1.0, -0.14, 0.5);
    let culet: Point = [0.0, 1.0, 0.0];
    let mut facets: Vec<Vec<Point>> = vec![table.clone()];
    for side in 0..SIDES {
        let next = (side + 1) % SIDES;
        facets.push(vec![table[side], crown[next], table[next]]);
        facets.push(vec![table[side], crown[side], bezel[side]]);
        facets.push(vec![table[side], bezel[side], crown[next]]);
        facets.push(vec![crown[side], girdle[side], between[side], bezel[side]]);
        facets.push(vec![bezel[side], between[side], girdle[next], crown[next]]);
        facets.push(vec![girdle[side], culet, between[side]]);
        facets.push(vec![between[side], culet, girdle[next]]);
    }
    let scale = size * 0.46;
    let centre = [size / 2.0, size / 2.0 + scale * 0.02];
    let project = |v: &Point| [centre[0] + v[0] * scale, centre[1] + v[1] * scale];
    let shade_of = |light: f32| {
        let amount = if dark {
            0.10 + 0.78 * light
        } else {
            0.8 - 0.66 * light
        };
        ground.mix(ink, amount.clamp(0.0, 1.0))
    };
    // A solid silhouette under the facets, so their antialiased shared
    // edges blend into the gem rather than the card.
    let outline: Vec<[f32; 2]> = facets
        .iter()
        .flatten()
        .map(|point| project(&look(*point)))
        .collect();
    let mut shape = Shape::new().fill(hull(outline), shade_of(0.4));
    for facet in facets {
        let view: Vec<Point> = facet.iter().map(|point| look(*point)).collect();
        let normal = outward(&view);
        if normal[2] <= 0.02 {
            continue;
        }
        let points: Vec<[f32; 2]> = view.iter().map(project).collect();
        shape = shape.fill(points, shade_of(shade(normal)));
    }
    shape
}

/// The convex hull of `points`, counter-clockwise (monotone chain).
fn hull(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f32; 2]> = Vec::new();
    for &point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper: Vec<[f32; 2]> = Vec::new();
    for &point in points.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A model point as the tilted camera sees it: x right, y down, z toward
/// the viewer.
fn look([x, y, z]: Point) -> Point {
    let (sin, cos) = PITCH.sin_cos();
    [x, y * cos + z * sin, z * cos - y * sin]
}

/// The facet's unit normal, pointing out of the gem (which contains the
/// origin).
fn outward(points: &[Point]) -> Point {
    let sub = |a: Point, b: Point| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (u, v) = (sub(points[1], points[0]), sub(points[2], points[0]));
    let mut n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let centre = points
        .iter()
        .fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
    if n[0] * centre[0] + n[1] * centre[1] + n[2] * centre[2] < 0.0 {
        n = n.map(|c| -c);
    }
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    n.map(|c| c / length)
}

/// How lit a facet facing `normal` looks, 0 to 1: soft light from the top
/// left, a dimmer fill from the lower right that the pavilion catches as it
/// turns, and a small sharp highlight where the key light glints.
fn shade(normal: Point) -> f32 {
    let dot = |a: Point, b: Point| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let key = unit([-0.45, -0.8, 0.55]);
    let fill = unit([0.7, 0.35, 0.6]);
    let half = unit([key[0], key[1], key[2] + 1.0]);
    let diffuse = dot(normal, key).max(0.0);
    let bounce = dot(normal, fill).max(0.0).powi(3);
    let glint = dot(normal, half).max(0.0).powi(40);
    (0.1 + 0.45 * diffuse + 0.4 * bounce + 0.7 * glint).min(1.0)
}

fn unit(v: Point) -> Point {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Some frame shows at every moment, the slot boundaries included, so
    /// the gem never blinks out.
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
