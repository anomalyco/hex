//! Shared retained-history pane for the macOS, Linux, and Windows shells.
//!
//! The native roots own settings persistence and translate pane actions into
//! their existing runtime calls. This module owns the history handle, bounded
//! search snapshot, valid selection, copy/delete/clear behavior, selectable
//! detail text, and the complete GPUI list-and-detail composition.

use std::sync::Arc;

use gpui::{AnyElement, Context, Entity, FontWeight, div, prelude::*, px, rgb};

use crate::desktop_ui::{
    CRITICAL, FAINT, LINE, MUTED, PANE_LIST_WIDTH, SUCCESS, SURFACE_HOVER, SURFACE_SELECTED,
    TEXT_SOFT, compact_panel, empty_message, error_message, event_age, header_button, pane_body,
    pane_content, pane_header_with_action, section_label, tr,
};
use crate::history::{History, HistoryEntry, HistoryRetention};
use crate::text_input::TextInput;

/// Behavior and bounded data snapshot behind the desktop History pane.
pub(crate) struct HistoryPaneState {
    history: Option<History>,
    query: String,
    entries: Arc<[HistoryEntry]>,
    selected: Option<u64>,
    copied: Option<u64>,
    clear_armed: bool,
    error: Option<String>,
    detail_text: Option<(u64, Entity<TextInput>)>,
}

impl HistoryPaneState {
    pub(crate) fn new(history: Option<History>, error: Option<String>) -> Self {
        let mut state = Self {
            history,
            query: String::new(),
            entries: Arc::from([]),
            selected: None,
            copied: None,
            clear_armed: false,
            error,
            detail_text: None,
        };
        state.reload();
        state
    }

    /// Replace the live store when an existing macOS shell is reopened.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn replace_history(&mut self, history: Option<History>, error: Option<String>) {
        self.history = history;
        self.error = error;
        self.detail_text = None;
        self.reload();
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn selected_id(&self) -> Option<u64> {
        self.selected
    }

    pub(crate) fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.selected
            .and_then(|id| self.entries.iter().find(|entry| entry.id == id))
    }

    #[cfg(test)]
    pub(crate) fn clear_armed(&self) -> bool {
        self.clear_armed
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    pub(crate) fn disarm_clear(&mut self) {
        self.clear_armed = false;
    }

    pub(crate) fn select(&mut self, id: u64) {
        if self.entries.iter().any(|entry| entry.id == id) {
            self.selected = Some(id);
            self.clear_armed = false;
        }
    }

    pub(crate) fn set_query(&mut self, query: String) -> bool {
        if self.query == query {
            return false;
        }
        self.query = query;
        self.reload()
    }

    /// Refresh from the store and preserve a valid selection. List-and-detail
    /// panes select the newest matching entry when no valid selection remains.
    pub(crate) fn reload(&mut self) -> bool {
        let previous_entries = std::mem::take(&mut self.entries);
        let previous_selected = self.selected;
        let previous_copied = self.copied;
        let Some(history) = &self.history else {
            self.selected = None;
            self.copied = None;
            return !previous_entries.is_empty()
                || previous_selected.is_some()
                || previous_copied.is_some();
        };

        self.entries = history.search(&self.query).into();
        if self
            .selected
            .is_none_or(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.selected = self.entries.first().map(|entry| entry.id);
        }
        if self
            .copied
            .is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.copied = None;
        }

        self.entries != previous_entries
            || self.selected != previous_selected
            || self.copied != previous_copied
    }

    pub(crate) fn set_retention(&mut self, retention: HistoryRetention) {
        let Some(history) = &self.history else {
            return;
        };
        match history.set_retention(retention) {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error.to_string()),
        }
        self.reload();
    }

    /// Build the owned presentation snapshot and keep one selectable text
    /// entity stable while the same history entry remains selected.
    pub(crate) fn view<T: 'static>(
        &mut self,
        retention: HistoryRetention,
        search: Entity<TextInput>,
        cx: &mut Context<T>,
    ) -> HistoryPaneView {
        let selected = self
            .selected_entry()
            .map(|entry| (entry.id, entry.final_text.clone()));
        let stale = match (&self.detail_text, &selected) {
            (Some((text_id, input)), Some((entry_id, final_text))) => {
                text_id != entry_id || input.read(cx).text() != final_text
            }
            (None, None) => false,
            _ => true,
        };
        if stale {
            self.detail_text = selected.map(|(id, text)| {
                (
                    id,
                    cx.new(|cx| TextInput::read_only_multiline(cx, text, px(220.0))),
                )
            });
        }

        HistoryPaneView {
            retention,
            search,
            entries: Arc::clone(&self.entries),
            selected: self.selected,
            copied: self.copied,
            clear_armed: self.clear_armed,
            error: self.error.clone(),
            detail_text: self.detail_text.as_ref().map(|(_, input)| input.clone()),
        }
    }

    pub(crate) fn copy(&mut self, id: u64) {
        let Some(entry) = self.entries.iter().find(|entry| entry.id == id) else {
            return;
        };
        match arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(entry.final_text.clone()))
        {
            Ok(()) => {
                self.copied = Some(id);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(crate) fn delete(&mut self, id: u64) {
        if let Some(history) = &self.history {
            match history.delete(id) {
                Ok(_) => self.error = None,
                Err(error) => self.error = Some(error.to_string()),
            }
            self.reload();
        }
    }

    /// Arm on the first invocation and clear the store on the second.
    pub(crate) fn clear(&mut self) {
        if !self.clear_armed {
            self.clear_armed = true;
            return;
        }
        self.clear_armed = false;
        if let Some(history) = &self.history {
            match history.clear() {
                Ok(()) => self.error = None,
                Err(error) => self.error = Some(error.to_string()),
            }
            self.reload();
        }
    }
}

pub(crate) struct HistoryPaneView {
    retention: HistoryRetention,
    search: Entity<TextInput>,
    entries: Arc<[HistoryEntry]>,
    selected: Option<u64>,
    copied: Option<u64>,
    clear_armed: bool,
    error: Option<String>,
    detail_text: Option<Entity<TextInput>>,
}

pub(crate) enum HistoryPaneAction {
    SetRetention(HistoryRetention),
    Select(u64),
    Copy(u64),
    Delete(u64),
    Clear,
}

pub(crate) trait HistoryPaneDelegate: Sized + 'static {
    fn handle_history_action(&mut self, action: HistoryPaneAction, cx: &mut Context<Self>);
}

pub(crate) fn render_history_pane<T: HistoryPaneDelegate>(
    view: HistoryPaneView,
    cx: &mut Context<T>,
) -> AnyElement {
    let retention = view.retention;
    let next_retention = {
        let all = HistoryRetention::ALL;
        let index = all
            .iter()
            .position(|choice| *choice == retention)
            .unwrap_or(0);
        all[(index + 1) % all.len()]
    };
    let retention_control = header_button(format!("{}: {}", tr("Keep"), tr(retention.label())))
        .id("history-retention")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.handle_history_action(HistoryPaneAction::SetRetention(next_retention), cx);
        }))
        .into_any_element();
    let clear = header_button(if view.clear_armed {
        tr("Really clear all?")
    } else {
        tr("Clear all")
    })
    .id("history-clear")
    .when(view.clear_armed, |button| button.text_color(rgb(CRITICAL)))
    .on_click(cx.listener(|this, _, _, cx| {
        this.handle_history_action(HistoryPaneAction::Clear, cx);
    }))
    .into_any_element();
    let header_action = div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().w(px(220.0)).child(view.search.clone()))
        .child(retention_control)
        .child(clear)
        .into_any_element();

    let rows = view
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let id = entry.id;
            let selected = view.selected == Some(id);
            let preview = entry.final_text.replace('\n', " ");
            let mut metadata = tr(entry.kind.label()).to_string();
            if let Some(application) = &entry.application {
                metadata.push_str(" · ");
                metadata.push_str(application);
            }
            div()
                .id(("history-entry", index))
                .w_full()
                .px_4()
                .py_3()
                .flex()
                .items_start()
                .justify_between()
                .gap_4()
                .border_b_1()
                .border_color(rgb(LINE))
                .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .w_full()
                                .text_size(px(12.0))
                                .text_color(rgb(TEXT_SOFT))
                                .line_height(px(18.0))
                                .truncate()
                                .child(preview),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(FAINT))
                                .child(metadata),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(10.0))
                        .text_color(rgb(FAINT))
                        .child(event_age(entry.timestamp_ms)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.handle_history_action(HistoryPaneAction::Select(id), cx);
                }))
        })
        .collect::<Vec<_>>();

    let detail = render_history_detail(&view, cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(pane_header_with_action("History", Some(header_action)))
        .child(
            pane_body().px_8().pt_5().pb_7().child(
                pane_content()
                    .flex_row()
                    .gap_5()
                    .child(
                        compact_panel()
                            .id("history-list")
                            .w(px(PANE_LIST_WIDTH))
                            .h_full()
                            .flex_none()
                            .overflow_y_scroll()
                            .when(retention.is_off(), |list| {
                                list.child(empty_message(
                                    "History is off. New dictations are not retained.",
                                ))
                            })
                            .when(
                                !retention.is_off()
                                    && view.entries.is_empty()
                                    && view.error.is_none(),
                                |list| list.child(empty_message("No dictations retained yet.")),
                            )
                            .when_some(view.error, |list, error| {
                                list.child(error_message("History could not be loaded.", error))
                            })
                            .child(
                                div()
                                    .w(px(PANE_LIST_WIDTH - 2.0))
                                    .flex()
                                    .flex_col()
                                    .children(rows),
                            ),
                    )
                    .child(
                        compact_panel()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(detail),
                    ),
            ),
        )
        .into_any_element()
}

fn render_history_detail<T: HistoryPaneDelegate>(
    view: &HistoryPaneView,
    cx: &mut Context<T>,
) -> AnyElement {
    let Some(entry) = view
        .selected
        .and_then(|id| view.entries.iter().find(|entry| entry.id == id))
    else {
        return detail_placeholder("Select a history entry.");
    };
    let id = entry.id;
    let copied = view.copied == Some(id);
    let show_raw = entry.raw_text.trim() != entry.final_text.trim();
    let mut latency = format!(
        "{} ms audio · {} ms inference",
        entry.audio_ms, entry.inference_ms
    );
    if entry.total_ms > 0 {
        latency.push_str(&format!(" · {} ms total", entry.total_ms));
    }

    div()
        .id("history-detail")
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .overflow_y_scroll()
        .px_6()
        .py_6()
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(tr(entry.kind.label())),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(FAINT))
                                .child(event_age(entry.timestamp_ms)),
                        ),
                )
                .child(
                    div()
                        .pt_4()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            header_button(if copied { tr("Copied") } else { tr("Copy") })
                                .id("history-copy")
                                .when(copied, |button| button.text_color(rgb(SUCCESS)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.handle_history_action(HistoryPaneAction::Copy(id), cx);
                                })),
                        )
                        .child(header_button(tr("Delete")).id("history-delete").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.handle_history_action(HistoryPaneAction::Delete(id), cx);
                            }),
                        )),
                )
                .child(
                    div()
                        .mt_5()
                        .pt_5()
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .child(section_label("Final text"))
                        .when_some(view.detail_text.clone(), |detail, input| {
                            detail.child(div().pt_3().child(input))
                        }),
                )
                .when(show_raw, |detail| {
                    detail.child(
                        div()
                            .mt_5()
                            .pt_5()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .child(section_label("Raw transcript"))
                            .child(
                                div()
                                    .pt_3()
                                    .text_size(px(12.0))
                                    .line_height(px(19.0))
                                    .text_color(rgb(MUTED))
                                    .child(entry.raw_text.clone()),
                            ),
                    )
                })
                .child(
                    div()
                        .mt_5()
                        .pt_5()
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .when_some(entry.application.clone(), |detail, application| {
                            detail.child(detail_row("Application", application))
                        })
                        .when_some(entry.processing.clone(), |detail, processing| {
                            let mut summary =
                                format!("{} · {} ms", processing.profile, processing.latency_ms);
                            if let Some(fallback) = &processing.fallback {
                                summary.push_str(&format!(" · fell back: {fallback}"));
                            }
                            detail.child(detail_row("Processing", summary))
                        })
                        .child(detail_row("Latency", latency)),
                ),
        )
        .into_any_element()
}

fn detail_placeholder(message: &'static str) -> AnyElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(12.0))
        .text_color(rgb(FAINT))
        .child(tr(message))
        .into_any_element()
}

fn detail_row(label: &'static str, value: impl Into<String>) -> AnyElement {
    div()
        .py_3()
        .flex()
        .items_start()
        .gap_4()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .w(px(92.0))
                .flex_none()
                .text_size(px(11.0))
                .text_color(rgb(FAINT))
                .child(tr(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(12.0))
                .line_height(px(18.0))
                .text_color(rgb(TEXT_SOFT))
                .child(value.into()),
        )
        .into_any_element()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use crate::history::{HistoryDraft, HistoryKind, HistoryStore};

    use super::*;

    fn history(name: &str) -> History {
        let directory =
            std::env::temp_dir().join(format!("hex-history-pane-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        History::new(HistoryStore::open(
            directory.join("history.json"),
            HistoryRetention::Forever,
            0,
        ))
    }

    fn draft(text: &str) -> HistoryDraft {
        HistoryDraft {
            kind: HistoryKind::Dictation,
            raw_text: text.into(),
            final_text: text.into(),
            application: None,
            processing: None,
            audio_ms: 0,
            inference_ms: 0,
            total_ms: 0,
        }
    }

    fn record(history: &History, text: &str) -> u64 {
        history.record(draft(text)).unwrap().unwrap()
    }

    #[test]
    fn search_and_reload_keep_selection_valid() {
        let history = history("selection");
        let older = record(&history, "older alpha");
        let newer = record(&history, "newer beta");
        let mut state = HistoryPaneState::new(Some(history), None);

        assert_eq!(state.selected_id(), Some(newer));
        state.select(older);
        assert_eq!(state.selected_id(), Some(older));

        state.set_query("beta".into());
        assert_eq!(state.selected_id(), Some(newer));
        assert_eq!(state.entries().len(), 1);

        state.set_query("missing".into());
        assert_eq!(state.selected_id(), None);
        assert!(state.entries().is_empty());
    }

    #[test]
    fn delete_and_confirmed_clear_mutate_the_shared_store() {
        let history = history("delete-clear");
        let older = record(&history, "older");
        let newer = record(&history, "newer");
        let mut state = HistoryPaneState::new(Some(history.clone()), None);

        state.delete(newer);
        assert_eq!(state.selected_id(), Some(older));
        assert_eq!(history.search("").len(), 1);

        state.clear();
        assert!(state.clear_armed());
        assert_eq!(history.search("").len(), 1);
        state.disarm_clear();
        assert!(!state.clear_armed());
        assert_eq!(history.search("").len(), 1);
        state.clear();
        assert!(state.clear_armed());
        state.clear();
        assert!(!state.clear_armed());
        assert!(history.search("").is_empty());
        assert!(state.entries().is_empty());
    }

    #[test]
    fn replacing_a_missing_store_recovers_the_pane() {
        let mut state = HistoryPaneState::new(None, Some("unavailable".into()));
        let history = history("replace");
        let id = record(&history, "available");

        state.replace_history(Some(history), None);

        assert_eq!(state.selected_id(), Some(id));
        assert_eq!(state.error(), None);
    }

    #[test]
    fn retention_changes_apply_through_the_shared_handle() {
        let history = history("retention");
        record(&history, "kept");
        let mut state = HistoryPaneState::new(Some(history.clone()), None);

        state.set_retention(HistoryRetention::Off);

        assert_eq!(history.record(draft("dropped")).unwrap(), None);
        assert_eq!(state.entries().len(), 1);
        assert_eq!(state.error(), None);
    }
}
