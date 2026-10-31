//! Microphone capsule button on the try screen (`t421`), sized and styled so
//! it pairs cleanly beside `[ ⌥  Change shortcut ]` on the same horizontal
//! spine.

use rune::{El, Spring, Transition, icon, row, text};

use super::super::controls::{DESCRIPTION, Paint};

const H: f32 = 28.0;
const NAME: &str = "MacBook Pro Microphone";

/// Standalone microphone line used by the earlier option stories.
pub(crate) fn mic_line(p: Paint, shown: bool, peak: Option<f32>) -> El {
    row()
        .key("mic-line")
        .h(H)
        .center()
        .children(shown.then(|| mic_capsule(p, peak).transition(Transition::opacity())))
}

/// Tactile `28 pt` microphone selector capsule with live 3-bar input meter.
pub(crate) fn mic_capsule(p: Paint, peak: Option<f32>) -> El {
    let k = p.k;
    let c = &k.c;
    let lvl = level(peak);
    let mut cap = row()
        .key("mic-capsule")
        .role("button")
        .label(format!("Microphone, {NAME}"))
        .focusable(true)
        .focus_ring(1.5, 1.0, c.border_focus)
        .h(H)
        .pl(10.0)
        .pr(8.0)
        .gap(7.0)
        .center_y()
        .radius(H * 0.5)
        .fill(c.surface_raised)
        .border(p.hair, c.border_weak)
        .hover(c.hover)
        .press(c.pressed)
        .child(bars(p, lvl))
        .child(
            text(NAME)
                .key("name")
                .font(DESCRIPTION)
                .color(p.muted())
                .truncate_fade(),
        )
        .child(
            icon(k.glyph.chevron_down, 12.0)
                .key("chevron")
                .color(c.icon_muted),
        );
    for shadow in k.shadow_layers(&k.elevation.button_neutral) {
        cap = cap.box_shadow(shadow);
    }
    cap
}

fn bars(p: Paint, level: f32) -> El {
    const WEIGHTS: [f32; 3] = [0.6, 1.0, 0.75];
    let mut out = rune::el().key("level").size(12.0, 14.0);
    for (i, w) in WEIGHTS.into_iter().enumerate() {
        let h = 2.0 + w * 3.5 + level * w * 7.5;
        out = out.child(
            rune::el()
                .key(format!("b{i}"))
                .absolute(1.0 + i as f32 * 4.0, (14.0 - h) * 0.5)
                .size(2.0, h)
                .motion(Spring::new(0.12, 0.0))
                .radius(1.0)
                .fill(if level > 0.0 { p.text() } else { p.faint() }),
        );
    }
    out
}

fn level(peak: Option<f32>) -> f32 {
    peak.map_or(0.0, |peak| (peak * 3.0).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_rests_until_hex_listens_then_follows_speech() {
        assert_eq!(level(None), 0.0);
        assert!((level(Some(0.1)) - 0.3).abs() < 1e-6);
        assert_eq!(level(Some(0.9)), 1.0);
    }
}
