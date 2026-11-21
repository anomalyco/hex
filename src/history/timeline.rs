//! A projection, not a payload store. The archive has more identities than
//! the paste command can accept. A library path is deliberately not a receipt.
use super::{History, HistoryEntry, paste_source as words};
use crate::capture_host::paste_source::{self as scrawls, Scope};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum PasteId {
    Capture(scrawls::Id),
    Dictation(words::Id),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Owner {
    File(PathBuf),
    Words(u64),
    Paste(PasteId),
}

#[derive(Clone)]
pub(crate) struct Row<I = PasteId> {
    pub ready_at: u64,
    pub id: I,
    pub cancelled: bool,
    pub preview: String,
}
impl Row<Owner> {
    pub fn words(e: &HistoryEntry, pasteable: bool) -> Self {
        Self {
            ready_at: e.timestamp_ms,
            id: if pasteable {
                Owner::Paste(PasteId::Dictation(words::Id::Retained(e.id)))
            } else {
                Owner::Words(e.id)
            },
            cancelled: e.cancelled,
            preview: e.final_text.chars().take(100).collect(),
        }
    }
    pub fn file(at: u64, path: &Path, receipt: Option<&scrawls::Id>) -> Self {
        Self {
            ready_at: at,
            id: receipt
                .map(|id| Owner::Paste(PasteId::Capture(id.clone())))
                .unwrap_or_else(|| Owner::File(path.into())),
            cancelled: false,
            preview: String::new(),
        }
    }
    fn into_paste(self) -> Option<Row> {
        let Owner::Paste(id) = self.id else {
            return None;
        };
        Some(Row {
            id,
            ready_at: self.ready_at,
            cancelled: self.cancelled,
            preview: self.preview,
        })
    }
}
impl<I: Ord> Row<I> {
    pub fn order(&self, other: &Self) -> std::cmp::Ordering {
        other
            .ready_at
            .cmp(&self.ready_at)
            .then_with(|| other.id.cmp(&self.id))
    }
}

pub fn marked(scope: &Scope<'_>, limit: usize, now: u64) -> impl Iterator<Item = Row<Owner>> {
    scope
        .rows(limit, now)
        .into_iter()
        .map(|(id, ready_at, preview)| Row {
            id: Owner::Paste(PasteId::Capture(id)),
            ready_at,
            preview,
            cancelled: false,
        })
}

/// An automatic default always fits, even after eight consecutive cancels.
/// It stays at its chronological position (last in that case).
pub(crate) fn snapshot(
    history: &History,
    scope: Option<&Scope<'_>>,
    limit: usize,
    now: u64,
) -> Vec<Row> {
    if limit == 0 {
        return Vec::new();
    }
    let mut rows = history
        .paste_rows(usize::MAX, now)
        .into_iter()
        .map(|r| Row {
            id: Owner::Paste(PasteId::Dictation(r.id)),
            ready_at: r.ready_at,
            cancelled: r.cancelled,
            preview: r.preview,
        })
        .collect::<Vec<_>>();
    if let Some(scope) = scope {
        rows.extend(marked(scope, limit, now));
    }
    rows.sort_unstable_by(Row::order);
    if let Some(i) = rows.iter().position(|r| !r.cancelled)
        && i >= limit
    {
        rows.swap(i, limit - 1);
    }
    rows.into_iter()
        .take(limit)
        .filter_map(Row::into_paste)
        .collect()
}
pub(crate) fn latest(history: &History, scope: Option<&Scope<'_>>, now: u64) -> Option<PasteId> {
    snapshot(history, scope, 1, now)
        .pop()
        .filter(|r| !r.cancelled)
        .map(|r| r.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryRetention, HistoryStore, now_ms};
    #[test]
    fn recovery_is_text_only_and_clear_or_retention_changes_reject_late_results() {
        let root = std::env::temp_dir().join(format!("hex-recovery-fence-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let history = History::new(HistoryStore::open(
            root.join("history.json"),
            HistoryRetention::Day,
            now_ms(),
        ));
        let gen0 = history.cancel_generation().unwrap();
        history.clear().unwrap();
        assert!(
            history
                .keep_cancelled_words(gen0, "cleared".into(), 7)
                .unwrap()
                .is_none()
        );
        let epoch = history.cancel_generation().unwrap();
        assert!(
            history
                .keep_cancelled_words(epoch, "  ".into(), 7)
                .unwrap()
                .is_none()
        );
        let id = history
            .keep_cancelled_words(epoch, "local words".into(), 1000)
            .unwrap()
            .unwrap();
        let key = words::Id::Retained(id);
        assert_eq!(
            history.resolve_paste(key, now_ms()).as_deref(),
            Some("local words")
        );
        assert!(latest(&history, None, now_ms()).is_none());
        assert!(history.resolve_paste(key, now_ms() + 86_400_001).is_none());
        assert!(history.saved_recording(id).is_none());
        history.set_retention(HistoryRetention::Off).unwrap();
        assert!(history.cancel_generation().is_none());
        assert!(history.resolve_paste(key, now_ms()).is_none());
        history.set_retention(HistoryRetention::Day).unwrap();
        assert!(
            history
                .keep_cancelled_words(epoch, "too late".into(), 2)
                .unwrap()
                .is_none()
        );
        history.clear().unwrap();
        assert!(history.resolve_paste(key, now_ms()).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
