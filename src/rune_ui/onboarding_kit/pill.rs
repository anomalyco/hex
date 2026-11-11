//! The download pill and the inline language capsule button.
//!
//! [`pill`] carries [`PILL`] as a shared element, so the centerpiece loader on
//! `"Setting up Hex"` flies smoothly up to the top bar during the demo and
//! permissions, then back to centre stage for `"Hex is ready"`.

use crate::desktop::i18n::{tr, tr_named};
use rune::{Align, El, Spring, Transition, el, icon, row, text};
#[cfg(not(test))]
use std::sync::Mutex;

use super::super::controls::Paint;
use super::fixture::{Download, Language};
use super::hex_meter::hex_meter;
use super::stories::{Flow, Onboard, Step};

/// Shared-element id carried by the thin single-hexagon mark from the top-bar
/// pill to the Ready landing.
pub(crate) const PILL: &str = "hex-mark";
/// Shared-element id of the download capsule itself, so the centred progress
/// capsule and the top-bar pill are one shape that moves and widens.
pub(crate) const CAPSULE: &str = "download-capsule";
/// Zero-bounce flight spring (`0.42 s / 0.0`).
pub(crate) const FLY: Spring = Spring::new(0.42, 0.0);
/// When a [`FLY`] flight is within 2% of its place: what the words it
/// crosses wait for, so the download never passes over them.
const FLY_LANDS: f64 = 0.38;
/// How words the download flew past come in once it lands.
const AFTER_FLIGHT: Spring = Spring::new(0.24, 0.0);
const HEIGHT: f32 = 30.0;

/// Whether the download sits in the top bar rather than on the page.
pub(crate) fn up(s: &Onboard) -> bool {
    s.flow == Flow::FirstRun
        && !s.had_model
        && s.download.is_some()
        && s.step != Step::Ready
        && !(matches!(s.step, Step::Pick | Step::Started | Step::Waiting)
            && !super::sketches::playing(s))
}

/// Whether the download has landed from its last flight between the top
/// bar and the page. A flight starts on the frame the page changes, before
/// the story's next poll records it.
pub(crate) fn landed(s: &Onboard) -> bool {
    s.reduced.get() || (s.pill_up == up(s) && s.now - s.pill_flew >= FLY_LANDS)
}

/// `el` hidden while the download flies past it, then faded in.
pub(crate) fn after_flight(el: El, s: &Onboard) -> El {
    el.opacity(if landed(s) { 1.0 } else { 0.0 })
        .opacity_motion(AFTER_FLIGHT)
}

/// What the real first run calls [`Language::Other`]: the language it
/// downloads. Each run replaces it, so a later setup never shows the
/// language of an earlier one. Tests keep one per thread, as `i18n` does.
#[cfg(not(test))]
static OTHER: Mutex<String> = Mutex::new(String::new());

#[cfg(test)]
thread_local! {
    static OTHER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// The story fixture's own other language, until a real run sets one.
const OTHER_FALLBACK: &str = "French";

#[cfg(test)]
fn set_other(value: String) {
    OTHER.with(|other| *other.borrow_mut() = value);
}

#[cfg(not(test))]
fn set_other(value: String) {
    *OTHER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

pub(crate) fn set_other_label(label: Option<&str>) {
    set_other(label.map_or_else(String::new, str::to_owned));
}

#[cfg(test)]
fn other_label() -> String {
    OTHER.with(|other| other.borrow().clone())
}

#[cfg(not(test))]
fn other_label() -> String {
    OTHER.lock().map_or_else(
        |poisoned| poisoned.into_inner().clone(),
        |label| label.clone(),
    )
}

pub(crate) fn language_label(language: Language) -> String {
    match language {
        Language::English => "English".into(),
        Language::Other => {
            let label = other_label();
            if label.is_empty() {
                OTHER_FALLBACK.into()
            } else {
                label
            }
        }
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
        let next_label = language_label(next);
        cap = cap
            .role("button")
            .label(tr("Language"))
            .description(tr_named(
                "Choosing it switches the language to {language}",
                &[("language", &next_label)],
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
