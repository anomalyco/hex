//! The two ends of onboarding: the language pick that starts the download,
//! and the Ready landing that sends you off to use Hex anywhere.

use rune::{El, Spring, SpringPair, TextStyle, Transition, col, row, text};
use rune_kit::{Button, ButtonVariant};

use super::super::controls::{KeySize, Paint, PushSize, key_tray, keycap};
use super::fixture::Language;
use super::hex_meter::hex_meter;
use super::pill::{FLY, PILL};

/// Every step's heading, as the tour's.
pub(crate) const TITLE: TextStyle = TextStyle::new(26.0, 600.0).lh(32.0).tracking(-0.3);
pub(crate) const LEAD: TextStyle = TextStyle::new(15.0, 400.0).lh(22.0);
const ARRIVE: Spring = Spring::new(0.3, 0.15);

/// Arrives on Kit's 0.3 s spring, scaled up from 0.96 with a fade.
pub(crate) fn arrives() -> Transition {
    Transition::scale(0.96).springs(SpringPair::with_exit_ratio(ARRIVE, 0.8))
}

/// A heading and the line under it, centred.
pub(crate) fn heading(p: Paint, key: &str, title: &str, lead: &str) -> El {
    col()
        .key(key)
        .gap(10.0)
        .items_center()
        .transition(Transition::blur_replace())
        .child(text(title).key("title").font(TITLE).color(p.text()))
        .child(
            text(lead)
                .key("lead")
                .font(LEAD)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .max_w(440.0),
        )
}

/// Download English, or pick another language. English carries the
/// pill's id, so pressing it grows the button into the download pill.
pub(crate) fn language_pick<S: 'static>(p: Paint, pick: fn(&mut S, Language)) -> El {
    row()
        .key("language-pick")
        .gap(10.0)
        .center_y()
        .child(
            Button::new("Download English")
                .variant(ButtonVariant::Contrast)
                .on_press(move |s: &mut S| pick(s, Language::English))
                .build_push(p.k)
                .key(PILL)
                .matched(PILL)
                .motion(FLY)
                .default_button(),
        )
        .child(
            Button::new("Other language")
                .variant(ButtonVariant::Neutral)
                .on_press(move |s: &mut S| pick(s, Language::Other))
                .build_push(p.k)
                .key("other"),
        )
}

/// The shortcut's keys in a tray, `down` while held.
pub(crate) fn keys(p: Paint, key: &str, caps: &[String], down: bool) -> El {
    key_tray(
        p,
        key,
        KeySize::Small,
        caps.iter()
            .enumerate()
            .map(|(i, cap)| keycap(p, i, cap, KeySize::Small, down)),
    )
}

/// The mark, whole, over "Hex is ready" and how to start. With `matched`,
/// the mark is the element of that id, flying in from where it was.
pub(crate) fn ready(p: Paint, caps: &[String], matched: Option<&str>) -> El {
    let mark = hex_meter(p, "meter", 72.0, 1.0, 1.0);
    let mark = match matched {
        Some(id) => rune::el().key(id).matched(id).motion(FLY).child(mark),
        None => mark.key("mark"),
    };
    col()
        .key("ready")
        .gap(20.0)
        .items_center()
        .transition(arrives())
        .child(mark)
        .child(
            text("Hex is ready")
                .key("title")
                .font(TITLE)
                .color(p.text()),
        )
        .child(hold_anywhere(p, caps))
}

/// "Hold ⌥ anywhere, then talk."
pub(crate) fn hold_anywhere(p: Paint, caps: &[String]) -> El {
    row()
        .key("hold-anywhere")
        .gap(8.0)
        .center_y()
        .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", caps, false))
        .child(
            text("anywhere, then talk.")
                .key("anywhere")
                .font(LEAD)
                .color(p.muted()),
        )
}
