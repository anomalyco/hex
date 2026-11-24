//! Labs › Vocabulary and replacement files: Hex words files (`.hexwords`,
//! `docs/specs/hex-words.md`) from the Vocabulary sheet's footer. Open…
//! reads a file into memory on a worker thread, so even an 8 MiB file never
//! stalls the window, and shows its review sheet; nothing changes until
//! Add, which merges the file into the vocabulary and the Global mode's
//! replacements through the window's one settings save. Undo puts both lists
//! back until either changes or another file is added, across restarts.
//! Export… writes those two lists; other modes' replacements stay here.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

use rune::El;
use rune_kit::{Menu as KitMenu, MenuItem, Placement};

use crate::desktop::file_dialog::{Choice, Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::dictation::hex_words::{
    self, Export, Limits, Lists, Merged, OpenError, Parts, Reason, Refusal, Skip, Undo, WordsFile,
};
use crate::rune_ui::controls::{ButtonKind, Paint, button, quiet_button};
use crate::rune_ui::{Menu, Modal, Ui};

mod review;

pub(in crate::rune_ui) use review::modal;

/// A file read and waiting for Add or Cancel. It lives only in memory, and
/// only until its sheet closes: the person's file is never copied.
pub(super) struct Review {
    file: WordsFile,
    parts: Parts,
    /// The file's replacements switched to "Use this one", by index.
    theirs: BTreeSet<usize>,
    /// What Add would do with both parts on, against the lists as the sheet
    /// found them.
    preview: Merged,
}

/// The words-file dialogs, the open review, and the last import's Undo.
#[derive(Default)]
pub(in crate::rune_ui) struct WordsFiles {
    open_chooser: Option<Pending>,
    export_chooser: Option<Pending>,
    /// A chosen file being read and checked off the window's thread.
    reading: Option<Receiver<Result<WordsFile, OpenError>>>,
    review: Option<Review>,
    undo: Option<Undo>,
    /// Where the Undo copy is kept. Scenes and tests have none and keep it in
    /// memory, so they never touch the data folder.
    store: Option<PathBuf>,
}

impl WordsFiles {
    /// The window's, with the Undo copy a last import left in the data
    /// folder, if any.
    pub(in crate::rune_ui) fn load() -> Self {
        Self::at(Undo::path())
    }

    /// Keeping the Undo copy at `store`, and starting from the one there.
    pub(super) fn at(store: Option<PathBuf>) -> Self {
        let undo = store.as_deref().and_then(Undo::load);
        Self {
            store,
            undo,
            ..Self::default()
        }
    }
}

/// Labs › Vocabulary and replacement files: off, none of this shows or runs.
fn on(ui: &Ui) -> bool {
    crate::labs::Feature::VocabularyFiles.on(&ui.settings)
}

/// The two lists as the window holds them, typing not yet saved included.
fn current(ui: &Ui) -> Lists {
    Lists::of(&ui.drafted())
}

/// The footer's words-file menu, which the window's Escape closes before
/// the sheet.
pub(in crate::rune_ui) const MENU: Menu = Menu::Pick("words-file");

/// Words file…, beside the CSV buttons: a menu with Open…, Export…, and,
/// while an import can be taken back, Undo last import. One button keeps
/// the footer on one line in every language.
pub(super) fn menu_button(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let mut menu = KitMenu::new("words-file-menu")
        .placement(Placement::TopStart)
        .gutter(4.0)
        .min_width(200.0)
        .open(ui.open == Some(MENU))
        .on_close(|ui: &mut Ui| ui.open = None)
        .item(
            MenuItem::new(tr("Open words file…")).on_select(|ui: &mut Ui| {
                ui.open = None;
                choose_open(ui);
            }),
        )
        .item(
            MenuItem::new(tr("Export words file…")).on_select(|ui: &mut Ui| {
                ui.open = None;
                choose_export(ui);
            }),
        );
    if can_undo(ui) {
        menu = menu
            .separator()
            .item(
                MenuItem::new(tr("Undo last import")).on_select(|ui: &mut Ui| {
                    ui.open = None;
                    undo(ui);
                }),
            );
    }
    let trigger = button(
        p,
        "words-file",
        tr("Words file…"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| ui.open = (ui.open != Some(MENU)).then_some(MENU),
    );
    menu.trigger(trigger).build(p.k, layer)
}

fn words_filter() -> Filter {
    Filter {
        label: tr("Hex words files").into(),
        extensions: &[hex_words::EXTENSION],
    }
}

/// Open…: one file, `*.hexwords` first, with All files for a file a mail
/// app renamed. The content decides, never the name.
fn choose_open(ui: &mut Ui) {
    if ui.vocabulary.words.open_chooser.is_some() {
        return;
    }
    super::clear_status(ui);
    ui.vocabulary.words.open_chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Open a Hex words file").into(),
            target: Target::File(vec![
                words_filter(),
                Filter {
                    label: tr("All files").into(),
                    extensions: &["*"],
                },
            ]),
            directory: crate::app_paths::downloads_dir(),
        },
        super::wake(ui),
    ));
}

/// Export…: a save dialog offering `hex-words.hexwords`.
fn choose_export(ui: &mut Ui) {
    if ui.vocabulary.words.export_chooser.is_some() {
        return;
    }
    super::clear_status(ui);
    ui.vocabulary.words.export_chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Export a Hex words file").into(),
            target: Target::Save {
                filters: vec![words_filter()],
                file_name: hex_words::FILE_NAME.into(),
            },
            directory: crate::app_paths::downloads_dir(),
        },
        super::wake(ui),
    ));
}

/// The dialogs' answers, and an Undo the lists outgrew. True when the
/// window changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    if !on(ui) {
        let words = &mut ui.vocabulary.words;
        words.open_chooser = None;
        words.export_chooser = None;
        words.reading = None;
        return false;
    }
    let mut changed = forget_stale_undo(ui);
    // A dialog or a read that finishes after its sheet closed changes
    // nothing.
    if !ui.is_modal(Modal::Vocabulary) {
        ui.vocabulary.words.open_chooser = None;
        ui.vocabulary.words.export_chooser = None;
        ui.vocabulary.words.reading = None;
        return changed;
    }
    let read = match ui.vocabulary.words.reading.as_ref().map(Receiver::try_recv) {
        None | Some(Err(TryRecvError::Empty)) => None,
        Some(Ok(result)) => Some(Some(result)),
        Some(Err(TryRecvError::Disconnected)) => Some(None),
    };
    if let Some(result) = read {
        ui.vocabulary.words.reading = None;
        match result {
            Some(result) => read_done(ui, result),
            None => super::say(ui, tr("Couldn't read that file").into(), true),
        }
        changed = true;
    }
    if let Some(choice) = ui
        .vocabulary
        .words
        .open_chooser
        .as_ref()
        .and_then(Pending::poll)
    {
        ui.vocabulary.words.open_chooser = None;
        opened(ui, choice);
        changed = true;
    }
    if let Some(choice) = ui
        .vocabulary
        .words
        .export_chooser
        .as_ref()
        .and_then(Pending::poll)
    {
        ui.vocabulary.words.export_chooser = None;
        export_to(ui, choice);
        changed = true;
    }
    changed
}

/// Drops the Undo once either list changed after the import: undoing then
/// would throw that edit away.
fn forget_stale_undo(ui: &mut Ui) -> bool {
    let stale = ui
        .vocabulary
        .words
        .undo
        .as_ref()
        .is_some_and(|undo| !undo.applies_to(&current(ui)));
    if stale {
        forget_undo(ui);
    }
    stale
}

fn forget_undo(ui: &mut Ui) {
    ui.vocabulary.words.undo = None;
    if let Some(store) = &ui.vocabulary.words.store {
        Undo::forget(store);
    }
}

pub(super) fn opened(ui: &mut Ui, choice: Choice) {
    let path = match choice {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            super::say(ui, tr_fill("Couldn't open that file: {}", &error), true);
            return;
        }
    };
    if ui.vocabulary.words.reading.is_some() {
        return;
    }
    let (send, receive) = sync_channel(1);
    let wake = super::wake(ui);
    let reader = std::thread::Builder::new()
        .name("hex-words-read".into())
        .spawn(move || {
            // A window that stopped waiting needs no answer.
            let _ = send.send(hex_words::open(&path));
            wake();
        });
    match reader {
        Ok(_) => {
            ui.vocabulary.words.reading = Some(receive);
            super::say(ui, tr("Reading that file…").into(), false);
        }
        Err(error) => super::say(
            ui,
            tr_fill("Couldn't open that file: {}", &error.to_string()),
            true,
        ),
    }
}

/// What reading the chosen file gave: its review, or why it was refused.
pub(super) fn read_done(ui: &mut Ui, result: Result<WordsFile, OpenError>) {
    match result {
        Ok(file) => show_review(ui, file),
        Err(error) => {
            if let OpenError::Refused(refusal) = &error {
                tracing::info!(reason = refusal.name(), "words file refused");
            }
            let why = refusal_text(error);
            super::say(ui, tr_fill("Couldn't open that file: {}", &why), true);
        }
    }
}

/// What the open review would do with both parts on.
#[cfg(test)]
pub(super) fn preview(ui: &Ui) -> Option<&Merged> {
    Some(&ui.vocabulary.words.review.as_ref()?.preview)
}

/// Waits for the read `opened` started, as the window's poll would take it.
#[cfg(test)]
pub(super) fn finish_reading(ui: &mut Ui) {
    let receiver = ui
        .vocabulary
        .words
        .reading
        .take()
        .expect("a read in flight");
    read_done(ui, receiver.recv().expect("the reader answers"));
}

/// Why a file wasn't opened, in a few words. An OS error keeps its own text.
fn refusal_text(error: OpenError) -> String {
    let why = match error {
        OpenError::Io(text) => return text,
        OpenError::NotFile => tr("it isn't a regular file"),
        OpenError::Refused(Refusal::TooLarge) => tr("the file is larger than 8 MB"),
        OpenError::Refused(Refusal::NotUtf8) => tr("the file isn't UTF-8 text"),
        OpenError::Refused(Refusal::Empty) => tr("the file is empty"),
        OpenError::Refused(Refusal::TooDeep) => tr("it's nested more than 8 levels deep"),
        OpenError::Refused(Refusal::DuplicateKey) => tr("it names the same field twice"),
        OpenError::Refused(Refusal::NotJson) => tr("it isn't valid JSON"),
        OpenError::Refused(Refusal::NotAnObject | Refusal::NotHexWords) => {
            tr("it isn't a Hex words file")
        }
        OpenError::Refused(Refusal::BadVersion) => tr("it has no version Hex can read"),
    };
    why.to_owned()
}

/// Shows `file`'s review sheet in place of the Vocabulary sheet. Its counts
/// and skip reasons, never its words, go to the log.
fn show_review(ui: &mut Ui, file: WordsFile) {
    let mut skipped: Vec<(Reason, usize)> = Vec::new();
    for skip in &file.skipped {
        match skipped
            .iter_mut()
            .find(|(reason, _)| *reason == skip.reason)
        {
            Some((_, count)) => *count += 1,
            None => skipped.push((skip.reason, 1)),
        }
    }
    let skipped: Vec<String> = skipped
        .iter()
        .map(|(reason, count)| format!("{}:{count}", reason.name()))
        .collect();
    tracing::info!(
        version = file.version,
        terms = file.vocabulary.len(),
        replacements = file.replacements.len(),
        skipped = %skipped.join(","),
        "words file read"
    );
    if file.is_empty() && file.skipped.is_empty() {
        super::say(ui, tr("No words in that file").into(), false);
        return;
    }
    let preview = hex_words::merge(
        &current(ui),
        &file.vocabulary,
        &file.replacements,
        Limits::default(),
        Parts::default(),
        &BTreeSet::new(),
    );
    ui.vocabulary.words.review = Some(Review {
        file,
        parts: Parts::default(),
        theirs: BTreeSet::new(),
        preview,
    });
    ui.open_modal(Modal::WordsReview);
}

/// The review sheet closed some way: the file it read goes with it.
pub(in crate::rune_ui) fn on_close(ui: &mut Ui) {
    ui.vocabulary.words.review = None;
}

/// Cancel, Escape, or a press outside: nothing changes.
pub(super) fn cancel(ui: &mut Ui) {
    if ui.vocabulary.words.review.take().is_some() {
        tracing::info!("words file cancelled");
    }
    super::open(ui);
}

/// Add: merges the file with the person's switches and choices into the
/// lists as they are now, saves both in one settings save, and keeps the
/// lists from before for Undo. Another import ends the last one's Undo.
pub(super) fn add(ui: &mut Ui) {
    let Some(review) = ui.vocabulary.words.review.take() else {
        return;
    };
    let before = current(ui);
    let merged = hex_words::merge(
        &before,
        &review.file.vocabulary,
        &review.file.replacements,
        Limits::default(),
        review.parts,
        &review.theirs,
    );
    drop(review);
    forget_undo(ui);
    let summary = merged.summary;
    tracing::info!(
        added_terms = summary.added_terms,
        updated_terms = summary.updated_terms,
        added_replacements = summary.added_replacements,
        changed_replacements = summary.changed_replacements,
        kept_yours = summary.kept_yours,
        over_limit = merged.over_limit.len(),
        "words file added"
    );
    super::open(ui);
    if !summary.changed() {
        super::say(ui, added_line(&merged), false);
        return;
    }
    if !save(ui, &merged.lists) {
        let error = ui.error.clone().unwrap_or_default();
        super::say(
            ui,
            tr_fill("Couldn't save the vocabulary: {}", &error),
            true,
        );
        return;
    }
    let undo = Undo::new(before, current(ui));
    if let Some(store) = &ui.vocabulary.words.store
        && let Err(error) = undo.save(store)
    {
        // Undo still works until Hex quits.
        tracing::warn!(%error, "could not keep the words file undo copy");
    }
    ui.vocabulary.words.undo = Some(undo);
    super::say_with_undo(ui, added_line(&merged));
}

/// Saves `lists` through the window's settings save, as the Vocabulary
/// sheet's rows and the Global mode's replacements.
fn save(ui: &mut Ui, lists: &Lists) -> bool {
    ui.vocabulary.reload(&lists.vocabulary);
    if ui.update(|settings| lists.put(settings)) {
        return true;
    }
    ui.vocabulary.reload(&ui.settings.vocabulary);
    false
}

/// Undo: puts back both lists from before the last import.
pub(super) fn undo(ui: &mut Ui) {
    let Some(undo) = ui.vocabulary.words.undo.take() else {
        return;
    };
    if !undo.applies_to(&current(ui)) {
        forget_undo(ui);
        super::say(
            ui,
            tr("Your lists changed since that import, so it can't be undone").into(),
            true,
        );
        return;
    }
    if !save(ui, undo.before()) {
        let error = ui.error.clone().unwrap_or_default();
        ui.vocabulary.words.undo = Some(undo);
        super::say(
            ui,
            tr_fill("Couldn't save the vocabulary: {}", &error),
            true,
        );
        return;
    }
    forget_undo(ui);
    tracing::info!("words file import undone");
    super::say(ui, tr("Undid the last import").into(), false);
}

/// Whether Undo is on offer: an import's lists are still as it left them.
pub(super) fn can_undo(ui: &Ui) -> bool {
    on(ui) && ui.vocabulary.words.undo.is_some()
}

/// The confirmation line after Add, with its Undo.
pub(super) fn with_undo(p: Paint, line: El) -> El {
    line.center_y().gap(8.0).child(quiet_button(
        p,
        "undo-import",
        tr("Undo"),
        tr("Undo last import"),
        undo,
    ))
}

pub(super) fn export_to(ui: &mut Ui, choice: Choice) {
    let path = match choice {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            super::say(
                ui,
                tr_fill("Couldn't export the words file: {}", &error),
                true,
            );
            return;
        }
    };
    let path = super::export_path(&path, hex_words::EXTENSION);
    let drafted = ui.drafted();
    let export = hex_words::export(&Lists::of(&drafted));
    let others = drafted
        .dictation_processing
        .modes
        .iter()
        .any(|mode| !mode.replacements.is_empty());
    match crate::durable::write(&path, &export.bytes) {
        Ok(()) => {
            tracing::info!(
                terms = export.terms,
                replacements = export.replacements,
                left_out = export.left_out.len(),
                "words file exported"
            );
            super::say(ui, exported_line(&export, others), false);
        }
        Err(error) => super::say(
            ui,
            tr_fill("Couldn't export the words file: {}", &error.to_string()),
            true,
        ),
    }
}

/// "Added 3 terms and 2 replacements", or what of that there is.
fn added(terms: usize, replacements: usize) -> Option<String> {
    let (t, r) = (terms.to_string(), replacements.to_string());
    Some(match (terms, replacements) {
        (0, 0) => return None,
        (1, 0) => tr("Added 1 term").into(),
        (_, 0) => tr_fill("Added {} terms", &t),
        (0, 1) => tr("Added 1 replacement").into(),
        (0, _) => tr_fill("Added {} replacements", &r),
        (1, 1) => tr("Added 1 term and 1 replacement").into(),
        (1, _) => tr_fill("Added 1 term and {} replacements", &r),
        (_, 1) => tr_fill("Added {} terms and 1 replacement", &t),
        _ => tr_named(
            "Added {terms} terms and {replacements} replacements",
            &[("terms", &t), ("replacements", &r)],
        ),
    })
}

/// Terms that gained spellings, starting the line or after its start.
fn updated(count: usize, first: bool) -> Option<String> {
    let n = count.to_string();
    Some(match (count, first) {
        (0, _) => return None,
        (1, true) => tr("Updated 1 term").into(),
        (_, true) => tr_fill("Updated {} terms", &n),
        (1, false) => tr("updated 1 term").into(),
        (_, false) => tr_fill("updated {} terms", &n),
    })
}

/// Clashes switched to the file's output.
fn changed(count: usize, first: bool) -> Option<String> {
    let n = count.to_string();
    Some(match (count, first) {
        (0, _) => return None,
        (1, true) => tr("Changed 1 replacement").into(),
        (_, true) => tr_fill("Changed {} replacements", &n),
        (1, false) => tr("changed 1 replacement").into(),
        (_, false) => tr_fill("changed {} replacements", &n),
    })
}

/// The line after Add: "Added 3 terms and 2 replacements · 1 already there ·
/// kept yours for 1", or "No new words in that file".
fn added_line(merged: &Merged) -> String {
    let summary = merged.summary;
    let mut parts: Vec<String> = Vec::new();
    parts.extend(added(summary.added_terms, summary.added_replacements));
    parts.extend(updated(summary.updated_terms, parts.is_empty()));
    parts.extend(changed(summary.changed_replacements, parts.is_empty()));
    if parts.is_empty() {
        return tr("No new words in that file").into();
    }
    let already = summary.already_have_terms + summary.already_have_replacements;
    if already > 0 {
        parts.push(tr_fill("{} already there", &already.to_string()));
    }
    if summary.kept_yours > 0 {
        parts.push(tr_fill(
            "kept yours for {}",
            &summary.kept_yours.to_string(),
        ));
    }
    if !merged.over_limit.is_empty() {
        parts.push(tr_fill(
            "{} didn't fit",
            &merged.over_limit.len().to_string(),
        ));
    }
    parts.join(" · ")
}

/// The line after Export…: what went in, what was left out and why, and
/// that other modes' replacements stay on this computer.
fn exported_line(export: &Export, others: bool) -> String {
    let (t, r) = (export.terms.to_string(), export.replacements.to_string());
    let mut parts = vec![match (export.terms, export.replacements) {
        (0, 0) => tr("Exported an empty words file").into(),
        (1, 0) => tr("Exported 1 term").into(),
        (_, 0) => tr_fill("Exported {} terms", &t),
        (0, 1) => tr("Exported 1 replacement").into(),
        (0, _) => tr_fill("Exported {} replacements", &r),
        (1, 1) => tr("Exported 1 term and 1 replacement").into(),
        (1, _) => tr_fill("Exported 1 term and {} replacements", &r),
        (_, 1) => tr_fill("Exported {} terms and 1 replacement", &t),
        _ => tr_named(
            "Exported {terms} terms and {replacements} replacements",
            &[("terms", &t), ("replacements", &r)],
        ),
    }];
    if !export.left_out.is_empty() {
        parts.push(tr_fill("left out {}", &reasons(&export.left_out)));
    }
    if others {
        parts.push(tr("other modes' replacements stay on this computer").into());
    }
    parts.join(" · ")
}

/// How many were skipped for each reason, in the spec's order: "2 too
/// long, 1 with hidden characters".
fn reasons(skipped: &[Skip]) -> String {
    const ORDER: [Reason; 9] = [
        Reason::BadEntry,
        Reason::UnknownField,
        Reason::HiddenCharacters,
        Reason::NoLetters,
        Reason::TooLong,
        Reason::TooManyWords,
        Reason::Duplicate,
        Reason::SameAsTerm,
        Reason::OverLimit,
    ];
    let group = |reason: Reason| match reason {
        // A part, entry, or field of the wrong shape reads as one kind.
        Reason::BadPart | Reason::BadField => Reason::BadEntry,
        other => other,
    };
    ORDER
        .into_iter()
        .filter_map(|reason| {
            let count = skipped
                .iter()
                .filter(|skip| group(skip.reason) == reason)
                .count();
            (count > 0).then(|| reason_text(reason, count))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn reason_text(reason: Reason, count: usize) -> String {
    let count = count.to_string();
    match reason {
        Reason::BadPart | Reason::BadEntry | Reason::BadField => {
            tr_fill("{} in the wrong shape", &count)
        }
        Reason::UnknownField => tr_fill("{} unknown to this Hex", &count),
        Reason::HiddenCharacters => tr_fill("{} with hidden characters", &count),
        Reason::NoLetters => tr_fill("{} without letters or digits", &count),
        Reason::TooLong => tr_fill("{} too long", &count),
        Reason::TooManyWords => tr_fill("{} over 8 words", &count),
        Reason::Duplicate => tr_fill("{} listed twice", &count),
        Reason::SameAsTerm => tr_fill("{} same as their term", &count),
        Reason::OverLimit => tr_fill("{} over the file's limits", &count),
    }
}

/// A file from a phone, read: a term with a new spelling, one already
/// listed, new terms in other scripts, a clash, a replacement already
/// there, and one the file itself left out.
const FIXTURE: &str = r#"{"format": "hex.words", "version": 1,
    "madeBy": {"app": "Hex for iPhone", "version": "1.0.3"},
    "vocabulary": [{"term": "OpenCode", "soundsLike": ["open code", "open coat"]},
        {"term": "Rune"}, {"term": "Next.js"}, {"term": "Zoë"}, {"term": "שלום"}],
    "replacements": [{"heard": "jet hub", "write": "GitHub"},
        {"heard": "cloud code", "write": "Claude Code"}, {"heard": "um", "write": ""},
        {"heard": "toda", "write": "תודה"}, {"heard": "new\u200bline", "write": "x"}]}"#;

/// Scenes: the Vocabulary sheet's terms with the lab on, the Global mode's
/// replacements, and either the review of [`FIXTURE`] or the line after
/// adding it, with Undo.
pub(in crate::rune_ui) fn fixture(ui: &mut Ui, review: bool) {
    use crate::dictation::text_replacements::TextReplacement;
    ui.settings.labs.vocabulary_files = true;
    super::fixture(ui, true);
    ui.settings.dictation_processing.default_mode.replacements = vec![
        TextReplacement {
            matched_phrase: "jet hub".into(),
            output: "Git Hub".into(),
        },
        TextReplacement {
            matched_phrase: "um".into(),
            output: String::new(),
        },
    ];
    ui.modes = crate::rune_ui::modes::ModesPane::new(&ui.settings);
    let Ok(file) = hex_words::read(FIXTURE.as_bytes()) else {
        return;
    };
    if review {
        show_review(ui, file);
        return;
    }
    let before = current(ui);
    let merged = hex_words::merge(
        &before,
        &file.vocabulary,
        &file.replacements,
        Limits::default(),
        Parts::default(),
        &BTreeSet::new(),
    );
    ui.vocabulary.reload(&merged.lists.vocabulary);
    merged.lists.put(&mut ui.settings);
    ui.modes = crate::rune_ui::modes::ModesPane::new(&ui.settings);
    ui.vocabulary.words.undo = Some(Undo::new(before, current(ui)));
    super::say_with_undo(ui, added_line(&merged));
}

/// Scenes: the review of a long file, whose parts and clashes scroll.
pub(in crate::rune_ui) fn long_fixture(ui: &mut Ui) {
    fixture(ui, false);
    let terms: Vec<serde_json::Value> = (1..=24)
        .map(|n| serde_json::json!({"term": format!("Project {n}"), "soundsLike": [format!("pro jet {n}")]}))
        .collect();
    let rules: Vec<serde_json::Value> = (1..=16)
        .map(|n| serde_json::json!({"heard": format!("shortcut {n}"), "write": format!("Expanded text {n}")}))
        .collect();
    let file = serde_json::json!({
        "format": "hex.words",
        "version": 2,
        "madeBy": {"app": "Hex for Android", "version": "0.2.2"},
        "vocabulary": terms,
        "replacements": rules,
    });
    if let Ok(file) = hex_words::read(file.to_string().as_bytes()) {
        show_review(ui, file);
    }
}

/// The cross-check's files exported by Hex for iPhone and Hex for Android
/// (`words_file/from-*.hexwords`, byte for byte): scenes and tests read
/// them with the real reader and merger.
pub(in crate::rune_ui) const FROM_IPHONE: &[u8] = include_bytes!("words_file/from-iphone.hexwords");
pub(in crate::rune_ui) const FROM_ANDROID: &[u8] =
    include_bytes!("words_file/from-android.hexwords");

/// Scenes and tests: the cross-check's receiving lists (vocabulary NextJS,
/// Global jet hub → Git Hub) with the lab on and the Vocabulary sheet open.
pub(in crate::rune_ui) fn receiving_fixture(ui: &mut Ui) {
    use crate::dictation::text_replacements::TextReplacement;
    use crate::dictation::vocabulary::VocabularyEntry;
    ui.settings.labs.vocabulary_files = true;
    ui.settings.vocabulary = vec![VocabularyEntry {
        term: "NextJS".into(),
        sounds_like: Vec::new(),
    }];
    ui.settings.dictation_processing.default_mode.replacements = vec![TextReplacement {
        matched_phrase: "jet hub".into(),
        output: "Git Hub".into(),
    }];
    ui.vocabulary.reload(&ui.settings.vocabulary);
    ui.modes = crate::rune_ui::modes::ModesPane::new(&ui.settings);
    super::open(ui);
}

/// Scenes: `bytes` read and reviewed over the receiving lists.
pub(in crate::rune_ui) fn received_fixture(ui: &mut Ui, bytes: &[u8]) {
    receiving_fixture(ui);
    if let Ok(file) = hex_words::read(bytes) {
        show_review(ui, file);
    }
}
