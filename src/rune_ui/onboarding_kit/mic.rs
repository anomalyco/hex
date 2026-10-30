//! Microphone level bars (`mic_level`) for reply cards and the microphone capsule.

use rune::{El, LineCap, Shape, canvas};

use super::super::controls::Paint;

const BARS: usize = 5;
const WEIGHTS: [f32; BARS] = [0.55, 0.85, 1.0, 0.8, 0.5];

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
