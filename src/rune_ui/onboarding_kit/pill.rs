//! The download pill and the inline language capsule button.
//!
//! [`pill`] carries [`PILL`] as a shared element, so the centerpiece loader on
//! `"Setting up Hex"` flies smoothly up to the top bar during the demo and
//! permissions, then back to centre stage for `"Hex is ready"`.

use crate::desktop::i18n::{tr, tr_named};
use rune::{Align, El, Spring, Transition, el, icon, row, text};

use super::super::controls::Paint;
use super::fixture::{Download, Language};
use super::hex_meter::hex_meter;

/// Shared-element id carried by the thin single-hexagon mark from the top-bar
/// pill to the Ready landing.
pub(crate) const PILL: &str = "hex-mark";
/// Shared-element id of the download capsule itself, so the centred progress
/// capsule and the top-bar pill are one shape that moves and widens.
pub(crate) const CAPSULE: &str = "download-capsule";
/// Zero-bounce flight spring (`0.42 s / 0.0`).
pub(crate) const FLY: Spring = Spring::new(0.42, 0.0);
const HEIGHT: f32 = 30.0;

/// What the real first run calls [`Language::Other`]: the Mac's language.
static OTHER: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub(crate) fn set_other_label(label: &str) {
    let _ = OTHER.set(label.to_owned());
}

pub(crate) fn language_label(language: Language) -> &'static str {
    match language {
        Language::English => "English",
        Language::Other => OTHER.get().map_or("French", String::as_str),
    }
}

/// Top-bar download capsule. The shell carries [`CAPSULE`], so the centred
/// progress capsule travels up into it as one shape, width leading; the
/// `16 pt` hexagon mark carries [`PILL`] on to the Ready landing.
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
                text(tr("Ready"))
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
        .matched(CAPSULE)
        .motion(FLY)
        .width_leads()
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

/// The language capsule inside `"Downloading [ English ] speech model"`.
///
/// With a press handler it switches between English and the system
/// language's recommended model, and says so to assistive technology.
/// Without one it is a plain label: no focus, no hover, nothing to press.
pub(crate) fn language_capsule<S: 'static>(
    p: Paint,
    language: Language,
    press: Option<fn(&mut S)>,
) -> El {
    let k = p.k;
    let c = &k.c;
    let mut cap = row()
        .key("language-capsule")
        .h(26.0)
        .pl(10.0)
        .pr(10.0)
        .gap(4.0)
        .center_y()
        .radius(13.0)
        .fill(c.surface_raised)
        .border(p.hair, c.border_base);
    if let Some(press) = press {
        let next = match language {
            Language::English => Language::Other,
            Language::Other => Language::English,
        };
        cap = cap
            .role("button")
            .label(tr_named(
                "Choosing it switches the language to {language}",
                &[("language", language_label(next))],
            ))
            .a11y_value(language_label(language))
            .focusable(true)
            .focus_ring(1.5, 1.0, c.border_focus)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(press);
    }
    cap = cap.child(
        text(language_label(language))
            .key("name")
            .font(k.ty.ui_medium.lh(16.0))
            .color(p.text()),
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
