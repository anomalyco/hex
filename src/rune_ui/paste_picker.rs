//! The focus-free, single-selection surface. A row returns its frozen owner
//! identity; the output worker, not the view, resolves what will be inserted.
#![allow(
    dead_code,
    reason = "staged until the non-activating overlay is installed"
)]

use super::controls::Paint;
use crate::desktop::i18n::{event_age_at, tr};
use crate::paste::recent::{Id, Row};
use rune::{Anim, El, col, el, row, text};

pub(super) fn card<S: 'static>(
    rows: &[Row],
    cursor: usize,
    now: u64,
    p: Paint,
    pick: fn(&mut S, Id),
) -> El {
    let c = &p.k.c;
    let mut list = col().key("paste-rows").padding(4.).gap(2.);
    for (i, item) in rows.iter().enumerate() {
        let kind = match &item.id {
            Id::Dictation(_) => tr("Dictation"),
            Id::Capture(_) => tr("Annotate"),
        };
        let id = item.id.clone();
        let mut r = col()
            .key(format!("paste-row-{i}"))
            .role("button")
            .label(format!("{kind}: {}", item.preview))
            .focusable(false)
            .px(12.)
            .py(9.)
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
                    .max_lines(2),
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
