//! The dictation owner's projection for Paste Last. Only an output that has
//! no retained entry needs a RAM slot. Row previews never authorize a paste.
#![allow(dead_code, reason = "staged until the mixed shortcut is enabled")]

use super::{HistoryEntry, HistoryKind, HistoryStore};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Id {
    Retained(u64),
    Session(u64),
}

pub(crate) struct Row {
    pub cancelled: bool,
    pub id: Id,
    pub ready_at: u64,
    pub preview: String,
}

pub(super) struct Recovery {
    id: Id,
    ready_at: u64,
    text: String,
}

impl HistoryStore {
    pub(crate) fn recover_paste(&mut self, text: String, now: u64) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        if !text.trim().is_empty() {
            self.recovery = Some(Recovery {
                id: Id::Session(NEXT.fetch_add(1, Ordering::Relaxed)),
                ready_at: now,
                text,
            });
        }
    }

    pub(super) fn pasteable(&self, e: &HistoryEntry, now: u64) -> bool {
        !self.retention.is_off()
            && self
                .retention
                .cutoff(now)
                .is_none_or(|cut| e.timestamp_ms >= cut)
            && matches!(
                e.kind,
                HistoryKind::Dictation | HistoryKind::Raw | HistoryKind::Send
            )
            && !e.final_text.trim().is_empty()
    }

    pub(crate) fn paste_rows(&self, count: usize, now: u64) -> Vec<Row> {
        let mut rows: Vec<_> = self
            .entries()
            .filter(|e| self.pasteable(e, now))
            .map(|e| {
                (
                    e.timestamp_ms,
                    Id::Retained(e.id),
                    e.final_text.as_str(),
                    e.cancelled,
                )
            })
            .collect();
        if let Some(r) = &self.recovery
            && self
                .retention
                .cutoff(now)
                .is_none_or(|cut| r.ready_at >= cut)
        {
            rows.push((r.ready_at, r.id, &r.text, false));
        }
        rows.sort_unstable_by_key(|(at, id, ..)| std::cmp::Reverse((*at, *id)));
        rows.into_iter()
            .take(count)
            .map(|(ready_at, id, text, cancelled)| Row {
                cancelled,
                ready_at,
                id,
                preview: text.chars().take(100).collect(),
            })
            .collect()
    }

    pub(crate) fn resolve_paste(&self, id: Id, now: u64) -> Option<&str> {
        match id {
            Id::Session(_) => self
                .recovery
                .as_ref()
                .filter(|r| {
                    r.id == id
                        && self
                            .retention
                            .cutoff(now)
                            .is_none_or(|cut| r.ready_at >= cut)
                })
                .map(|r| r.text.as_str()),
            Id::Retained(id) => self
                .entries()
                .find(|e| e.id == id && self.pasteable(e, now))
                .map(|e| e.final_text.as_str()),
        }
    }
}

impl super::History {
    /// A clear (including an empty clear) or retention change invalidates
    /// a local inference result that has not yet reached the store.
    #[cfg(hex_capture)]
    pub(crate) fn cancel_generation(&self) -> Option<u64> {
        let store = self.synced();
        (!store.retention.is_off()).then_some(store.cancel_generation)
    }

    #[cfg(hex_capture)]
    pub(crate) fn keep_cancelled_words(
        &self,
        generation: u64,
        text: String,
        audio_ms: u64,
    ) -> std::io::Result<Option<u64>> {
        let text = text.trim();
        let mut store = self.synced();
        if text.is_empty()
            || generation != store.cancel_generation
            || crate::desktop::process_exit::quitting()
        {
            return Ok(None);
        }
        store.record_result(
            super::HistoryDraft {
                kind: HistoryKind::Dictation,
                raw_text: text.into(),
                final_text: text.into(),
                application: None,
                processing: None,
                audio_ms,
                inference_ms: 0,
                total_ms: 0,
                recording: None,
                model: None,
            },
            super::now_ms(),
            true,
        )
    }
    pub(crate) fn paste_rows(&self, count: usize, now: u64) -> Vec<Row> {
        self.synced().paste_rows(count, now)
    }
    pub(crate) fn resolve_paste(&self, id: Id, now: u64) -> Option<String> {
        self.synced().resolve_paste(id, now).map(str::to_owned)
    }
    /// The ordered output worker hands its only unretained text to the same
    /// owner Clear and retention use. No file read or write to recover it.
    pub(crate) fn recover_output(&self, text: String) {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .recover_paste(text, super::now_ms());
    }

    pub(crate) fn recovered_output(&self) -> Option<String> {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .resolve_paste(store.recovery.as_ref()?.id, super::now_ms())
            .map(str::to_owned)
    }
}
