//! The History pane: retained dictations (newest first) with search, the
//! retention choice, two-step Clear all, and a detail view with Copy and
//! Delete. It shares the host's store, which the listener records into.

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::{Select, SelectAppearance, TextInput};

use super::controls::{ButtonKind, DESCRIPTION, METRICS, Paint, button};
use super::lists::{message, placeholder, section_label, source_item, source_list, split};
use super::{Menu, Ui};
use crate::desktop_i18n::{event_age, tr, tr_fill, tr_named};
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention};

#[derive(Default)]
pub(super) struct HistoryPane {
    store: Option<History>,
    pub(super) entries: Vec<HistoryEntry>,
    search: TextEdit,
    /// The query the entries were loaded for.
    searched: String,
    pub(super) selected: Option<u64>,
    pub(super) clear_armed: bool,
    pub(super) copied: Option<u64>,
    error: Option<String>,
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

    /// Refreshes the bounded snapshot for the current query.
    pub(super) fn reload(&mut self) {
        self.searched = self.search.text.clone();
        let Some(store) = &self.store else {
            return;
        };
        self.entries = store.search(&self.searched);
        if self
            .selected
            .is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.selected = None;
        }
    }

    /// While the pane shows, picks up new dictations and search edits.
    pub(super) fn poll(&mut self, visible: bool) -> bool {
        if !visible || self.store.is_none() {
            return false;
        }
        let previous = std::mem::take(&mut self.entries);
        self.reload();
        self.entries != previous
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
        rewritten.processing = Some(crate::events::DictationProcessing {
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
                    HistoryKind::VoiceAction,
                    "make this a polite reply saying I'll review it today",
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

/// The header actions (search, retention, Clear all) and the list + detail body.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let pane = &ui.history;
    let search = TextInput::new(&pane.search)
        .placeholder(tr("Search history"))
        .width(Some(220.0))
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.history.search)
        .build(p.k)
        .key("history-search");
    let labels: Vec<&str> = HistoryRetention::ALL
        .iter()
        .map(|r| tr(r.label()))
        .collect();
    let current = tr_fill("Keep: {}", tr(ui.settings.history_retention.label()));
    let retention = Select::new("history-retention", &labels)
        .appearance(SelectAppearance::Field)
        .width(160.0)
        .current(Some(&current))
        .open(ui.open == Some(Menu::HistoryRetention))
        .on_toggle(|ui: &mut Ui| {
            ui.open = (ui.open != Some(Menu::HistoryRetention)).then_some(Menu::HistoryRetention);
        })
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(|ui: &mut Ui, choice: &str, _: &mut Tx| {
            if let Some(retention) = HistoryRetention::ALL
                .into_iter()
                .find(|r| tr(r.label()) == choice)
            {
                set_retention(ui, retention);
            }
        })
        .build(p.k, layer);
    let clear_button = if pane.clear_armed {
        button(
            p,
            "history-clear",
            tr("Really clear all?"),
            ButtonKind::Danger,
            |ui: &mut Ui, _| clear(ui),
        )
    } else {
        button(
            p,
            "history-clear",
            tr("Clear all"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| clear(ui),
        )
    };
    let actions = vec![search, retention, clear_button];

    let c = &p.k.c;
    let mut list = col().key("history-list");
    let empty = if ui.settings.history_retention.is_off() {
        Some(tr("History is off. New dictations are not retained."))
    } else if pane.entries.is_empty() && pane.error.is_none() {
        Some(if pane.searched.trim().is_empty() {
            tr("No dictations retained yet.")
        } else {
            tr("No dictations match.")
        })
    } else {
        None
    };
    if let Some(error) = &pane.error {
        list = list.child(message(
            p,
            "history-error",
            tr("History could not be loaded."),
            Some(error),
        ));
    }
    if let Some(empty) = empty {
        list = list.child(message(p, "history-empty", empty, None));
    }
    for entry in &pane.entries {
        let id = entry.id;
        let selected = pane.selected == Some(id);
        let mut meta = tr(entry.kind.label()).to_string();
        if let Some(application) = &entry.application {
            meta.push_str(" · ");
            meta.push_str(application);
        }
        let preview = entry.final_text.replace('\n', " ");
        let mut item = col()
            .key(format!("history-entry{id}"))
            .role("button")
            .label(preview.clone())
            .px(10.0)
            .py(8.0)
            .gap(2.0)
            .radius(p.k.radius.md)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| {
                ui.history.selected = Some(id);
                ui.history.clear_armed = false;
            })
            .child(
                row()
                    .key("top")
                    .gap(8.0)
                    .child(
                        text(preview)
                            .key("preview")
                            .font(p.k.ty.ui_medium)
                            .color(p.text())
                            .grow(1.0)
                            .truncate(),
                    )
                    .child(
                        text(event_age(entry.timestamp_ms))
                            .key("age")
                            .font(p.k.ty.small)
                            .color(p.faint()),
                    ),
            )
            .child(
                text(meta)
                    .key("meta")
                    .font(p.k.ty.small)
                    .color(p.muted())
                    .truncate(),
            );
        if selected {
            item = item.fill(c.layer_02);
        }
        list = list.child(source_item(p, item, selected));
    }
    let body = split(
        p,
        "history-body",
        super::scroll_body("history-scroll", source_list(list)),
        detail(ui, p),
    );
    (actions, body)
}

fn detail(ui: &Ui, p: Paint) -> El {
    let pane = &ui.history;
    let Some(entry) = pane.selected.and_then(|id| pane.entry(id)) else {
        return placeholder(p, "history-detail", tr("Select a history entry."));
    };
    let id = entry.id;
    let copied = pane.copied == Some(id);
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
    let header = row()
        .key("detail-header")
        .gap(8.0)
        .center_y()
        .child(
            col()
                .key("title")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(tr(entry.kind.label()))
                        .key("kind")
                        .font(p.k.ty.body_medium)
                        .color(p.text()),
                )
                .child(
                    text(event_age(entry.timestamp_ms))
                        .key("age")
                        .font(p.k.ty.small)
                        .color(p.muted()),
                ),
        )
        .child(button(
            p,
            "history-copy",
            if copied { tr("Copied") } else { tr("Copy") },
            ButtonKind::Secondary,
            move |ui: &mut Ui, tx: &mut Tx| {
                if let Some(entry) = ui.history.entry(id) {
                    tx.copy(entry.final_text.clone());
                    ui.history.copied = Some(id);
                    ui.history.error = None;
                }
            },
        ))
        .child(button(
            p,
            "history-delete",
            tr("Delete"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| delete(ui, id),
        ));
    let mut body = col().key("detail-body").gap(20.0).pt(16.0).child(block(
        p,
        "final",
        tr("Final text"),
        &entry.final_text,
        p.text(),
    ));
    if entry.raw_text.trim() != entry.final_text.trim() {
        body = body.child(block(
            p,
            "raw",
            tr("Raw transcript"),
            &entry.raw_text,
            p.muted(),
        ));
    }
    let mut facts = col().key("facts").gap(8.0);
    if let Some(application) = &entry.application {
        facts = facts.child(fact(
            p,
            "application",
            tr("Application"),
            application.clone(),
        ));
    }
    if let Some(processing) = &entry.processing {
        let mut summary = format!("{} · {} ms", processing.profile, processing.latency_ms);
        if let Some(fallback) = &processing.fallback {
            summary.push_str(&format!(" · {}", tr_fill("fell back: {}", fallback)));
        }
        facts = facts.child(fact(p, "processing", tr("Processing"), summary));
    }
    facts = facts.child(fact(p, "latency", tr("Latency"), latency));
    body = body.child(facts);
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .px(METRICS.gutter)
        .child(header)
        .child(super::scroll_body("detail-scroll", body))
}

fn block(p: Paint, key: &str, label: &str, value: &str, color: rune::Color) -> El {
    col()
        .key(key)
        .gap(6.0)
        .child(section_label(p, "label", label))
        .child(
            text(value)
                .key("value")
                .font(p.k.ty.body)
                .color(color)
                .wrap(),
        )
}

fn fact(p: Paint, key: &str, label: &str, value: String) -> El {
    row()
        .key(key)
        .gap(16.0)
        .child(
            text(label)
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted())
                .w(92.0),
        )
        .child(
            text(value)
                .key("value")
                .font(DESCRIPTION)
                .color(p.text())
                .wrap()
                .grow(1.0)
                .shrink(),
        )
}
