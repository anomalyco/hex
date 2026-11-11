//! Labs › Select multiple dictations: which History entries are selected,
//! how a row's press changes that, and the card that copies or deletes them
//! together. Off, History keeps its one selected entry and none of this
//! runs, so every press selects just its row as it always has.

use rune::{Drag, El, Phase, Tx, col, row, text};

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button};
use super::super::list_detail as ld;
use super::HistoryPane;
use crate::desktop::i18n::{tr, tr_fill};

/// Whether History selects several entries. The pane reads its own switch,
/// as it reads Labs › Keep recordings; `labs::history_multi_select` is the
/// same switch for this process.
pub(super) fn on(ui: &Ui) -> bool {
    crate::labs::Feature::HistoryMultiSelect.on(&ui.settings)
}

/// How far a press may travel and still be a click, as Rune pairs double
/// clicks.
const CLICK_SLOP: f32 = 4.0;

/// The modifiers a row's press was made with, read from `Tx::mods`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Click {
    /// Shift: select the range from the anchor to the row.
    shift: bool,
    /// Command on macOS, Control on Windows and Linux: toggle the row.
    cmd: bool,
}

impl Click {
    fn of(tx: &Tx) -> Self {
        let mods = tx.mods();
        Self {
            shift: mods.shift,
            cmd: mods.cmd,
        }
    }
}

/// Every selected entry's id, and the row a range starts from.
#[derive(Default)]
pub(in crate::rune_ui) struct Selection {
    ids: Vec<u64>,
    /// The row a Shift-click or a drag extends from: the last plain or
    /// Command press.
    anchor: Option<u64>,
    /// The row a pointer press landed on, whose release leaves it alone.
    pressed: Option<u64>,
    /// The row a pointer is dragging from, while its button is down.
    drag: Option<u64>,
    /// Whether that drag has reached a row other than the one it started on. Until it has,
    /// the press is still a click, so pointer movement selects nothing.
    drag_left: bool,
    /// Delete asks, and waits for its confirmation.
    pub(in crate::rune_ui) delete_armed: bool,
    /// Copy was pressed; its button says Copied until the selection changes.
    pub(in crate::rune_ui) copied: bool,
}

impl Selection {
    /// How many entries are selected.
    pub(in crate::rune_ui) fn len(&self) -> usize {
        self.ids.len()
    }

    pub(in crate::rune_ui) fn contains(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    /// The selected entry the detail card shows while one entry is selected:
    /// the anchor while it is still selected, else the newest selected one.
    pub(in crate::rune_ui) fn primary(&self) -> Option<u64> {
        self.anchor
            .filter(|id| self.ids.contains(id))
            .or_else(|| self.ids.first().copied())
    }

    /// Forgets the selection and its Delete confirmation.
    pub(in crate::rune_ui) fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
        self.pressed = None;
        self.drag = None;
        self.drag_left = false;
        self.delete_armed = false;
        self.copied = false;
    }

    fn select_only(&mut self, id: u64) {
        self.ids.clear();
        self.ids.push(id);
        self.anchor = Some(id);
        self.restated();
    }

    /// Selects `ids`, as a range press would, for a preview's representative
    /// state.
    pub(in crate::rune_ui) fn select(&mut self, ids: &[u64]) {
        self.ids = ids.to_vec();
        self.anchor = ids.last().copied();
    }

    fn toggle(&mut self, id: u64) {
        match self.ids.iter().position(|selected| *selected == id) {
            Some(index) => {
                self.ids.remove(index);
            }
            None => self.ids.push(id),
        }
        self.anchor = Some(id);
        self.restated();
    }

    /// Every row from `from` through `to` in `visible`, either way round.
    fn select_range(&mut self, from: u64, to: u64, visible: &[u64]) {
        let (Some(from), Some(to)) = (position(visible, from), position(visible, to)) else {
            // A row that left the list (a new search): keep the one pressed.
            self.select_only(to);
            return;
        };
        let (start, end) = if from <= to { (from, to) } else { (to, from) };
        self.ids = visible[start..=end].to_vec();
        self.restated();
    }

    /// A changed selection has not been copied and asks nothing.
    fn restated(&mut self) {
        self.delete_armed = false;
        self.copied = false;
    }
}

fn position(visible: &[u64], id: u64) -> Option<usize> {
    visible.iter().position(|row| *row == id)
}

/// The ids the list shows, newest first: the rows a range spans.
fn visible(pane: &HistoryPane) -> Vec<u64> {
    pane.entries.iter().map(|entry| entry.id).collect()
}

/// A press on entry `id`. Labs off, it selects that row alone, as History
/// always has; on, Shift selects the range from the anchor, Command (Control
/// on Windows and Linux) toggles the row, and a plain press selects it and
/// makes it the anchor.
fn choose(ui: &mut Ui, id: u64, click: Click) {
    ui.history.clear_armed = false;
    ui.history.delete_armed = None;
    if !on(ui) {
        ui.history.selected = Some(id);
        return;
    }
    let previous = ui.history.selection.anchor.or(ui.history.selected);
    let rows = visible(&ui.history);
    let selection = &mut ui.history.selection;
    if click.cmd {
        if selection.ids.is_empty() {
            // The one selected row, with nothing else in the set: Command
            // deselects it, as it would any other selected row.
            if ui.history.selected == Some(id) {
                selection.anchor = None;
                selection.restated();
                ui.history.selected = None;
                return;
            }
            // A row selected before the lab was turned on is part of the set: a
            // Command press adds it first, so toggling another row keeps it.
            if let Some(other) = ui.history.selected {
                selection.select_only(other);
            }
        }
        selection.toggle(id);
        let primary = selection.primary();
        ui.history.selected = primary;
        return;
    }
    match (click.shift, previous) {
        (true, Some(anchor)) => selection.select_range(anchor, id, &rows),
        _ => selection.select_only(id),
    }
    ui.history.selected = Some(id);
}

/// The pointer's press on entry `id`, which selected it on the way down.
fn press(ui: &mut Ui, id: u64, click: Click) {
    if on(ui) {
        ui.history.selection.pressed = Some(id);
    }
    choose(ui, id, click);
}

/// Whether this row's press belongs to the pointer press that already chose it
/// on the way down: its release presses nothing, whether or not the row is the
/// one the card shows. Space and the arrows press the focused row without a
/// pointer press, so they always choose.
fn pressed_here(ui: &mut Ui, id: u64) -> bool {
    ui.history.selection.pressed.take() == Some(id)
}

/// One History row's press: a plain press selects its row alone, Shift and
/// Command extend or toggle the selection, and a press dragged over other
/// rows selects from this one through the row under the pointer. `primary`
/// is whether this is the one selected row the detail card shows.
pub(super) fn gesture(row: El, id: u64, primary: bool) -> El {
    let gesture = move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| match drag.phase {
        Phase::Down => {
            tx.instant();
            if on(ui) {
                ui.history.selection.drag = Some(id);
                ui.history.selection.drag_left = false;
            }
            press(ui, id, Click::of(tx));
        }
        Phase::Move => dragged(ui, id, drag, tx),
        Phase::Up => {
            ui.history.selection.drag = None;
            // A release on the pressed row is still its click, even after a
            // drag out and back, so it keeps `pressed` for the Press that
            // follows on this same release to swallow instead of choosing the
            // row a second time. A release elsewhere, or a cancelled gesture,
            // presses nothing: the next keyboard activation of this row still
            // selects it.
            let x = drag.x;
            let y = drag.y;
            let inside = (0.0..drag.w).contains(&x) && (0.0..drag.h).contains(&y);
            if drag.cancelled() || !inside {
                ui.history.selection.pressed = None;
            }
        }
    };
    row.on_drag(gesture)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            // A pointer press chose this row on the way down, so its release
            // presses nothing and leaves no `pressed` behind for the next
            // keyboard activation. Space and the arrows come here without one.
            if pressed_here(ui, id) {
                return;
            }
            if !primary {
                tx.instant();
                choose(ui, id, Click::of(tx));
            }
        })
}

/// A drag from row `from`: selects through the row under the pointer, found
/// in the frame's drawn rows by its `history-entry{id}` key. Pointer jitter
/// under the click slop, and movement over the row the drag started on before
/// it reaches another, leaves the selection alone: a click with a shaky hand
/// still toggles or ranges what it landed on. Once the drag has reached another
/// row it is a drag, so coming back within the slop of the press still ranges
/// through the rows between.
fn dragged(ui: &mut Ui, from: u64, drag: &Drag, tx: &mut Tx) {
    if !on(ui) {
        return;
    }
    if !ui.history.selection.drag_left && drag.dx.abs() <= CLICK_SLOP && drag.dy.abs() <= CLICK_SLOP
    {
        return;
    }
    let point = drag.window_point();
    tx.element_at::<Ui>(
        point[0],
        point[1],
        move |ui: &mut Ui, picked, _: &mut Tx| {
            let Some(id) = picked.as_ref().and_then(entry_of) else {
                return;
            };
            if ui.history.selection.drag != Some(from) {
                return;
            }
            if id == from && !ui.history.selection.drag_left {
                return;
            }
            ui.history.selection.drag_left = true;
            ui.history.delete_armed = None;
            let rows = visible(&ui.history);
            ui.history.selection.select_range(from, id, &rows);
            ui.history.selected = Some(id);
        },
    );
}

/// The entry a picked element belongs to: a row keyed `history-entry{id}`,
/// or any element drawn inside one.
fn entry_of(picked: &rune::Picked) -> Option<u64> {
    picked
        .key()
        .and_then(entry_id)
        .or_else(|| picked.path().split('/').find_map(entry_id))
}

fn entry_id(part: &str) -> Option<u64> {
    part.strip_prefix("history-entry")?.parse().ok()
}

/// After the entries change (a search, a delete, new dictations): drops the
/// selected ids that are gone and gives the detail card one that remains.
pub(super) fn prune(pane: &mut HistoryPane) -> bool {
    let rows = visible(pane);
    let selection = &mut pane.selection;
    let before = selection.ids.len();
    selection.ids.retain(|id| rows.contains(id));
    if selection
        .anchor
        .is_some_and(|anchor| !rows.contains(&anchor))
    {
        selection.anchor = selection.ids.first().copied();
    }
    if selection
        .pressed
        .is_some_and(|pressed| !rows.contains(&pressed))
    {
        selection.pressed = None;
    }
    if selection.drag.is_some_and(|drag| !rows.contains(&drag)) {
        selection.drag = None;
        selection.drag_left = false;
    }
    let mut changed = selection.ids.len() != before;
    if changed {
        selection.restated();
    }
    if !selection.ids.is_empty() && pane.selected.is_none_or(|id| !selection.contains(id)) {
        pane.selected = selection.primary();
        changed = true;
    }
    changed
}

/// Labs › Select multiple dictations off: forgets the rest of a multiple
/// selection, keeping the one entry the detail card shows.
pub(super) fn collapse(pane: &mut HistoryPane) -> bool {
    if pane.selection.ids.is_empty() {
        return false;
    }
    if pane.selected.is_none() {
        pane.selected = pane.selection.primary();
    }
    pane.selection.clear();
    true
}

/// The selected entries' final texts, oldest first, a blank line between.
/// Entries with no text are left out, so Copy never pastes blank lines.
fn joined(pane: &HistoryPane) -> String {
    pane.entries
        .iter()
        .rev()
        .filter(|entry| pane.selection.contains(entry.id))
        .filter_map(|entry| {
            let text = entry.final_text.trim();
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Copies every selected entry's text.
fn copy(ui: &mut Ui, tx: &mut Tx) {
    let joined = joined(&ui.history);
    if joined.is_empty() {
        return;
    }
    tx.copy(joined);
    ui.history.selection.copied = true;
    ui.history.error = None;
}

/// Deletes every selected entry with one save, then forgets them. A failed
/// save keeps them selected, so the delete can be retried.
fn delete(ui: &mut Ui) {
    let ids = ui.history.selection.ids.clone();
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.delete_many(&ids) {
            ui.history.error = Some(error.to_string());
            ui.history.reload();
            return;
        }
        ui.history.reload();
    } else {
        ui.history.entries.retain(|entry| !ids.contains(&entry.id));
        ui.history.selected = None;
    }
    ui.history.selection.clear();
}

/// The detail area's card while two or more entries are selected: how many,
/// what Copy does, and Delete with its one confirmation.
pub(super) fn card(ui: &Ui, p: Paint) -> El {
    let selection = &ui.history.selection;
    let count = selection.len().to_string();
    let mut actions = row().key("actions").gap(8.0).child(button(
        p,
        "history-selection-copy",
        if selection.copied {
            tr("Copied")
        } else {
            tr("Copy")
        },
        ButtonKind::Secondary,
        |ui: &mut Ui, tx: &mut Tx| copy(ui, tx),
    ));
    actions = if selection.delete_armed {
        actions
            .child(button(
                p,
                "history-selection-confirm-delete",
                &tr_fill("Delete {} dictations", &count),
                ButtonKind::Danger,
                |ui: &mut Ui, _| delete(ui),
            ))
            .child(button(
                p,
                "history-selection-cancel-delete",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.history.selection.delete_armed = false,
            ))
    } else {
        actions.child(button(
            p,
            "history-selection-delete",
            tr("Delete…"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.history.selection.delete_armed = true,
        ))
    };
    col()
        .key("history-selection")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, &tr_fill("{} dictations selected", &count)),
            Vec::new(),
            Vec::new(),
        ))
        .child(ld::card_scroll(
            p,
            "selection-scroll",
            col()
                .key("selection-body")
                .gap(20.0)
                .child(
                    text(tr("Copy puts the selected dictations on the clipboard, oldest first, with a blank line between each"))
                        .key("hint")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                )
                .child(actions),
        ))
}
