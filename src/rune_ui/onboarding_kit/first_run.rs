//! The first run Kit and Dax agreed on (t405), as one story:
//!
//! 1. The speech model for the Mac's language starts downloading at once,
//!    big in the middle, so it's plain something is downloading.
//! 2. It moves up to the top bar while the Versus demo shows how Hex works.
//! 3. Microphone and Accessibility, with macOS's prompt and the floating
//!    guide beside System Settings. Never before the demo.
//! 4. If the model's still coming, it takes the middle again until it lands.
//! 5. Your turn: hold the shortcut, say anything, and the words fly in hot
//!    and burst on the field as on the site.
//!
//! Anything already done is skipped: no download view for a model that's
//! here, no permissions already granted.

use rune::{Anim, Color, El, Spring, TextStyle, Transition, col, el, row, text};
use rune_kit::{Button, ButtonVariant, glyphs};

use super::super::Ui;
use super::super::controls::{ButtonKind, Paint, PushSize, card, icon_row};
use super::super::lists::{PermissionControl, allowed, permission_control};
use super::field_text::Field;
use super::fixture::{ANSWER_TRY, Download, Language};
use super::flight::{Flight, caret_x, ember, flicker, flying, srgb, typeset};
use super::flows::continue_button;
use super::hex_meter::hex_meter;
use super::landing::{LEAD, TITLE, keys, ready};
use super::mic::mouth;
use super::mic_line::mic_line;
use super::pill::{FLY, PILL, pill, slot};
use super::sparks::impacts;
use super::stories::{Onboard, Step};
use super::versus;

/// How long the permissions step plays.
pub(crate) const PERMISSIONS_SECS: f64 = 6.6;
const TOP_BAR: f32 = 48.0;
const ACTION_H: f32 = 32.0;
const BIG_METER: f32 = 148.0;

/// The permissions, by seconds into the step.
const MIC_ASK: f64 = 0.9;
const MIC_PRESS: f64 = 1.9;
const MIC_GRANT: f64 = 2.2;
const AX_ASK: f64 = 2.9;
const DRAG: (f64, f64) = (3.5, 4.3);
const SWITCH_ON: f64 = 4.6;
const AX_GRANT: f64 = 4.9;
const SETTINGS_CLOSE: f64 = 5.4;
const CONTINUE_AT: f64 = 5.6;
const PERMISSIONS_W: f32 = 460.0;
const SYSTEM: [f32; 2] = [440.0, 244.0];

/// The try: the Hex the words leave from over a big field.
const TRY_W: f32 = 560.0;
const TRY_MOUTH: [f32; 2] = [TRY_W / 2.0, 40.0];
const TRY_MOUTH_D: f32 = 64.0;
const TRY_FIELD_X: f32 = 24.0;
const TRY_FIELD_Y: f32 = 112.0;
const TRY_FIELD_W: f32 = TRY_W - 2.0 * TRY_FIELD_X;
const TRY_PAD: f32 = 18.0;
const TRY_TEXT: Field = Field {
    font: TextStyle::new(22.0, 500.0),
    line: 30.0,
    width: TRY_FIELD_W - 2.0 * TRY_PAD,
};
/// Your own words fly hotter than the demo's calm ones: past 0.42 they
/// spray sparks and open a ring where they land.
const HOT: f32 = 0.62;
/// The site clamps how far a card is knocked.
const KNOCK: f32 = 12.0;
const BURST: f64 = 0.06;

pub(super) fn view(p: Paint, s: &Onboard) -> Vec<El> {
    let centred = matches!(s.step, Step::Pick | Step::Started | Step::Waiting);
    let top = row()
        .key("top-bar")
        .w_full()
        .h(TOP_BAR)
        .px(20.0)
        .center_y()
        .justify_center()
        .child(match s.download {
            Some(d) if !s.had_model && !centred && s.step != Step::Ready => pill(p, d, s.closed),
            _ => slot(),
        });
    let page = match s.step {
        Step::Pick | Step::Started => download_page(p, s, false),
        Step::Waiting => download_page(p, s, true),
        Step::Parked | Step::YourTurn | Step::Hearing | Step::Heard | Step::Done => demo_page(p, s),
        Step::Permissions => permissions_page(p, s),
        Step::Try | Step::TryHearing | Step::TryHeard => try_page(p, s),
        Step::Ready => ready(p, &s.caps, (!s.had_model).then_some(PILL)),
    };
    vec![
        top,
        col()
            .key("middle")
            .grow(1.0)
            .shrink()
            .w_full()
            .center()
            .pb(TOP_BAR)
            .child(page),
    ]
}

fn action(content: El) -> El {
    row().key("action").h(ACTION_H).center().child(content)
}

fn none() -> El {
    el().key("none")
}

/// The download, or the one about to start for the Mac's language before
/// the story plays.
fn download(s: &Onboard) -> Download {
    s.download.unwrap_or(Download {
        language: s.wants,
        downloaded: 0,
        total: s.wants.bytes(),
    })
}

fn lead(p: Paint, copy: impl Into<String>) -> El {
    text(copy.into())
        .key("lead")
        .font(LEAD)
        .color(p.muted())
        .align(rune::Align::Center)
        .transition(Transition::opacity())
}

fn heading(p: Paint, key: &str, title: &str, under: El) -> El {
    col()
        .key(key.to_string())
        .gap(10.0)
        .items_center()
        .transition(Transition::blur_replace())
        .child(text(title).key("title").font(TITLE).color(p.text()))
        .child(row().key("lead-row").h(22.0).center().child(under))
}

/// "Hold ⌥ and …", the keys down while held.
fn hold(p: Paint, s: &Onboard, rest: &str, down: bool) -> El {
    row()
        .key("hold")
        .gap(8.0)
        .center_y()
        .transition(Transition::opacity())
        .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", &s.caps, down))
        .child(text(rest).key("rest").font(LEAD).color(p.muted()))
}

/// The Mac's language, as the story's Mac is set.
fn language_name(language: Language) -> &'static str {
    match language {
        Language::English => "English",
        Language::Other => "French",
    }
}

/// The download, big in the middle: first, and again while waiting on it.
fn download_page(p: Paint, s: &Onboard, waiting: bool) -> El {
    let d = download(s);
    let model = match d.language {
        Language::English => "English",
        Language::Other => "multilingual",
    };
    let (key, title, copy) = if waiting {
        (
            "waiting",
            "Almost ready",
            "Hex needs the speech model to hear you.".to_string(),
        )
    } else {
        (
            "download",
            "Setting up Hex",
            format!("Downloading the {model} speech model."),
        )
    };
    let status = if d.done() {
        allowed(p, "status", "Ready").transition(Transition::blur_replace())
    } else {
        text(d.megabytes())
            .key("status")
            .font(p.k.ty.small.features("'tnum' 1"))
            .color(p.faint())
    };
    let note = (!waiting).then(|| {
        text(format!(
            "{}, from your Mac's language. Change it any time in Settings.",
            language_name(d.language)
        ))
        .key("note")
        .font(p.k.ty.small)
        .color(p.faint())
    });
    col()
        .key(key)
        .gap(28.0)
        .items_center()
        .child(
            el().key("meter-stage")
                .size(BIG_METER, BIG_METER)
                .child(lamp(p, s))
                .child(
                    el().key(PILL)
                        .absolute(0.0, 0.0)
                        .matched(PILL)
                        .motion(FLY)
                        .child(hex_meter(p, "meter", BIG_METER, d.fraction(), s.closed)),
                ),
        )
        .child(heading(p, "head", title, lead(p, copy)))
        .child(
            col()
                .key("progress")
                .gap(8.0)
                .items_center()
                .child(row().key("status-row").h(20.0).center().child(status))
                .children(note),
        )
}

/// A soft light behind the big meter, breathing slowly while it fills.
fn lamp(p: Paint, s: &Onboard) -> El {
    let d = BIG_METER * 1.5;
    let breath = if s.reduced.get() {
        1.0
    } else {
        0.85 + 0.15 * (s.now * std::f64::consts::TAU / 3.2).sin() as f32
    };
    let tint = if p.k.dark {
        srgb(0.76, 0.86, 1.0, 0.16)
    } else {
        srgb(0.45, 0.55, 0.75, 0.12)
    };
    el().key("lamp")
        .absolute((BIG_METER - d) / 2.0, (BIG_METER - d) / 2.0)
        .size(d, d)
        .radius(d / 2.0)
        .fill(tint)
        .blur(BIG_METER * 0.22)
        .opacity(breath)
        .opacity_motion(Anim::INSTANT)
}

/// How Hex works, while the model downloads: the Versus demo.
fn demo_page(p: Paint, s: &Onboard) -> El {
    col()
        .key("demo")
        .gap(20.0)
        .items_center()
        .child(versus::heading(p, s))
        .child(versus::stage(p, s))
        .child(action(if s.step == Step::Done {
            continue_button(p)
        } else {
            none()
        }))
}

/// Seconds into the permissions step.
fn permissions_elapsed(s: &Onboard) -> f64 {
    s.permissions_from.map_or(0.0, |from| s.now - from)
}

fn control(e: f64, ask: f64, grant: f64) -> PermissionControl {
    if e < ask {
        PermissionControl::Allow
    } else if e < grant {
        PermissionControl::Waiting
    } else {
        PermissionControl::Allowed
    }
}

/// Microphone and Accessibility, as the tour asks for them, with what
/// macOS shows for each beneath.
fn permissions_page(p: Paint, s: &Onboard) -> El {
    let e = permissions_elapsed(s);
    let mic = control(e, MIC_ASK, MIC_GRANT);
    let ax = control(e, AX_ASK, AX_GRANT);
    let kind = |next: bool| {
        if next {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        }
    };
    let rows = vec![
        icon_row(
            p,
            "setup-microphone",
            &glyphs::MIC,
            "Microphone",
            Some("Hears you only while you hold the shortcut"),
            permission_control(
                p,
                "mic-grant",
                mic,
                kind(mic == PermissionControl::Allow),
                |_: &mut Ui| {},
            ),
        ),
        icon_row(
            p,
            "setup-accessibility",
            &glyphs::CURSOR_TEXT,
            "Accessibility",
            Some(if ax == PermissionControl::Waiting {
                "Drag Hex into the list, or switch it on if it is there"
            } else {
                "Notices the shortcut and pastes your words in any app"
            }),
            permission_control(
                p,
                "ax-grant",
                ax,
                kind(mic == PermissionControl::Allowed && ax == PermissionControl::Allow),
                |_: &mut Ui| {},
            ),
        ),
    ];
    col()
        .key("permissions")
        .gap(24.0)
        .items_center()
        .transition(Transition::blur_replace())
        .child(heading(
            p,
            "permissions-head",
            "Let Hex hear you and type for you",
            lead(
                p,
                "macOS asks once. You can change these in System Settings.",
            ),
        ))
        .child(card(p, "setup-card", rows).w(PERMISSIONS_W))
        .child(system(p, s, e))
        .child(action(if e >= CONTINUE_AT {
            continue_button(p)
        } else {
            none()
        }))
}

/// What macOS shows: its microphone prompt, then System Settings with the
/// floating guide under it.
fn system(p: Paint, s: &Onboard, e: f64) -> El {
    let mut out = el().key("system").size(SYSTEM[0], SYSTEM[1]);
    if (MIC_ASK..MIC_GRANT).contains(&e) {
        out = out.child(
            microphone_prompt(p, e >= MIC_PRESS)
                .absolute((SYSTEM[0] - PROMPT_W) / 2.0, 0.0)
                .transition(super::landing::arrives()),
        );
    }
    if (AX_ASK..SETTINGS_CLOSE).contains(&e) {
        out = out.child(
            settings_and_guide(p, s, e)
                .absolute(0.0, 0.0)
                .transition(super::landing::arrives()),
        );
    }
    out
}

const PROMPT_W: f32 = 260.0;

/// macOS's "would like to access the microphone" alert, Allow `pressed`.
fn microphone_prompt(p: Paint, pressed: bool) -> El {
    let k = p.k;
    let button = |key: &str, label: &str, variant: ButtonVariant| {
        Button::new(label)
            .variant(variant)
            .on_press(|_: &mut Onboard| {})
            .build_push(k)
            .key(key.to_string())
            .grow(1.0)
    };
    let alert = col()
        .key("mic-prompt")
        .w(PROMPT_W)
        .padding(18.0)
        .gap(10.0)
        .items_center()
        .radius(22.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(hex_meter(p, "icon", 40.0, 1.0, 1.0))
        .child(
            text("“Hex” would like to access the microphone.")
                .key("title")
                .font(TextStyle {
                    weight: 600.0,
                    ..k.ty.ui_medium
                })
                .color(p.text())
                .align(rune::Align::Center)
                .wrap()
                .w(PROMPT_W - 36.0),
        )
        .child(
            text("Hex hears you only while you hold the shortcut.")
                .key("body")
                .font(k.ty.small)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(PROMPT_W - 36.0),
        )
        .child(
            row()
                .key("buttons")
                .w_full()
                .gap(8.0)
                .pt(4.0)
                .child(button("deny", "Don't Allow", ButtonVariant::Neutral))
                .child(
                    button("allow", "Allow", ButtonVariant::Contrast)
                        .scale(if pressed { 0.95 } else { 1.0 })
                        .opacity(if pressed { 0.8 } else { 1.0 })
                        .motion(Spring::new(0.12, 0.0)),
                ),
        );
    k.elevate(alert, &k.elevation.floating)
}

const WINDOW_X: f32 = 10.0;
const WINDOW_W: f32 = SYSTEM[0] - 2.0 * WINDOW_X;
const WINDOW_H: f32 = 150.0;
const LIST: [f32; 2] = [16.0, 60.0];
const LIST_ROW: f32 = 28.0;
const GUIDE_Y: f32 = 162.0;
const GUIDE_H: f32 = 82.0;
const TILE: [f32; 2] = [18.0, 38.0];
const CHIP: [f32; 2] = [78.0, 28.0];

/// System Settings' Accessibility list, and the guide floating under it
/// with Hex to drag in, as `permission_guide.m` draws it.
fn settings_and_guide(p: Paint, s: &Onboard, e: f64) -> El {
    let k = p.k;
    let dragged = drag_progress(s, e);
    let landed = e >= DRAG.1;
    let dot = |key: &str, color: Color| {
        el().key(key.to_string())
            .size(10.0, 10.0)
            .radius(5.0)
            .fill(color)
    };
    let bar = row()
        .key("title-bar")
        .absolute(0.0, 0.0)
        .w(WINDOW_W)
        .h(30.0)
        .px(12.0)
        .gap(7.0)
        .center_y()
        .child(dot("close", srgb(1.0, 0.37, 0.34, 1.0)))
        .child(dot("minimize", srgb(1.0, 0.74, 0.18, 1.0)))
        .child(dot("zoom", srgb(0.16, 0.78, 0.25, 1.0)))
        .child(
            text("Accessibility")
                .key("title")
                .font(TextStyle {
                    weight: 600.0,
                    ..k.ty.ui
                })
                .color(p.text())
                .pl(18.0),
        );
    let mut list = col()
        .key("list")
        .absolute(LIST[0], LIST[1])
        .w(WINDOW_W - 2.0 * LIST[0])
        .radius(8.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(app_row(
            p,
            "raycast",
            "Raycast",
            app_icon(srgb(1.0, 0.39, 0.39, 1.0)),
            true,
        ))
        .child(app_row(
            p,
            "rectangle",
            "Rectangle",
            app_icon(srgb(0.35, 0.55, 1.0, 1.0)),
            true,
        ));
    if landed {
        list = list.child(
            app_row(
                p,
                "hex",
                "Hex",
                hex_meter(p, "icon", 16.0, 1.0, 1.0),
                e >= SWITCH_ON,
            )
            .transition(Transition::opacity()),
        );
    }
    let window = el()
        .key("settings")
        .absolute(WINDOW_X, 0.0)
        .size(WINDOW_W, WINDOW_H)
        .radius(12.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(bar)
        .child(
            text("Allow the apps below to control your computer.")
                .key("explain")
                .font(k.ty.small)
                .color(p.muted())
                .absolute(LIST[0], 36.0),
        )
        .child(list);
    let tile = row()
        .key("tile")
        .absolute(TILE[0], TILE[1])
        .w(WINDOW_W - 2.0 * TILE[0])
        .h(34.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .opacity(if dragged.is_some() || landed {
            0.45
        } else {
            1.0
        })
        .child(hex_meter(p, "icon", 20.0, 1.0, 1.0))
        .child(text("Hex").key("name").font(k.ty.ui_medium).color(p.text()));
    let guide = el()
        .key("guide")
        .absolute(WINDOW_X, GUIDE_Y)
        .size(WINDOW_W, GUIDE_H)
        .radius(12.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(
            text("↑  Drag Hex into the Accessibility list above")
                .key("instruction")
                .font(k.ty.ui_medium)
                .color(p.text())
                .absolute(18.0, 12.0),
        )
        .child(tile);
    let mut out = el()
        .key("settings-guide")
        .size(SYSTEM[0], SYSTEM[1])
        .child(k.elevate(window, &k.elevation.floating))
        .child(k.elevate(guide, &k.elevation.floating));
    if let Some(u) = dragged {
        out = out.child(drag_chip(p, u));
    }
    out
}

/// How far along the drag is, 0 to 1, while it's under way.
fn drag_progress(s: &Onboard, e: f64) -> Option<f32> {
    if s.reduced.get() || !(DRAG.0..DRAG.1).contains(&e) {
        return None;
    }
    let u = ((e - DRAG.0) / (DRAG.1 - DRAG.0)) as f32;
    Some(u * u * (3.0 - 2.0 * u))
}

/// Hex on its way from the guide's tile to the list's next row.
fn drag_chip(p: Paint, u: f32) -> El {
    let k = p.k;
    let from = [WINDOW_X + TILE[0] + 4.0, GUIDE_Y + TILE[1] + 3.0];
    let to = [WINDOW_X + LIST[0] + 4.0, LIST[1] + 2.0 * LIST_ROW];
    let x = from[0] + (to[0] - from[0]) * u;
    // A little arc up and over, as a hand drags.
    let y = from[1] + (to[1] - from[1]) * u - 18.0 * (u * std::f32::consts::PI).sin();
    let chip = row()
        .key("drag-chip")
        .absolute(x, y)
        .size(CHIP[0], CHIP[1])
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(7.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_focus)
        .scale(1.0 + 0.06 * (u * std::f32::consts::PI).sin())
        .motion(Spring::new(0.016, 0.0))
        .offset_motion(Anim::INSTANT)
        .child(hex_meter(p, "icon", 16.0, 1.0, 1.0))
        .child(text("Hex").key("name").font(k.ty.ui_medium).color(p.text()));
    k.elevate(chip, &k.elevation.floating)
}

fn app_icon(color: Color) -> El {
    el().key("icon")
        .size(16.0, 16.0)
        .radius(4.0)
        .continuous_corners()
        .fill(color)
}

/// A row of System Settings' list: the app and its switch.
fn app_row(p: Paint, key: &str, name: &str, icon: El, on: bool) -> El {
    let k = p.k;
    row()
        .key(key.to_string())
        .h(LIST_ROW)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .child(icon)
        .child(
            text(name)
                .key("name")
                .font(k.ty.small)
                .color(p.text())
                .grow(1.0),
        )
        .child(switch(p, on))
}

/// A macOS switch.
fn switch(p: Paint, on: bool) -> El {
    let k = p.k;
    let knob = el()
        .key("knob")
        .absolute(if on { 12.0 } else { 1.5 }, 1.5)
        .size(12.0, 12.0)
        .radius(6.0)
        .fill(k.c.white)
        .motion(Spring::new(0.25, 0.1));
    el().key("switch")
        .size(26.0, 15.0)
        .radius(7.5)
        .fill(if on { p.accent() } else { k.c.border_base })
        .child(knob)
}

/// When each word of the try is heard, from the key going down.
fn try_heard_at(s: &Onboard) -> Vec<f64> {
    let count = ANSWER_TRY.text().split_whitespace().count();
    (0..count)
        .map(|i| {
            if s.streams() {
                ANSWER_TRY
                    .updates
                    .iter()
                    .find(|(_, partial)| partial.split_whitespace().count() > i)
                    .map_or(ANSWER_TRY.release, |(at, _)| *at)
            } else {
                ANSWER_TRY.release + i as f64 * BURST
            }
        })
        .collect()
}

fn try_lines() -> usize {
    TRY_TEXT.wrap(ANSWER_TRY.text()).len()
}

fn try_field_h() -> f32 {
    try_lines() as f32 * TRY_TEXT.line + 2.0 * TRY_PAD
}

fn try_text_origin() -> [f32; 2] {
    [TRY_FIELD_X + TRY_PAD, TRY_FIELD_Y + TRY_PAD]
}

fn try_flights(s: &Onboard) -> Vec<(Flight, super::field_text::Slot<'static>)> {
    let Some(from) = s.take_from.filter(|_| s.step >= Step::TryHearing) else {
        return Vec::new();
    };
    let origin = try_text_origin();
    TRY_TEXT
        .slots(ANSWER_TRY.text())
        .into_iter()
        .zip(try_heard_at(s))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            (
                Flight::new(TRY_MOUTH, to, from + at, HOT, TRY_TEXT.font.size, i),
                slot,
            )
        })
        .collect()
}

/// Your turn: hold the shortcut and say anything.
fn try_page(p: Paint, s: &Onboard) -> El {
    let head = match s.step {
        Step::TryHeard => heading(
            p,
            "try-heard",
            "That's Hex",
            lead(p, "It types wherever your cursor is, in any app."),
        ),
        _ => heading(
            p,
            "try",
            "Your turn",
            hold(p, s, "and say anything.", s.step == Step::TryHearing),
        ),
    };
    col()
        .key("try-page")
        .gap(20.0)
        .items_center()
        .transition(Transition::blur_replace())
        .child(head)
        .child(try_stage(p, s))
        .child(
            col()
                .key("try-foot")
                .gap(10.0)
                .items_center()
                .child(action(if s.step == Step::TryHeard {
                    continue_button(p)
                } else {
                    none()
                }))
                .child(mic_line(p, true, s.listening_peak()))
                .child(
                    text("Change shortcut")
                        .key("change-shortcut")
                        .font(p.k.ty.small)
                        .color(p.faint()),
                ),
        )
}

fn try_stage(p: Paint, s: &Onboard) -> El {
    let reduced = s.reduced.get();
    let flights = try_flights(s);
    let (knock, flash) = if reduced {
        ([0.0, 0.0], 0.0)
    } else {
        let knock = flights.iter().fold([0.0, 0.0], |[x, y], (f, _)| {
            let [dx, dy] = f.hit(s.now);
            [x + dx, y + dy]
        });
        let flash = flights
            .iter()
            .map(|(f, _)| f.flash(s.now))
            .fold(0.0, f32::max);
        (knock.map(|v| v.clamp(-KNOCK, KNOCK)), flash)
    };
    let h = TRY_FIELD_Y + try_field_h() + 16.0;
    let k = p.k;
    let hearing = s.step == Step::TryHearing;
    let border = if hearing {
        k.c.border_focus
    } else {
        k.c.border_base
    };
    let speaking = flights.iter().any(|(f, _)| s.now >= f.start());
    let field = el()
        .key("try-field")
        .absolute(TRY_FIELD_X, TRY_FIELD_Y)
        .size(TRY_FIELD_W, try_field_h())
        .radius(14.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair * 1.5, border.mix(ember(0.6), (flash * 1.3).min(1.0)))
        .offset(knock[0], knock[1])
        .offset_motion(Anim::INSTANT)
        .children((!speaking).then(|| {
            text("Say anything")
                .key("placeholder")
                .font(TRY_TEXT.font.lh(TRY_TEXT.line))
                .color(p.faint())
                .absolute(TRY_PAD + 3.0, TRY_PAD)
                .transition(Transition::opacity())
        }));
    let field = k.elevate(field, &k.elevation.raised);
    let only: Vec<Flight> = flights.iter().map(|(f, _)| *f).collect();
    el().key("try-stage")
        .size(TRY_W, h)
        .child(mouth(p, "mouth", TRY_MOUTH, TRY_MOUTH_D, s.meter, hearing))
        .child(field)
        .child(try_words(p, s, &flights, knock, h))
        .child(impacts(p, "impacts", [TRY_W, h], &only, s.now, reduced).absolute(0.0, 0.0))
}

/// The try's words in flight and landed, and the caret after them.
fn try_words(
    p: Paint,
    s: &Onboard,
    flights: &[(Flight, super::field_text::Slot<'static>)],
    knock: [f32; 2],
    h: f32,
) -> El {
    let reduced = s.reduced.get();
    let origin = try_text_origin();
    let font = TRY_TEXT.font.lh(TRY_TEXT.line);
    let weight = 520.0 + 300.0 * HOT;
    let mut out = el()
        .key("try-words")
        .absolute(0.0, 0.0)
        .size(TRY_W, h)
        .instant_changes();
    let mut targets = Vec::new();
    let mut line = 0;
    for (i, (flight, slot)) in flights.iter().enumerate() {
        if let Some(landed) = flight.landed(s.now, reduced) {
            let at = [
                origin[0] + slot.centre[0] + knock[0],
                origin[1] + slot.centre[1] + knock[1],
            ];
            out = out.children(typeset(
                p,
                &format!("w{i}"),
                slot.word,
                font,
                at,
                landed,
                flicker(s.now, i),
            ));
            if slot.line != line {
                targets.clear();
                line = slot.line;
            }
            targets.push((flight.arrives(), slot.right + 1.0));
        }
        if let Some(pose) = flight.pose(s.now, reduced) {
            out = out.child(flying(p, &format!("f{i}"), slot.word, weight, pose));
        }
    }
    if matches!(s.step, Step::Try | Step::TryHearing) {
        let x = if reduced {
            targets.last().map_or(0.0, |t| t.1)
        } else {
            caret_x(0.0, &targets, s.now)
        };
        out = out.child(
            el().key("caret-at")
                .absolute(origin[0] + knock[0], origin[1] + knock[1])
                .child(TRY_TEXT.caret(p, "caret", x, line)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_try_fits_its_field_and_flies_hot() {
        assert!(try_lines() <= 2, "{}", try_lines());
        const { assert!(HOT > 0.42) };
        let [x, y] = try_text_origin();
        assert!(x > TRY_FIELD_X && y > TRY_MOUTH[1] + TRY_MOUTH_D / 2.0);
    }

    #[test]
    fn each_permission_asks_then_is_granted() {
        assert_eq!(control(0.0, MIC_ASK, MIC_GRANT), PermissionControl::Allow);
        assert_eq!(control(1.0, MIC_ASK, MIC_GRANT), PermissionControl::Waiting);
        assert_eq!(control(2.5, MIC_ASK, MIC_GRANT), PermissionControl::Allowed);
        const { assert!(MIC_GRANT < AX_ASK && AX_GRANT < CONTINUE_AT) };
        const { assert!(CONTINUE_AT < PERMISSIONS_SECS) };
    }
}
