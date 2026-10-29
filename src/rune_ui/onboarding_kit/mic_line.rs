//! The microphone choice on each option's try screen (t421): the current
//! tour's "Give it a try" line, its menu button showing the microphone
//! and its live level. It's part of trying it, never a step of its own.

use rune::{El, Transition, row};

use super::super::controls::Paint;
use super::super::onboarding::mic_trigger;

/// The line's height on the current tour.
const H: f32 = 24.0;
/// The microphone the stories record from.
const NAME: &str = "MacBook Pro Microphone";

/// The line, always `H` tall, with the picker while `shown`. `peak` is
/// the meter's, 0 to 1, while Hex is listening.
pub(crate) fn mic_line(p: Paint, shown: bool, peak: Option<f32>) -> El {
    row().key("mic-line").h(H).center().children(shown.then(|| {
        row()
            .key("tour-mic")
            .h(H)
            .center_y()
            .transition(Transition::opacity())
            .child(mic_trigger(p, NAME, level(peak), false, false))
    }))
}

/// The tour's level: three times the peak, in six steps.
fn level(peak: Option<f32>) -> f32 {
    peak.map_or(0.0, |peak| {
        ((peak * 3.0).clamp(0.0, 1.0) * 6.0).round() / 6.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_rests_until_hex_listens_then_steps() {
        assert_eq!(level(None), 0.0);
        assert_eq!(level(Some(0.1)), 2.0 / 6.0);
        assert_eq!(level(Some(0.9)), 1.0);
    }
}
