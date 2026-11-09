//! What happens to the whole stage at once: the camera pushing in and
//! shaking on autocorrect's hit, the autocorrect title slamming on, and the
//! `"Voice-first mode"` call as `"With Hex"` takes over.

use rune::icon;
use rune_kit::glyphs;

use super::*;

/// The autocorrect title: how long it takes to land, and how far it shakes.
const SLAM_IN: f64 = 0.09;
const SLAM_SHAKE: f32 = 9.0;
const SLAM_COPY: &str = "AUTOCORRECT";

/// The camera's push in on the draft as autocorrect's slow motion starts,
/// how much further the hit punches it, and how hard the hit shakes it.
const PUSH_IN: f32 = 0.16;
const PUNCH_IN: f32 = 0.09;
const CAMERA_SHAKE: f32 = 13.0;
/// How long the camera takes to pull back out once time comes back.
const PULL_OUT: f64 = 0.32;

/// The camera over the stage at scene time `t`: how far it has zoomed in on
/// the draft, and its shake, in points. Autocorrect's slow motion pushes in
/// on the field, the hit punches further in and shakes it like a blow
/// landing, and the send pulls back out. Reduce Motion keeps it still.
pub(super) fn camera(s: &Onboard, t: f64) -> (f32, [f32; 2]) {
    if s.reduced.get() {
        return (1.0, [0.0, 0.0]);
    }
    let Some(&(from, to)) = s
        .scene
        .slows()
        .iter()
        .find(|&&(from, to)| from <= t && t < to + PULL_OUT)
    else {
        return (1.0, [0.0, 0.0]);
    };
    let push =
        smoothstep(((t - from) / 0.5) as f32) * (1.0 - smoothstep(((t - to) / PULL_OUT) as f32));
    let Some(hit) = s.scene.slams().find(|&at| from <= at && at <= to) else {
        return (1.0 + PUSH_IN * push, [0.0, 0.0]);
    };
    let since = t - hit;
    if since < 0.0 {
        return (1.0 + PUSH_IN * push, [0.0, 0.0]);
    }
    // A fast blow in, then the punch eases back to the push.
    let punch = (1.0 - (-since / 0.02).exp()) as f32 * (-since / 0.22).exp() as f32;
    let trauma = (-since / 0.16).exp() as f32;
    let wobble = |f: f64, phase: f64| (std::f64::consts::TAU * (f * since + phase)).sin() as f32;
    (
        1.0 + PUSH_IN * push + PUNCH_IN * punch,
        [
            CAMERA_SHAKE * trauma * (0.7 * wobble(19.0, 0.1) + 0.3 * wobble(31.0, 0.6)),
            CAMERA_SHAKE * trauma * (0.6 * wobble(23.0, 0.4) + 0.2 * wobble(41.0, 0.0)),
        ],
    )
}

/// Where the camera zooms to: the draft's field, which stays put.
pub(super) fn camera_focus() -> [f32; 2] {
    Layout::chosen()
        .card()
        .map([CENTRE_X, card_bottom(WITHOUT_LINES) - 24.0])
}

/// The autocorrect title slamming onto the stage like a fighting game's
/// round call: it drops in oversized, punches past its size, shakes the
/// stage with a flash, holds, then snaps away as time comes back. Reduce
/// Motion fades it in and out where it lands.
pub(super) fn slam(p: Paint, s: &Onboard, t: f64) -> Option<El> {
    let since = s.scene.slam(t)?;
    let reduced = s.reduced.get();
    let k = p.k;
    let u = (since / SLAM_IN).min(1.0) as f32;
    let (scale, shake) = if reduced {
        (1.0, [0.0, 0.0])
    } else {
        // In from 2.4x, overshooting to 0.9x, then a damped punch to rest.
        let settle = (since - SLAM_IN).max(0.0);
        let punch = if since < SLAM_IN {
            2.4 - 1.5 * smoothstep(u)
        } else {
            1.0 - 0.1
                * (std::f64::consts::TAU * settle / 0.16).cos() as f32
                * (-settle / 0.09).exp() as f32
        };
        let decay = (-settle / 0.12).exp() as f32 * (since >= SLAM_IN) as u8 as f32;
        let wobble =
            |f: f64, phase: f64| (std::f64::consts::TAU * (f * settle + phase)).sin() as f32;
        (
            punch,
            [
                SLAM_SHAKE * decay * wobble(23.0, 0.0),
                0.6 * SLAM_SHAKE * decay * wobble(17.0, 0.3),
            ],
        )
    };
    let opacity = if reduced {
        smoothstep(u)
    } else {
        u.min(1.0).sqrt()
    };
    let flash = if reduced {
        0.0
    } else {
        0.55 * (-(since - SLAM_IN).max(0.0) / 0.07).exp() as f32
            * (since >= SLAM_IN * 0.6) as u8 as f32
    };
    let title = text(SLAM_COPY.to_string())
        .key("slam-copy")
        .font(TextStyle::new(52.0, 900.0).lh(56.0).tracking(-1.0))
        .color(k.c.submit_fg);
    let plate = row()
        .key("slam-plate")
        .px(26.0)
        .py(10.0)
        .radius(14.0)
        .continuous_corners()
        .fill(k.c.bg_accent)
        .shadow(8.0, 24.0, k.c.black.alpha(0.55))
        .rotate(-0.07)
        .child(title);
    Some(
        el().key("slam")
            .absolute(0.0, 0.0)
            .size(STAGE_W, STAGE_H)
            .instant_changes()
            // It snaps away as time comes back, not fades.
            .transition(Transition::opacity().springs(SpringPair::new(
                Spring::new(0.05, 0.0),
                Spring::new(0.04, 0.0),
            )))
            .child(
                el().key("slam-flash")
                    .absolute(0.0, 0.0)
                    .size(STAGE_W, STAGE_H)
                    .fill(k.c.white)
                    .opacity(flash),
            )
            .child(
                row()
                    .key("slam-stage")
                    .absolute(0.0, 0.0)
                    .size(STAGE_W, STAGE_H * 0.62)
                    .center()
                    .child(
                        plate
                            .scale(scale)
                            .offset(shake[0], shake[1])
                            .opacity(opacity),
                    ),
            ),
    )
}

/// How long the mode call takes to land.
const CALL_IN: f64 = 0.14;
/// `"Voice-first mode"` lands as the rewind finishes, then shrinks into the
/// HUD rising behind it; its chord rings as the HUD rises.
pub(crate) const VOICE_CALL_AT: f64 = REWIND * 0.75;
const VOICE_CALL_HOLD: f64 = 0.75;
/// How long the voice call takes to shrink into the HUD.
const INTO_HUD: f64 = 0.24;
// It has landed before the HUD rises, and gone before the key goes down.
const _: () = assert!(VOICE_CALL_AT + CALL_IN < HUD_RISES);
const _: () = assert!(VOICE_CALL_AT + VOICE_CALL_HOLD < BEFORE_KEY);

/// `"Voice-first mode"` blooming in across the keyboard as `"With Hex"`
/// rewinds, then shrinking into the HUD. `"Without Hex"` has no call: the
/// heading's chip already names the keyboard. Reduce Motion fades it in and
/// out where it sits.
pub(super) fn mode_call(p: Paint, s: &Onboard) -> Option<El> {
    let reduced = s.reduced.get();
    let since = hex_time(s)? - VOICE_CALL_AT;
    if !(0.0..VOICE_CALL_HOLD).contains(&since) {
        return None;
    }
    let u = smoothstep((since / CALL_IN) as f32);
    let gone = smoothstep(((since - (VOICE_CALL_HOLD - INTO_HUD)) / INTO_HUD) as f32);
    let (scale, opacity, lift) = if reduced {
        (1.0, u * (1.0 - gone), 0.0)
    } else {
        // Blooms in past its size, settles, then shrinks into the HUD.
        let settle = (since - CALL_IN).max(0.0);
        let bloom = if since < CALL_IN {
            0.6 + 0.48 * u
        } else {
            1.0 + 0.08 * (-settle / 0.12).exp() as f32
        };
        (bloom * (1.0 - 0.8 * gone), u * (1.0 - gone), gone * 24.0)
    };
    let k = p.k;
    let ink = k.c.submit_fg;
    let plate = row()
        .key("voice-call")
        .px(22.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(14.0)
        .continuous_corners()
        .fill(k.c.bg_accent)
        .border(p.hair, k.c.border_base)
        .shadow(6.0, 26.0, k.c.bg_accent.alpha(0.6))
        .child(icon(&glyphs::MIC, 26.0).key("glyph").color(ink))
        .child(
            text("VOICE-FIRST MODE".to_string())
                .key("copy")
                .font(TextStyle::new(30.0, 900.0).lh(34.0).tracking(-0.5))
                .color(ink),
        );
    Some(
        el().key("mode-call")
            .absolute(0.0, hud_centre()[1] - 40.0)
            .size(STAGE_W, 80.0)
            .center()
            .instant_changes()
            .motion_intent(if reduced {
                MotionIntent::NONE
            } else {
                MotionIntent::KICK
            })
            .child(plate.scale(scale).offset(0.0, lift).opacity(opacity)),
    )
}

/// The mode the heading names beside its title: the keyboard while
/// `"Without Hex"` plays, the voice once `"With Hex"` takes over.
pub(super) fn mode_chip(p: Paint, voice: bool) -> El {
    let k = p.k;
    let (glyph, copy, fill, ink) = if voice {
        (
            &glyphs::MIC,
            "Voice-first mode",
            k.c.bg_accent,
            k.c.submit_fg,
        )
    } else {
        (
            &glyphs::KEYBOARD,
            "Keyboard mode",
            k.c.surface_raised,
            p.muted(),
        )
    };
    row()
        .key(if voice { "voice-mode" } else { "keyboard-mode" })
        .h(24.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(12.0)
        .continuous_corners()
        .fill(fill)
        .border(p.hair, k.c.border_weak)
        .child(icon(glyph, 14.0).key("glyph").color(ink))
        .child(
            text(copy.to_string())
                .key("copy")
                .font(TextStyle::new(12.0, 650.0).lh(16.0))
                .color(ink),
        )
}

/// The laptop layout's lid: a bezel round the conversation, its screen.
pub(super) fn laptop_lid(p: Paint) -> El {
    let k = p.k;
    let [x0, y0, x1, y1] = Layout::Laptop.laptop_lid();
    let lid = el()
        .key("laptop-lid")
        .absolute(x0, y0)
        .size(x1 - x0, y1 - y0)
        .radius(22.0)
        .continuous_corners()
        .fill(k.c.bg_deep)
        .border(p.hair, k.c.border_base)
        .shadow(10.0, 30.0, k.c.black.alpha(0.45))
        .child(
            el().key("camera-dot")
                .absolute((x1 - x0) * 0.5 - 2.5, LAPTOP_BEZEL * 0.5 - 2.5)
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(k.c.border_strong),
        );
    lid.transition(Transition::opacity())
}

/// The laptop layout's deck under the keyboard, seen from above: wider at
/// the front edge, with the hinge along its back.
pub(super) fn laptop_deck(p: Paint) -> El {
    let k = p.k;
    let [x0, _, x1, y1] = Layout::Laptop.laptop_lid();
    let top = y1 + 2.0;
    let bottom = STAGE_H;
    let flare = 22.0;
    let deck = rune::Shape::new()
        .fill(
            vec![
                [x0 - 4.0, top],
                [x1 + 4.0, top],
                [x1 + flare, bottom],
                [x0 - flare, bottom],
            ],
            k.c.bg_base,
        )
        .fill(
            vec![
                [x0 - 4.0, top],
                [x1 + 4.0, top],
                [x1 + 5.0, top + 3.0],
                [x0 - 5.0, top + 3.0],
            ],
            k.c.border_strong,
        );
    rune::canvas(deck)
        .key("laptop-deck")
        .absolute(0.0, 0.0)
        .size(STAGE_W, STAGE_H)
        .transition(Transition::opacity())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autocorrect_pushes_in_hits_with_a_shake_and_pulls_back_out() {
        let s = Onboard::new(Flow::Versus);
        let &(from, to) = s.scene.slows().first().expect("autocorrect slows time");
        let hit = s.scene.slams().next().expect("and hits");
        assert_eq!(camera(&s, from - 0.1), (1.0, [0.0, 0.0]));
        let (pushed, still) = camera(&s, hit - 0.01);
        assert!(pushed > 1.05 && still == [0.0, 0.0], "{pushed}");
        let (punched, shake) = camera(&s, hit + 0.05);
        assert!(punched > pushed, "{punched} > {pushed}");
        assert!(shake[0].abs() + shake[1].abs() > 2.0, "{shake:?}");
        assert_eq!(camera(&s, to + PULL_OUT), (1.0, [0.0, 0.0]));
        let s = Onboard::new(Flow::Versus);
        s.reduced.set(true);
        assert_eq!(camera(&s, hit + 0.05), (1.0, [0.0, 0.0]), "Reduce Motion");
    }
}
