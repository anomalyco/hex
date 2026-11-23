//! The focus-free, single-selection surface, centred on the display under
//! the pointer. A row returns its frozen owner identity; the output worker,
//! not the view, resolves what will be inserted. While open it covers that
//! display, so a click outside the card closes it without reaching the app.

use std::time::Duration;

use super::controls::{Paint, PushSize};
use super::list_detail::{Item, item_body};
use crate::desktop::i18n::{event_age_at, tr};
use crate::paste::recent::{Id, Row};
use rune::{Anim, El, col, el, row, text};
use rune_kit::{Button, ButtonVariant};

const WIDTH: f32 = 420.;
/// A calm appear and a quicker leave; the card never moves or scales.
const FADE_IN: Duration = Duration::from_millis(120);
const FADE_OUT: Duration = Duration::from_millis(100);

pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use crate::paste::browse::{self, View};
    use rune::{
        DisplayTarget, KeyPolicy, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement,
        OverlaySpec, ScreenEdge,
    };
    // Installed whatever the lab says, hidden until a picker opens, so turning
    // Labs › Paste Last picker on applies without a restart.
    let spec = OverlaySpec {
        name: "Hex Paste Last",
        size: (WIDTH, 560.),
        edge: ScreenEdge::Top,
        margin: 0.,
        align: rune::Align::Center,
        inset: 0.,
        interactive: true,
        view: |v: &View, cx: &mut rune::Cx<View>| {
            let p = Paint::new(cx.system_dark(), cx.env().scale);
            let epoch = v.epoch;
            stage(
                &v.rows,
                v.cursor,
                crate::history::now_ms(),
                p,
                move |_: &mut View, id| browse::click(epoch, id),
                move |_: &mut View| {
                    browse::dismiss(epoch);
                },
                move |_: &mut View| {
                    if browse::dismiss(epoch) {
                        crate::macos_rune::open_paste_history();
                    }
                },
            )
        },
    }
    .options(OverlayOptions {
        placement: Some(OverlayPlacement::FullScreen),
        input: OverlayInput::Regions,
        keys: KeyPolicy::None,
        display: DisplayTarget::UnderPointer,
        level: OverlayLevel::AboveOverlays,
        exclude_from_capture: true,
        ..Default::default()
    })
    .release_gpu_when_hidden();
    let panel = app.overlay(spec, View::default());
    browse::install(Box::new(move |next| {
        let handle = panel.clone();
        panel.update(move |v, tx| {
            if next.revision <= v.revision {
                return;
            }
            let was = v.open;
            if next.open {
                *v = next;
            } else {
                // The fade out keeps the rows and selection it closed on.
                v.revision = next.revision;
                v.epoch = next.epoch;
                v.open = false;
            }
            tx.instant();
            if v.open && !was {
                handle.fade_in(FADE_IN);
            } else if !v.open && was {
                handle.fade_out(FADE_OUT);
            }
        });
    }));
}

/// The whole overlay: the card centred over a clear layer that closes the
/// picker when pressed outside it.
pub(super) fn stage<S: 'static>(
    rows: &[Row],
    cursor: usize,
    now: u64,
    p: Paint,
    pick: impl Fn(&mut S, Id) + Copy + 'static,
    dismiss: impl Fn(&mut S) + 'static,
    history: impl Fn(&mut S) + 'static,
) -> El {
    let footer = row().key("paste-footer").px(13.).py(8.).child(
        Button::new(tr("Show all History"))
            .variant(ButtonVariant::Ghost)
            .on_press(history)
            .build_push(p.k)
            .key("paste-show-history"),
    );
    col()
        .key("paste-stage")
        .role("none")
        .w_full()
        .h_full()
        .center()
        .hit()
        .on_press(dismiss)
        .child(
            card(rows, cursor, now, p, pick)
                .child(el().w_full().h(p.hair).fill(p.k.c.border_weak))
                .child(footer),
        )
}

/// The card: a title over History's rows (the words, their kind, and their
/// age), the selection in History's selected fill.
pub(super) fn card<S: 'static>(
    rows: &[Row],
    cursor: usize,
    now: u64,
    p: Paint,
    pick: impl Fn(&mut S, Id) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    // The list's 8 pt inset plus a row's 16 pt padding: the title, every
    // row's words, and the footer's button text share one left edge.
    let mut list = col().key("paste-rows").role("list").padding(8.).gap(2.);
    for (i, item) in rows.iter().enumerate() {
        let kind = match &item.id {
            Id::Dictation(_) => tr("Dictation"),
            Id::Capture(_) => tr("Annotation"),
        };
        let kind = if item.cancelled {
            format!("{kind} · {}", tr("Cancelled"))
        } else {
            kind.to_owned()
        };
        let id = item.id.clone();
        let body = item_body(
            p,
            Item {
                key: format!("paste-row-{i}"),
                title: item.preview.clone(),
                meta: kind,
                meta_ink: None,
                time: event_age_at(item.ready_at, now),
                hover: Vec::new(),
                selected: i == cursor,
            },
            None,
        );
        list = list.child(
            body.hit()
                .focusable(false)
                .press(c.pressed)
                .on_press(move |s: &mut S| pick(s, id.clone())),
        );
    }
    let mut panel = col()
        .key("paste-card")
        .role("dialog")
        .label(tr("Paste"))
        .w(WIDTH)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Children sit inside the border, so their fills never paint over it.
        .padding(p.hair)
        .clip_soft(0.)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
        .hit()
        // Presses inside the card stay inside.
        .on_press(|_: &mut S| {})
        .child(
            row().key("paste-header").h(44.).px(24.).center_y().child(
                text(tr("Paste"))
                    .role("heading")
                    .level(2)
                    .font(p.k.ty.body_medium)
                    .color(p.text()),
            ),
        )
        .child(el().w_full().h(p.hair).fill(c.border_weak))
        .child(list);
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel
}

/// The picker over the window for the `mac-paste-picker` scene, on fixed
/// rows that resolve to nothing.
pub(super) fn preview(p: Paint) -> El {
    use crate::history::paste_source;
    const NOW: u64 = 1_795_262_400_000;
    const MIN: u64 = 60_000;
    let words = |n, ago, cancelled, preview: &str| Row {
        ready_at: NOW - ago * MIN,
        id: Id::Dictation(paste_source::Id::Retained(n)),
        cancelled,
        preview: preview.into(),
    };
    let rows = [
        words(5, 1, false, "Let's move the launch review to Thursday."),
        Row {
            ready_at: NOW - 8 * MIN,
            id: Id::Capture(crate::capture_host::paste_source::Id::fixture(
                NOW - 8 * MIN,
            )),
            cancelled: false,
            preview: "Move the caption below the chart.".into(),
        },
        words(4, 14, true, "Actually, scratch that last part."),
        words(
            3,
            52,
            false,
            "Thanks, that works. I'll send the notes after lunch.",
        ),
        words(
            2,
            190,
            false,
            "Can you check whether the Windows build still fails?",
        ),
    ];
    stage(
        &rows,
        0,
        NOW,
        p,
        |_: &mut super::Ui, _| {},
        |_: &mut super::Ui| {},
        |_: &mut super::Ui| {},
    )
    .absolute(0., 0.)
}

#[cfg(test)]
mod proof;
