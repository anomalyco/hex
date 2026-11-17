//! Narrow input ownership for a single nonactivating picker. Row identity
//! and invocation are both frozen: a late click can't act on the next list.
use super::recent::{Id, Row};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Default)]
pub(crate) struct View {
    pub epoch: u64,
    pub revision: u64,
    pub rows: Arc<[Row]>,
    pub cursor: usize,
    pub open: bool,
}
#[derive(Default)]
struct State {
    view: View,
    outcome: Option<Choice>,
    armed: bool,
}
pub(crate) enum Choice {
    Cancel,
    Item(Id),
}
#[derive(Clone, Copy)]
pub(crate) enum Nav {
    Cancel,
    Previous,
    Next,
    Commit,
}

impl State {
    fn begin(&mut self) -> u64 {
        self.view.epoch += 1;
        self.outcome = None;
        self.armed = true;
        self.view.open = false;
        self.view.rows = Arc::default();
        self.view.epoch
    }
    fn show(&mut self, epoch: u64, rows: Vec<Row>) {
        if self.armed && self.view.epoch == epoch && self.outcome.is_none() {
            self.view.rows = rows.into();
            self.view.cursor = self
                .view
                .rows
                .iter()
                .position(|r| !r.cancelled)
                .unwrap_or(self.view.rows.len());
            self.view.open = true;
        }
    }
    fn finish(&mut self, choice: Choice) {
        if self.armed {
            self.armed = false;
            self.outcome = Some(choice);
            self.view.open = false;
            self.view.rows = Arc::default();
        }
    }
    fn tap(&mut self, epoch: u64) -> bool {
        if !self.armed || self.view.epoch != epoch || self.view.open {
            return false;
        }
        self.armed = false;
        true
    }
    fn navigate(&mut self, nav: Nav) -> bool {
        if !self.armed {
            return false;
        }
        if matches!(nav, Nav::Cancel) {
            self.finish(Choice::Cancel);
            return true;
        }
        if !self.view.open {
            return false;
        }
        let n = self.view.rows.len();
        match nav {
            Nav::Cancel => unreachable!(),
            Nav::Previous if n > 0 => self.view.cursor = (self.view.cursor + n - 1) % n,
            Nav::Next if n > 0 => {
                self.view.cursor = if self.view.cursor >= n {
                    0
                } else {
                    (self.view.cursor + 1) % n
                }
            }
            Nav::Commit => {
                let choice = self
                    .view
                    .rows
                    .get(self.view.cursor)
                    .map(|r| Choice::Item(r.id.clone()))
                    .unwrap_or(Choice::Cancel);
                self.finish(choice);
            }
            _ => {}
        }
        true
    }
    fn pick(&mut self, epoch: u64, id: Id) {
        if self.view.open && self.view.epoch == epoch && self.view.rows.iter().any(|r| r.id == id) {
            self.finish(Choice::Item(id));
        }
    }
}

static STATE: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(Default::default);
static PHASE: AtomicU8 = AtomicU8::new(0);
type Watch = Box<dyn Fn(View) + Send + Sync>;
static WATCH: OnceLock<Watch> = OnceLock::new();
pub(crate) fn install(watch: Watch) {
    let _ = WATCH.set(watch);
}
fn update<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let (r, view) = {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let r = f(&mut s);
        s.view.revision += 1;
        PHASE.store(
            if s.view.open { 2 } else { u8::from(s.armed) },
            Ordering::Release,
        );
        (r, s.view.clone())
    };
    if let Some(watch) = WATCH.get() {
        watch(view);
    }
    r
}
pub(crate) fn begin() -> u64 {
    update(State::begin)
}
pub(crate) fn available() -> bool {
    WATCH.get().is_some()
}
pub(crate) fn tap(epoch: u64) -> bool {
    update(|s| s.tap(epoch))
}
pub(crate) fn show(epoch: u64, rows: Vec<Row>) {
    update(|s| s.show(epoch, rows));
}
pub(crate) fn active() -> bool {
    PHASE.load(Ordering::Acquire) != 0
}
pub(crate) fn open() -> bool {
    PHASE.load(Ordering::Acquire) == 2
}
pub(crate) fn nav(key: Nav) -> bool {
    active() && update(|s| s.navigate(key))
}
pub(crate) fn click(epoch: u64, id: Id) {
    update(|s| s.pick(epoch, id));
}
/// Opening History is navigation, never a paste. Consume this invocation
/// before activating the window so a late modifier-up cannot commit a row.
pub(crate) fn close_for_history(epoch: u64) -> bool {
    update(|s| s.view.epoch == epoch && s.view.open && s.navigate(Nav::Cancel))
}
pub(crate) fn take(epoch: u64) -> Option<Choice> {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    (s.view.epoch == epoch).then(|| s.outcome.take()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::paste_source;
    fn row(id: u64) -> Row {
        Row {
            cancelled: false,
            id: Id::Dictation(paste_source::Id::Retained(id)),
            preview: format!("note {id}"),
            ready_at: id,
        }
    }
    #[test]
    fn tap_and_cancel_claim_the_invocation_only_once() {
        let mut state = State::default();
        let old = state.begin();
        state.navigate(Nav::Cancel);
        assert!(!state.tap(old));
        let next = state.begin();
        assert!(!state.tap(old));
        assert!(state.tap(next));
        assert!(!state.navigate(Nav::Cancel));
        assert!(!state.tap(next));
    }

    #[test]
    fn cancelled_needs_an_explicit_selection_even_when_it_is_the_only_row() {
        let mut state = State::default();
        let mut cancelled = row(3);
        cancelled.cancelled = true;
        let epoch = state.begin();
        state.show(epoch, vec![cancelled.clone(), row(2)]);
        assert_eq!(state.view.cursor, 1);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(),Some(Choice::Item(id)) if id==row(2).id));
        let epoch = state.begin();
        state.show(epoch, vec![cancelled.clone()]);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(), Some(Choice::Cancel)));
        let epoch = state.begin();
        state.show(epoch, vec![cancelled.clone()]);
        state.navigate(Nav::Next);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(),Some(Choice::Item(id)) if id==cancelled.id));
    }
    #[test]
    fn escape_and_delayed_click_never_commit_to_another_invocation() {
        let mut s = State::default();
        let one = s.begin();
        s.show(one, vec![row(1)]);
        s.navigate(Nav::Cancel);
        let two = s.begin();
        s.show(two, vec![row(2), row(1)]);
        s.pick(one, row(1).id);
        assert!(s.outcome.is_none());
        s.navigate(Nav::Next);
        s.navigate(Nav::Commit);
        assert!(matches!(s.outcome.take(),Some(Choice::Item(id)) if id==row(1).id));
        s.pick(two, row(2).id);
        assert!(s.outcome.is_none());
    }
}
