//! The microphone's level as five rounded bars, from the meter events the
//! HUD already receives (`DictationIndicatorEvent::Meter`). The middle bars
//! follow the peak, the outer ones the average, so speech reads as a shape
//! rather than one bouncing block.

use rune::{El, LineCap, Shape, canvas, el};

use super::super::controls::Paint;
use super::hex_meter::hex_meter;

const BARS: usize = 5;
/// Each bar's share of the meter: the peak in the middle, the average out.
const WEIGHTS: [f32; BARS] = [0.55, 0.85, 1.0, 0.8, 0.5];

/// `average` and `peak` are 0 to 1. `height` sets the tallest bar; idle
/// bars rest as dots.
pub(crate) fn mic_level(
    p: Paint,
    key: &str,
    height: f32,
    average: f32,
    peak: f32,
    live: bool,
) -> El {
    let stroke = (height / 5.0).clamp(2.0, 4.0);
    let gap = stroke * 1.25;
    let width = BARS as f32 * stroke + (BARS - 1) as f32 * gap;
    let color = if live { p.text() } else { p.faint() };
    let mut shape = Shape::new();
    for (i, weight) in WEIGHTS.into_iter().enumerate() {
        let level = if i == BARS / 2 { peak } else { average };
        let reach = (height - stroke) * (level * weight).clamp(0.0, 1.0);
        let x = stroke / 2.0 + i as f32 * (stroke + gap);
        let middle = height / 2.0;
        shape = shape.line(
            [x, middle - reach / 2.0],
            [x, middle + reach / 2.0 + 0.01],
            stroke,
            LineCap::Round,
            color,
        );
    }
    canvas(shape).key(key).size(width, height)
}

/// Where the words leave from: the Hex mark in a disc `d` across, centred
/// on `centre`, swelling with the meter while `live` under a warm glow, as
/// the site's face lights as it talks.
pub(crate) fn mouth(
    p: Paint,
    key: &str,
    centre: [f32; 2],
    d: f32,
    meter: (f32, f32),
    live: bool,
) -> El {
    let k = p.k;
    let (average, peak) = if live { meter } else { (0.0, 0.0) };
    let glow_d = d * 1.9;
    let glow = el()
        .key("glow")
        .absolute((d - glow_d) / 2.0, (d - glow_d) / 2.0)
        .size(glow_d, glow_d)
        .radius(glow_d / 2.0)
        .fill(super::flight::ember(0.55).alpha(if k.dark { 0.5 } else { 0.35 }))
        .blur(d * 0.28)
        .opacity(if live {
            (0.25 + 1.4 * average).min(1.0)
        } else {
            0.0
        })
        .opacity_motion(rune::Spring::new(0.12, 0.0));
    let disc = el()
        .key("disc")
        .absolute(0.0, 0.0)
        .size(d, d)
        .radius(d / 2.0)
        .center()
        .fill(k.c.surface_raised)
        .border(
            p.hair,
            if live {
                k.c.border_focus
            } else {
                k.c.border_weak
            },
        )
        .child(
            hex_meter(p, "mark", d * 0.46, 1.0, 1.0)
                .scale(1.0 + 0.16 * peak.min(1.0))
                .motion(rune::Spring::new(0.08, 0.0)),
        );
    el().key(key.to_string())
        .absolute(centre[0] - d / 2.0, centre[1] - d / 2.0)
        .size(d, d)
        .child(glow)
        .child(disc)
}
