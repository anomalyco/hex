//! The History pane: retained dictations (newest first) with search, one
//! menu for retention, Keep recordings, and two-step Clear all history, and
//! a detail view with Copy and Delete. It shares the host's store, which the listener records into.
//! With Labs › Keep recordings on, an entry that kept its audio can Play it
//! through the output device and offers Transcribe again with any installed
//! model, run on a worker; Replace keeps the earlier transcripts as versions
//! to copy or switch back to. Every entry names the model that made its text,
//! when it is known.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rune::{El, TextEdit, Tx, col, el, row, text};
use rune_kit::{Menu as KitMenu, MenuItem, Placement, Select, SelectAppearance, TextInput};

use super::controls::{ButtonKind, Paint, button, modal_scrim_transition, modal_transition};
use super::list_detail::{self as ld, Item};
use super::lists::{Tone, error_panel, section_label, small_button};
use super::{Menu, Ui};
use crate::desktop::i18n::{event_age, tr, tr_fill, tr_named};
use crate::history::playback::Playback;
use crate::history::stats::{AppCount, HistoryStats};
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention, Transcript};
use crate::transcription::models::TranscriptionModelId;

mod selection;

/// The stats dialog's width and padding.
const STATS_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;

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
    /// The shown entry whose Delete… was pressed, waiting for its
    /// confirmation.
    pub(super) delete_armed: Option<u64>,
    pub(super) copied: Option<u64>,
    error: Option<String>,
    /// Labs › Dictation stats: every retained entry summed up, while on.
    pub(super) stats: Option<HistoryStats>,
    /// The stats dialog is open.
    pub(super) stats_open: bool,
    /// Labs › Select multiple dictations: every selected entry, and the
    /// Delete confirmation while the card shows one.
    pub(super) selection: selection::Selection,
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
    /// Labs › Keep recordings: the entry playing through the output, if one
    /// started from the card.
    pub(super) playback: Option<Playback>,
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
    /// A reload, as entering the pane does, stops any Delete… confirmation.
    pub(super) fn reload(&mut self) {
        self.delete_armed = None;
        self.reload_entries();
        self.reload_stats();
    }

    fn reload_entries(&mut self) {
        let requery = self.searched != self.search.text;
        self.searched = self.search.text.clone();
        if requery {
            self.delete_armed = None;
        }
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
        // A search, a delete, or new dictations leave no selected id behind.
        selection::prune(self);
        if self
            .delete_armed
            .is_some_and(|id| self.selected != Some(id))
        {
            self.delete_armed = None;
        }
    }

    /// Escape: Delete… stops asking. Returns whether it was asking.
    pub(super) fn cancel_delete(&mut self) -> bool {
        self.delete_armed.take().is_some()
    }

    /// Delete… asks for `id`, and stops any other confirmation.
    pub(super) fn arm_delete(&mut self, id: u64) {
        self.clear_armed = false;
        self.delete_armed = Some(id);
    }

    /// Sums up every retained entry, only while Labs › Dictation stats is
    /// on; off, nothing is read for it.
    fn reload_stats(&mut self) {
        self.stats = match &self.store {
            Some(store) if crate::labs::history_stats() => Some(store.stats()),
            _ => None,
        };
        if self
            .stats
            .as_ref()
            .is_none_or(|stats| stats.dictations == 0)
        {
            self.stats_open = false;
        }
    }

    /// While the pane shows, picks up new dictations and search edits. It
    /// searches again only when the query or the store's revision changed,
    /// and sums the stats again when the entries or the toggle changed, or
    /// once when the dialog closes because the stats emptied.
    pub(super) fn poll(&mut self, visible: bool, keep_audio: bool, multi: bool) -> bool {
        let answered = self.poll_again();
        // Playback's answer returns the card to Play; it is not a reason to
        // read the stats again.
        let played = self.poll_playback();
        let mut changed = false;
        // Labs › Select multiple dictations off, History keeps one selected
        // entry: the others fall away.
        if !multi && selection::collapse(self) {
            changed = true;
        }
        // A playback belongs to the card that shows its Stop: leaving the
        // entry or History, turning the lab off, or hiding the window
        // quietly ends it.
        let left = self.playback.as_ref().is_some_and(|playback| {
            playback.playing() && (!visible || !keep_audio || self.selected != Some(playback.id))
        });
        if left {
            if let Some(playback) = &mut self.playback {
                playback.stop();
            }
            changed = true;
        }
        // The dialog belongs to the pane and its switch: leaving History,
        // turning Dictation stats off, or emptying the stats closes it.
        if self.stats_open
            && (!visible
                || !crate::labs::history_stats()
                || self
                    .stats
                    .as_ref()
                    .is_none_or(|stats| stats.dictations == 0))
        {
            self.stats_open = false;
            changed = true;
        }
        let Some(store) = self.store.as_ref().filter(|_| visible) else {
            return answered || played || changed;
        };
        let stale = self.search.text != self.searched || self.revision != Some(store.revision());
        if stale {
            let previous = std::mem::take(&mut self.entries);
            self.reload_entries();
            changed |= self.entries != previous;
        }
        if changed || self.stats.is_some() != crate::labs::history_stats() {
            self.reload_stats();
            return true;
        }
        answered || played || changed
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

    /// Takes the playback's answer once it ends or fails.
    fn poll_playback(&mut self) -> bool {
        self.playback
            .as_mut()
            .is_some_and(|playback| playback.poll())
    }

    fn entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Whether row `id` draws as selected: the one entry, or any of the
    /// several Labs › Select multiple dictations holds.
    pub(super) fn is_selected(&self, id: u64) -> bool {
        self.selected == Some(id) || self.selection.contains(id)
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

    /// Labs › Select multiple dictations: two entries selected, for the
    /// preview's card.
    pub(super) fn fixture_selection() -> Self {
        let mut pane = Self::fixture();
        pane.selected = Some(2);
        pane.selection.select(&[3, 2]);
        pane
    }

    /// A representative summary for the stats dialog's snapshots and tests.
    pub(super) fn fixture_stats() -> HistoryStats {
        HistoryStats {
            dictations: 128,
            words: 1_482,
            audio_ms: 720_000,
            words_per_minute: Some(123),
            changed_by_output: 34,
            streak_days: 9,
            active_days: 21,
            top_apps: vec![
                AppCount {
                    application: "Slack".into(),
                    dictations: 41,
                },
                AppCount {
                    application: "WezTerm".into(),
                    dictations: 33,
                },
                AppCount {
                    application: "Firefox".into(),
                    dictations: 27,
                },
            ],
        }
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
    ui.history.delete_armed = None;
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
        ui.history.selection.clear();
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

/// Labs › Keep recordings: starts entry `id`'s saved audio, or stops it
/// while it plays.
fn toggle_playback(ui: &mut Ui, id: u64) {
    if let Some(playback) = &mut ui.history.playback
        && playback.is_playing(id)
    {
        playback.stop();
        return;
    }
    let Some(path) = ui
        .history
        .store
        .as_ref()
        .and_then(|store| store.recording_path(id))
    else {
        ui.history.playback = Some(Playback::failed(id, "The recording is gone."));
        return;
    };
    let waker = ui.modes.waker.clone();
    ui.history.playback = Some(Playback::start(id, &path, move || {
        if let Some(waker) = &waker {
            waker();
        }
    }));
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
    ui.history.delete_armed = None;
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.delete(id) {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else {
        ui.history.entries.retain(|entry| entry.id != id);
        ui.history.selected = None;
        ui.history.selection.clear();
    }
}

/// Labs › Select multiple dictations, as the pane reads it while drawing,
/// polling, and taking a press.
pub(super) fn multi(ui: &Ui) -> bool {
    selection::on(ui)
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
            Tone::Danger,
        ));
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
                let selected = pane.is_selected(id);
                let primary = pane.selected == Some(id);
                list = list.child(
                    ld::item_with_press(
                        p,
                        Item {
                            key: format!("history-entry{id}"),
                            title: entry.final_text.replace('\n', " "),
                            meta,
                            meta_ink: None,
                            time: event_age(entry.timestamp_ms),
                            hover: vec![copy_action(ui, p, id, true)],
                            selected,
                        },
                        move |row| selection::gesture(row, id, primary),
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
    if let Some(modal) = stats_modal(ui, p) {
        layer.extend(modal);
    }
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

/// Labs › Dictation stats: every retained dictation summed up, in the
/// dialog the header's Stats button opens.
fn stats_lines(p: Paint, stats: &HistoryStats) -> Vec<El> {
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
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            text(line)
                .key(format!("line{index}"))
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap()
        })
        .collect()
}

/// The stats dialog's scrim and panel, while its button and stats show.
fn stats_modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.history.stats_open || !ui.settings.labs.history_stats {
        return None;
    }
    let stats = ui
        .history
        .stats
        .as_ref()
        .filter(|stats| stats.dictations > 0)?;
    let c = &p.k.c;
    let mut panel = col()
        .key("history-stats-dialog")
        .role("dialog")
        .label(tr("Dictation stats"))
        .w(STATS_WIDTH)
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
            text(tr("Dictation stats"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .children(stats_lines(p, stats))
        .child(
            row().key("footer").justify_end().child(
                button(
                    p,
                    "history-stats-close",
                    tr("Close"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| close_stats(ui),
                )
                .default_button(),
            ),
        );
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
        layer("history-stats-scrim")
            .fill(c.scrim)
            .on_press(close_stats)
            .transition(modal_scrim_transition(p)),
        layer("history-stats-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the Stats button that opened it.
            .child(panel.modal(true, close_stats)),
    ])
}

/// Closes the stats dialog, from its button, Escape, or the scrim.
pub(super) fn close_stats(ui: &mut Ui) {
    ui.history.stats_open = false;
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
    let menu = menu.build(p.k, layer);
    if ui.settings.labs.history_stats
        && ui
            .history
            .stats
            .as_ref()
            .is_some_and(|stats| stats.dictations > 0)
    {
        return vec![
            row()
                .key("history-actions")
                .gap(8.0)
                .child(menu)
                .child(button(
                    p,
                    "history-stats-open",
                    tr("Stats"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| {
                        ui.open = None;
                        ui.history.stats_open = true;
                    },
                )),
        ];
    }
    vec![menu]
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
    if multi(ui) && pane.selection.len() >= 2 {
        return selection::card(ui, p);
    }
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
    let armed = pane.delete_armed == Some(id);
    let mut actions = Vec::new();
    let confirm = armed.then(|| {
        row()
            .key("delete-confirm")
            .gap(8.0)
            .pt(14.0)
            .child(button(
                p,
                "history-confirm-delete",
                tr("Confirm delete"),
                ButtonKind::Danger,
                move |ui: &mut Ui, _| delete(ui, id),
            ))
            .child(button(
                p,
                "history-cancel-delete",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.history.delete_armed = None,
            ))
    });
    if !armed {
        actions.push(copy_action(ui, p, id, false));
        actions.push(ld::ghost_icon(
            p,
            "history-delete",
            ld::TRASH_ICON,
            tr("Delete…"),
            move |ui: &mut Ui, _| ui.history.arm_delete(id),
        ));
    }
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, tr(entry.kind.label())),
            vec![ld::meta_text(p, &event_age(entry.timestamp_ms))],
            actions,
        ))
        .children(confirm)
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
        .child(
            row()
                .key("again-head")
                .gap(8.0)
                .center_y()
                .child(section_label(p, "label", tr("Saved recording")).grow(1.0))
                .child(small_button(
                    p,
                    "again-play",
                    if ui
                        .history
                        .playback
                        .as_ref()
                        .is_some_and(|playback| playback.is_playing(id))
                    {
                        tr("Stop")
                    } else {
                        tr("Play")
                    },
                    move |ui: &mut Ui, _| toggle_playback(ui, id),
                )),
        )
        .child(menu);
    if let Some(error) = ui
        .history
        .playback
        .as_ref()
        .and_then(|playback| playback.error(id))
    {
        section = section.child(
            text(tr_fill("Could not play: {}", tr(error)))
                .key("again-play-error")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    }
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
