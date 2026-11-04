//! A lab's drawer, over a dim at the window's right edge: what the lab
//! does, a preview, where it shows up, and its switch. Escape, the
//! close button, or a press on the dim closes it.

use rune::{Edge, El, Transition, Tx, col, el, row, text};
use rune_kit::{ButtonVariant, IconButton, IconButtonVariant};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint, modal_scrim_transition};
use super::category::Place;
use super::store::{feedback_button, lab_switch, tile};
use super::{Lab, Row, close_drawer, shown, unload_after};
use crate::desktop::i18n::tr;
use crate::labs::Feature;

const WIDTH: f32 = 360.0;
/// The drawer's inset from the window's edges, as the content panel's.
const INSET: f32 = 8.0;
const PADDING: f32 = 20.0;

/// The dim and the drawer while Labs shows one.
pub(in crate::rune_ui) fn drawer(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.labs_open {
        return None;
    }
    let lab = ui.lab_store.drawer?;
    let rows = shown(ui);
    let entry = rows.iter().find(|row| row.lab == lab)?;
    let newest = rows.iter().map(|row| row.lab.added()).max().unwrap_or(0);
    let new = newest > 0 && lab.added() == newest;
    let c = &p.k.c;
    let mut panel = col()
        .key("lab-drawer")
        .role("dialog")
        .label(entry.title)
        .pin(None, Some(INSET), Some(INSET), Some(INSET))
        .w(WIDTH)
        .radius(p.k.radius.lg)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the drawer stay inside.
        .on_press(|_: &mut Ui| {})
        .child(header(p, entry, new))
        .child(body(ui, p, entry))
        .child(footer(p, entry));
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let slide = Transition::move_edge(Edge::Trailing).springs(rune::SpringPair::new(
        p.k.motion.overlay,
        p.k.motion.overlay,
    ));
    Some([
        el().key("lab-drawer-scrim")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(c.scrim)
            .on_press(close_drawer)
            .transition(modal_scrim_transition(p)),
        panel.transition(slide).modal(true, close_drawer),
    ])
}

fn header(p: Paint, entry: &Row, new: bool) -> El {
    let category = if new {
        format!("{} · {}", entry.category.name(), tr("New"))
    } else {
        entry.category.name().to_string()
    };
    let close = IconButton::new(p.k.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .label(tr("Close"))
        .on_press(close_drawer)
        .build(p.k)
        .key("lab-drawer-close");
    row()
        .key("header")
        .pt(PADDING)
        .px(PADDING)
        .gap(12.0)
        .center_y()
        .child(tile(p, entry.icon, 44.0))
        .child(
            col()
                .key("names")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(
                    text(entry.title)
                        .key("title")
                        .font(p.k.ty.body_medium)
                        .color(p.text())
                        .wrap(),
                )
                .child(
                    text(category)
                        .key("category")
                        .font(DESCRIPTION)
                        .color(p.muted()),
                ),
        )
        .child(close)
}

fn body(ui: &Ui, p: Paint, entry: &Row) -> El {
    let preview = col()
        .key("preview")
        .h(160.0)
        .center()
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .border(p.hair, p.k.c.border_weak)
        .child(
            text(tr("A short preview goes here"))
                .key("preview-copy")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
    let mut body = col()
        .key("body")
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .padding(PADDING)
        .gap(16.0)
        .child(
            text(entry.about)
                .key("about")
                .font(p.k.ty.ui)
                .color(p.text())
                .wrap(),
        )
        .child(preview)
        .child(fact(
            p,
            "shows-up-in",
            tr("Shows up in"),
            place(ui, p, entry.lab),
        ));
    if entry.lab == Lab::Feature(Feature::UnloadIdle) && entry.on {
        body = body.child(fact(
            p,
            "unload-after",
            tr("Unload after"),
            unload_after(ui, p),
        ));
    }
    body
}

/// A label and its value, side by side.
fn fact(p: Paint, key: &str, label: &str, value: El) -> El {
    row()
        .key(key)
        .gap(8.0)
        .center_y()
        .child(text(label).key("label").font(DESCRIPTION).color(p.muted()))
        .child(value)
}

/// Where the lab shows up: a link to its pane while the sidebar shows it.
fn place(ui: &Ui, p: Paint, lab: Lab) -> El {
    let place = Place::of(lab);
    let linked = match place {
        Place::Pane(pane) => pane.shown(ui),
        Place::Playground => ui.settings.labs.playground,
        Place::Elsewhere(_) => false,
    };
    let copy = text(place.label())
        .key("place")
        .font(DESCRIPTION)
        .color(if linked { p.accent() } else { p.text() });
    if !linked {
        return copy;
    }
    copy.role("link")
        .label(place.label())
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| go(ui, place, tx))
}

fn go(ui: &mut Ui, place: Place, tx: &mut Tx) {
    close_drawer(ui);
    match place {
        Place::Pane(pane) => ui.select_pane(pane, tx),
        Place::Playground => tx.open_playground(),
        Place::Elsewhere(_) => {}
    }
}

fn footer(p: Paint, entry: &Row) -> El {
    let state = text(if entry.on { tr("On") } else { tr("Off") })
        .key("state")
        .font(p.k.ty.ui)
        .color(if entry.on { p.text() } else { p.muted() });
    let toggle = row()
        .key("toggle")
        .gap(10.0)
        .center_y()
        .child(state)
        .child(lab_switch(p, entry).key("lab-drawer-toggle"));
    let actions = row()
        .key("actions")
        .px(PADDING)
        .py(14.0)
        .gap(8.0)
        .center_y()
        .child(feedback_button(p, "lab-feedback", ButtonVariant::Ghost))
        .child(el().key("spacer").grow(1.0))
        .child(toggle);
    col()
        .key("footer")
        .child(el().key("rule").h(p.hair).w_full().fill(p.k.c.border_weak))
        .child(actions)
}
