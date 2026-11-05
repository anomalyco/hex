//! The History pane: retained dictations (newest first) with search, one
//! menu for retention, Keep recordings, and two-step Clear all history, and
//! a detail view with Copy and Delete. It shares the host's store, which the listener records into.
//! With Labs › Keep recordings on, an entry that kept its audio offers
//! Transcribe again with any installed model, run on a worker; Replace keeps
//! the earlier transcripts as versions to copy or switch back to. Every
//! entry names the model that made its text, when it is known.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::{Menu as KitMenu, MenuItem, Placement, Select, SelectAppearance, TextInput};

use super::controls::{ButtonKind, Paint, button};
use super::list_detail::{self as ld, Item};
use super::lists::{error_panel, section_label};
use super::{Menu, Ui};
use crate::desktop::i18n::{event_age, tr, tr_fill, tr_named};
use crate::history::stats::HistoryStats;
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention, Transcript};
use crate::transcription::models::TranscriptionModelId;

#[derive(Default)]
pub(super) struct HistoryPane {
    store: Option<History>,
    pub(super) entries: Vec<HistoryEntry>,
    search: TextEdit,
    /// The query the entries were loaded for.
    searched: String,
    /// The store's revision the entries were loaded at; `None` before the
    /// first load.
    revision: Option<u64>,
    pub(super) selected: Option<u64>,
    pub(super) clear_armed: bool,
    pub(super) copied: Option<u64>,
    error: Option<String>,
    /// Labs › Dictation stats: every retained entry summed up, while on.
    pub(super) stats: Option<HistoryStats>,
    /// Labs › Keep recordings: the installed models Transcribe again
    /// offers, read when its menu opens.
    models: Vec<(TranscriptionModelId, &'static str)>,
    /// The latest Transcribe again, for one entry.
    pub(super) again: Option<Again>,
    /// A model chosen for Transcribe again that takes a language hint,
    /// waiting for its language and Transcribe.
    pub(super) pending: Option<Pending>,
    /// The version whose Copy was pressed last: entry and index.
    pub(super) version_copied: Option<(u64, usize)>,
}

/// Transcribe again's chosen model and the language for this one run.
pub(super) struct Pending {
    pub(super) id: u64,
    model: TranscriptionModelId,
    name: &'static str,
    pub(super) language: String,
}

/// One entry's recording transcribed again with another model.
pub(super) struct Again {
    pub(super) id: u64,
    /// The model's name.
    pub(super) model: String,
    /// The language chosen for the run; `None` when the model chose.
    pub(super) language: Option<String>,
    worker: Option<Receiver<Result<String, String>>>,
    pub(super) result: Option<Result<String, String>>,
    pub(super) copied: bool,
}

impl Again {
    /// A finished run, as if its worker had answered.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn finished(id: u64, model: &str, result: Result<String, String>) -> Self {
        Self {
            id,
            model: model.into(),
            language: None,
            worker: None,
            result: Some(result),
            copied: false,
        }
    }
}

impl HistoryPane {
    /// The host's store; `None` when it could not open one.
    pub(super) fn shared(store: Option<History>) -> Self {
        Self {
            error: store
                .is_none()
                .then(|| tr("History could not be loaded.").to_string()),
            store,
            ..Self::default()
        }
    }

    /// Refreshes the bounded snapshot for the current query, and the stats.
    pub(super) fn reload(&mut self) {
        self.reload_entries();
        self.reload_stats();
    }

    fn reload_entries(&mut self) {
        self.searched = self.search.text.clone();
        let Some(store) = &self.store else {
            return;
        };
        let (revision, entries) = store.search_at(&self.searched);
        self.revision = Some(revision);
        self.entries = entries;
        if self
            .selected
            .is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.selected = None;
        }
    }

    /// Sums up every retained entry, only while Labs › Dictation stats is
    /// on; off, nothing is read for it.
    fn reload_stats(&mut self) {
        self.stats = match &self.store {
            Some(store) if crate::labs::history_stats() => Some(store.stats()),
            _ => None,
        };
    }

    /// While the pane shows, picks up new dictations and search edits. It
    /// searches again only when the query or the store's revision changed,
    /// and sums the stats again only when the entries or the toggle changed.
    pub(super) fn poll(&mut self, visible: bool) -> bool {
        let answered = self.poll_again();
        let Some(store) = self.store.as_ref().filter(|_| visible) else {
            return answered;
        };
        let stale = self.search.text != self.searched || self.revision != Some(store.revision());
        let mut changed = false;
        if stale {
            let previous = std::mem::take(&mut self.entries);
            self.reload_entries();
            changed = self.entries != previous;
        }
        if changed || self.stats.is_some() != crate::labs::history_stats() {
            self.reload_stats();
            return true;
        }
        answered
    }

    /// Takes Transcribe again's answer once its worker sends it.
    fn poll_again(&mut self) -> bool {
        let Some(again) = &mut self.again else {
            return false;
        };
        let result = match again.worker.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err(tr("Transcription stopped.").into()),
            Some(Err(TryRecvError::Empty)) | None => return false,
        };
        again.worker = None;
        again.result = Some(result);
        true
    }

    fn entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub(super) fn fixture() -> Self {
        let now = crate::history::now_ms();
        let entry =
            |id: u64, minutes: u64, kind, raw: &str, final_text: &str, app: Option<&str>| {
                HistoryEntry {
                    id,
                    timestamp_ms: now - minutes * 60_000 - 30_000,
                    kind,
                    raw_text: raw.into(),
                    final_text: final_text.into(),
                    application: app.map(str::to_owned),
                    processing: None,
                    audio_ms: 4200,
                    inference_ms: 180,
                    total_ms: 640,
                    recording_bytes: 0,
                    model: Some("Parakeet v3".into()),
                    language: None,
                    text_ms: None,
                    versions: Vec::new(),
                }
            };
        let mut rewritten = entry(
            2,
            42,
            HistoryKind::Dictation,
            "so um the deploy failed because the the cache key changed",
            "The deploy failed because the cache key changed.",
            Some("Slack"),
        );
        rewritten.processing = Some(crate::desktop::events::DictationProcessing {
            profile: "Chat".into(),
            latency_ms: 910,
            fallback: None,
        });
        Self {
            entries: vec![
                entry(
                    3,
                    2,
                    HistoryKind::Dictation,
                    "Let's move the standup to ten thirty tomorrow.",
                    "Let's move the standup to ten thirty tomorrow.",
                    Some("WezTerm"),
                ),
                rewritten,
                entry(
                    1,
                    180,
                    HistoryKind::Dictation,
                    "Thanks for sending this over! I'll review it today and follow up with comments.",
                    "Thanks for sending this over! I'll review it today and follow up with comments.",
                    Some("Firefox"),
                ),
            ],
            selected: Some(2),
            ..Self::default()
        }
    }

    pub(super) fn fixture_empty() -> Self {
        Self::default()
    }
}

fn set_retention(ui: &mut Ui, retention: HistoryRetention) {
    ui.open = None;
    if ui.settings.history_retention == retention {
        return;
    }
    if !ui.update(|settings| settings.history_retention = retention) {
        return;
    }
    if let Some(store) = &ui.history.store
        && let Err(error) = store.set_retention(retention)
    {
        ui.history.error = Some(error.to_string());
    }
    ui.history.reload();
}

fn clear(ui: &mut Ui) {
    if !ui.history.clear_armed {
        ui.history.clear_armed = true;
        return;
    }
    ui.history.clear_armed = false;
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.clear() {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else {
        ui.history.entries.clear();
        ui.history.selected = None;
    }
}

/// The models Transcribe again can use: every available one that is
/// installed for the language it would run in.
fn installed_models(ui: &Ui) -> Vec<(TranscriptionModelId, &'static str)> {
    crate::transcription::models::all_models()
        .into_iter()
        .filter(|model| model.available())
        .filter(|model| {
            let selection = ui.settings.transcription_for_model(model.id);
            crate::transcription::models::is_installed(model, &selection.language)
        })
        .map(|model| (model.id, model.name))
        .collect()
}

/// The languages Transcribe again offers for `model`: Auto when it detects
/// the language, then the ones it supports. `None` when it takes no hint, or
/// has only one, so there is nothing to choose.
pub(super) fn language_choices(model: TranscriptionModelId) -> Option<Vec<&'static str>> {
    let model = crate::transcription::models::definition(model);
    let choices = model.language_choices();
    (model.accepts_language_hint && choices.len() > 1).then_some(choices)
}

/// A model chosen from Transcribe again's menu: one that takes a language
/// waits for it, starting at the saved language; any other runs at once.
pub(super) fn choose_model(ui: &mut Ui, id: u64, model: TranscriptionModelId, name: &'static str) {
    ui.open = None;
    let Some(choices) = language_choices(model) else {
        ui.history.pending = None;
        transcribe_again(ui, id, model, name, None);
        return;
    };
    let saved = &ui.settings.transcription.language;
    let language = if choices.contains(&saved.as_str()) {
        saved.clone()
    } else {
        ui.settings.transcription_for_model(model).language
    };
    ui.history.pending = Some(Pending {
        id,
        model,
        name,
        language,
    });
}

/// Runs the pending model with its chosen language.
fn transcribe_pending(ui: &mut Ui) {
    if let Some(pending) = ui.history.pending.take() {
        transcribe_again(
            ui,
            pending.id,
            pending.model,
            pending.name,
            Some(&pending.language),
        );
    }
}

/// Starts transcribing entry `id`'s recording again with `model`, in
/// `language` for this run only, or the model's usual language.
fn transcribe_again(
    ui: &mut Ui,
    id: u64,
    model: TranscriptionModelId,
    name: &str,
    language: Option<&str>,
) {
    ui.open = None;
    if ui
        .history
        .again
        .as_ref()
        .is_some_and(|again| again.worker.is_some())
    {
        return;
    }
    let Some(path) = ui
        .history
        .store
        .as_ref()
        .and_then(|store| store.recording_path(id))
    else {
        return;
    };
    let mut selection = ui.settings.transcription_for_model(model);
    if let Some(language) = language {
        selection.language = language.into();
    }
    let (sender, receiver) = channel();
    let waker = ui.modes.waker.clone();
    let started = std::thread::Builder::new()
        .name("hex-transcribe-again".into())
        .spawn(move || {
            let _ = sender.send(crate::history::recordings::retranscribe(&path, selection));
            if let Some(waker) = &waker {
                waker();
            }
        });
    ui.history.again = Some(Again {
        id,
        model: name.into(),
        language: language
            .filter(|language| *language != crate::transcription::models::AUTO_LANGUAGE)
            .map(str::to_owned),
        worker: started.is_ok().then_some(receiver),
        result: started.err().map(|error| Err(error.to_string())),
        copied: false,
    });
}

/// Replaces entry `id`'s text with what Transcribe again found, keeping
/// the current one as its newest version.
fn replace(ui: &mut Ui, id: u64) {
    let Some(transcript) = ui
        .history
        .again
        .as_ref()
        .filter(|again| again.id == id)
        .and_then(|again| match &again.result {
            Some(Ok(text)) => Some(Transcript {
                text: text.clone(),
                model: Some(again.model.clone()),
                language: again.language.clone(),
            }),
            _ => None,
        })
    else {
        return;
    };
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.replace_text(id, transcript) {
            ui.history.error = Some(error.to_string());
            return;
        }
        ui.history.again = None;
        ui.history.reload();
    } else if let Some(entry) = ui.history.entries.iter_mut().find(|entry| entry.id == id) {
        entry.replace(transcript, crate::history::now_ms());
        ui.history.again = None;
    }
}

/// Brings entry `id`'s version `index` back as its text.
fn restore(ui: &mut Ui, id: u64, index: usize) {
    ui.history.version_copied = None;
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.restore_version(id, index) {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else if let Some(entry) = ui.history.entries.iter_mut().find(|entry| entry.id == id) {
        entry.restore(index);
    }
}

fn delete(ui: &mut Ui, id: u64) {
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.delete(id) {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else {
        ui.history.entries.retain(|entry| entry.id != id);
        ui.history.selected = None;
    }
}

/// The header's retention menu and the list + detail body.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let pane = &ui.history;
    let actions = header_actions(ui, p, layer);
    let mut notices = Vec::new();
    if let Some(error) = &pane.error {
        notices.push(error_panel(
            p,
            "history-error",
            tr("History could not be loaded."),
            error,
        ));
    }
    if ui.settings.labs.history_stats
        && let Some(stats) = pane.stats.as_ref().filter(|stats| stats.dictations > 0)
    {
        notices.push(stats_block(p, stats));
    }
    let searching = !pane.searched.trim().is_empty();
    let body = match empty(ui, p).filter(|_| !searching) {
        Some(empty) => empty,
        None => {
            let mut list = col().key("history-list").label(tr("History"));
            let count = pane.entries.len();
            for (index, entry) in pane.entries.iter().enumerate() {
                let id = entry.id;
                let mut meta = tr(entry.kind.label()).to_string();
                if let Some(application) = &entry.application {
                    meta.push_str(" · ");
                    meta.push_str(application);
                }
                list = list.child(
                    ld::item(
                        p,
                        Item {
                            key: format!("history-entry{id}"),
                            title: entry.final_text.replace('\n', " "),
                            meta,
                            meta_ink: None,
                            time: event_age(entry.timestamp_ms),
                            hover: vec![copy_action(ui, p, id, true)],
                            selected: pane.selected == Some(id),
                        },
                        move |ui: &mut Ui| {
                            ui.history.selected = Some(id);
                            ui.history.clear_armed = false;
                        },
                    )
                    .position_in_set(index + 1, count),
                );
            }
            let search = TextInput::new(&pane.search)
                .placeholder(tr("Search history"))
                .width(None)
                .leading_icon(p.k.glyph.search)
                .on_edit(|ui: &mut Ui| &mut ui.history.search)
                .build(p.k)
                .key("history-search");
            let column = ld::list_column(p, search, "history-scroll", list);
            ld::body(p, "history-body", column, detail(ui, p, layer))
        }
    };
    (
        actions,
        col()
            .key("history-pane")
            .grow(1.0)
            .shrink()
            .children((!notices.is_empty()).then(|| ld::notices(notices)))
            .child(body),
    )
}

/// Labs › Dictation stats: every retained dictation summed up, above the
/// list. Shown only while something is retained.
fn stats_block(p: Paint, stats: &HistoryStats) -> El {
    let wpm = stats
        .words_per_minute
        .map_or_else(|| "–".to_owned(), |wpm| wpm.to_string());
    let mut lines = vec![
        tr_named(
            "Dictations: {count} · Words: {words} · Words per minute: {wpm}",
            &[
                ("count", &stats.dictations.to_string()),
                ("words", &stats.words.to_string()),
                ("wpm", &wpm),
            ],
        ),
        tr_named(
            "Changed by Output: {percent}% · Daily streak: {days}",
            &[
                (
                    "percent",
                    &stats.changed_percent().unwrap_or_default().to_string(),
                ),
                ("days", &stats.streak_days.to_string()),
            ],
        ),
    ];
    if !stats.top_apps.is_empty() {
        let apps = stats
            .top_apps
            .iter()
            .map(|app| format!("{} ({})", app.application, app.dictations))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(tr_fill("Top apps: {}", &apps));
    }
    col()
        .key("history-stats")
        .gap(4.0)
        .px(12.0)
        .py(10.0)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .child(section_label(p, "label", tr("Dictation stats")))
        .children(lines.into_iter().enumerate().map(|(index, line)| {
            text(line)
                .key(format!("line{index}"))
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap()
        }))
}

/// What the retention menu's trigger says: how long History keeps
/// dictations, or that it is off.
fn kept_label(retention: HistoryRetention) -> String {
    match retention {
        HistoryRetention::Off => tr("History off").to_string(),
        HistoryRetention::Forever => tr("Kept forever").to_string(),
        other => tr_fill("Kept {}", tr(other.label())),
    }
}

/// One menu for everything about keeping: how long, Labs › Keep recordings
/// where that lab shows, and Clear all history… last. Once Clear all is
/// chosen, the menu gives way to its confirmation until it is answered.
fn header_actions(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<El> {
    if ui.history.clear_armed {
        return vec![
            button(
                p,
                "history-confirm-clear",
                tr("Clear all history"),
                ButtonKind::Danger,
                |ui: &mut Ui, _| clear(ui),
            ),
            button(
                p,
                "history-cancel-clear",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.history.clear_armed = false,
            ),
        ];
    }
    let saved = ui.settings.history_retention;
    let open = ui.open == Some(Menu::HistoryRetention);
    let current = kept_label(saved);
    let trigger = Select::new::<&str>("history-retention-field", &[])
        .appearance(SelectAppearance::Inline)
        .current(Some(&current))
        .open(open)
        .on_toggle(|ui: &mut Ui| {
            ui.open = (ui.open != Some(Menu::HistoryRetention)).then_some(Menu::HistoryRetention);
        })
        .trigger(p.k)
        .label(tr("History retention"))
        .a11y_value(current.clone());
    let mut menu = KitMenu::new("history-retention")
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(320.0)
        .open(open)
        .on_close(|ui: &mut Ui| ui.open = None);
    for retention in HistoryRetention::ALL {
        let mut item = MenuItem::new(tr(retention.label())).checked(saved == retention);
        if retention.is_off() {
            item = item.description(tr("Keeps no new dictations"));
        }
        menu = menu.item(item.on_select(move |ui: &mut Ui| set_retention(ui, retention)));
    }
    let keep_audio = crate::labs::Feature::KeepRecordings;
    if keep_audio
        .audience()
        .reaches(crate::dev_build::is_dev_build())
        && keep_audio.managed().is_none()
    {
        menu = menu.separator().item(
            MenuItem::new(tr("Keep audio too"))
                .badge(tr("Lab"))
                .description(tr("Lets you transcribe a dictation again"))
                .checked(ui.settings.labs.keep_recordings)
                .on_select(move |ui: &mut Ui| {
                    ui.open = None;
                    let on = !ui.settings.labs.keep_recordings;
                    ui.update(|settings| keep_audio.set(settings, on));
                }),
        );
    }
    let nothing = ui.history.entries.is_empty() && ui.history.searched.trim().is_empty();
    menu = menu.separator().item(
        MenuItem::new(tr("Clear all history…"))
            .danger(true)
            .disabled(nothing)
            .on_select(|ui: &mut Ui| {
                ui.open = None;
                clear(ui);
            }),
    );
    vec![menu.build(p.k, layer)]
}

/// Copy as an icon: a row's hover action, or the card's.
fn copy_action(ui: &Ui, p: Paint, id: u64, in_row: bool) -> El {
    let glyph = if ui.history.copied == Some(id) {
        p.k.glyph.check_small
    } else {
        ld::COPY_ICON
    };
    let press = move |ui: &mut Ui, tx: &mut Tx| {
        if let Some(entry) = ui.history.entry(id) {
            tx.copy(entry.final_text.clone());
            ui.history.copied = Some(id);
            ui.history.error = None;
        }
    };
    if in_row {
        ld::row_action(p, "row-copy", glyph, tr("Copy"), press)
    } else {
        ld::ghost_icon(p, "history-copy", glyph, tr("Copy"), press)
    }
}

/// Nothing retained, or nothing matching: what History keeps and for how
/// long.
fn empty(ui: &Ui, p: Paint) -> Option<El> {
    let pane = &ui.history;
    if !pane.entries.is_empty() {
        return None;
    }
    let retention = ui.settings.history_retention;
    Some(if !pane.searched.trim().is_empty() {
        ld::empty_state(
            p,
            "history-empty",
            ld::SEARCH_ICON,
            tr("No dictations match."),
            None,
        )
    } else if retention.is_off() {
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("History is off."),
            Some(tr("Choose how long to keep dictations to see them here.")),
        )
    } else {
        let hint = match retention {
            HistoryRetention::Forever => tr("Everything you dictate is kept here.").to_string(),
            other => tr_fill(
                "Everything you dictate is kept here for {}.",
                tr(other.label()),
            ),
        };
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("No dictations retained yet."),
            Some(&hint),
        )
    })
}

fn detail(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let pane = &ui.history;
    let Some(entry) = pane.selected.and_then(|id| pane.entry(id)) else {
        return empty(ui, p).unwrap_or_else(|| {
            ld::empty_state(
                p,
                "history-detail",
                ld::HISTORY_ICON,
                tr("Select a history entry."),
                None,
            )
        });
    };
    let id = entry.id;
    let mut latency = tr_named(
        "{audio} ms audio · {inference} ms inference",
        &[
            ("audio", &entry.audio_ms.to_string()),
            ("inference", &entry.inference_ms.to_string()),
        ],
    );
    if entry.total_ms > 0 {
        latency.push_str(&format!(
            " · {}",
            tr_fill("{} ms total", &entry.total_ms.to_string())
        ));
    }
    let mut facts = Vec::new();
    if let Some(application) = &entry.application {
        facts.push((tr("Application"), application.clone()));
    }
    if let Some(processing) = &entry.processing {
        let mut summary = format!("{} · {} ms", processing.profile, processing.latency_ms);
        if let Some(fallback) = &processing.fallback {
            summary.push_str(&format!(" · {}", tr_fill("fell back: {}", fallback)));
        }
        facts.push((tr("Processing"), summary));
    }
    if let Some(model) = model_label(entry.model.as_deref(), entry.language.as_deref()) {
        facts.push((tr("Speech model"), model));
    }
    facts.push((tr("Latency"), latency));
    let mut body = col().key("detail-body").gap(20.0).child(
        text(entry.final_text.clone())
            .key("final")
            .font(p.k.ty.body)
            .color(p.text())
            .wrap(),
    );
    if entry.raw_text.trim() != entry.final_text.trim() {
        body = body.child(
            col()
                .key("raw")
                .gap(6.0)
                .child(section_label(p, "label", tr("Raw transcript")))
                .child(
                    text(entry.raw_text.clone())
                        .key("value")
                        .font(p.k.ty.body)
                        .color(p.muted())
                        .wrap(),
                ),
        );
    }
    body = body.child(ld::facts(p, facts));
    if ui.settings.labs.keep_recordings && entry.has_recording() {
        body = body.child(again_section(ui, p, id, layer));
    }
    if ui.settings.labs.keep_recordings && !entry.versions.is_empty() {
        body = body.child(versions_section(ui, p, entry));
    }
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, tr(entry.kind.label())),
            vec![ld::meta_text(p, &event_age(entry.timestamp_ms))],
            vec![
                copy_action(ui, p, id, false),
                ld::ghost_icon(
                    p,
                    "history-delete",
                    ld::TRASH_ICON,
                    tr("Delete"),
                    move |ui: &mut Ui, _| delete(ui, id),
                ),
            ],
        ))
        .child(ld::card_scroll(p, "detail-scroll", body))
}

/// Labs › Keep recordings: Transcribe again's menu of installed models, and
/// the latest run for this entry with Copy and Replace.
fn again_section(ui: &Ui, p: Paint, id: u64, layer: &mut Vec<El>) -> El {
    let again = ui.history.again.as_ref().filter(|again| again.id == id);
    let running = again.is_some_and(|again| again.worker.is_some());
    let labels: Vec<&str> = ui.history.models.iter().map(|(_, name)| *name).collect();
    let current = match again {
        Some(again) if running => tr_fill("Transcribing with {}…", &again.model),
        _ => tr("Transcribe again").to_string(),
    };
    let menu = Select::new("history-again", &labels)
        .appearance(SelectAppearance::Inline)
        .current(Some(&current))
        .open(!running && ui.open == Some(Menu::HistoryAgain))
        .on_toggle(|ui: &mut Ui| {
            if ui.open == Some(Menu::HistoryAgain) {
                ui.open = None;
            } else {
                ui.history.models = installed_models(ui);
                ui.open = Some(Menu::HistoryAgain);
            }
        })
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(move |ui: &mut Ui, choice: &str, _: &mut Tx| {
            if let Some(&(model, name)) = ui.history.models.iter().find(|(_, name)| *name == choice)
            {
                choose_model(ui, id, model, name);
            }
        })
        .build(p.k, layer)
        .label(tr("Transcribe again"))
        .a11y_value(current.clone());
    let mut section = col()
        .key("again")
        .gap(6.0)
        .child(section_label(p, "label", tr("Saved recording")))
        .child(menu);
    if let Some(pending) = ui
        .history
        .pending
        .as_ref()
        .filter(|pending| pending.id == id && !running)
    {
        section = section.child(language_row(ui, p, pending, layer));
    }
    match again.and_then(|again| again.result.as_ref().map(|result| (again, result))) {
        Some((again, Ok(found))) => {
            section = section
                .child(section_label(
                    p,
                    "again-label",
                    &tr_fill("Transcribed again with {}", &again.model),
                ))
                .child(
                    text(if found.is_empty() {
                        tr("No speech was found.").to_string()
                    } else {
                        found.clone()
                    })
                    .key("again-text")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap(),
                );
            if !found.is_empty() {
                section = section.child(
                    row()
                        .key("again-actions")
                        .gap(8.0)
                        .child(button(
                            p,
                            "again-copy",
                            if again.copied {
                                tr("Copied")
                            } else {
                                tr("Copy")
                            },
                            ButtonKind::Secondary,
                            move |ui: &mut Ui, tx: &mut Tx| {
                                if let Some(again) =
                                    ui.history.again.as_mut().filter(|again| again.id == id)
                                    && let Some(Ok(text)) = &again.result
                                {
                                    tx.copy(text.clone());
                                    again.copied = true;
                                }
                            },
                        ))
                        .child(button(
                            p,
                            "again-replace",
                            tr("Replace"),
                            ButtonKind::Secondary,
                            move |ui: &mut Ui, _| replace(ui, id),
                        )),
                );
            }
        }
        Some((_, Err(error))) => {
            section = section.child(
                text(tr_fill("Could not transcribe again: {}", tr(error)))
                    .key("again-error")
                    .font(super::controls::DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
            );
        }
        None => {}
    }
    section
}

/// The chosen model's language for this run, and Transcribe.
fn language_row(ui: &Ui, p: Paint, pending: &Pending, layer: &mut Vec<El>) -> El {
    use crate::transcription::models::language_name;
    let choices = language_choices(pending.model).unwrap_or_default();
    let labels: Vec<&str> = choices.iter().map(|code| language_name(code)).collect();
    let current = tr_named(
        "{model} · Language: {language}",
        &[
            ("model", pending.name),
            ("language", language_name(&pending.language)),
        ],
    );
    let languages = Select::new("again-language", &labels)
        .appearance(SelectAppearance::Inline)
        .current(Some(&current))
        .open(ui.open == Some(Menu::HistoryAgainLanguage))
        .on_toggle(|ui: &mut Ui| {
            ui.open =
                (ui.open != Some(Menu::HistoryAgainLanguage)).then_some(Menu::HistoryAgainLanguage);
        })
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(|ui: &mut Ui, choice: &str, _: &mut Tx| {
            ui.open = None;
            if let Some(pending) = &mut ui.history.pending
                && let Some(code) = language_choices(pending.model).and_then(|choices| {
                    choices
                        .into_iter()
                        .find(|code| crate::transcription::models::language_name(code) == choice)
                })
            {
                pending.language = code.into();
            }
        })
        .build(p.k, layer)
        .label(tr("Language"))
        .a11y_value(current.clone());
    row()
        .key("again-pending")
        .gap(8.0)
        .child(languages)
        .child(button(
            p,
            "again-start",
            tr("Transcribe"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| transcribe_pending(ui),
        ))
}

/// The model that made a text, with the language chosen for its run.
fn model_label(model: Option<&str>, language: Option<&str>) -> Option<String> {
    let model = model?;
    Some(match language {
        Some(language) => format!(
            "{model} · {}",
            crate::transcription::models::language_name(language)
        ),
        None => model.to_owned(),
    })
}

/// Labs › Keep recordings: the texts Replace kept, newest first, each with
/// its model and age, Copy, and Use this version.
fn versions_section(ui: &Ui, p: Paint, entry: &HistoryEntry) -> El {
    let id = entry.id;
    let mut section =
        col()
            .key("versions")
            .gap(12.0)
            .child(section_label(p, "label", tr("Earlier versions")));
    for (index, version) in entry.versions.iter().enumerate() {
        let mut meta = model_label(version.model.as_deref(), version.language.as_deref())
            .unwrap_or_else(|| tr("Unknown model").to_owned());
        meta.push_str(" · ");
        meta.push_str(&event_age(version.timestamp_ms));
        let copied = ui.history.version_copied == Some((id, index));
        section = section.child(
            col()
                .key(format!("version{index}"))
                .gap(6.0)
                .child(
                    text(meta)
                        .key("meta")
                        .font(super::controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(
                    text(version.final_text.clone())
                        .key("text")
                        .font(p.k.ty.body)
                        .color(p.text())
                        .wrap(),
                )
                .child(
                    row()
                        .key("actions")
                        .gap(8.0)
                        .child(button(
                            p,
                            "copy",
                            if copied { tr("Copied") } else { tr("Copy") },
                            ButtonKind::Secondary,
                            move |ui: &mut Ui, tx: &mut Tx| {
                                if let Some(version) = ui
                                    .history
                                    .entry(id)
                                    .and_then(|entry| entry.versions.get(index))
                                {
                                    tx.copy(version.final_text.clone());
                                    ui.history.version_copied = Some((id, index));
                                }
                            },
                        ))
                        .child(button(
                            p,
                            "use",
                            tr("Use this version"),
                            ButtonKind::Secondary,
                            move |ui: &mut Ui, _| restore(ui, id, index),
                        )),
                ),
        );
    }
    section
}
