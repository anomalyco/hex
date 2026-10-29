//! Microphone level bars and the Hex voice scope (`t573` single-hexagon
//! outline + live scrolling speech wave, with crisp geometric echo rings
//! instead of a blurred radial glow).

use rune::{El, LineCap, Shape, Spring, canvas};

use super::super::controls::Paint;
use super::hex_meter::{ACTIVE_GLYPH, wave_pts};
use super::stories::Onboard;

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

/// Where spoken words leave from: the thin single-hexagon outline with a live
/// speech wave inside and crisp geometric hexagon ripples around it while
/// `live`, centred on `centre`. No blurred background glow.
pub(crate) fn mouth(p: Paint, key: &str, centre: [f32; 2], d: f32, s: &Onboard, live: bool) -> El {
    let spec = ACTIVE_GLYPH;
    let bg = p.k.c.bg_deep;
    let reduced = s.reduced.get();
    let now = s.now;
    let (average, peak) = if live { s.meter } else { (0.0, 0.0) };
    // Extra room inside the canvas so expanding echo rings never clip.
    let box_d = d * 1.5;
    let base_scale = d / box_d;
    let breath = if live && !reduced {
        1.0 + 0.05 * peak.clamp(0.0, 1.0)
    } else {
        1.0
    };

    let mut shape = Shape::new();

    // Two clean concentric hairline hexagon ripples while speaking.
    if live && !reduced {
        for i in 0..2 {
            let cycle = ((now * 0.95 + i as f64 * 0.5).fract()) as f32;
            let expand = base_scale * (1.06 + 0.34 * cycle);
            let fade = (1.0 - cycle).powi(2) * (0.22 + 0.42 * average.min(1.0));
            if fade > 0.015 {
                let mut echo = spec.poly(box_d, expand);
                echo.pop();
                let ink = bg.mix(p.k.c.border_focus, fade.min(0.65));
                shape = shape.stroke_with(echo, true, 1.0, LineCap::Round, ink);
            }
        }
    }

    // Main thin single-hexagon outline.
    let w = spec.stroke_width(d);
    let mut ring = spec.poly(box_d, base_scale * breath);
    ring.pop();
    let ring_ink = if live {
        p.text()
    } else {
        bg.mix(p.k.c.border_strong, 0.9)
    };
    shape = shape.stroke_with(ring, true, w, LineCap::Round, ring_ink);

    // Interior speech waveform (`t573` scope): calm horizon at rest, dancing
    // live wave while held.
    let amp = if live {
        if reduced {
            0.45
        } else {
            (0.18 + 1.05 * average).clamp(0.18, 1.05)
        }
    } else {
        0.06
    };
    let phase = if live && !reduced {
        -(now as f32) * (3.6 + 4.2 * average)
    } else {
        0.0
    };
    let wave_ink = if live {
        p.text()
    } else {
        bg.mix(p.muted(), 0.55)
    };
    let offset = (box_d - d) * 0.5;
    let wave: Vec<[f32; 2]> = wave_pts(d, amp, phase, 1.0, true)
        .into_iter()
        .map(|[x, y]| [x + offset, y + offset])
        .collect();
    shape = shape.stroke(wave, (w * 0.82).max(1.25), wave_ink);

    canvas(shape)
        .key(key.to_string())
        .absolute(centre[0] - box_d * 0.5, centre[1] - box_d * 0.5)
        .size(box_d, box_d)
        .motion(Spring::new(0.18, 0.0))
}
