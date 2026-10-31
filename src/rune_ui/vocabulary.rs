//! Vocabulary: a sheet from General's Vocabulary row with one row per term
//! and its "sounds like" spellings, edited in place and saved when typing
//! pauses, like a mode's corrections. The boost has no control here; it
//! stays a setting (`hex settings set vocabulary_boost`).

use rune::tok::motion::{SETTLE, SETTLE_EXIT};
use rune::{El, SpringPair, TextEdit, Transition, Tx, col, edit_access, el, row, text};
use rune_kit::{ControlSize, TextInput};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, modal_scrim_transition, modal_transition, note,
};
use super::lists::remove_button;
use crate::desktop_i18n::{tr, tr_fill};
use crate::vocabulary::{self, VocabularyEntry};

const MODAL_WIDTH: f32 = 620.0;
const PADDING: f32 = 18.0;
/// One term row: a regular text field and its padding.
const ROW_HEIGHT: f32 = 36.0;
/// The term list's fades; the bottom one is the gap before the footer at
/// rest.
const FADES: (f32, f32) = (0.0, 6.0);
/// The term rows scroll past eight whole rows, so a list scrolled to its
/// end shows no sliver of a row above.
const ROWS_HEIGHT: f32 = 8.0 * ROW_HEIGHT + FADES.0 + FADES.1;
/// Room beside the rows inside the list's clip, so the fields' borders
/// aren't cut at its sides.
const SIDE_ROOM: f32 = 2.0;
const TERM_WIDTH: f32 = 200.0;

/// One term row: its term and "sounds like" fields, and an identity that
/// stays with the row as rows before it are removed, so a callback built
/// for a removed row never reaches another one.
struct TermRow {
    id: u64,
    term: TextEdit,
    sounds_like: TextEdit,
}

/// The term rows, and whether the sheet is open.
#[derive(Default)]
pub(super) struct VocabularyEditor {
    rows: Vec<TermRow>,
    next_id: u64,
    /// What an edit for a row that is gone lands in; never saved.
    detached: TextEdit,
    /// The row Add term made last while the sheet is open: only it arrives
    /// with motion, so opening the sheet never animates its rows.
    added: Option<u64>,
    pub(super) open: bool,
}

impl VocabularyEditor {
    pub(super) fn new(entries: &[VocabularyEntry]) -> Self {
        let mut editor = Self::default();
        editor.reload(entries);
        editor
    }

    /// Replaces the rows with `entries`, keeping the sheet open or shut.
    pub(super) fn reload(&mut self, entries: &[VocabularyEntry]) {
        self.rows.clear();
        self.added = None;
        for entry in entries {
            self.push(
                TextEdit::new(entry.term.clone()),
                TextEdit::new(entry.sounds_like.join(", ")),
            );
        }
    }

    /// Adds a row at the end and returns its identity.
    pub(super) fn push(&mut self, term: TextEdit, sounds_like: TextEdit) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(TermRow {
            id,
            term,
            sounds_like,
        });
        id
    }

    /// The term (or, with `sounds_like`, the spellings) field of row `id`.
    /// A row already removed gets a detached field instead, so a late edit
    /// or focus change for it touches nothing that is saved.
    fn field(&mut self, id: u64, sounds_like: bool) -> &mut TextEdit {
        match self.rows.iter_mut().find(|row| row.id == id) {
            Some(row) if sounds_like => &mut row.sounds_like,
            Some(row) => &mut row.term,
            None => {
                self.detached = TextEdit::default();
                &mut self.detached
            }
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn last_row(&self) -> Option<u64> {
        self.rows.last().map(|row| row.id)
    }

    /// The rows as saved entries: blank terms are left out and repeated
    /// terms merge, as `hex vocabulary add` does.
    pub(super) fn project(&self) -> Vec<VocabularyEntry> {
        self.projection().0
    }

    /// Whether saving cuts or leaves out text that doesn't fit the limits.
    fn clamped(&self) -> bool {
        self.projection().1
    }

    fn projection(&self) -> (Vec<VocabularyEntry>, bool) {
        let mut entries = Vec::new();
        let mut clamped = false;
        for row in &self.rows {
            clamped |= vocabulary::add_clamped(
                &mut entries,
                &row.term.text,
                &vocabulary::parse_sounds_like(&row.sounds_like.text),
            );
        }
        (entries, clamped)
    }
}

/// What General's Vocabulary row shows: how many terms are saved.
pub(super) fn summary(ui: &Ui) -> String {
    match ui.settings.vocabulary.len() {
        0 => tr("None").to_string(),
        1 => tr("1 term").to_string(),
        count => tr_fill("{} terms", &count.to_string()),
    }
}

/// Why the list doesn't reach transcription, when the selected model takes
/// none (Parakeet biases toward it; Whisper and Qwen3-ASR read it as a
/// prompt).
pub(super) fn unused_note(ui: &Ui) -> Option<String> {
    let model = crate::transcription::models::definition(ui.settings.transcription.model);
    (!model.uses_vocabulary()).then(|| {
        tr_fill(
            "{} doesn't use these terms; rewriting and search still do",
            model.name,
        )
    })
}

/// Opens the sheet alone, with an empty row to type into when the list is
/// empty.
pub(super) fn open(ui: &mut Ui) {
    super::settings::cancel_capture(ui);
    super::picker::close(ui);
    super::explainer::close(ui);
    super::sound_sets::close(ui);
    ui.open = None;
    ui.vocabulary.open = true;
    if ui.vocabulary.rows.is_empty() {
        add_row(ui);
    }
}

/// Saves what was typed, drops rows left blank, and closes the sheet.
pub(super) fn close(ui: &mut Ui) {
    if !ui.vocabulary.open {
        return;
    }
    ui.vocabulary.open = false;
    ui.vocabulary.added = None;
    ui.flush_drafts();
    ui.vocabulary
        .rows
        .retain(|row| !row.term.text.trim().is_empty() || !row.sounds_like.text.trim().is_empty());
}

fn add_row(ui: &mut Ui) -> u64 {
    for row in &mut ui.vocabulary.rows {
        row.term.focused = false;
        row.sounds_like.focused = false;
    }
    let mut term = TextEdit::default();
    term.focused = true;
    ui.vocabulary.push(term, TextEdit::default())
}

/// Add term: a new focused row at the end, which the list glides down to
/// as the row arrives.
fn add_term(ui: &mut Ui, tx: &mut Tx) {
    ui.vocabulary.added = Some(add_row(ui));
    tx.scroll_to_bottom("term-scroll");
}

fn row_key(id: u64) -> String {
    format!("term{id}")
}

/// A new row rises a few points into place as it fades in; under Reduce
/// Motion only the fade remains. Its removal is the usual fade.
fn arrival() -> Transition {
    Transition::asymmetric(
        Transition::offset(0.0, 8.0).combined(Transition::opacity()),
        Transition::opacity(),
    )
    .springs(SpringPair::new(SETTLE, SETTLE_EXIT))
}

/// Removes row `id`; a second press for a row already gone does nothing.
fn remove_row(ui: &mut Ui, id: u64) {
    if let Some(index) = ui.vocabulary.rows.iter().position(|row| row.id == id) {
        ui.vocabulary.rows.remove(index);
        ui.flush_drafts();
    }
}

fn term_row(p: Paint, entry: &TermRow, added: bool) -> El {
    let id = entry.id;
    let term_row = row()
        .key(row_key(id))
        .h(ROW_HEIGHT)
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(&entry.term)
                .placeholder(tr("Term (e.g. abattoir)"))
                .width(Some(TERM_WIDTH))
                .on_edit(edit_access(move |ui: &mut Ui| {
                    ui.vocabulary.field(id, false)
                }))
                .build(p.k)
                .key("term"),
        )
        .child(
            TextInput::new(&entry.sounds_like)
                .placeholder(tr("Sounds like (e.g. avatar, a bat war)"))
                .width(None)
                .on_edit(edit_access(move |ui: &mut Ui| {
                    ui.vocabulary.field(id, true)
                }))
                .build(p.k)
                .key("sounds-like")
                .grow(1.0)
                .shrink(),
        )
        .child(remove_button(
            p,
            ControlSize::Regular,
            move |ui: &mut Ui| remove_row(ui, id),
        ));
    if added {
        term_row.transition(arrival())
    } else {
        term_row
    }
}

pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.vocabulary.open {
        return None;
    }
    let c = &p.k.c;
    let rows = col().key("terms").px(SIDE_ROOM).children(
        ui.vocabulary
            .rows
            .iter()
            .map(|entry| term_row(p, entry, ui.vocabulary.added == Some(entry.id))),
    );
    let height = ui.vocabulary.rows.len() as f32 * ROW_HEIGHT + FADES.0 + FADES.1;
    let terms = col()
        .key("term-column")
        .h(height.min(ROWS_HEIGHT))
        .mx(-SIDE_ROOM)
        .child(super::faded_body("term-scroll", rows, FADES));
    let add = button(
        p,
        "add-term",
        tr("Add term"),
        ButtonKind::Secondary,
        add_term,
    );
    let footer = row()
        .key("footer")
        .gap(12.0)
        .center_y()
        .child(add)
        .child(el().key("spacer").grow(1.0))
        .child(
            button(
                p,
                "close-vocabulary",
                tr("Done"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| close(ui),
            )
            .default_button(),
        );
    let mut heading = col()
        .key("heading")
        .gap(4.0)
        .child(
            text(tr("Vocabulary"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(
            text(tr(
                "Transcription and rewriting spell them right, and search finds them when misheard",
            ))
            .key("description")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap(),
        )
        .child(
            text(tr("Terms with spaces match only when said in full"))
                .key("phrases")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    let biases = crate::transcription::models::definition(ui.settings.transcription.model)
        .vocabulary_use()
        == crate::transcription::models::VocabularyUse::Bias;
    let unused = unused_note(ui).or_else(|| {
        (biases && !ui.settings.labs.vocabulary_biasing).then(|| {
            tr("Transcription uses these only with Labs › Vocabulary biasing on; rewriting and search always do")
                .to_string()
        })
    });
    if let Some(unused) = unused {
        heading = heading.child(
            text(unused)
                .key("unused")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    }
    let mut panel = col()
        .key("vocabulary")
        .role("dialog")
        .label(tr("Vocabulary"))
        .w(MODAL_WIDTH)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(12.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(heading)
        .child(terms);
    if ui.vocabulary.clamped() {
        panel = panel.child(
            note(
                "limits",
                tr("Up to 16 spellings per term are saved, and 64 characters per term or spelling."),
                p.danger(),
            )
            .mx(-super::controls::METRICS.row_px),
        );
    }
    panel = panel.child(footer);
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: &str| {
        el().key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    Some([
        layer("vocabulary-scrim")
            .fill(c.scrim)
            .on_press(close)
            .transition(modal_scrim_transition(p)),
        layer("vocabulary-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the row's button that opened it.
            .child(panel.modal(true, close)),
    ])
}

/// A few terms, one with spellings, with the sheet open.
pub(super) fn fixture(ui: &mut Ui, filled: bool) {
    ui.settings.vocabulary = if filled {
        let mut entries = Vec::new();
        for (term, sounds_like) in [
            ("abattoir", "avatar, a bat war"),
            ("OpenCode", "open code"),
            ("Rune", "roon"),
            ("Parakeet", ""),
            ("Dax", "Dex, Docs"),
        ] {
            let _ = vocabulary::add(
                &mut entries,
                term,
                &vocabulary::parse_sounds_like(sounds_like),
            );
        }
        entries
    } else {
        Vec::new()
    };
    ui.vocabulary = VocabularyEditor::new(&ui.settings.vocabulary);
    open(ui);
    if let Some(row) = ui.vocabulary.rows.last_mut() {
        row.term.focused = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::with(
            crate::app_settings::AppSettings::default(),
            super::super::Service::Fixture(Vec::new()),
        )
    }

    #[test]
    fn the_sheet_starts_empty_lists_with_a_row_and_saves_on_close() {
        let mut ui = ui();
        open(&mut ui);
        assert!(ui.vocabulary.open);
        assert_eq!(ui.vocabulary.rows.len(), 1);
        close(&mut ui);
        assert!(!ui.vocabulary.open);
        assert!(ui.vocabulary.rows.is_empty());
        assert_eq!(summary(&ui), "None");

        open(&mut ui);
        ui.vocabulary.rows[0].term = TextEdit::new("abattoir");
        add_row(&mut ui);
        close(&mut ui);
        assert_eq!(ui.vocabulary.rows.len(), 1);
        assert_eq!(ui.settings.vocabulary[0].term, "abattoir");
        assert_eq!(summary(&ui), "1 term");

        open(&mut ui);
        assert_eq!(ui.vocabulary.rows.len(), 1);
    }

    #[test]
    fn rows_project_to_cleaned_merged_entries() {
        let mut editor = VocabularyEditor::new(&[VocabularyEntry {
            term: "abattoir".into(),
            sounds_like: vec!["avatar".into(), "a bat war".into()],
        }]);
        assert_eq!(editor.rows[0].sounds_like.text, "avatar, a bat war");
        editor.push(TextEdit::default(), TextEdit::new("ignored"));
        editor.push(TextEdit::new(" Abattoir "), TextEdit::new("abba toy,"));
        editor.push(TextEdit::new("Rune"), TextEdit::new(" roon "));
        assert_eq!(
            editor.project(),
            [
                VocabularyEntry {
                    term: "abattoir".into(),
                    sounds_like: vec!["avatar".into(), "a bat war".into(), "abba toy".into()],
                },
                VocabularyEntry {
                    term: "Rune".into(),
                    sounds_like: vec!["roon".into()],
                },
            ]
        );
        assert!(!editor.clamped());
    }

    #[test]
    fn rows_past_the_limits_keep_their_term_and_what_fits() {
        let spellings: Vec<String> = (0..17).map(|index| format!("spelling {index}")).collect();
        let mut editor = VocabularyEditor::new(&[]);
        editor.push(
            TextEdit::new("abattoir"),
            TextEdit::new(format!("{}, {}", spellings.join(", "), "y".repeat(65))),
        );
        editor.push(TextEdit::new("Rune"), TextEdit::new("roon"));
        let entries = editor.project();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].term, "abattoir");
        assert_eq!(entries[0].sounds_like, spellings[..16]);
        assert_eq!(entries[1].sounds_like, ["roon"]);
        assert!(editor.clamped());
    }

    /// The sheet open on four terms, and their row identities.
    fn four_terms() -> (Ui, Vec<u64>) {
        let mut ui = ui();
        fixture(&mut ui, false);
        ui.vocabulary.rows.clear();
        for term in ["one", "two", "three", "four"] {
            let sounds_like = TextEdit::new(format!("{term} sound"));
            ui.vocabulary.push(TextEdit::new(term), sounds_like);
        }
        let ids = ui.vocabulary.rows.iter().map(|row| row.id).collect();
        (ui, ids)
    }

    fn terms(ui: &Ui) -> Vec<&str> {
        ui.vocabulary
            .rows
            .iter()
            .map(|row| row.term.text.as_str())
            .collect()
    }

    #[test]
    fn removing_a_focused_row_leaves_the_others_and_late_edits_touch_nothing() {
        let (mut ui, ids) = four_terms();
        ui.vocabulary.rows[1].term.focused = true;
        remove_row(&mut ui, ids[1]);
        assert_eq!(terms(&ui), ["one", "three", "four"]);
        // The removed field's queued edit and focus loss arrive afterwards.
        let stale = ui.vocabulary.field(ids[1], false);
        stale.text = "edited".into();
        stale.focused = false;
        let stale = ui.vocabulary.field(ids[1], true);
        stale.text = "edited".into();
        assert_eq!(terms(&ui), ["one", "three", "four"]);
        assert_eq!(ui.vocabulary.rows[1].sounds_like.text, "three sound");
        assert_eq!(
            ui.settings
                .vocabulary
                .iter()
                .map(|entry| entry.term.as_str())
                .collect::<Vec<_>>(),
            ["one", "three", "four"]
        );
    }

    #[test]
    fn removing_the_last_row_then_editing_it_is_harmless() {
        let (mut ui, ids) = four_terms();
        remove_row(&mut ui, ids[3]);
        ui.vocabulary.field(ids[3], false).text = "late".into();
        ui.vocabulary.field(ids[3], true).text = "late".into();
        assert_eq!(terms(&ui), ["one", "two", "three"]);
        remove_row(&mut ui, ids[3]);
        assert_eq!(terms(&ui), ["one", "two", "three"]);
    }

    #[test]
    fn rapid_removals_from_one_frame_remove_only_their_own_rows() {
        let (mut ui, ids) = four_terms();
        // Presses built in one frame, run before the next: the same row
        // twice, then the rows after it.
        remove_row(&mut ui, ids[1]);
        remove_row(&mut ui, ids[1]);
        remove_row(&mut ui, ids[2]);
        remove_row(&mut ui, ids[3]);
        assert_eq!(terms(&ui), ["one"]);
        ui.vocabulary.field(ids[3], false).text = "late".into();
        remove_row(&mut ui, ids[0]);
        assert!(ui.vocabulary.rows.is_empty());
        ui.vocabulary.field(ids[0], true).text = "late".into();
        assert!(ui.vocabulary.project().is_empty());
    }

    #[test]
    fn a_reload_retires_the_old_rows() {
        let (mut ui, ids) = four_terms();
        ui.vocabulary.reload(&[VocabularyEntry {
            term: "Rune".into(),
            sounds_like: vec!["roon".into()],
        }]);
        ui.vocabulary.field(ids[0], false).text = "late".into();
        assert_eq!(terms(&ui), ["Rune"]);
    }
}
