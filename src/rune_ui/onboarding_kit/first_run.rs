//! The first-run onboarding flow (`t405`), redesigned as one coherent piece:
//!
//! 1. **Download (`"Setting up Hex"`):** thin single-hexagon loader with an
//!    interior calibrating scope wave (no radial glow), an inline interactive
//!    language capsule button (`"Downloading [ English ▾ ] speech model"`),
//!    and a clean tabular percentage.
//! 2. **Without Hex / With Hex demo:** centred stage, 3D-tilted keyboard,
//!    awkward slow typing with five typos vs spoken words three times faster.
//! 3. **Permissions:** calm zero-bounce grants and the escaping Accessibility
//!    helper panel ([`super::permissions`]).
//! 4. **Almost ready:** shown only if a slow connection is still downloading.
//! 5. **Try (`"Your turn"` -> `"That's Hex"`):** thin single-hexagon voice
//!    scope, focused practice field, and a matched instrument bar pairing the
//!    microphone capsule with a tactile `[ ⌥  Change shortcut ]` button.
//! 6. **Ready (`"Hex is ready"`):** top pill's hexagon lands at centre stage
//!    over staggered copy and a quiet macOS menu-bar preview.

use rune::{Anim, El, TextStyle, Transition, col, el, row, text};

use super::super::controls::{DESCRIPTION, KeySize, Paint, keycap};
use super::super::lists::allowed;
use super::field_text::Field;
use super::fixture::{ANSWER_TRY, Download};
use super::flight::{Flight, caret_x, ember_tone, flicker, flying, typeset};
use super::flows::continue_button;
use super::hex_meter::hex_loader;
use super::landing::{LEAD, clean_heading, keys, ready, staggered, title_font};
use super::mic::mouth;
use super::mic_line::mic_capsule;
use super::pill::{FLY, PILL, language_capsule, pill, slot};
use super::sparks::impacts;
use super::stories::{Onboard, Step};
use super::{permissions, versus};

const TOP_BAR: f32 = 48.0;
const ACTION_H: f32 = 32.0;
const BIG_METER: f32 = 124.0;

const TRY_W: f32 = 520.0;
const TRY_MOUTH: [f32; 2] = [TRY_W * 0.5, 34.0];
const TRY_MOUTH_D: f32 = 52.0;
const TRY_FIELD_X: f32 = 20.0;
const TRY_FIELD_Y: f32 = 96.0;
const TRY_FIELD_W: f32 = TRY_W - 2.0 * TRY_FIELD_X; // 480.0
const TRY_PAD: f32 = 18.0;
const TRY_TEXT: Field = Field {
    font: TextStyle::new(18.0, 460.0),
    line: 26.0,
    width: TRY_FIELD_W - 2.0 * TRY_PAD,
};
const HOT: f32 = 0.52;
const KNOCK: f32 = 8.0;
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
        Step::Permissions => permissions::page(p, s),
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
    clean_heading(
        col()
            .key(key.to_string())
            .gap(8.0)
            .items_center()
            .child(text(title).key("title").font(title_font(p)).color(p.text()))
            .child(row().key("lead-row").h(28.0).center().child(under)),
    )
}

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

/// `"Setting up Hex"` and `"Almost ready"`: thin single-hexagon loader, inline
/// language capsule button (`"Downloading [ English ▾ ] speech model"`), and
/// clean tabular percentage.
fn download_page(p: Paint, s: &Onboard, waiting: bool) -> El {
    let d = download(s);
    let head = if waiting {
        heading(
            p,
            "waiting-head",
            "Almost ready",
            lead(p, "Finishing the speech model so Hex can hear you."),
        )
    } else {
        let subtitle = row()
            .key("download-line")
            .gap(7.0)
            .center_y()
            .child(text("Downloading").key("pre").font(LEAD).color(p.muted()))
            .child(language_capsule(p, d.language, Onboard::cycle_language))
            .child(text("speech model").key("post").font(LEAD).color(p.muted()));
        heading(p, "download-head", "Setting up Hex", subtitle)
    };

    let pct = (d.fraction() * 100.0).round() as u32;
    let status = if d.done() {
        allowed(p, "status", "Ready").transition(Transition::opacity())
    } else {
        text(format!("{pct}%"))
            .key("status")
            .font(TextStyle::new(13.0, 500.0).lh(18.0).features("'tnum' 1"))
            .color(p.muted())
    };

    col()
        .key(if waiting { "waiting" } else { "download" })
        .gap(26.0)
        .items_center()
        .child(staggered(
            el().key("meter-stage").size(BIG_METER, BIG_METER).child(
                el().key(PILL)
                    .absolute(0.0, 0.0)
                    .matched(PILL)
                    .motion(FLY)
                    .child(hex_loader(
                        p,
                        "meter",
                        BIG_METER,
                        d.fraction(),
                        s.closed,
                        s.now,
                        s.reduced.get(),
                    )),
            ),
            0.0,
            6.0,
        ))
        .child(staggered(head, 0.08, 8.0))
        .child(staggered(
            row().key("status-row").h(20.0).center().child(status),
            0.16,
            6.0,
        ))
}

fn demo_page(p: Paint, s: &Onboard) -> El {
    col()
        .key("demo")
        .gap(18.0)
        .items_center()
        .child(versus::heading(p, s))
        .child(versus::stage(p, s))
        .child(action(if s.step == Step::Done {
            continue_button(p)
        } else {
            none()
        }))
}

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
        .gap(22.0)
        .items_center()
        .child(head)
        .child(staggered(try_stage(p, s), 0.08, 10.0))
        .child(staggered(
            col()
                .key("try-foot")
                .gap(14.0)
                .items_center()
                .child(action(if s.step == Step::TryHeard {
                    continue_button(p)
                } else {
                    none()
                }))
                .child(instrument_bar(p, s)),
            0.16,
            8.0,
        ))
}

/// Matched `28 pt` secondary instrument row pairing the live microphone
/// selector capsule with a tactile `[ ⌥  Change shortcut ]` button.
fn instrument_bar(p: Paint, s: &Onboard) -> El {
    row()
        .key("instrument-bar")
        .gap(8.0)
        .center_y()
        .child(mic_capsule(p, s.listening_peak()))
        .child(shortcut_capsule(p, &s.caps, s.step == Step::TryHearing))
}

fn shortcut_capsule(p: Paint, caps: &[String], down: bool) -> El {
    let k = p.k;
    let c = &k.c;
    let mut pill = row()
        .key("change-shortcut")
        .role("button")
        .label("Change shortcut")
        .focusable(true)
        .focus_ring(1.5, 1.0, c.border_focus)
        .h(28.0)
        .pl(5.0)
        .pr(11.0)
        .gap(7.0)
        .center_y()
        .radius(14.0)
        .fill(c.surface_raised)
        .border(p.hair, c.border_weak)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(Onboard::cycle_shortcut)
        .child(
            row().key("mini-keys").gap(2.0).center_y().children(
                caps.iter()
                    .enumerate()
                    .map(|(i, cap)| keycap(p, i, cap, KeySize::Small, down).scale(0.9)),
            ),
        )
        .child(
            text("Change shortcut")
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
    for shadow in k.shadow_layers(&k.elevation.button_neutral) {
        pill = pill.box_shadow(shadow);
    }
    pill
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
    let h = TRY_FIELD_Y + try_field_h() + 14.0;
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
        .border(
            p.hair,
            border.mix(ember_tone(p, 0.65), (flash * 0.9).min(0.85)),
        )
        .offset(knock[0], knock[1])
        .offset_motion(Anim::INSTANT)
        .children((!speaking).then(|| {
            text("Say anything…")
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
        .child(mouth(p, "mouth", TRY_MOUTH, TRY_MOUTH_D, s, hearing))
        .child(field)
        .child(try_words(p, s, &flights, knock, h))
        .child(impacts(p, "impacts", [TRY_W, h], &only, s.now, reduced).absolute(0.0, 0.0))
}

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
    let weight = 500.0 + 220.0 * HOT;
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
        }
        if reduced || s.now >= flight.arrives() - 0.10 {
            if slot.line != line {
                targets.clear();
                line = slot.line;
            }
            targets.push((flight.arrives() - 0.10, slot.right + 1.5));
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
}
