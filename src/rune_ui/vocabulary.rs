//! Vocabulary: a sheet from General's Vocabulary row with one row per term
//! and its "sounds like" spellings, edited in place and saved when typing
//! pauses, like a mode's corrections. The boost has no control here; it
//! stays a setting (`hex settings set vocabulary_boost`).

use std::path::{Path, PathBuf};

use rune::tok::motion::{SETTLE, SETTLE_EXIT};
use rune::{El, SpringPair, TextEdit, Transition, Tx, col, edit_access, el, row, text};
use rune_kit::{ControlSize, TextInput};

use super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, modal_scrim_transition, modal_transition, note,
};
use super::lists::remove_button;
use super::{Modal, Ui};
use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::dictation::vocabulary::csv::{self, MergeSummary};
use crate::dictation::vocabulary::{self, VocabularyEntry};

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

/// What the last Import or Export did, under the rows.
struct Status {
    message: String,
    /// A read, parse, or write failure, shown in the danger colour.
    failed: bool,
}

/// The term rows, whether the sheet is open, and the file dialogs its
/// footer asked for.
#[derive(Default)]
pub(super) struct VocabularyEditor {
    rows: Vec<TermRow>,
    next_id: u64,
    /// What an edit for a row that is gone lands in; never saved.
    detached: TextEdit,
    /// The row Add term made last while the sheet is open: only it arrives
    /// with motion, so opening the sheet never animates its rows.
    added: Option<u64>,
    /// Labs › Vocabulary files' last outcome, kept while the sheet stays
    /// open.
    status: Option<Status>,
    /// The file dialogs Import… and Export… asked for.
    import_chooser: Option<Pending>,
    export_chooser: Option<Pending>,
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
            &model.name,
        )
    })
}

/// How many terms the selected model reads, when that is fewer than the
/// list holds: Parakeet's bias cap, or Whisper's and Qwen3-ASR's prompt
/// budget, as transcription applies them.
fn budget_note(ui: &Ui) -> Option<String> {
    let entries = ui.vocabulary.project();
    let read = crate::transcription::models::definition(ui.settings.transcription.model)
        .vocabulary_terms_read(&entries);
    (read < entries.len()).then(|| {
        tr_named(
            "This model reads the first {read} of {total} terms",
            &[
                ("read", &read.to_string()),
                ("total", &entries.len().to_string()),
            ],
        )
    })
}

/// Opens the sheet alone, with an empty row to type into when the list is
/// empty.
pub(super) fn open(ui: &mut Ui) {
    super::settings::cancel_capture(ui);
    ui.open_modal(Modal::Vocabulary);
    ui.vocabulary.status = None;
    if ui.vocabulary.rows.is_empty() {
        add_row(ui);
    }
}

/// Saves what was typed, drops rows left blank, and closes the sheet.
pub(super) fn close(ui: &mut Ui) {
    ui.close_modal_if(Modal::Vocabulary);
}

pub(super) fn on_close(ui: &mut Ui) {
    ui.vocabulary.added = None;
    ui.vocabulary.status = None;
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

/// Labs › Vocabulary files on: the one switch the buttons, the status line,
/// and the dialog answers read.
fn on(ui: &Ui) -> bool {
    crate::labs::Feature::VocabularyFiles.on(&ui.settings)
}

/// Redraws the window once a file dialog answers.
fn wake(ui: &Ui) -> impl Fn() + Send + 'static {
    let waker = ui.modes.waker.clone();
    move || {
        if let Some(waker) = &waker {
            waker();
        }
    }
}

/// Import…: one CSV or TXT file, starting where downloads usually land.
fn choose_import(ui: &mut Ui) {
    if ui.vocabulary.import_chooser.is_some() {
        return;
    }
    ui.vocabulary.status = None;
    ui.vocabulary.import_chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Import vocabulary").into(),
            target: Target::File(vec![
                Filter {
                    label: tr("CSV files").into(),
                    extensions: &["csv"],
                },
                Filter {
                    label: tr("Text files").into(),
                    extensions: &["txt"],
                },
            ]),
            directory: crate::app_paths::downloads_dir(),
        },
        wake(ui),
    ));
}

/// Export…: the whole list as one CSV file.
fn choose_export(ui: &mut Ui) {
    if ui.vocabulary.export_chooser.is_some() {
        return;
    }
    ui.vocabulary.status = None;
    ui.vocabulary.export_chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Export vocabulary").into(),
            target: Target::Save {
                filters: vec![Filter {
                    label: tr("CSV files").into(),
                    extensions: &["csv"],
                }],
                file_name: "hex-vocabulary.csv".into(),
            },
            directory: crate::app_paths::downloads_dir(),
        },
        wake(ui),
    ));
}

/// The file dialogs' answers, since the last call. True when one changed
/// what the sheet shows or what settings hold.
pub(super) fn poll(ui: &mut Ui) -> bool {
    // A closed sheet or the lab off takes no answer: a dialog that finishes
    // after either changes nothing.
    if !ui.is_modal(Modal::Vocabulary) || !on(ui) {
        ui.vocabulary.import_chooser = None;
        ui.vocabulary.export_chooser = None;
        if !on(ui) {
            ui.vocabulary.status = None;
        }
        return false;
    }
    let mut changed = false;
    if let Some(choice) = ui
        .vocabulary
        .import_chooser
        .as_ref()
        .and_then(Pending::poll)
    {
        ui.vocabulary.import_chooser = None;
        import_chosen(ui, choice);
        changed = true;
    }
    if let Some(choice) = ui
        .vocabulary
        .export_chooser
        .as_ref()
        .and_then(Pending::poll)
    {
        ui.vocabulary.export_chooser = None;
        export_chosen(ui, choice);
        changed = true;
    }
    changed
}

fn import_chosen(ui: &mut Ui, choice: Result<Option<PathBuf>, String>) {
    let path = match choice {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            fail(ui, tr_fill("Couldn't import that file: {}", &error));
            return;
        }
    };
    match apply_import(ui, &path) {
        Ok(message) => {
            ui.vocabulary.status = Some(Status {
                message,
                failed: false,
            });
        }
        Err(error) => import_failed(ui, error),
    }
}

/// Every import failure in the sheet's one wording, with the fixed reasons
/// translated and the OS's own text left as it is.
fn import_failed(ui: &mut Ui, error: csv::ImportError) {
    use crate::dictation::vocabulary::csv::ImportError;
    let message = match error {
        ImportError::TooLarge => tr_fill(
            "Couldn't import that file: {}",
            tr("the file is larger than 1 MB"),
        ),
        ImportError::NotText => tr_fill(
            "Couldn't import that file: {}",
            tr("the file isn't UTF-8 text"),
        ),
        ImportError::UnclosedQuote => tr_fill(
            "Couldn't import that file: {}",
            tr("a quote is never closed"),
        ),
        ImportError::NotFile => tr_fill(
            "Couldn't import that file: {}",
            tr("it isn't a regular file"),
        ),
        ImportError::Io(error) => tr_fill("Couldn't import that file: {}", &error),
        ImportError::NotSaved(error) => tr_fill("Couldn't save the vocabulary: {}", &error),
    };
    fail(ui, message);
}

/// Merges the file's terms into the sheet's rows, then saves through the
/// same draft path typing uses. Returns the status line.
fn apply_import(ui: &mut Ui, path: &Path) -> Result<String, csv::ImportError> {
    let imported = csv::read(path)?;
    if imported.is_empty() {
        return Ok(tr("No terms found in that file").to_string());
    }
    let mut entries = ui.vocabulary.project();
    let summary = csv::merge(&mut entries, &imported);
    ui.vocabulary.reload(&entries);
    if !ui.flush_drafts() {
        return Err(csv::ImportError::NotSaved(
            ui.error.clone().unwrap_or_default(),
        ));
    }
    Ok(merge_status(summary))
}

/// "Added 12 terms", "Added 3 terms, updated 2, skipped 3", "Updated 2
/// terms", or why nothing changed.
fn merge_status(summary: MergeSummary) -> String {
    let (added, updated, skipped) = (summary.added, summary.updated, summary.skipped);
    if added == 0 && updated == 0 {
        return if skipped == 0 {
            tr("No new terms in that file").to_string()
        } else {
            tr_fill("No terms added; skipped {}", &skipped.to_string())
        };
    }
    let (added_text, updated_text, skipped_text) =
        (added.to_string(), updated.to_string(), skipped.to_string());
    if skipped > 0 {
        return match (added, updated) {
            (0, 1) => tr_fill("Updated 1 term, skipped {}", &skipped_text),
            (0, _) => tr_named(
                "Updated {updated} terms, skipped {skipped}",
                &[("updated", &updated_text), ("skipped", &skipped_text)],
            ),
            (1, 0) => tr_fill("Added 1 term, skipped {}", &skipped_text),
            (1, _) => tr_named(
                "Added 1 term, updated {updated}, skipped {skipped}",
                &[("updated", &updated_text), ("skipped", &skipped_text)],
            ),
            (_, 0) => tr_named(
                "Added {added} terms, skipped {skipped}",
                &[("added", &added_text), ("skipped", &skipped_text)],
            ),
            (_, _) => tr_named(
                "Added {added} terms, updated {updated}, skipped {skipped}",
                &[
                    ("added", &added_text),
                    ("updated", &updated_text),
                    ("skipped", &skipped_text),
                ],
            ),
        };
    }
    match (added, updated) {
        (1, 0) => tr("Added 1 term").to_string(),
        (0, 1) => tr("Updated 1 term").to_string(),
        (0, _) => tr_fill("Updated {} terms", &updated_text),
        (1, _) => tr_named(
            "Added 1 term, updated {updated}",
            &[("updated", &updated_text)],
        ),
        (_, 0) => tr_fill("Added {} terms", &added_text),
        (_, _) => tr_named(
            "Added {added} terms, updated {updated}",
            &[("added", &added_text), ("updated", &updated_text)],
        ),
    }
}

/// The file to write: the chosen path, or its `.csv` name when the person
/// left the extension off and that name is free. A name that is just
/// `.csv` is already a CSV file, so it is never doubled.
fn export_path(chosen: &Path) -> PathBuf {
    if chosen.extension().is_some()
        || chosen
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(".csv"))
    {
        return chosen.to_path_buf();
    }
    let with_extension = chosen.with_extension("csv");
    if with_extension.exists() {
        chosen.to_path_buf()
    } else {
        with_extension
    }
}

fn export_chosen(ui: &mut Ui, choice: Result<Option<PathBuf>, String>) {
    let path = match choice {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            fail(ui, tr_fill("Couldn't export the vocabulary: {}", &error));
            return;
        }
    };
    let path = export_path(&path);
    let entries = ui.vocabulary.project();
    match crate::durable::write(&path, csv::render(&entries).as_bytes()) {
        Ok(()) => {
            let message = if entries.len() == 1 {
                tr("Exported 1 term").to_string()
            } else {
                tr_fill("Exported {} terms", &entries.len().to_string())
            };
            ui.vocabulary.status = Some(Status {
                message,
                failed: false,
            });
        }
        Err(error) => fail(
            ui,
            tr_fill("Couldn't export the vocabulary: {}", &error.to_string()),
        ),
    }
}

fn fail(ui: &mut Ui, message: String) {
    ui.vocabulary.status = Some(Status {
        message,
        failed: true,
    });
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
                .placeholder(tr("Term (e.g. cappuccino)"))
                .width(Some(TERM_WIDTH))
                .on_edit(edit_access(move |ui: &mut Ui| {
                    ui.vocabulary.field(id, false)
                }))
                .build(p.k)
                .key("term"),
        )
        .child(
            TextInput::new(&entry.sounds_like)
                .placeholder(tr("Sounds like (e.g. capuchin, cap a chino)"))
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

pub(super) fn modal(ui: &Ui, p: Paint) -> [El; 2] {
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
        .child(super::faded_body(p.k, "term-scroll", rows, FADES));
    let add = button(
        p,
        "add-term",
        tr("Add term"),
        ButtonKind::Secondary,
        add_term,
    );
    let mut footer = row().key("footer").gap(12.0).center_y().child(add);
    // Labs › Vocabulary files: two more ways to fill the list, never main
    // actions, so both stay Secondary.
    if on(ui) {
        footer = footer
            .child(button(
                p,
                "import-vocabulary",
                tr("Import…"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| choose_import(ui),
            ))
            .child(button(
                p,
                "export-vocabulary",
                tr("Export…"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| choose_export(ui),
            ));
    }
    let footer = footer.child(el().key("spacer").grow(1.0)).child(
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
    let unused = unused_note(ui).or_else(|| budget_note(ui));
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
            .mx(-METRICS.row_px),
        );
    }
    if on(ui)
        && let Some(status) = &ui.vocabulary.status
    {
        panel = panel.child(
            note(
                "file-status",
                &status.message,
                if status.failed { p.danger() } else { p.muted() },
            )
            .mx(-METRICS.row_px),
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
    [
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
    ]
}

/// A few terms, one with spellings, with the sheet open.
pub(super) fn fixture(ui: &mut Ui, filled: bool) {
    ui.settings.vocabulary = if filled {
        let mut entries = Vec::new();
        for (term, sounds_like) in [
            ("cappuccino", "capuchin, cap a chino"),
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

    /// A temporary file for one import or export, removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "hex-vocabulary-sheet-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            Self(directory)
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
        assert!(ui.is_modal(Modal::Vocabulary));
        assert_eq!(ui.vocabulary.rows.len(), 1);
        close(&mut ui);
        assert!(!ui.is_modal(Modal::Vocabulary));
        assert!(ui.vocabulary.rows.is_empty());
        assert_eq!(summary(&ui), "None");

        open(&mut ui);
        ui.vocabulary.rows[0].term = TextEdit::new("cappuccino");
        add_row(&mut ui);
        close(&mut ui);
        assert_eq!(ui.vocabulary.rows.len(), 1);
        assert_eq!(ui.settings.vocabulary[0].term, "cappuccino");
        assert_eq!(summary(&ui), "1 term");

        open(&mut ui);
        assert_eq!(ui.vocabulary.rows.len(), 1);
    }

    #[test]
    fn rows_project_to_cleaned_merged_entries() {
        let mut editor = VocabularyEditor::new(&[VocabularyEntry {
            term: "cappuccino".into(),
            sounds_like: vec!["capuchin".into(), "cap a chino".into()],
        }]);
        assert_eq!(editor.rows[0].sounds_like.text, "capuchin, cap a chino");
        editor.push(TextEdit::default(), TextEdit::new("ignored"));
        editor.push(
            TextEdit::new(" Cappuccino "),
            TextEdit::new("cap you chino,"),
        );
        editor.push(TextEdit::new("Rune"), TextEdit::new(" roon "));
        assert_eq!(
            editor.project(),
            [
                VocabularyEntry {
                    term: "cappuccino".into(),
                    sounds_like: vec![
                        "capuchin".into(),
                        "cap a chino".into(),
                        "cap you chino".into()
                    ],
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
            TextEdit::new("cappuccino"),
            TextEdit::new(format!("{}, {}", spellings.join(", "), "y".repeat(65))),
        );
        editor.push(TextEdit::new("Rune"), TextEdit::new("roon"));
        let entries = editor.project();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].term, "cappuccino");
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
    fn the_sheet_says_how_many_terms_the_model_reads() {
        use crate::transcription::models::TranscriptionModelId;
        let mut ui = ui();
        let entries: Vec<_> = (0..52)
            .map(|n| VocabularyEntry {
                term: format!("A longer project name {n}"),
                sounds_like: Vec::new(),
            })
            .collect();
        ui.vocabulary = VocabularyEditor::new(&entries);
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        let read = vocabulary::terms_read(&entries, "whisper");
        assert!(read < entries.len());
        assert_eq!(
            budget_note(&ui).unwrap(),
            format!("This model reads the first {read} of 52 terms")
        );
        ui.settings.transcription.model = TranscriptionModelId::ParakeetV3;
        assert_eq!(budget_note(&ui), None);
        for n in 52..70 {
            ui.vocabulary
                .push(TextEdit::new(format!("Extra {n}")), TextEdit::default());
        }
        assert_eq!(
            budget_note(&ui).unwrap(),
            "This model reads the first 64 of 70 terms"
        );
        ui.settings.transcription.model = TranscriptionModelId::SenseVoiceSmall;
        assert!(unused_note(&ui).is_some());
    }

    #[test]
    fn an_import_merges_into_the_sheet_and_saves() {
        let mut ui = ui();
        open(&mut ui);
        // Typed but not yet saved: the import merges into the draft too.
        ui.vocabulary.rows[0].term = TextEdit::new("Rune");
        let scratch = Scratch::new("import");
        let path = scratch.path("terms.csv");
        std::fs::write(
            &path,
            "Term (required),Sounds like\nRune,roon\n\"open code\",\n\n",
        )
        .unwrap();

        assert_eq!(
            apply_import(&mut ui, &path).unwrap(),
            "Added 1 term, updated 1"
        );
        assert_eq!(
            ui.settings.vocabulary,
            [
                VocabularyEntry {
                    term: "Rune".into(),
                    sounds_like: vec!["roon".into()],
                },
                VocabularyEntry {
                    term: "open code".into(),
                    sounds_like: Vec::new(),
                },
            ]
        );
        assert_eq!(terms(&ui), ["Rune", "open code"]);
        assert_eq!(summary(&ui), "2 terms");
    }

    #[test]
    fn an_import_with_no_terms_says_so_and_changes_nothing() {
        let mut ui = ui();
        let scratch = Scratch::new("empty");
        let path = scratch.path("empty.csv");
        std::fs::write(&path, "Term (required),Sounds like\n\n,\n").unwrap();
        assert_eq!(
            apply_import(&mut ui, &path).unwrap(),
            "No terms found in that file"
        );
        assert!(ui.settings.vocabulary.is_empty());

        std::fs::write(&path, "Term (required),Sounds like\nRune,roon\n").unwrap();
        ui.settings.vocabulary = vec![VocabularyEntry {
            term: "Rune".into(),
            sounds_like: vec!["roon".into()],
        }];
        ui.vocabulary.reload(&ui.settings.vocabulary);
        assert_eq!(
            apply_import(&mut ui, &path).unwrap(),
            "No new terms in that file"
        );

        // A new spelling for a term already listed is an update, not nothing.
        std::fs::write(&path, "Term (required),Sounds like\nRune,\"roon, ruin\"\n").unwrap();
        assert_eq!(apply_import(&mut ui, &path).unwrap(), "Updated 1 term");
        assert_eq!(ui.settings.vocabulary[0].sounds_like, ["roon", "ruin"]);
    }

    #[test]
    fn an_unreadable_import_reports_why() {
        let scratch = Scratch::new("missing");
        let error = csv::read(&scratch.path("missing.csv")).unwrap_err();
        assert!(
            matches!(&error, csv::ImportError::Io(text) if !text.is_empty()),
            "{error:?}"
        );
    }

    #[test]
    fn an_export_writes_the_sheet_and_appends_csv() {
        let mut ui = ui();
        ui.vocabulary = VocabularyEditor::new(&[
            VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec!["roon".into(), "ruin".into()],
            },
            VocabularyEntry {
                term: "Dax, Inc.".into(),
                sounds_like: Vec::new(),
            },
        ]);
        let scratch = Scratch::new("export");
        let path = scratch.path("hex-vocabulary");
        export_chosen(&mut ui, Ok(Some(path.clone())));
        assert_eq!(
            ui.vocabulary.status.as_ref().unwrap().message,
            "Exported 2 terms"
        );
        let written = scratch.path("hex-vocabulary.csv");
        assert_eq!(
            std::fs::read_to_string(&written).unwrap(),
            format!("{}\nRune,\"roon, ruin\",,\n\"Dax, Inc.\",,,\n", csv::HEADER)
        );
        assert!(!ui.vocabulary.status.as_ref().unwrap().failed);
    }

    #[test]
    fn an_export_keeps_a_taken_csv_name_and_a_dot_csv_file() {
        let scratch = Scratch::new("export-names");
        let path = scratch.path("terms");

        // Free: the .csv name is added.
        assert_eq!(export_path(&path), scratch.path("terms.csv"));
        // Taken: the chosen name is written exactly.
        std::fs::write(scratch.path("terms.csv"), "taken").unwrap();
        assert_eq!(export_path(&path), path);
        // Any extension is kept as chosen.
        assert_eq!(
            export_path(&scratch.path("terms.txt")),
            scratch.path("terms.txt")
        );
        assert_eq!(
            export_path(&scratch.path("terms.csv")),
            scratch.path("terms.csv")
        );
        // A file named just ".csv" is already a CSV file.
        assert_eq!(export_path(&scratch.path(".csv")), scratch.path(".csv"));
    }

    #[test]
    fn the_import_status_names_what_changed() {
        let status = |added, updated, skipped| {
            merge_status(MergeSummary {
                added,
                updated,
                skipped,
            })
        };
        assert_eq!(status(12, 0, 3), "Added 12 terms, skipped 3");
        assert_eq!(status(1, 0, 3), "Added 1 term, skipped 3");
        assert_eq!(status(12, 0, 0), "Added 12 terms");
        assert_eq!(status(1, 0, 0), "Added 1 term");
        assert_eq!(status(0, 1, 0), "Updated 1 term");
        assert_eq!(status(0, 2, 0), "Updated 2 terms");
        assert_eq!(status(3, 2, 0), "Added 3 terms, updated 2");
        assert_eq!(status(1, 2, 0), "Added 1 term, updated 2");
        assert_eq!(status(0, 2, 1), "Updated 2 terms, skipped 1");
        assert_eq!(status(0, 1, 1), "Updated 1 term, skipped 1");
        assert_eq!(status(1, 2, 1), "Added 1 term, updated 2, skipped 1");
        assert_eq!(status(3, 2, 1), "Added 3 terms, updated 2, skipped 1");
        assert_eq!(status(3, 0, 1), "Added 3 terms, skipped 1");
        assert_eq!(status(0, 0, 2), "No terms added; skipped 2");
        assert_eq!(status(0, 0, 0), "No new terms in that file");
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
