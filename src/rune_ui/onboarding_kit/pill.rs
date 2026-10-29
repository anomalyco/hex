//! The download as one pill: a small hex meter, what it is, and how far
//! along. It's matched by [`PILL`], so the button that started the download
//! becomes the pill, and the pill flies wherever it's placed next (the top
//! bar, the top of the screen) instead of fading out and in.

use rune::{Align, El, Spring, Transition, el, row, text};

use super::super::controls::Paint;
use super::fixture::Download;
use super::hex_meter::hex_meter;

/// The shared-element id the pill and the button it grows from carry.
pub(crate) const PILL: &str = "download-pill";
/// Flights cross the window, so they take a little longer than a control's
/// 0.3 s, with the same small bounce.
pub(crate) const FLY: Spring = Spring::new(0.42, 0.14);
const HEIGHT: f32 = 32.0;

/// The pill for `download`. `closed` closes the meter into the mark once
/// it's done.
pub(crate) fn pill(p: Paint, download: Download, closed: f32) -> El {
    let k = p.k;
    let done = download.done();
    let status = if done {
        text("Ready")
            .key("ready")
            .font(k.ty.small_medium)
            .color(p.success())
    } else {
        text(format!("{}%", (download.fraction() * 100.0).floor()))
            .key("percent")
            .font(k.ty.small.features("'tnum' 1"))
            .color(p.faint())
            .w(32.0)
            .align(Align::End)
    };
    let pill = row()
        .key(PILL)
        .matched(PILL)
        .motion(FLY)
        .h(HEIGHT)
        .pl(8.0)
        .pr(12.0)
        .gap(8.0)
        .center_y()
        .radius(HEIGHT / 2.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(hex_meter(p, "meter", 18.0, download.fraction(), closed))
        .child(
            text(match download.language {
                super::fixture::Language::English => "English",
                super::fixture::Language::Other => "Speech model",
            })
            .key("label")
            .font(k.ty.small_medium)
            .color(p.text()),
        )
        .child(status.transition(Transition::blur_replace()));
    k.elevate(pill, &k.elevation.raised)
}

/// An empty slot the pill's size, so what's beside it holds still while
/// the pill is elsewhere.
pub(crate) fn slot() -> El {
    el().key("pill-slot").h(HEIGHT)
}
