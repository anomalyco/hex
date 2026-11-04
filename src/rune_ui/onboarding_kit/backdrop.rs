//! What could sit behind `"Setting up Hex"` so the first screen isn't an
//! empty page: a phosphor dot field rippling out of the HUD, things people
//! say drifting up past it, or a horizon of scope traces. Prototypes, each
//! a story variant, until one is picked; the first run shows none.

use rune::{El, LineCap, Shape, TextStyle, Transition, canvas, el, text};

use super::super::controls::Paint;
use super::stories::{Onboard, Step};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Backdrop {
    #[default]
    None,
    Field,
    Words,
    Horizon,
}

const SIZE: [f32; 2] = [980.0, 760.0];
/// The first screen's HUD, where the field ripples from.
const HUD: [f32; 2] = [490.0, 308.0];

/// Scenes: `HEX_BACKDROP` (`field`, `words`, or `horizon`) behind the
/// first screen, `HEX_BACKDROP_SECS` into its motion. Unset, nothing.
pub(in crate::rune_ui) fn fixture(ui: &mut super::super::Ui) {
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    s.backdrop = match std::env::var("HEX_BACKDROP").as_deref() {
        Ok("field") => Backdrop::Field,
        Ok("words") => Backdrop::Words,
        Ok("horizon") => Backdrop::Horizon,
        _ => return,
    };
    s.now += std::env::var("HEX_BACKDROP_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
}

/// `s`'s backdrop behind the first screen, fading out as it moves on.
/// Reduce Motion holds it still.
pub(super) fn backdrop(p: Paint, s: &Onboard) -> Option<El> {
    if !matches!(s.step, Step::Pick | Step::Started) {
        return None;
    }
    let t = if s.reduced.get() { 0.0 } else { s.now as f32 };
    let inner = match s.backdrop {
        Backdrop::None => return None,
        Backdrop::Field => field(p, t),
        Backdrop::Words => words(p, t),
        Backdrop::Horizon => horizon(p, t),
    };
    Some(
        el().key("backdrop")
            .absolute(0.0, 0.0)
            .size(SIZE[0], SIZE[1])
            .transition(Transition::opacity())
            .child(inner),
    )
}

fn smoothstep(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// A grid of phosphor dots, the HUD's own language, lit by rings of sound
/// spreading out of the HUD and fading toward the window's edges.
fn field(p: Paint, t: f32) -> El {
    const GAP: f32 = 22.0;
    const RINGS: usize = 3;
    const PERIOD: f32 = 3.2;
    const REACH: f32 = 560.0;
    let ink = p.text();
    let mut shape = Shape::new();
    let (cols, rows) = ((SIZE[0] / GAP) as usize + 1, (SIZE[1] / GAP) as usize + 1);
    let ox = (SIZE[0] - (cols - 1) as f32 * GAP) * 0.5;
    let oy = (SIZE[1] - (rows - 1) as f32 * GAP) * 0.5;
    for row in 0..rows {
        for col in 0..cols {
            let (x, y) = (ox + col as f32 * GAP, oy + row as f32 * GAP);
            let d = ((x - HUD[0]).powi(2) + ((y - HUD[1]) * 1.25).powi(2)).sqrt();
            let lit: f32 = (0..RINGS)
                .map(|k| {
                    let age = (t / PERIOD + k as f32 / RINGS as f32).fract();
                    let r = age * REACH;
                    (-((d - r) / 34.0).powi(2)).exp() * (1.0 - age)
                })
                .sum();
            let lit = lit.min(1.0);
            let edge = 1.0 - smoothstep((d - 80.0) / 520.0);
            let page = (((x - HUD[0]) / 190.0).powi(2) + ((y - 352.0) / 70.0).powi(2)).sqrt();
            let clear = 0.25 + 0.75 * smoothstep(page - 0.6);
            let a = (0.07 + 0.50 * lit) * (0.30 + 0.70 * edge) * clear;
            let r = 1.1 + 0.9 * lit;
            shape = shape.line([x, y], [x + 0.01, y], r * 2.0, LineCap::Round, ink.alpha(a));
        }
    }
    canvas(shape).key("field").size(SIZE[0], SIZE[1])
}

/// Things people say, drifting up in two columns either side of the page
/// at three depths, the way Hex will hear them.
fn words(p: Paint, t: f32) -> El {
    const SAID: [&str; 12] = [
        "running 5 min late",
        "¿Nos vemos a las 8?",
        "Ship it.",
        "Merci pour tout !",
        "can you send the deck?",
        "Love you, see you tonight",
        "LGTM, one nit",
        "Wir sind gleich da",
        "push the review to Thursday",
        "お疲れさまでした",
        "Grabbing coffee, want one?",
        "Fixed the flaky test",
    ];
    const COLUMN: f32 = 250.0;
    const MARGIN: f32 = 36.0;
    let ink = p.text();
    let mut out = el().key("words").absolute(0.0, 0.0).size(SIZE[0], SIZE[1]);
    let per_side = SAID.len() / 2;
    let span = SIZE[1] + 60.0;
    for (i, said) in SAID.iter().enumerate() {
        let right = i % 2 == 1;
        let slot = i / 2;
        let depth = [0.5, 1.0, 0.72][slot % 3];
        let size = 12.0 + 6.0 * depth;
        let width: f32 = said
            .chars()
            .map(|c| if c > '\u{3000}' { size } else { size * 0.52 })
            .sum();
        let x = if right {
            SIZE[0] - MARGIN - width.min(COLUMN)
        } else {
            MARGIN + (COLUMN - width).max(0.0) * 0.35 * (slot % 2) as f32
        };
        let speed = 10.0 + 8.0 * depth;
        let phase = (slot as f32 + if right { 0.5 } else { 0.0 }) / per_side as f32;
        let y = span - (t * speed + phase * span) % span - 30.0;
        let edge = smoothstep((y - 20.0) / 120.0) * smoothstep((SIZE[1] - 30.0 - y) / 120.0);
        let a = (0.10 + 0.16 * depth) * edge;
        out = out.child(
            text(said.to_string())
                .key(format!("said-{i}"))
                .absolute(x, y)
                .font(TextStyle::new(size, 520.0))
                .color(ink.alpha(a)),
        );
    }
    out
}

/// A horizon of scope traces across the bottom of the window, each nearer
/// one louder, rising out of the middle like the HUD's waveform laid flat
/// and stretched into the distance.
fn horizon(p: Paint, t: f32) -> El {
    const LINES: usize = 16;
    const POINTS: usize = 120;
    let ink = p.text();
    let ground = p.k.c.bg_deep;
    let mut shape = Shape::new();
    for line in 0..LINES {
        let near = line as f32 / (LINES - 1) as f32;
        let base = 520.0 + 220.0 * near.powf(1.6);
        let loud = 0.35 + 0.65 * near;
        let points: Vec<[f32; 2]> = (0..=POINTS)
            .map(|i| {
                let x = SIZE[0] * i as f32 / POINTS as f32;
                let u = (x - SIZE[0] * 0.5) / 210.0;
                let envelope = (-u * u).exp();
                let seed = line as f32 * 1.7;
                let wave = (x / 23.0 + t * 1.9 + seed).sin() * 0.5
                    + (x / 11.0 - t * 2.7 + seed * 2.3).sin() * 0.3
                    + (x / 47.0 + t * 0.8 + seed * 0.7).sin() * 0.2;
                [x, base - envelope * (18.0 + 26.0 * wave.abs()) * loud]
            })
            .collect();
        let mut under = points.clone();
        under.push([SIZE[0], SIZE[1]]);
        under.push([0.0, SIZE[1]]);
        shape = shape
            .fill(under, ground)
            .stroke(points, 1.2, ink.alpha(0.08 + 0.22 * near));
    }
    canvas(shape).key("horizon").size(SIZE[0], SIZE[1])
}
