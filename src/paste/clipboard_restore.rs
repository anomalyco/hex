//! Generation-guarded clipboard restoration policy shared across macOS, Linux,
//! and Windows.
//!
//! Each OS supplies its own snapshot representation `S` and clipboard mark `M`
//! (pasteboard change count on macOS, sequence number on Windows, inserted MIME
//! and bytes on Linux). The restore delay and the rules for rapid back-to-back
//! pastes, Labs › Keep dictation on clipboard, external clipboard changes, and
//! failed or superseded pastes live here once.

use std::time::Duration;

/// How long after a paste Hex waits before restoring the previous clipboard.
/// Nothing acknowledges that the target read the paste; Windows remote desktop
/// clients override this with a longer window.
pub(crate) const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(500);

/// What a scheduled restore timer should do when it wakes up.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RestoreDecision<S> {
    /// A newer paste, copy, or `keep` superseded this generation.
    Superseded,
    /// Another application changed the clipboard after Hex pasted.
    ChangedExternally,
    /// Nothing was saved to restore (for example, an unreadable format).
    NothingToRestore,
    /// Generation matches and the clipboard still holds our paste: restore `S`.
    Restore(S),
}

/// Generation-guarded state for restoring the user's clipboard after a paste.
pub(crate) struct ClipboardRestore<S, M> {
    generation: u64,
    original: Option<S>,
    inserted: Option<M>,
}

impl<S, M> Default for ClipboardRestore<S, M> {
    fn default() -> Self {
        Self {
            generation: 0,
            original: None,
            inserted: None,
        }
    }
}

impl<S, M> ClipboardRestore<S, M> {
    /// Registers a committed paste whose pre-write and post-write clipboard
    /// marks can be compared directly (macOS changeCount, Windows sequence
    /// number). When `previous_mark` matches the last paste Hex wrote, the
    /// earlier paste's original snapshot is preserved instead of adopting the
    /// transient dictation text.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub(crate) fn register(
        &mut self,
        previous: Option<S>,
        previous_mark: M,
        inserted_mark: M,
    ) -> u64
    where
        M: PartialEq,
    {
        self.generation = self.generation.wrapping_add(1);
        if self.inserted.as_ref() != Some(&previous_mark) {
            self.original = previous;
        }
        self.inserted = Some(inserted_mark);
        self.generation
    }

    /// Begins a two-phase paste where the clipboard mark is known only after
    /// writing (Linux): advances the generation, keeps the prior paste's
    /// original when the clipboard still holds that paste, and otherwise takes
    /// `original` or snapshots afresh.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn prepare(
        &mut self,
        original: Option<S>,
        still_holds: impl FnOnce(&M) -> bool,
        snapshot: impl FnOnce() -> Option<S>,
    ) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        let previous_pending = self.inserted.take().is_some_and(|mark| still_holds(&mark));
        if let Some(original) = original {
            self.original = Some(original);
        } else if !previous_pending {
            self.original = snapshot();
        }
        self.generation
    }

    /// Commits the mark of a paste prepared with [`Self::prepare`] and returns
    /// its generation for the restore timer.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn commit(&mut self, inserted_mark: M) -> u64 {
        self.inserted = Some(inserted_mark);
        self.generation
    }

    /// Drops the pending snapshot and mark after a paste fails to send, leaving
    /// whatever reached the clipboard untouched.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn clear_pending(&mut self) {
        self.inserted = None;
        self.original = None;
    }

    /// Cancels any pending paste restore and hands back its original snapshot
    /// when the clipboard still holds that paste (used by capture pastes and
    /// typed-text jobs that manage their own restore).
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(crate) fn take_pending(&mut self, still_holds: impl FnOnce(&M) -> bool) -> Option<S> {
        self.generation = self.generation.wrapping_add(1);
        let held = self.inserted.take().is_some_and(|mark| still_holds(&mark));
        if held {
            self.original.take()
        } else {
            self.original = None;
            None
        }
    }

    /// Labs › Keep dictation on clipboard (or a copy-only dictation): the paste
    /// stays on the clipboard, any earlier pending restore stands down, and no
    /// older snapshot may come back.
    pub(crate) fn keep(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.original = None;
        self.inserted = None;
    }

    /// Evaluates whether a scheduled restore for `generation` should restore
    /// the saved snapshot. Only the latest generation whose clipboard still
    /// holds our inserted mark restores.
    pub(crate) fn decide_restore(
        &mut self,
        generation: u64,
        still_holds: impl FnOnce(&M) -> bool,
    ) -> RestoreDecision<S> {
        if self.generation != generation {
            return RestoreDecision::Superseded;
        }
        let mark = self.inserted.take();
        let original = self.original.take();
        let (Some(mark), Some(original)) = (mark, original) else {
            return RestoreDecision::NothingToRestore;
        };
        if !still_holds(&mark) {
            return RestoreDecision::ChangedExternally;
        }
        RestoreDecision::Restore(original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_delay_is_half_a_second() {
        assert_eq!(CLIPBOARD_RESTORE_DELAY, Duration::from_millis(500));
    }

    #[test]
    fn rapid_pastes_preserve_the_original_clipboard_and_supersede_older_timers() {
        let mut restore = ClipboardRestore::default();
        let first = restore.register(Some("original"), 10, 11);
        let second = restore.register(Some("first paste"), 11, 12);

        assert_ne!(first, second);
        assert_eq!(
            restore.decide_restore(first, |_| panic!(
                "superseded timer must not probe clipboard"
            )),
            RestoreDecision::Superseded
        );
        assert_eq!(
            restore.decide_restore(second, |&mark| mark == 12),
            RestoreDecision::Restore("original")
        );
        // Once restored, a duplicate wake has nothing left to restore.
        assert_eq!(
            restore.decide_restore(second, |_| true),
            RestoreDecision::NothingToRestore
        );
    }

    #[test]
    fn external_clipboard_change_before_second_paste_adopts_new_snapshot() {
        let mut restore = ClipboardRestore::default();
        let first = restore.register(Some("before first"), 10, 11);
        // User copied something else (mark moved 11 -> 15) before second paste.
        let second = restore.register(Some("user copy"), 15, 16);

        assert_ne!(first, second);
        assert_eq!(
            restore.decide_restore(second, |&mark| mark == 16),
            RestoreDecision::Restore("user copy")
        );
    }

    #[test]
    fn external_clipboard_change_after_paste_skips_restore_and_clears_state() {
        let mut restore = ClipboardRestore::default();
        let generation = restore.register(Some("original"), 10, 11);

        assert_eq!(
            restore.decide_restore(generation, |&mark| mark == 12),
            RestoreDecision::ChangedExternally
        );
        assert_eq!(
            restore.decide_restore(generation, |_| true),
            RestoreDecision::NothingToRestore
        );
    }

    #[test]
    fn keep_cancels_pending_restore_and_prevents_old_original_from_returning() {
        let mut restore = ClipboardRestore::default();
        let pending = restore.register(Some("before"), 10, 12);

        restore.keep();

        assert_eq!(
            restore.decide_restore(pending, |_| true),
            RestoreDecision::Superseded
        );
        // A later restoring paste snapshots what is there now, not "before".
        let next = restore.register(Some("kept dictation"), 12, 14);
        assert_eq!(
            restore.decide_restore(next, |&mark| mark == 14),
            RestoreDecision::Restore("kept dictation")
        );
    }

    #[test]
    fn unreadable_clipboard_registers_nothing_to_restore_without_probing() {
        let mut restore = ClipboardRestore::default();
        restore.register(Some("old"), 1, 2);

        let generation = restore.register(None, 10, 11);
        assert_eq!(
            restore.decide_restore(generation, |_| panic!("no snapshot means no probe needed")),
            RestoreDecision::NothingToRestore
        );
    }

    #[test]
    fn two_phase_prepare_commit_preserves_original_across_rapid_pastes() {
        let mut restore = ClipboardRestore::default();
        let mut snapshots = 0;

        restore.prepare(
            None,
            |_| false,
            || {
                snapshots += 1;
                Some("original")
            },
        );
        let first = restore.commit("first paste");
        assert_eq!(snapshots, 1);

        // Second paste while the clipboard still holds "first paste" keeps "original"
        // without taking another snapshot.
        restore.prepare(
            None,
            |&mark| mark == "first paste",
            || {
                snapshots += 1;
                Some("unexpected snapshot")
            },
        );
        let second = restore.commit("second paste");
        assert_eq!(snapshots, 1);
        assert_ne!(first, second);
        assert_eq!(
            restore.decide_restore(first, |_| true),
            RestoreDecision::Superseded
        );
        assert_eq!(
            restore.decide_restore(second, |&mark| mark == "second paste"),
            RestoreDecision::Restore("original")
        );
    }

    #[test]
    fn two_phase_clear_pending_drops_snapshot_after_failed_paste() {
        let mut restore: ClipboardRestore<&str, &str> = ClipboardRestore::default();
        let generation = restore.prepare(Some("original"), |_| false, || None);
        restore.clear_pending();

        assert_eq!(
            restore.decide_restore(generation, |_| true),
            RestoreDecision::NothingToRestore
        );
    }

    #[test]
    fn take_pending_adopts_original_only_while_paste_is_still_held() {
        let mut restore = ClipboardRestore::default();
        let pending = restore.register(Some("before dictation"), 10, 11);

        assert_eq!(
            restore.take_pending(|&mark| mark == 11),
            Some("before dictation")
        );
        assert_eq!(
            restore.decide_restore(pending, |_| true),
            RestoreDecision::Superseded
        );
        assert_eq!(restore.take_pending(|_| true), None);

        // If another app changed the clipboard first, take_pending drops the stale original.
        let stale = restore.register(Some("stale"), 20, 21);
        assert_eq!(restore.take_pending(|&mark| mark == 99), None);
        assert_eq!(
            restore.decide_restore(stale, |_| true),
            RestoreDecision::Superseded
        );
    }
}
