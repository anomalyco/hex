//! The first-run onboarding flow (`t405`), featuring:
//!
//! - Flat, crisp Rune 2D UI (no embossed cards or shader starbursts).
//! - Hex's real `Dot scope` phosphor dot-matrix HUD (`rune_hud.wgsl`) on
//!   `"Setting up Hex"`, `"With Hex"`, and `"Your turn"`.
//! - Live interactive `"Your turn"` in desktop windows: pauses with a focused
//!   [`rune_kit::TextArea`] so holding `⌥` and speaking into the running Hex
//!   listener pastes real dictated words right into the card.
//! - Calm permission grants and the escaping Accessibility panel (`permissions`).
//! - Auditionable typing key-sound candidates and `#292` `feedback` tones (`sfx`).

use rune::{Anim, El, TextStyle, Transition, col, el, row, text};
use rune_kit::{Badge, Button, ButtonVariant, RadioItem, TextArea, glyphs};

use super::super::controls::{DESCRIPTION, KeySize, METRICS, Paint, PushSize, keycap};
use super::super::found_choice::ChoiceRow;
use super::super::lists::allowed;
use super::dictated::Stage;
use super::field_text::Field;
use super::fixture::{ANSWER_TRY, Download};
use super::flight::{Flight, caret_x, ember_tone, flicker, flying, typeset};
use super::flows::continue_button;
use super::hex_meter::hex_meter;
use super::hud_stage::{flow_box, frame_slot, voice_or_idle_slot};
use super::landing::{LEAD, arrives, keys, ready, title_font};
use super::live::{Live, ModelStep};
use super::login::{self, Login, login_ask};
use super::mic_line::mic_capsule;
use super::pill::{CAPSULE, FLY, PILL, language_capsule, pill, slot};
use super::sparks::impacts;
use super::stories::{Onboard, Step, is_live_window};
use super::{live, permissions, versus};
use crate::desktop::i18n::{tr, tr_fill};

const TOP_BAR: f32 = 48.0;
const ACTION_H: f32 = 32.0;

const TRY_W: f32 = 468.0;
const TRY_HUD: [f32; 2] = [TRY_W * 0.5, 36.0];
const TRY_FIELD_X: f32 = 14.0;
const TRY_FIELD_Y: f32 = 96.0;
const TRY_FIELD_W: f32 = TRY_W - 2.0 * TRY_FIELD_X;
const TRY_PAD: f32 = 18.0;
const TRY_TEXT: Field = Field {
    font: TextStyle::new(18.0, 460.0),
    line: 26.0,
    width: TRY_FIELD_W - 2.0 * TRY_PAD,
};
const HOT: f32 = 0.52;
const PRACTICE_HUD_H: f32 = 74.0;
const PRACTICE_GAP: f32 = 16.0;
/// Lines the practice field grows to before it scrolls.
const PRACTICE_ROWS: usize = 5;
const KNOCK: f32 = 8.0;
const BURST: f64 = 0.06;

pub(super) fn view(p: Paint, s: &Onboard) -> Vec<El> {
    let sketch = super::sketches::playing(s);
    let centred = matches!(s.step, Step::Pick | Step::Started | Step::Waiting) && !sketch;
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
    let top = top
        .children(back(p, s))
        .children(reopen(p, s))
        .children(skip(p, s));
    // The real window has no title bar: the top of the page moves it.
    let top = if s.live.is_some() {
        super::super::title_bar(top)
    } else {
        top
    };
    let page = match s.step {
        Step::Pick | Step::Started => download_page(p, s, false),
        Step::Waiting if sketch => sketch_page(p, s),
        Step::Waiting => download_page(p, s, true),
        Step::Parked | Step::YourTurn | Step::Hearing | Step::Heard | Step::Done => demo_page(p, s),
        Step::Microphone | Step::Accessibility => permissions::page(p, s),
        Step::Try | Step::TryHearing | Step::TryHeard => {
            super::game::page(p, s).unwrap_or_else(|| try_page(p, s))
        }
        Step::Ready => ready_page(p, s),
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

/// Skip, quiet in the top-right corner of every page but the last: ends
/// onboarding at once, with the model still downloading behind it.
fn skip(p: Paint, s: &Onboard) -> Option<El> {
    s.live.as_ref().filter(|_| s.step != Step::Ready).map(|_| {
        let label = tr("Skip");
        let quiet = p.muted();
        row()
            .key("skip-corner")
            .absolute(0.0, 8.0)
            .w_full()
            .pr(20.0)
            .justify_end()
            .child(
                Button::new(label)
                    .variant(ButtonVariant::Ghost)
                    .label_with(move |font, _| text(label).key("label").font(font).color(quiet))
                    .on_press(live::skip)
                    .build_push(p.k)
                    .key("skip"),
            )
    })
}

/// Hex's shortcut didn't start after setup and Hex won't relaunch itself
/// again: one line at the top with Reopen Hex.
fn reopen(p: Paint, s: &Onboard) -> Option<El> {
    s.live.as_ref().filter(|live| live.reopen).map(|_| {
        row()
            .key("reopen")
            .absolute(0.0, 8.0)
            .w_full()
            .gap(10.0)
            .center()
            .transition(arrives())
            .child(
                text(tr("Hex's shortcut didn't start."))
                    .key("why")
                    .font(LEAD)
                    .color(p.muted()),
            )
            .child(
                Button::new(tr("Reopen Hex"))
                    .variant(ButtonVariant::Neutral)
                    .on_press(live::reopen)
                    .build_push(p.k)
                    .key("reopen-hex"),
            )
    })
}

/// Back to the model step, on the pages after it, once the person chose
/// from it.
fn back(p: Paint, s: &Onboard) -> Option<El> {
    let chosen = s
        .live
        .as_ref()
        .is_some_and(|live| live.model_step == ModelStep::Chosen);
    let after = matches!(
        s.step,
        Step::Parked
            | Step::YourTurn
            | Step::Hearing
            | Step::Heard
            | Step::Done
            | Step::Microphone
            | Step::Waiting
    );
    (chosen && after).then(|| {
        Button::new(tr("Back"))
            .variant(ButtonVariant::Ghost)
            .on_press(live::back_to_models)
            .build_push(p.k)
            .key("back")
            .absolute(20.0, 8.0)
            .transition(arrives())
    })
}

/// The model step: the models found on this computer, Hex's recommended
/// one first and selected. A pick starts at once; Continue moves on once it
/// is under way.
fn models_page(p: Paint, s: &Onboard, live: &Live) -> El {
    let k = p.k;
    let mut list = permissions::card(p).key("model-list").py(4.0);
    for (index, model) in live.models.iter().enumerate() {
        if index > 0 {
            list = list.child(permissions::separator(p, &format!("sep{index}")));
        }
        list = list.child(model_row(p, index, model));
    }
    let list = k.elevate(list, &k.elevation.raised);
    col()
        .key("models")
        .gap(22.0)
        .items_center()
        .child(heading(
            p,
            s,
            "models-head",
            tr("Choose a speech model"),
            lead(p, tr("Hex found speech models already on this computer.")),
        ))
        .child(s.motion.staggered(list, 1.0, 8.0))
        .child(action(continue_model_button(p)))
}

fn model_row(p: Paint, index: usize, model: &ChoiceRow) -> El {
    let radio = RadioItem::new(&model.name, model.selected)
        .description(&model.line)
        .on_select_tx(move |ui: &mut super::super::Ui, tx: &mut rune::Tx| {
            tx.instant();
            live::pick_model(index)(ui);
        })
        .build(p.k)
        .key("radio")
        .grow(1.0);
    let tag = model
        .recommended
        .then(|| Badge::new(tr("Recommended")).build(p.k).key("recommended"));
    row()
        .key(format!("model{index}"))
        .min_h(METRICS.row_h_described)
        .mx(4.0)
        .px(12.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(p.k.c.hover)
        .on_press_tx(move |ui: &mut super::super::Ui, tx: &mut rune::Tx| {
            tx.instant();
            live::pick_model(index)(ui);
        })
        .child(radio)
        .children(tag)
}

fn continue_model_button(p: Paint) -> El {
    let label = tr("Continue");
    Button::new(label)
        .variant(ButtonVariant::Contrast)
        .label_with(move |font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text(label).key("label").font(font).color(ink))
                .child(
                    rune::icon(&glyphs::ARROW_RIGHT, 15.0)
                        .key("arrow")
                        .color(ink),
                )
        })
        .on_press(live::continue_model)
        .build_push(p.k)
        .key("continue")
        .default_button()
        .transition(arrives())
}

fn ready_page(p: Paint, s: &Onboard) -> El {
    let core = ready(
        p,
        &s.caps,
        (!s.had_model).then_some(PILL),
        s.ready_age(),
        s.reduced.get(),
        s.motion,
        s.home,
    );

    if s.live.is_some() {
        return col()
            .key("ready-live-wrap")
            .gap(20.0)
            .items_center()
            .child(core)
            .children(
                login_ask(p, &s.login, s.now, s.reduced.get(), login::toggle)
                    .map(|ask| ask.transition(arrives())),
            )
            .child(
                row()
                    .key("ready-actions")
                    .gap(10.0)
                    .center_y()
                    .transition(arrives())
                    .child(
                        Button::new(tr("Try again"))
                            .variant(ButtonVariant::Neutral)
                            .on_press(live::on_ui(Onboard::jump_to_try))
                            .build_push(p.k)
                            .key("try-again"),
                    )
                    .child(
                        Button::new(tr("Start using Hex"))
                            .variant(ButtonVariant::Contrast)
                            .on_press(live::finish)
                            .build_push(p.k)
                            .key("start-using-hex")
                            .default_button(),
                    ),
            );
    }
    let ask = login_ask(p, &s.login, s.now, s.reduced.get(), |s: &mut Onboard| {
        let next = if s.login.login == Login::On {
            Login::Off
        } else {
            Login::On
        };
        if s.login.set(next, s.now) {
            s.sfx.on_login(s.now);
        }
    });
    if !is_live_window() {
        return col()
            .key("ready-live-wrap")
            .gap(20.0)
            .items_center()
            .child(core)
            .children(ask);
    }
    col()
        .key("ready-live-wrap")
        .gap(20.0)
        .items_center()
        .child(core)
        .children(ask)
        .child(
            Button::new("Try dictating again")
                .variant(ButtonVariant::Neutral)
                .on_press(Onboard::jump_to_try)
                .build_push(p.k)
                .key("try-again")
                .transition(arrives()),
        )
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

fn heading(p: Paint, s: &Onboard, key: &str, title: &str, under: El) -> El {
    s.motion.heading(
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
        .child(text(tr("Hold")).key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", &s.caps, down))
        .child(text(rest).key("rest").font(LEAD).color(p.muted()))
}

fn download_page(p: Paint, s: &Onboard, waiting: bool) -> El {
    if let Some(live) = s
        .live
        .as_ref()
        .filter(|live| !waiting && live.model_step != ModelStep::None)
    {
        return models_page(p, s, live);
    }
    let d = download(s);

    let head = if waiting {
        heading(
            p,
            s,
            "waiting-head",
            tr("Almost ready"),
            lead(p, tr("Finishing the speech model so Hex can hear you.")),
        )
    } else {
        let subtitle = row()
            .key("download-line")
            .gap(7.0)
            .center_y()
            .child(
                text(tr("Downloading"))
                    .key("pre")
                    .font(LEAD)
                    .color(p.muted()),
            )
            .child(if s.live.is_some() {
                language_capsule(p, d.language, live::cycle_language)
            } else {
                language_capsule(p, d.language, Onboard::cycle_language)
            })
            .child(
                text(tr("speech model"))
                    .key("post")
                    .font(LEAD)
                    .color(p.muted()),
            );
        heading(p, s, "download-head", tr("Setting up Hex"), subtitle)
    };

    let pct = (d.fraction() * 100.0).round() as u32;
    let status_inner = if d.done() {
        allowed(p, "status", tr("Ready")).transition(Transition::opacity())
    } else {
        text(format!("{pct}%"))
            .key("status")
            .font(TextStyle::new(13.0, 520.0).lh(18.0).features("'tnum' 1"))
            .color(p.text())
    };
    let progress_capsule = row()
        .key("download-progress-pill")
        .h(32.0)
        .pl(10.0)
        .pr(13.0)
        .gap(8.0)
        .center_y()
        .radius(16.0)
        .fill(p.k.c.surface_raised)
        .border(p.hair, p.k.c.border_weak)
        .matched(CAPSULE)
        .motion(FLY)
        .width_leads()
        .child(hex_meter(p, "meter", 18.0, d.fraction(), s.closed))
        .child(status_inner);
    // "Almost ready" receives the download back from the top bar as a
    // quiet bar, by that flight rather than a staggered entrance, with
    // "Watch another one" beside it.
    let progress_capsule = if waiting {
        super::sketches::progress(p, s, d)
    } else {
        s.motion.staggered(progress_capsule, 2.0, 6.0)
    };

    // A network that blocks the download: why, then retry or use a model
    // file brought some other way (#894).
    let retry = s.live.as_ref().filter(|live| live.failed).map(|live| {
        let why = live
            .failure
            .clone()
            .unwrap_or_else(|| tr("The download stopped.").to_owned());
        col()
            .key("download-failed")
            .gap(12.0)
            .items_center()
            .transition(arrives())
            .child(
                text(why)
                    .key("why")
                    .font(LEAD)
                    .color(p.muted())
                    .align(rune::Align::Center)
                    .wrap()
                    .w(TRY_W),
            )
            .child(
                row()
                    .key("actions")
                    .gap(10.0)
                    .center_y()
                    .child(
                        Button::new(if live.using_file {
                            tr("Adding model…")
                        } else {
                            tr("Use a model file…")
                        })
                        .variant(ButtonVariant::Neutral)
                        .on_press(live::use_file)
                        .build_push(p.k)
                        .key("use-file"),
                    )
                    .child(
                        Button::new(tr("Try again"))
                            .variant(ButtonVariant::Neutral)
                            .on_press(live::retry_download)
                            .build_push(p.k)
                            .key("retry-download"),
                    ),
            )
    });
    col()
        .key(if waiting { "waiting" } else { "download" })
        .gap(22.0)
        .items_center()
        .child(flow_box(
            if waiting {
                "waiting-hud"
            } else {
                "download-hud"
            },
            1.24,
            s,
        ))
        .child(head)
        .child(progress_capsule)
        .children(retry)
}

/// A sketch from `"Watch another one"`, the download up in the top pill.
fn sketch_page(p: Paint, s: &Onboard) -> El {
    col()
        .key("sketch")
        .gap(16.0)
        .items_center()
        .child(heading(
            p,
            s,
            "without",
            tr("Without Hex"),
            lead(p, super::sketches::lead(s).unwrap_or_default()),
        ))
        .children(super::sketches::card(p, s))
}

fn demo_page(p: Paint, s: &Onboard) -> El {
    col()
        .key("demo")
        .gap(16.0)
        .items_center()
        .child(versus::heading(p, s))
        .child(versus::stage(p, s))
        .child(action(if s.step == Step::Done {
            continue_button(p, s)
        } else if versus::skippable(s) {
            super::flows::skip_button(p, s)
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
                Flight::new(TRY_HUD, to, from + at, HOT, TRY_TEXT.font.size, i),
                slot,
            )
        })
        .collect()
}

fn try_page(p: Paint, s: &Onboard) -> El {
    let holding = s.step == Step::TryHearing || s.live_hold_from.is_some();
    let head = match s.step {
        Step::TryHeard => heading(
            p,
            s,
            "try-heard",
            tr("That's Hex"),
            lead(p, tr("It types wherever your cursor is, in any app.")),
        ),
        _ => heading(
            p,
            s,
            "try",
            tr("Your turn"),
            hold(p, s, tr("and say anything."), holding),
        ),
    };
    let stage = if s.interactive_try() {
        interactive_try_stage(p, s)
    } else {
        try_stage(p, s)
    };
    col()
        .key("try-page")
        .gap(22.0)
        .items_center()
        .child(head)
        .child(s.motion.staggered(stage, 1.0, 8.0))
        .child(
            s.motion.staggered(
                col()
                    .key("try-foot")
                    .gap(14.0)
                    .items_center()
                    .child(action(if s.step == Step::TryHeard {
                        continue_button(p, s)
                    } else {
                        none()
                    }))
                    .child(instrument_bar(p, s)),
                2.0,
                8.0,
            ),
        )
}

fn instrument_bar(p: Paint, s: &Onboard) -> El {
    let down = s.step == Step::TryHearing || s.live_hold_from.is_some();
    row()
        .key("instrument-bar")
        .gap(8.0)
        .center_y()
        .child(mic_capsule(p, s.listening_peak()))
        .child(shortcut_capsule(p, &s.caps, down, s.live.is_none()))
}

/// The real first run shows the saved shortcut, which Settings changes.
fn shortcut_capsule(p: Paint, caps: &[String], down: bool, cycles: bool) -> El {
    let k = p.k;
    let c = &k.c;
    let label = if cycles {
        tr("Change shortcut")
    } else {
        tr("Your shortcut")
    };
    let pill = row()
        .key("change-shortcut")
        .h(28.0)
        .pl(5.0)
        .pr(11.0)
        .gap(7.0)
        .center_y()
        .radius(14.0)
        .fill(c.surface_raised)
        .border(p.hair, c.border_weak)
        .child(
            row().key("mini-keys").gap(2.0).center_y().children(
                caps.iter()
                    .enumerate()
                    .map(|(i, cap)| keycap(p, i, cap, KeySize::Small, down).scale(0.9)),
            ),
        )
        .child(text(label).key("label").font(DESCRIPTION).color(p.muted()));
    let mut pill = if cycles {
        pill.role("button")
            .label(label)
            .focusable(true)
            .focus_ring(1.5, 1.0, c.border_focus)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(Onboard::cycle_shortcut)
    } else {
        pill
    };
    for shadow in k.shadow_layers(&k.elevation.button_neutral) {
        pill = pill.box_shadow(shadow);
    }
    pill
}

/// Live interactive `"Your turn"` stage: real `Dot scope` HUD above a focused
/// [`TextArea`] that receives real dictated pastes from the running Hex app (or
/// direct typing).
fn interactive_try_stage(p: Paint, s: &Onboard) -> El {
    let hud_el = if let Some(frame) = s.hud.frame() {
        Some(frame_slot("try-dot-hud", TRY_HUD, 1.14, frame))
    } else if s.hud.finished() {
        None
    } else {
        voice_or_idle_slot("try-dot-hud", TRY_HUD, 1.14, true, &ANSWER_TRY, None, s).0
    };

    let shortcut = s.caps.join("");
    let stage = practice_stage(p);
    let reduced = s.reduced.get();
    let editor = TextArea::new(&s.practice)
        .placeholder(if s.dictated.hearing() {
            String::new()
        } else {
            tr_fill("Hold {} and say a sentence…", &shortcut)
        })
        .width(Some(TRY_FIELD_W))
        .auto_grow(3, PRACTICE_ROWS)
        .spans(s.dictated.spans(p, &stage, s.now, reduced));
    let editor = if s.live.is_some() {
        editor
            .on_edit(live::practice)
            .on_change(live::practice_changed)
    } else {
        editor
            .on_edit(|s: &mut Onboard| &mut s.practice)
            .on_change(Onboard::on_practice_change)
    }
    .build(p.k)
    .key("try-live-editor");

    col()
        .key("try-stage")
        .w(TRY_W)
        .items_center()
        .gap(PRACTICE_GAP)
        .child(
            el().key("try-hud-box")
                .size(TRY_W, PRACTICE_HUD_H)
                .children(hud_el),
        )
        .child(editor)
        .children(s.live.as_ref().and_then(|live| live::hint(p, live)))
        .child(s.dictated.overlay(p, "dictated", &stage, s.now, reduced))
}

/// Where the practice field's text sits under the plate: the kit's
/// [`TextArea`] pads its text 8 points and sets it in the UI face.
fn practice_stage(p: Paint) -> Stage {
    const PAD: f32 = 8.0;
    let line = (13.0f32 * 1.35 * 2.0).round() / 2.0;
    let field_y = PRACTICE_HUD_H + PRACTICE_GAP;
    Stage {
        from: TRY_HUD,
        origin: [TRY_FIELD_X + PAD, field_y + PAD],
        field: Field {
            font: p.k.ty.ui.lh(line),
            line,
            width: TRY_FIELD_W - 2.0 * PAD,
        },
        rows: PRACTICE_ROWS,
        size: [TRY_W, field_y + PRACTICE_ROWS as f32 * line + 2.0 * PAD],
    }
}

fn try_stage(p: Paint, s: &Onboard) -> El {
    let reduced = s.reduced.get();
    let hearing = s.step == Step::TryHearing;
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

    let hud_el = voice_or_idle_slot(
        "try-dot-hud",
        TRY_HUD,
        1.14,
        s.step == Step::Try,
        &ANSWER_TRY,
        s.take_from.filter(|_| s.step >= Step::TryHearing),
        s,
    )
    .0;

    let k = p.k;
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
            text(tr("Say anything…"))
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
        .children(hud_el)
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
        assert!(x > TRY_FIELD_X && y > TRY_HUD[1] + 24.0);
    }
}
