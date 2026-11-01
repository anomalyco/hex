//! Shared typography, motion transitions, and the `"Hex is ready"` finale.

use rune::{El, Spring, SpringPair, TextStyle, Transition, col, el, row, text};
use rune_kit::{Button, ButtonVariant};

use crate::desktop::i18n::tr;

use super::super::controls::{KeySize, Paint, PushSize, key_tray, keycap};
use super::fixture::Language;
use super::hex_meter::{hex_meter, hex_meter_glowing};
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
const EXIT_SPRING: Spring = Spring::new(0.25, 0.0);

/// The story's live motion controls. The real first run uses the same defaults.
#[derive(Clone, Copy)]
pub(crate) struct Motion {
    pub enter: f32,
    pub exit: f32,
    pub travel: f32,
    pub stagger: f32,
    pub keyboard: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            enter: 0.34,
            exit: 0.26,
            travel: 8.0,
            stagger: 0.05,
            keyboard: 0.65,
        }
    }
}

impl Motion {
    pub fn roll(self, distance: f32) -> Transition {
        let fade = |y| Transition::offset(0.0, y).combined(Transition::opacity());
        Transition::asymmetric(fade(distance), fade(-distance)).springs(SpringPair::new(
            Spring::new(self.enter, 0.0),
            Spring::new(self.exit, 0.0),
        ))
    }

    pub fn heading(self, child: El) -> El {
        child.transition(self.roll(self.travel))
    }

    pub fn staggered(self, child: El, rank: f32, distance: f32) -> El {
        child
            .transition(self.roll(distance))
            .enter_delay(rank * self.stagger)
    }

    /// One critically damped progress value for a clock-driven pose.
    pub fn keyboard_progress(self, elapsed: f64) -> f32 {
        Spring::new(self.keyboard, 0.0)
            .sample(0.0, 0.0, 1.0, elapsed.max(0.0) as f32)
            .0
    }
}

/// Whole-state replacement in place, with a slightly shorter exit.
pub(crate) fn arrives() -> Transition {
    Transition::scale(0.97)
        .combined(Transition::opacity())
        .springs(SpringPair::new(UI_SPRING, EXIT_SPRING))
}

/// Incoming words rise as outgoing words lift away, on one presence spring.
pub(crate) fn step_replace() -> Transition {
    Motion::default().roll(Motion::default().travel)
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

/// Where Hex lives once onboarding closes, under `"Hex is ready"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Home {
    MenuBar,
    Tray,
    Launcher,
}

impl Home {
    fn copy(self) -> &'static str {
        match self {
            Self::MenuBar => tr("Lives quietly in your menu bar"),
            Self::Tray => tr("Lives quietly in your system tray"),
            Self::Launcher => tr("Open it again from your app launcher"),
        }
    }
}

/// Final `"Hex is ready"` landing: the thin single-hexagon mark arrives from
/// the top pill with a soft outline glow, followed by staggered copy and a
/// quiet macOS menu-bar preview showing where Hex lives.
pub(crate) fn ready(
    p: Paint,
    caps: &[String],
    matched: Option<&str>,
    now: f64,
    reduced: bool,
    motion: Motion,
    home: Home,
) -> El {
    let mark = hex_meter_glowing(p, "meter", 64.0, now, reduced);
    let mark = match matched {
        Some(id) => el().key(id).matched(id).motion(FLY).child(mark),
        None => motion.staggered(mark.key("mark"), 0.0, 8.0),
    };
    col()
        .key("ready")
        .gap(24.0)
        .items_center()
        .child(mark)
        .child(
            motion.staggered(
                col()
                    .key("ready-copy")
                    .gap(10.0)
                    .items_center()
                    .child(
                        text(tr("Hex is ready"))
                            .key("title")
                            .font(title_font(p))
                            .color(p.text()),
                    )
                    .child(hold_anywhere(p, caps)),
                1.0,
                8.0,
            ),
        )
        .child(motion.staggered(menubar_hint(p, home), 2.0, 8.0))
}

/// `"Hold [⌥] anywhere, then talk."`
pub(crate) fn hold_anywhere(p: Paint, caps: &[String]) -> El {
    row()
        .key("hold-anywhere")
        .gap(8.0)
        .center_y()
        .child(text(tr("Hold")).key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", caps, false))
        .child(
            text(tr("anywhere, then talk."))
                .key("anywhere")
                .font(LEAD)
                .color(p.muted()),
        )
}

/// Quiet capsule previewing the single-hexagon icon where Hex lives: the
/// macOS menu bar, the Windows tray, or the Linux app launcher.
fn menubar_hint(p: Paint, home: Home) -> El {
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
            text(home.copy())
                .key("menubar-copy")
                .font(k.ty.small)
                .color(p.muted()),
        );
    k.elevate(strip, &k.elevation.raised)
}
