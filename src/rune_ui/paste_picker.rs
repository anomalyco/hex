//! The focus-free, single-selection surface. A row returns its frozen owner
//! identity; the output worker, not the view, resolves what will be inserted.

use super::controls::Paint;
use crate::desktop::i18n::{event_age_at, tr};
use crate::paste::recent::{Id, Row};
use rune::{Anim, El, col, el, row, text};

pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use crate::paste::browse::{self, View};
    use rune::{
        DisplayTarget, KeyPolicy, OverlayInput, OverlayLevel, OverlayOptions, OverlaySpec,
        ScreenEdge,
    };
    if !crate::paste::recent::enabled() {
        return;
    }
    let spec = OverlaySpec {
        name: "Hex Paste Last",
        size: (422., 544.),
        edge: ScreenEdge::Top,
        margin: 40.,
        align: rune::Align::Center,
        inset: 0.,
        interactive: true,
        view: |v: &View, cx: &mut rune::Cx<View>| {
            let p = Paint::new(cx.system_dark(), cx.env().scale);
            let epoch = v.epoch;
            col().padding(16.).child(
                card(
                    &v.rows,
                    v.cursor,
                    crate::history::now_ms(),
                    p,
                    move |_: &mut View, id| browse::click(epoch, id),
                )
                .child(el().w_full().h(p.hair).fill(p.k.c.border_weak))
                .child(
                    row()
                        .key("paste-show-history")
                        .hit()
                        .role("button")
                        .label(tr("Show all History"))
                        .px(16.)
                        .py(9.)
                        .hover(p.k.c.hover)
                        .child(
                            text(tr("Show all History"))
                                .font(p.k.ty.small_medium)
                                .color(p.text()),
                        )
                        .on_press(move |_: &mut View| {
                            if browse::close_for_history(epoch) {
                                crate::macos_rune::open_paste_history();
                            }
                        }),
                ),
            )
        },
    }
    .options(OverlayOptions {
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
            *v = next;
            tx.instant();
            if v.open && !was {
                handle.show();
            } else if !v.open && was {
                handle.hide();
            }
        });
    }));
}

pub(super) fn card<S: 'static>(
    rows: &[Row],
    cursor: usize,
    now: u64,
    p: Paint,
    pick: impl Fn(&mut S, Id) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    // The header's 16 pt inset, so its title and every row's kind share one
    // left edge; rows fill from 8 so the highlight keeps its room.
    let mut list = col().key("paste-rows").padding(8.).gap(2.);
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
        let mut r = col()
            .key(format!("paste-row-{i}"))
            .hit()
            .role("button")
            .label(format!("{kind}: {}", item.preview))
            .focusable(false)
            .px(8.)
            .py(6.)
            .gap(3.)
            .radius(p.k.radius.md)
            .hover(c.hover)
            .press(c.pressed)
            .child(
                row()
                    .w_full()
                    .child(text(kind).font(p.k.ty.small_medium).color(p.muted()))
                    .child(el().grow(1.))
                    .child(
                        text(event_age_at(item.ready_at, now))
                            .font(p.k.ty.small)
                            .color(p.faint()),
                    ),
            )
            .child(
                text(&item.preview)
                    .font(p.k.ty.ui)
                    .color(p.text())
                    .wrap()
                    .max_lines(1),
            )
            .on_press(move |s: &mut S| pick(s, id.clone()));
        if i == cursor {
            r = r.fill(c.layer_02);
        }
        list = list.child(r);
    }
    let panel = col()
        .key("paste-card")
        .w(390.)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_base)
        .clip_soft(0.)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
        .child(
            row()
                .px(16.)
                .py(11.)
                .child(text(tr("Paste")).font(p.k.ty.ui_medium).color(p.text())),
        )
        .child(el().w_full().h(p.hair).fill(c.border_weak))
        .child(list);
    p.k.elevate(panel, &p.k.elevation.floating)
}

#[cfg(test)]
mod proof;
