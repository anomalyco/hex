//! Find a setting or lab in this window. Every result is a destination, never an action.
use super::*;

use rune::KeyEvent;
use rune_kit::{Placement, TextInput, popover};
use std::cell::Cell;

#[derive(Default)]
pub(super) struct Find {
    pub(super) open: bool,
    clear_layer: Cell<bool>,
    field: TextEdit,
    highlight: controls::Highlight,
    entries: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Setting(Pane, &'static str, bool), // pane, actual row key, needs Advanced
    Lab(labs::Lab),
}

#[derive(Clone)]
struct Entry {
    link: Link,
    title: &'static str,
    about: std::borrow::Cow<'static, str>,
    path: String,
}

mod catalog;
use catalog::entries;

#[cfg(test)]
mod tests;

impl Find {
    pub(super) fn local_find_allowed(&self, e: &KeyEvent) -> bool {
        e.is_command('f') && !e.mods.shift && !e.mods.alt && self.clear_layer.get()
    }
}

pub(super) fn open_key(ui: &mut Ui, e: &KeyEvent, tx: &mut Tx) -> bool {
    let general_find = ui.pane == Pane::General && !ui.labs_open && ui.find.local_find_allowed(e);
    if !(e.is_command('k') || general_find) || e.mods.shift || e.mods.alt || (cfg!(target_os="macos") && e.mods.ctrl)
        || !ui.find.clear_layer.get() || ui.tour.is_some() || ui.open.is_some()
        // Rune Kit's TextInput and TextArea focus their inner `input` element.
        || matches!(ui.capture, settings::Capture::Listening { .. } | settings::Capture::Saving { .. })
        || tx.focused() == Some("input") || tx.key_repeat()
    {
        return false;
    }
    ui.find.entries = entries(ui);
    ui.find.highlight = Default::default();
    ui.find.field = Default::default();
    ui.find.open = true;
    true
}

fn results(find: &Find) -> Vec<&Entry> {
    let q = find.field.text.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<_> = find
        .entries
        .iter()
        .filter_map(|e| {
            let title = e.title.to_lowercase();
            let rank = if title == q {
                0
            } else if title.starts_with(&q) {
                1
            } else if title.contains(&q) {
                2
            } else if e.about.to_lowercase().contains(&q) || e.path.to_lowercase().contains(&q) {
                3
            } else {
                return None;
            };
            Some((rank, e))
        })
        .collect();
    hits.sort_by_key(|(rank, _)| *rank);
    hits.into_iter().map(|(_, e)| e).collect()
}

pub(super) fn close(ui: &mut Ui) {
    ui.find.open = false;
    ui.find.entries.clear();
}

fn reveal(ui: &mut Ui, tx: &mut Tx, link: Link) {
    close(ui);
    // A service update can remove a destination between opening and Return.
    // Never leave a draft by forcing its normal pane-change save.
    if !entries(ui).iter().any(|e| e.link == link) || ui.drafted() != ui.settings {
        return;
    }
    match link {
        Link::Lab(lab) => labs::reveal_card(ui, lab, tx),
        Link::Setting(pane, key, advanced) => {
            ui.select_pane(pane, tx);
            if advanced && !ui.advanced.contains(&pane) {
                ui.advanced.push(pane);
            }
            tx.scroll_into_view(key);
            tx.focus(key, true); // Focus the row’s control, never press it.
        }
    }
}

fn row_key(i: usize) -> String {
    format!("find-result-{i}")
}

pub(super) fn layer(ui: &Ui, p: Paint, layer: &mut Vec<El>) {
    // Includes the pane's menus, model/app pickers, sheets and onboarding. Do
    // not keep a second (incomplete) list of which overlay is active.
    ui.find.clear_layer.set(layer.is_empty());
    if !ui.find.open || !layer.is_empty() {
        return;
    }
    let field = TextInput::new(&ui.find.field)
        .placeholder(tr("Find a setting or lab"))
        .leading_icon(p.k.glyph.search)
        .width(None)
        .on_edit(|ui: &mut Ui| &mut ui.find.field)
        .on_change(|_: &mut Ui, tx: &mut Tx| tx.scroll_into_view(row_key(0)))
        .on_key(|ui: &mut Ui, e: &KeyEvent, tx: &mut Tx| {
            let n = results(&ui.find).len();
            let q = &ui.find.field.text;
            if !ui.find.highlight.key(&e.key, q, n) {
                return false;
            }
            if let Some(i) = ui.find.highlight.index(q, n) {
                tx.scroll_into_view(row_key(i));
            }
            true
        })
        .on_submit(|ui: &mut Ui, tx: &mut Tx| {
            let hits = results(&ui.find);
            if let Some(i) = ui.find.highlight.index(&ui.find.field.text, hits.len()) {
                let link = hits[i].link;
                reveal(ui, tx, link);
            }
        })
        .build(p.k)
        .key("find-field");
    let hits = results(&ui.find);
    let selected = ui.find.highlight.index(&ui.find.field.text, hits.len());
    let mut rows = col().key("find-results").padding(4.0);
    let querying = !ui.find.field.text.trim().is_empty();
    if querying && hits.is_empty() {
        rows = rows.child(
            text(tr("No matching settings or labs"))
                .padding(12.0)
                .font(p.k.ty.ui)
                .color(p.muted()),
        );
    }
    for (i, hit) in hits.into_iter().enumerate() {
        let link = hit.link;
        let glyph = match link {
            Link::Lab(_) => &labs::LABS_ICON,
            Link::Setting(pane, ..) => pane.icon(),
        };
        let mut item = row()
            .key(row_key(i))
            .role("button")
            .label(format!("{}, {}", hit.title, hit.path))
            .px(10.0)
            .py(7.0)
            .gap(10.0)
            .center_y()
            .radius(p.k.radius.sm)
            .hover(p.k.c.hover)
            .press(p.k.c.pressed)
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| reveal(ui, tx, link));
        if selected == Some(i) {
            item = item.fill(p.k.c.hover);
        }
        rows = rows.child(
            item.child(icon(glyph, 16.0).color(p.muted())).child(
                col()
                    .grow(1.0)
                    .shrink()
                    .gap(2.0)
                    .child(
                        row()
                            .gap(10.0)
                            .center_y()
                            .child(
                                text(hit.title)
                                    .font(p.k.ty.ui_medium)
                                    .color(p.text())
                                    .grow(1.0)
                                    .shrink()
                                    .truncate(),
                            )
                            .child(text(hit.path.clone()).font(p.k.ty.small).color(p.muted())),
                    )
                    .child(
                        text(hit.about.clone())
                            .font(p.k.ty.small)
                            .color(p.muted())
                            .truncate(),
                    ),
            ),
        );
    }
    let panel = col()
        .key("find-popover")
        .role("dialog")
        .label(tr("Find a setting or lab"))
        .modal(true, close)
        .w(448.0)
        .min_h(0.0)
        .shrink()
        .child(col().padding(8.0).child(field))
        .children(querying.then(|| rune::rule(p.hair, p.k.c.border_weak)))
        .children(querying.then(|| popover::fit_scroll(rows).max_h(202.0)));
    layer.push(popover::anchored(
        p.k,
        popover::raised(p.k, panel, p.k.radius.lg),
        "pane-header",
        Placement::Bottom,
        2.0,
    ));
}
