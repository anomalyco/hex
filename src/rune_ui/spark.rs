//! A sparkle that flips on its vertical axis: four points with curved sides,
//! each side rising from its edge to a peak at the centre, drawn in the
//! theme's ink so the light rolls across each point as it turns. It rests
//! face-on, then turns half a turn with an ease in and out, which brings it
//! back to the same face, once per example of the agent card's copy, so the
//! copy changes as it turns edge-on. Rune turns shapes only in the plane, so
//! each frame of the turn is rendered here: a smooth-shaded mesh, lit per
//! pixel and supersampled, so no facet edges or seams show. The window's
//! poll swaps the frame in from the clock. Reduce Motion holds it face-on.

use std::cell::Cell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

use rune::color::srgb_encode;
use rune::{Color, El, Pixels, col};

use super::controls::Paint;

/// Seconds for one rest and one flip: one example of the card's copy.
pub(super) const PERIOD: f32 = 3.6;
/// The share of the loop it rests face-on.
const REST: f32 = 0.6;
/// Seconds into the loop it turns edge-on, halfway through the flip.
pub(super) const EDGE_ON: f32 = PERIOD * (1.0 + REST) / 2.0;
/// Frames in the flip, the face-on frame included: 60 a second.
const FRAMES: usize = 86;
/// How far the side points reach, against the vertical points' 1.
const REACH: f32 = 0.8;
/// How close each curved side passes to the centre: the smaller, the
/// thinner the points.
const PINCH: f32 = 0.08;
/// How far the peak stands out of the sparkle's plane.
const BEVEL: f32 = 0.25;
/// Points along each curved side.
const SAMPLES: usize = 24;
/// How far the normals lean out at the edge.
const SLOPE: f32 = 0.9;
/// Rings of the mesh from the peak out to the edge.
const RINGS: usize = 12;
/// Samples per pixel along each axis.
const SUPERSAMPLE: usize = 4;

/// The sparkle's image, kept across view rebuilds so the poll can turn it.
#[derive(Default)]
pub(super) struct Spark {
    pixels: Pixels,
    /// How the view last drew it.
    look: Cell<Option<Look>>,
    /// The frame the image holds, and for which look.
    shown: Cell<Option<(Look, usize)>>,
    /// The clock at the last poll.
    clock: Cell<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Look {
    dark: bool,
    /// The image's side in pixels.
    pixels: u32,
    reduced: bool,
}

/// The sparkle, `size` points square.
pub(super) fn spark(state: &Spark, p: Paint, key: &str, size: f32, reduced: bool) -> El {
    let look = Look {
        dark: p.k.dark,
        pixels: (size / p.hair).round().max(1.0) as u32,
        reduced,
    };
    state.look.set(Some(look));
    show(state, look, frame(look, state.clock.get()));
    col().key(key).size(size, size).pixels(&state.pixels)
}

/// Shows the frame for `clock`; returns how long until the image next
/// changes.
pub(super) fn poll(state: &Spark, clock: f64) -> f64 {
    state.clock.set(clock);
    if let Some(look) = state.look.get() {
        show(state, look, frame(look, clock));
    }
    let into = into(clock);
    if into >= REST {
        1.0 / 60.0
    } else {
        f64::from((REST - into) * PERIOD)
    }
}

/// How far into its loop the sparkle is at `clock`, 0 to 1.
fn into(clock: f64) -> f32 {
    (clock / f64::from(PERIOD)).rem_euclid(1.0) as f32
}

/// The frame of the flip to show at `clock`.
fn frame(look: Look, clock: f64) -> usize {
    let into = into(clock);
    if look.reduced || into < REST {
        0
    } else {
        (((into - REST) / (1.0 - REST)) * FRAMES as f32) as usize % FRAMES
    }
}

fn show(state: &Spark, look: Look, frame: usize) {
    if state.shown.get() == Some((look, frame)) {
        return;
    }
    state.shown.set(Some((look, frame)));
    let rgba = cached(look, frame);
    state
        .pixels
        .set(look.pixels, look.pixels, rgba.as_ref().clone());
}

/// Rendered frames by theme, image side and frame.
type Rendered = HashMap<(bool, u32, usize), Arc<Vec<u8>>>;

/// Each frame is rendered once per look.
fn cached(look: Look, frame: usize) -> Arc<Vec<u8>> {
    static FRAMES_SEEN: OnceLock<Mutex<Rendered>> = OnceLock::new();
    let key = (look.dark, look.pixels, frame);
    let cache = FRAMES_SEEN.get_or_init(Default::default);
    if let Some(rgba) = cache.lock().unwrap().get(&key) {
        return rgba.clone();
    }
    let kit = rune_theme_opencode::kit(look.dark);
    let turn = PI * ease(frame as f32 / FRAMES as f32);
    let rgba = Arc::new(render(
        turn,
        look.pixels as usize,
        kit.c.bg_base,
        kit.c.text_strong,
        look.dark,
    ));
    let mut cache = cache.lock().unwrap();
    // Theme switches and scale changes leave old looks behind; a few are
    // plenty to keep.
    if cache.len() > FRAMES * 4 {
        cache.clear();
    }
    cache.insert(key, rgba.clone());
    rgba
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

/// Both faces of the sparkle as triangles. Each point's normal leans
/// straight out from the middle, more toward the edge, as on a smooth dome:
/// the light rolls across with no ridge between the points.
struct Mesh {
    points: Vec<Point>,
    normals: Vec<Point>,
    triangles: Vec<[usize; 3]>,
}

fn mesh() -> &'static Mesh {
    static MESH: OnceLock<Mesh> = OnceLock::new();
    MESH.get_or_init(|| {
        let edge = boundary();
        let around = edge.len();
        let mut points = Vec::new();
        let mut normals = Vec::new();
        let mut triangles = Vec::new();
        for side in [1.0, -1.0] {
            let peak = points.len();
            points.push([0.0, 0.0, side * BEVEL]);
            normals.push([0.0, 0.0, side]);
            let first = points.len();
            for b in &edge {
                let out = unit(*b);
                for k in 1..=RINGS {
                    let r = k as f32 / RINGS as f32;
                    points.push([b[0] * r, b[1] * r, side * BEVEL * (1.0 - r).powf(1.5)]);
                    let tilt = SLOPE * r.sqrt();
                    normals.push(unit([out[0] * tilt, out[1] * tilt, side]));
                }
            }
            let at = |s: usize, k: usize| first + (s % around) * RINGS + (k - 1);
            for s in 0..around {
                triangles.push([peak, at(s, 1), at(s + 1, 1)]);
                for k in 1..RINGS {
                    triangles.push([at(s, k), at(s, k + 1), at(s + 1, k + 1)]);
                    triangles.push([at(s, k), at(s + 1, k + 1), at(s + 1, k)]);
                }
            }
        }
        Mesh {
            points,
            normals,
            triangles,
        }
    })
}

/// The sparkle turned `turn` radians about its vertical axis, `side` pixels
/// square: straight-alpha sRGB, each face lit per pixel and shaded from
/// `ground` toward `ink`.
fn render(turn: f32, side: usize, ground: Color, ink: Color, dark: bool) -> Vec<u8> {
    let mesh = mesh();
    let (sin, cos) = turn.sin_cos();
    let spin = |[x, y, z]: Point| [x * cos + z * sin, y, z * cos - x * sin];
    let fine = side * SUPERSAMPLE;
    let (scale, middle) = (fine as f32 * 0.48, fine as f32 / 2.0);
    let screen: Vec<Point> = mesh
        .points
        .iter()
        .map(|p| {
            let v = spin(*p);
            [middle + v[0] * scale, middle + v[1] * scale, v[2]]
        })
        .collect();
    let normals: Vec<Point> = mesh.normals.iter().map(|n| spin(*n)).collect();
    let mut depth = vec![f32::NEG_INFINITY; fine * fine];
    let mut normal = vec![[0.0f32; 3]; fine * fine];
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| screen[i]);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-6 {
            continue;
        }
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(fine);
        let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
        let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(fine);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let i = y * fine + x;
                if z <= depth[i] {
                    continue;
                }
                depth[i] = z;
                let [na, nb, nc] = t.map(|i| normals[i]);
                normal[i] = [0, 1, 2].map(|k| w0 * na[k] + w1 * nb[k] + w2 * nc[k]);
            }
        }
    }
    let mut rgba = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            let (mut sum, mut covered) = ([0.0f32; 3], 0usize);
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let i = (y * SUPERSAMPLE + sy) * fine + x * SUPERSAMPLE + sx;
                    if depth[i] == f32::NEG_INFINITY {
                        continue;
                    }
                    let mut n = unit(normal[i]);
                    // Seen from behind, a face shows its other side.
                    if n[2] < 0.0 {
                        n = n.map(|c| -c);
                    }
                    let light = shade(n);
                    let amount = if dark {
                        0.3 + 0.7 * light
                    } else {
                        0.95 - 0.5 * light
                    };
                    let color = ground.mix(ink, amount.clamp(0.0, 1.0));
                    sum = add(sum, [color.r, color.g, color.b]);
                    covered += 1;
                }
            }
            if covered == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            let n = covered as f32;
            rgba.extend(sum.map(|c| byte(srgb_encode(c / n))));
            rgba.push(byte(n / (SUPERSAMPLE * SUPERSAMPLE) as f32));
        }
    }
    rgba
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

    /// Face-on, the sparkle fills its middle and leaves its corners clear,
    /// with soft edges between.
    #[test]
    fn a_frame_covers_the_sparkle_with_soft_edges() {
        let side = 40;
        let rgba = render(0.0, side, Color::BLACK, Color::WHITE, true);
        let alpha = |x: usize, y: usize| rgba[(y * side + x) * 4 + 3];
        assert_eq!(alpha(side / 2, side / 2), 255);
        assert_eq!(alpha(1, 1), 0);
        assert!(rgba.chunks(4).any(|p| p[3] > 0 && p[3] < 255));
    }
}
