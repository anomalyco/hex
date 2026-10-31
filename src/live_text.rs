//! What the dictation being recorded has said so far, for the HUDs (the
//! Labs feature live text) and the first run's practice field, which
//! [`demand`]s it while it shows and reads it with [`current`].
//!
//! The capture loop publishes through its recording's [`Publisher`], which
//! the recording owns: dropping the recording (release, Escape, a short tap,
//! a lost microphone) clears the text, so no ending can leave it on screen.
//! A HUD registers a [`watch`]er and is told each change from the capture
//! loop's thread; it must hand the text to its own thread and never wait.
//! The text is provisional: it is only ever shown, never pasted or kept.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::transcription_engine::Partial;

type Watcher = Arc<dyn Fn(Option<&Partial>) + Send + Sync>;

struct Board {
    /// The publisher whose text is current, or 0 when none is.
    owner: u64,
    /// The current publisher's latest text.
    text: Option<Partial>,
    watchers: Vec<Watcher>,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    owner: 0,
    text: None,
    watchers: Vec::new(),
});
static NEXT_PUBLISHER: AtomicU64 = AtomicU64::new(1);
static DEMANDS: AtomicUsize = AtomicUsize::new(0);

fn board() -> MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(|error| error.into_inner())
}

/// Calls `watcher` with each new text, and with `None` once the recording
/// that published it ends.
pub(crate) fn watch(watcher: impl Fn(Option<&Partial>) + Send + Sync + 'static) {
    board().watchers.push(Arc::new(watcher));
}

/// The text the recording in progress has shown, if any.
pub(crate) fn current() -> Option<Partial> {
    board().text.clone()
}

/// Asks dictation streams to publish their text while it is held, without
/// the experiment (the first run's practice field). The HUD plate still
/// shows only with the experiment.
pub(crate) struct Demand(());

pub(crate) fn demand() -> Demand {
    DEMANDS.fetch_add(1, Ordering::AcqRel);
    Demand(())
}

impl Drop for Demand {
    fn drop(&mut self) {
        DEMANDS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Whether anything holds a [`Demand`].
pub(crate) fn demanded() -> bool {
    DEMANDS.load(Ordering::Acquire) > 0
}

/// One recording's hold on the live text.
pub(crate) struct Publisher {
    id: u64,
}

impl Publisher {
    pub(crate) fn new() -> Self {
        Self {
            id: NEXT_PUBLISHER.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Shows `partial`, replacing any other recording's text.
    pub(crate) fn show(&self, partial: &Partial) {
        let watchers = {
            let mut board = board();
            board.owner = self.id;
            board.text = Some(partial.clone());
            board.watchers.clone()
        };
        for watcher in watchers {
            watcher(Some(partial));
        }
    }
}

impl Drop for Publisher {
    /// Clears the text if it is still this recording's.
    fn drop(&mut self) {
        let watchers = {
            let mut board = board();
            if board.owner != self.id {
                return;
            }
            board.owner = 0;
            board.text = None;
            board.watchers.clone()
        };
        for watcher in watchers {
            watcher(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board is process-wide; these tests take turns with it.
    static TURN: Mutex<()> = Mutex::new(());

    fn turn() -> MutexGuard<'static, ()> {
        TURN.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn said(stable: &str) -> Partial {
        Partial {
            stable: stable.into(),
            tentative: String::new(),
        }
    }

    /// Records every call a watcher gets, as the stable text or `None`.
    fn watched() -> Arc<Mutex<Vec<Option<String>>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        watch(move |partial| {
            log.lock()
                .unwrap()
                .push(partial.map(|partial| partial.stable.clone()));
        });
        seen
    }

    #[test]
    fn a_recording_shows_its_text_and_clears_it_when_it_ends() {
        let _turn = turn();
        let seen = watched();
        let recording = Publisher::new();
        recording.show(&said("so the plan"));
        recording.show(&said("so the plan for today"));
        assert_eq!(current().unwrap().stable, "so the plan for today");
        drop(recording);
        assert_eq!(current(), None);
        assert_eq!(
            *seen.lock().unwrap(),
            [
                Some("so the plan".into()),
                Some("so the plan for today".into()),
                None
            ]
        );
    }

    #[test]
    fn an_older_recording_ending_never_clears_a_newer_ones_text() {
        let _turn = turn();
        let seen = watched();
        let older = Publisher::new();
        older.show(&said("old"));
        let newer = Publisher::new();
        newer.show(&said("new"));
        drop(older);
        assert_eq!(
            *seen.lock().unwrap(),
            [Some("old".into()), Some("new".into())]
        );
        drop(newer);
        assert_eq!(seen.lock().unwrap().last(), Some(&None));
    }

    #[test]
    fn a_recording_that_never_showed_text_clears_nothing() {
        let _turn = turn();
        let seen = watched();
        drop(Publisher::new());
        assert!(seen.lock().unwrap().is_empty());
    }
}
