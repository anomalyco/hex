//! The download pill and the inline language capsule button.
//!
//! [`pill`] carries [`PILL`] as a shared element, so the centerpiece loader on
//! `"Setting up Hex"` flies smoothly up to the top bar during the demo and
//! permissions, then back to centre stage for `"Hex is ready"`.

use rune::{Align, El, Spring, Transition, el, icon, row, text};

use super::super::controls::Paint;
use super::fixture::{Download, Language};
use super::hex_meter::hex_meter;

/// Shared-element id carried by the thin single-hexagon mark across the
/// centerpiece loader, top-bar pill, and Ready landing.
pub(crate) const PILL: &str = "hex-mark";
/// Zero-bounce flight spring (`0.42 s / 0.0`, Abattoir motion guide).
pub(crate) const FLY: Spring = Spring::new(0.42, 0.0);
const HEIGHT: f32 = 30.0;

pub(crate) fn language_label(language: Language) -> &'static str {
    match language {
        Language::English => "English",
        Language::Other => "French",
    }
}

/// Top-bar download capsule: only the `16 pt` hexagon mark carries [`PILL`],
/// so flying between the `124 pt` centerpiece and the top bar scales crisp
/// vector geometry without stretching pill text.
pub(crate) fn pill(p: Paint, download: Download, closed: f32) -> El {
    let k = p.k;
    let done = download.done();
    let status = if done {
        row()
            .key("ready")
            .gap(4.0)
            .center_y()
            .child(
                icon(k.glyph.check_small, 14.0)
                    .key("check")
                    .color(p.success()),
            )
            .child(
                text("Ready")
                    .key("ready-label")
                    .font(k.ty.small_medium)
                    .color(p.success()),
            )
    } else {
        text(format!("{}%", (download.fraction() * 100.0).round() as u32))
            .key("percent")
            .font(k.ty.small.features("'tnum' 1"))
            .color(p.muted())
            .w(28.0)
            .align(Align::End)
    };
    let pill = row()
        .key("download-pill-shell")
        .h(HEIGHT)
        .pl(9.0)
        .pr(11.0)
        .gap(8.0)
        .center_y()
        .radius(HEIGHT * 0.5)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .transition(super::landing::arrives())
        .child(
            el().key(PILL)
                .size(16.0, 16.0)
                .matched(PILL)
                .motion(FLY)
                .child(hex_meter(p, "meter", 16.0, download.fraction(), closed)),
        )
        .child(
            text(language_label(download.language))
                .key("label")
                .font(k.ty.small_medium)
                .color(p.text()),
        )
        .child(status.transition(Transition::opacity()));
    k.elevate(pill, &k.elevation.raised)
}

/// Interactive capsule button naming the model's language inside
/// `"Downloading [ English ▾ ] speech model"`.
pub(crate) fn language_capsule<S: 'static>(
    p: Paint,
    language: Language,
    press: impl Fn(&mut S) + 'static,
) -> El {
    let k = p.k;
    let c = &k.c;
    let mut cap = row()
        .key("language-capsule")
        .role("button")
        .label(format!("Language, {}", language_label(language)))
        .focusable(true)
        .focus_ring(1.5, 1.0, c.border_focus)
        .h(26.0)
        .pl(10.0)
        .pr(7.0)
        .gap(4.0)
        .center_y()
        .radius(13.0)
        .fill(c.surface_raised)
        .border(p.hair, c.border_base)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(press)
        .child(
            text(language_label(language))
                .key("name")
                .font(k.ty.ui_medium.lh(16.0))
                .color(p.text()),
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

/// Empty placeholder matching [`pill`]'s height so surrounding layout stays anchored.
pub(crate) fn slot() -> El {
    el().key("pill-slot").h(HEIGHT)
}
