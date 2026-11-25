//! Labs › Delete transcript lines: choosing saved turns in the Meetings pane,
//! the selection bar, and the one confirmed request that removes them.
//!
//! A displayed turn can cover several published entries; selecting it selects
//! all of them, and the request carries their exact keys. Counts speak of
//! turns, which is what the person clicked, not the lines underneath.

use rune::{Drag, El, Icon, Phase, Tx, col, row, text};

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button};
use super::super::list_detail as ld;
use super::{Meeting, Turn, ask};
use crate::desktop::i18n::{tr, tr_fill};
use crate::meeting::TranscriptEntryKey;
use crate::rune_ui::platform::MeetingRequest;

/// A list of lines with a check column, for the detail header.
static SELECT_ICON: Icon = Icon {
    name: "hex-select-lines",
    view: 16.0,
    body: r#"<path d="M2.5 4.5l1.5 1.5 3-3M2.5 9.5l1.5 1.5 3-3M2.5 14.5l1.5 1.5 3-3M10 5h4M10 10h4M10 15h4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// An unchecked line, before its turn.
static EMPTY_BOX: Icon = Icon {
    name: "hex-line-box",
    view: 16.0,
    body: r#"<rect x="2.75" y="2.75" width="10.5" height="10.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.4"/>"#,
};

/// A checked line, before its turn.
static CHECKED_BOX: Icon = Icon {
    name: "hex-line-checked",
    view: 16.0,
    body: r#"<rect x="2.75" y="2.75" width="10.5" height="10.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M5.2 8.2l1.8 1.8 3.8-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// The selected turns, each with the published lines it covers, and whether
/// the delete asks for its confirmation.
#[derive(Default)]
pub(in crate::rune_ui) struct Selection {
    /// A saved transcript is showing its selection checkboxes.
    active: bool,
    /// Every selected turn's published lines, in the order they were chosen.
    turns: Vec<Vec<TranscriptEntryKey>>,
    /// Delete asks, and waits for its confirmation.
    pub(in crate::rune_ui) delete_armed: bool,
    /// The turn a pointer press already toggled on the way down, whose
    /// release must not toggle it again.
    pressed: Option<Vec<TranscriptEntryKey>>,
}

impl Selection {
    pub(in crate::rune_ui) fn active(&self) -> bool {
        self.active
    }

    /// How many turns are selected, which is what the bar counts.
    pub(in crate::rune_ui) fn len(&self) -> usize {
        self.turns.len()
    }

    /// Starts selecting, replacing any earlier selection.
    pub(in crate::rune_ui) fn start(&mut self) {
        self.active = true;
        self.delete_armed = false;
        self.pressed = None;
        self.turns.clear();
    }

    /// Leaves selection mode.
    pub(in crate::rune_ui) fn stop(&mut self) {
        self.active = false;
        self.delete_armed = false;
        self.pressed = None;
        self.turns.clear();
    }

    /// Whether this turn, with exactly these lines, is selected.
    pub(super) fn contains(&self, turn: &Turn) -> bool {
        !turn.entries.is_empty() && self.turns.iter().any(|keys| keys == &turn.entries)
    }

    /// Selects the turn's lines, or clears them when it was already selected.
    fn toggle(&mut self, entries: &[TranscriptEntryKey]) {
        if entries.is_empty() {
            return;
        }
        match self.turns.iter().position(|keys| keys == entries) {
            Some(index) => {
                self.turns.remove(index);
            }
            None => self.turns.push(entries.to_vec()),
        }
        self.delete_armed = false;
    }

    /// Every selected turn's lines, for the one request that removes them.
    fn keys(&self) -> Vec<TranscriptEntryKey> {
        self.turns.iter().flatten().copied().collect()
    }

    /// Selects `keys`, one turn per key, as a preview's representative state.
    pub(in crate::rune_ui) fn select_lines(&mut self, keys: &[TranscriptEntryKey]) {
        self.start();
        self.turns = keys.iter().map(|key| vec![*key]).collect();
    }

    /// Escape: the confirmation first, then selection mode itself. False
    /// when nothing is being selected.
    pub(in crate::rune_ui) fn escape(&mut self) -> bool {
        if !self.active {
            return false;
        }
        if self.delete_armed {
            self.delete_armed = false;
        } else {
            self.stop();
        }
        true
    }
}

/// The detail header's way into selection mode.
pub(super) fn action(p: Paint) -> El {
    ld::ghost_icon(
        p,
        "meeting-select-lines",
        &SELECT_ICON,
        tr("Select lines"),
        |ui: &mut Ui, _| ui.meetings.selection.start(),
    )
}

/// The count, what deleting removes besides the lines, and Delete with its
/// one confirmation.
pub(super) fn bar(ui: &Ui, p: Paint, meeting: &Meeting) -> El {
    let selection = &ui.meetings.selection;
    let count = selection.len();
    let count_text = count.to_string();
    let armed_label = if count == 1 {
        tr("Delete 1 line").to_string()
    } else {
        tr_fill("Delete {} lines", &count_text)
    };
    let id = meeting.id.clone();
    let mut actions = row().key("selection-actions").gap(8.0);
    if selection.delete_armed {
        actions = actions
            .child(
                button(
                    p,
                    "meeting-confirm-delete-lines",
                    &armed_label,
                    ButtonKind::Danger,
                    move |ui: &mut Ui, _| confirm(ui, &id),
                )
                .default_button(),
            )
            .child(button(
                p,
                "meeting-cancel-delete-lines",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.meetings.selection.delete_armed = false,
            ));
    } else {
        if selection.len() > 0 {
            actions = actions.child(button(
                p,
                "meeting-delete-lines",
                tr("Delete lines…"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.meetings.selection.delete_armed = true,
            ));
        }
        actions = actions.child(button(
            p,
            "meeting-cancel-selection",
            tr("Cancel"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.meetings.selection.stop(),
        ));
    }
    col()
        .key("meeting-selection")
        .gap(8.0)
        .px(12.0)
        .py(8.0)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .child(
            text(if count == 1 {
                tr("1 line selected").to_string()
            } else {
                tr_fill("{} lines selected", &count_text)
            })
            .key("count")
            .font(p.k.ty.body_medium)
            .color(p.text()),
        )
        .children(
            (meeting.summary.is_some() || !meeting.chapters.is_empty()).then(|| {
                text(tr(
                    "Its summary and chapters will be removed. Summarize again for a fresh one.",
                ))
                .key("hint")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap()
            }),
        )
        .children((selection.len() == 0).then(|| {
            text(tr("Select the lines to delete."))
                .key("empty-hint")
                .font(DESCRIPTION)
                .color(p.faint())
        }))
        .child(actions)
}

/// Sends one request for every selected line. A changed selection clears the
/// request's own confirmation, so this only runs after that was answered.
fn confirm(ui: &mut Ui, id: &str) {
    let entries = ui.meetings.selection.keys();
    if entries.is_empty() {
        return;
    }
    if ask(
        ui,
        MeetingRequest::DeleteTranscript {
            id: id.into(),
            entries,
        },
    ) {
        ui.meetings.selection.stop();
    }
}

/// One turn in selection mode: a check column and the turn itself, pressed
/// as a whole. Adjacent entries under the turn go together.
pub(super) fn turn_row(p: Paint, index: usize, turn: &Turn, imported: bool, selected: bool) -> El {
    let box_icon = rune::icon(if selected { &CHECKED_BOX } else { &EMPTY_BOX }, 14.0)
        .key("check")
        .color(if selected { p.accent() } else { p.faint() })
        .pt(1.0)
        .a11y_hidden();
    let body = col()
        .key("content")
        .gap(0.0)
        .grow(1.0)
        .shrink()
        .children(super::turn_line(p, index, turn, imported));
    let mut line = row()
        .key(format!("select-turn{index}"))
        .role("checkbox")
        .label(if turn.mine { tr("You") } else { tr("Them") })
        .description(turn.text.clone())
        .checked(selected)
        .gap(8.0)
        .px(12.0)
        .py(8.0)
        .radius(p.k.radius.md)
        .hover(p.k.c.hover)
        .hover_group()
        .child(box_icon)
        .child(body);
    if selected {
        line = line.fill(p.k.c.layer_02);
    }
    if turn.entries.is_empty() {
        return line;
    }
    let down = turn.entries.clone();
    let release = turn.entries.clone();
    line.on_drag(move |ui: &mut Ui, drag: &mut Drag, tx: &mut Tx| {
        if drag.phase == Phase::Down {
            tx.instant();
            ui.meetings.selection.pressed = Some(down.clone());
            ui.meetings.selection.toggle(&down);
        }
    })
    .on_press_tx(move |ui: &mut Ui, _: &mut Tx| {
        if ui.meetings.selection.pressed.take().as_ref() == Some(&release) {
            return;
        }
        ui.meetings.selection.toggle(&release);
    })
}
