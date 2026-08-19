//! Shared retained-history pane behavior for the macOS and Windows shells.
//!
//! The native roots still own their search widgets, visual treatment, and
//! settings persistence. This state object owns the consequential behavior:
//! the shared history handle, bounded search snapshot, valid selection, copy,
//! delete, and confirmed clear transitions.

use crate::history::{History, HistoryEntry, HistoryRetention};

/// Behavior and bounded data snapshot behind the desktop History pane.
pub(crate) struct HistoryPaneState {
    history: Option<History>,
    query: String,
    entries: Vec<HistoryEntry>,
    selected: Option<u64>,
    copied: Option<u64>,
    clear_armed: bool,
    error: Option<String>,
}

impl HistoryPaneState {
    pub(crate) fn new(history: Option<History>, error: Option<String>) -> Self {
        let mut state = Self {
            history,
            query: String::new(),
            entries: Vec::new(),
            selected: None,
            copied: None,
            clear_armed: false,
            error,
        };
        state.reload();
        state
    }

    /// Replace the live store when an existing macOS shell is reopened.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn replace_history(&mut self, history: Option<History>, error: Option<String>) {
        self.history = history;
        self.error = error;
        self.reload();
    }

    pub(crate) fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub(crate) fn selected_id(&self) -> Option<u64> {
        self.selected
    }

    pub(crate) fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.selected
            .and_then(|id| self.entries.iter().find(|entry| entry.id == id))
    }

    pub(crate) fn copied_id(&self) -> Option<u64> {
        self.copied
    }

    pub(crate) fn clear_armed(&self) -> bool {
        self.clear_armed
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(target_os = "windows")]
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

        self.entries = history.search(&self.query);
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
