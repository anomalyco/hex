//! Vocabulary: a sheet from General's Vocabulary row with one row per term
//! and its "sounds like" spellings, edited in place and saved when typing
//! pauses, like a mode's corrections, and the boost.

use rune::{El, TextEdit, col, edit_access, el, row, text};
use rune_kit::{ControlSize, TextInput};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, modal_scrim_transition, modal_transition, note,
    segmented, setting_row,
};
use super::lists::remove_button;
use crate::desktop_i18n::{tr, tr_fill};
use crate::vocabulary::{self, VocabularyBoost, VocabularyEntry};

const MODAL_WIDTH: f32 = 620.0;
const PADDING: f32 = 18.0;
/// The term rows scroll past this.
const ROWS_HEIGHT: f32 = 300.0;
/// One term row: a regular text field and its padding.
const ROW_HEIGHT: f32 = 36.0;
/// The term list's fades; the bottom one is the gap before Boost at rest.
const FADES: (f32, f32) = (0.0, 6.0);
const TERM_WIDTH: f32 = 200.0;

/// The term and "sounds like" fields of each row, and whether the sheet is
/// open.
#[derive(Default)]
pub(super) struct VocabularyEditor {
    rows: Vec<(TextEdit, TextEdit)>,
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
        self.rows = entries
            .iter()
            .map(|entry| {
                (
                    TextEdit::new(entry.term.clone()),
                    TextEdit::new(entry.sounds_like.join(", ")),
                )
            })
            .collect();
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
        for (term, sounds_like) in &self.rows {
            clamped |= vocabulary::add_clamped(
                &mut entries,
                &term.text,
                &vocabulary::parse_sounds_like(&sounds_like.text),
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
    ui.flush_drafts();
    ui.vocabulary.rows.retain(|(term, sounds_like)| {
        !term.text.trim().is_empty() || !sounds_like.text.trim().is_empty()
    });
}

fn add_row(ui: &mut Ui) {
    for (term, sounds_like) in &mut ui.vocabulary.rows {
        term.focused = false;
        sounds_like.focused = false;
    }
    let mut term = TextEdit::default();
    term.focused = true;
    ui.vocabulary.rows.push((term, TextEdit::default()));
}

fn remove_row(ui: &mut Ui, index: usize) {
    if index < ui.vocabulary.rows.len() {
        ui.vocabulary.rows.remove(index);
        ui.flush_drafts();
    }
}

fn term_row(ui: &Ui, p: Paint, index: usize) -> El {
    let (term, sounds_like) = &ui.vocabulary.rows[index];
    row()
        .key(format!("term{index}"))
        .h(ROW_HEIGHT)
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(term)
                .placeholder(tr("Term (e.g. abattoir)"))
                .width(Some(TERM_WIDTH))
                .on_edit(edit_access(move |ui: &mut Ui| {
                    &mut ui.vocabulary.rows[index].0
                }))
                .build(p.k)
                .key("term"),
        )
        .child(
            TextInput::new(sounds_like)
                .placeholder(tr("Sounds like (e.g. avatar, a bat war)"))
                .width(None)
                .on_edit(edit_access(move |ui: &mut Ui| {
                    &mut ui.vocabulary.rows[index].1
                }))
                .build(p.k)
                .key("sounds-like")
                .grow(1.0)
                .shrink(),
        )
        .child(remove_button(
            p,
            ControlSize::Regular,
            move |ui: &mut Ui| remove_row(ui, index),
        ))
}

pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.vocabulary.open {
        return None;
    }
    let c = &p.k.c;
    let rows = col()
        .key("terms")
        .children((0..ui.vocabulary.rows.len()).map(|index| term_row(ui, p, index)));
    let height = ui.vocabulary.rows.len() as f32 * ROW_HEIGHT + FADES.0 + FADES.1;
    let terms = col()
        .key("term-column")
        .h(height.min(ROWS_HEIGHT))
        .child(super::faded_body("term-scroll", rows, FADES));
    let add = button(
        p,
        "add-term",
        tr("Add term"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| add_row(ui),
    );
    let boost = setting_row(
        p,
        "boost",
        tr("Boost"),
        Some(tr("How readily transcription swaps in a term")),
        segmented(
            p,
            "vocabulary-boost",
            &[tr("Off"), tr("Light"), tr("Normal"), tr("Strong")],
            VocabularyBoost::ALL
                .iter()
                .position(|&boost| boost == ui.settings.vocabulary_boost)
                .unwrap_or(2),
            |ui: &mut Ui, index| {
                if let Some(&boost) = VocabularyBoost::ALL.get(index) {
                    ui.update(|settings| settings.vocabulary_boost = boost);
                }
            },
        ),
    )
    .mx(-super::controls::METRICS.row_px);
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
        .child(
            col()
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
                ),
        )
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
    panel = panel.child(boost).child(footer);
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
    if let Some((term, _)) = ui.vocabulary.rows.last_mut() {
        term.focused = false;
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
        ui.vocabulary.rows[0].0 = TextEdit::new("abattoir");
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
        assert_eq!(editor.rows[0].1.text, "avatar, a bat war");
        editor
            .rows
            .push((TextEdit::default(), TextEdit::new("ignored")));
        editor
            .rows
            .push((TextEdit::new(" Abattoir "), TextEdit::new("abba toy,")));
        editor
            .rows
            .push((TextEdit::new("Rune"), TextEdit::new(" roon ")));
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
        editor.rows.push((
            TextEdit::new("abattoir"),
            TextEdit::new(format!("{}, {}", spellings.join(", "), "y".repeat(65))),
        ));
        editor
            .rows
            .push((TextEdit::new("Rune"), TextEdit::new("roon")));
        let entries = editor.project();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].term, "abattoir");
        assert_eq!(entries[0].sounds_like, spellings[..16]);
        assert_eq!(entries[1].sounds_like, ["roon"]);
        assert!(editor.clamped());
    }
}
