//! Shared typography, motion transitions, and the `"Hex is ready"` finale.

use rune::{Beat, El, Sequence, Spring, SpringPair, TextStyle, Transition, col, el, row, text};
use rune_kit::{Button, ButtonVariant};

use super::super::controls::{KeySize, Paint, PushSize, key_tray, keycap};
use super::fixture::Language;
use super::hex_meter::hex_meter;
use super::pill::{FLY, PILL};

/// One display heading style across every onboarding step (`26 pt`, softened
/// weight in dark mode so light ink on dark doesn't bloom).
pub(crate) const TITLE: TextStyle = TextStyle::new(26.0, 580.0).lh(32.0).tracking(-0.35);
pub(crate) fn title_font(p: Paint) -> TextStyle {
    TextStyle {
        weight: if p.k.dark { 550.0 } else { 600.0 },
        ..TITLE
    }
}

/// Subtitle line under every step's heading.
pub(crate) const LEAD: TextStyle = TextStyle::new(15.0, 410.0).lh(22.0);

/// Product UI spring: `0.30 s`, zero bounce.
pub(crate) const UI_SPRING: Spring = Spring::new(0.30, 0.0);
const EXIT_SPRING: Spring = Spring::new(0.11, 0.0);
const STEP_IN_BEATS: &[Beat] = &[
    Beat::new(0.0).opacity(0.0).scale(0.985),
    Beat::new(0.07).opacity(0.0).scale(0.985),
    Beat::new(0.32)
        .opacity(1.0)
        .scale(1.0)
        .easing(rune::Easing::EASE_OUT),
];

/// Calm entrance/exit (`scale 0.97`, zero bounce, faster exit so outgoing
/// elements clear before incoming ones settle).
pub(crate) fn arrives() -> Transition {
    Transition::scale(0.97)
        .combined(Transition::opacity())
        .springs(SpringPair::new(UI_SPRING, EXIT_SPRING))
}

/// Directional vertical replacement for step headings: outgoing lifts `-6 pt`
/// and clears in `0.11 s` before incoming rises `+8 pt`, preventing ghosted
/// double-exposure titles.
pub(crate) fn step_replace() -> Transition {
    let enter = Transition::offset(0.0, 8.0).combined(Transition::opacity());
    let exit = Transition::offset(0.0, -6.0).combined(Transition::opacity());
    Transition::asymmetric(enter, exit).springs(SpringPair::new(UI_SPRING, EXIT_SPRING))
}

/// Applies [`step_replace`] plus a brief `70 ms` hold so the outgoing heading
/// clears before the incoming text reaches contrast.
pub(crate) fn clean_heading(el: El) -> El {
    el.transition(step_replace())
        .sequence(Sequence::new(STEP_IN_BEATS))
}

const STAGGER_BEATS: &[Beat] = &[
    Beat::new(0.0).opacity(0.0).scale(0.975),
    Beat::new(0.32)
        .opacity(1.0)
        .scale(1.0)
        .easing(rune::Easing::EASE_OUT),
];

/// Staggers a child's entrance by `delay_secs` (`0.32 s` settle, no bounce).
pub(crate) fn staggered(child: El, delay_secs: f64, y_offset: f32) -> El {
    child
        .sequence(Sequence::new(STAGGER_BEATS).delay(delay_secs))
        .transition(
            Transition::offset(0.0, y_offset).springs(SpringPair::new(UI_SPRING, EXIT_SPRING)),
        )
}

/// A heading and the line under it, centred on the column spine.
pub(crate) fn heading(p: Paint, key: &str, title: &str, lead: &str) -> El {
    col()
        .key(key)
        .gap(8.0)
        .items_center()
        .transition(step_replace())
        .child(text(title).key("title").font(title_font(p)).color(p.text()))
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

/// Download English, or pick another language (used by the older option stories).
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

/// The shortcut's keys in a recessed tray, `down` while held.
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

/// Final `"Hex is ready"` landing: the thin single-hexagon mark arrives from
/// the top pill, followed by staggered copy and a quiet macOS menu-bar preview
/// showing where Hex lives.
pub(crate) fn ready(p: Paint, caps: &[String], matched: Option<&str>) -> El {
    let mark = hex_meter(p, "meter", 64.0, 1.0, 1.0);
    let mark = match matched {
        Some(id) => el().key(id).matched(id).motion(FLY).child(mark),
        None => staggered(mark.key("mark"), 0.0, 8.0),
    };
    col()
        .key("ready")
        .gap(24.0)
        .items_center()
        .child(mark)
        .child(staggered(
            col()
                .key("ready-copy")
                .gap(10.0)
                .items_center()
                .child(
                    text("Hex is ready")
                        .key("title")
                        .font(title_font(p))
                        .color(p.text()),
                )
                .child(hold_anywhere(p, caps)),
            0.08,
            8.0,
        ))
        .child(staggered(menubar_hint(p), 0.18, 10.0))
}

/// `"Hold [⌥] anywhere, then talk."`
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

/// Quiet capsule previewing the single-hexagon icon in the macOS menu bar.
fn menubar_hint(p: Paint) -> El {
    let k = p.k;
    let strip = row()
        .key("menubar-pill")
        .h(30.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(15.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(hex_meter(p, "menubar-hex", 14.0, 1.0, 1.0))
        .child(el().key("sep").size(p.hair, 12.0).fill(k.c.border_base))
        .child(
            text("Lives quietly in your menu bar")
                .key("menubar-copy")
                .font(k.ty.small)
                .color(p.muted()),
        );
    k.elevate(strip, &k.elevation.raised)
}
