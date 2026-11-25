//! Narrow input ownership for a single nonactivating picker. Row identity
//! and invocation are both frozen: a late click can't act on the next list.
//! Its rows are read from the press, so the picker opens the moment the hold
//! passes, and once open it stays open until a choice, Escape, or a click
//! outside it.
use super::recent::{Entry, Id};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Default)]
pub(crate) struct View {
    pub epoch: u64,
    pub revision: u64,
    pub rows: Arc<[Entry]>,
    pub cursor: usize,
    pub open: bool,
}
#[derive(Default)]
struct State {
    view: View,
    outcome: Option<Choice>,
    armed: bool,
    /// Rows read before the hold passed, shown once it does.
    staged: Option<Vec<Entry>>,
    /// The hold passed: rows open as soon as they are read.
    due: bool,
}
pub(crate) enum Choice {
    Cancel,
    Item(Id),
    /// A capture to open in the editor. Nothing is pasted.
    Open(Id),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
        self.staged = None;
        self.due = false;
        self.view.open = false;
        self.view.rows = Arc::default();
        self.view.epoch
    }
    fn current(&self, epoch: u64) -> bool {
        self.armed && self.view.epoch == epoch && self.outcome.is_none()
    }
    fn show(&mut self, epoch: u64, rows: Vec<Entry>) {
        if !self.current(epoch) {
            return;
        }
        if self.due {
            self.open(rows);
        } else {
            self.staged = Some(rows);
        }
    }
    fn reveal(&mut self, epoch: u64) {
        if !self.current(epoch) {
            return;
        }
        self.due = true;
        if let Some(rows) = self.staged.take() {
            self.open(rows);
        }
    }
    fn open(&mut self, rows: Vec<Entry>) {
        self.view.rows = rows.into();
        self.view.cursor = self
            .view
            .rows
            .iter()
            .position(|r| !r.row.cancelled)
            .unwrap_or(self.view.rows.len());
        self.view.open = true;
    }
    fn finish(&mut self, choice: Choice) {
        if self.armed {
            self.armed = false;
            self.staged = None;
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
        self.staged = None;
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
                    .map(|r| Choice::Item(r.row.id.clone()))
                    .unwrap_or(Choice::Cancel);
                self.finish(choice);
            }
            _ => {}
        }
        true
    }
    fn position(&self, epoch: u64, id: &Id) -> Option<usize> {
        (self.view.open && self.view.epoch == epoch)
            .then(|| self.view.rows.iter().position(|r| &r.row.id == id))
            .flatten()
    }
    fn pick(&mut self, epoch: u64, id: Id) {
        if self.position(epoch, &id).is_some() {
            self.finish(Choice::Item(id));
        }
    }
    /// A click on a row: it becomes the selection, and a click on the
    /// selection pastes it, so a double click pastes any row.
    fn select(&mut self, epoch: u64, id: Id) {
        match self.position(epoch, &id) {
            Some(at) if at == self.view.cursor => self.finish(Choice::Item(id)),
            Some(at) => self.view.cursor = at,
            None => {}
        }
    }
    /// Open in Editor: a capture leaves for its editor, and nothing pastes.
    fn open_capture(&mut self, epoch: u64, id: Id) {
        if matches!(id, Id::Capture(_)) && self.position(epoch, &id).is_some() {
            self.finish(Choice::Open(id));
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
pub(crate) fn show(epoch: u64, rows: Vec<Entry>) {
    update(|s| s.show(epoch, rows));
}
/// The hold passed: open now if the rows are read, else when they are.
pub(crate) fn reveal(epoch: u64) {
    update(|s| s.reveal(epoch));
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
/// Pastes row `id`: the preview's Paste.
pub(crate) fn click(epoch: u64, id: Id) {
    update(|s| s.pick(epoch, id));
}
pub(crate) fn select(epoch: u64, id: Id) {
    update(|s| s.select(epoch, id));
}
pub(crate) fn open_capture(epoch: u64, id: Id) {
    update(|s| s.open_capture(epoch, id));
}
/// Closes the open picker without a paste: a click outside it, or Show all
/// History, which consumes this invocation before activating the window.
pub(crate) fn dismiss(epoch: u64) -> bool {
    update(|s| s.view.epoch == epoch && s.view.open && s.navigate(Nav::Cancel))
}

/// A key press the session tap sees while a picker is armed or open.
#[derive(Clone, Copy)]
pub(crate) struct Key {
    pub code: u16,
    pub repeat: bool,
    /// The whole Paste Last chord.
    pub trigger: bool,
    /// Paste Last's key, whatever modifiers are down (V once ⌥⇧ are up).
    pub trigger_key: bool,
    /// Another of Hex's shortcuts.
    pub other_shortcut: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyUse {
    /// Taken from the app: move, choose, or close.
    Nav(Nav),
    /// Taken from the app, doing nothing (a held key's repeats).
    Swallow,
    /// Left for the app or the shortcut; the picker stays.
    Pass,
    /// Left for the app; the picker closes.
    Close,
}

/// What a key does to the picker. Once open it stays open with every key up:
/// the arrows, Tab, and Paste Last's key (alone or with its modifiers) move,
/// Return chooses, Escape closes, and any other key closes and reaches the
/// app.
pub(crate) fn key_use(key: Key, open: bool) -> KeyUse {
    match key.code {
        53 => KeyUse::Nav(Nav::Cancel),
        _ if key.other_shortcut => KeyUse::Close,
        125 | 48 => KeyUse::Nav(Nav::Next),
        126 => KeyUse::Nav(Nav::Previous),
        36 | 76 => KeyUse::Nav(Nav::Commit),
        _ if open && key.trigger_key && key.repeat => KeyUse::Swallow,
        _ if open && key.trigger_key => KeyUse::Nav(Nav::Next),
        _ if key.trigger => KeyUse::Pass,
        _ => KeyUse::Close,
    }
}
pub(crate) fn take(epoch: u64) -> Option<Choice> {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    (s.view.epoch == epoch).then(|| s.outcome.take()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::paste_source;
    use crate::paste::recent::Row;
    fn row(id: u64) -> Entry {
        Row {
            cancelled: false,
            id: Id::Dictation(paste_source::Id::Retained(id)),
            preview: format!("note {id}"),
            ready_at: id,
        }
        .into()
    }
    fn capture(at: u64) -> Entry {
        Row {
            cancelled: false,
            id: Id::Capture(crate::capture_host::paste_source::Id::fixture(at)),
            preview: "over the chart".into(),
            ready_at: at,
        }
        .into()
    }

    /// A click selects and a click on the selection pastes; Open in Editor
    /// sends a capture to its editor without pasting, and does nothing for
    /// a dictation.
    #[test]
    fn a_click_selects_a_second_pastes_and_a_capture_opens_in_its_editor() {
        let mut s = State::default();
        let epoch = s.begin();
        s.reveal(epoch);
        s.show(epoch, vec![row(3), capture(2), row(1)]);
        s.select(epoch, row(1).row.id);
        assert_eq!(s.view.cursor, 2);
        assert!(s.outcome.is_none() && s.view.open);
        s.select(epoch, row(1).row.id);
        assert!(matches!(s.outcome.take(), Some(Choice::Item(id)) if id == row(1).row.id));
        assert!(!s.view.open);

        let epoch = s.begin();
        s.reveal(epoch);
        s.show(epoch, vec![row(3), capture(2)]);
        s.open_capture(epoch, row(3).row.id);
        assert!(s.outcome.is_none() && s.view.open);
        s.open_capture(epoch, capture(2).row.id);
        assert!(matches!(s.outcome.take(), Some(Choice::Open(id)) if id == capture(2).row.id));
        assert!(!s.view.open);
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
        cancelled.row.cancelled = true;
        let epoch = state.begin();
        state.reveal(epoch);
        state.show(epoch, vec![cancelled.clone(), row(2)]);
        assert_eq!(state.view.cursor, 1);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(),Some(Choice::Item(id)) if id==row(2).row.id));
        let epoch = state.begin();
        state.reveal(epoch);
        state.show(epoch, vec![cancelled.clone()]);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(), Some(Choice::Cancel)));
        let epoch = state.begin();
        state.reveal(epoch);
        state.show(epoch, vec![cancelled.clone()]);
        state.navigate(Nav::Next);
        state.navigate(Nav::Commit);
        assert!(matches!(state.outcome.take(),Some(Choice::Item(id)) if id==cancelled.row.id));
    }
    #[test]
    fn escape_and_delayed_click_never_commit_to_another_invocation() {
        let mut s = State::default();
        let one = s.begin();
        s.reveal(one);
        s.show(one, vec![row(1)]);
        s.navigate(Nav::Cancel);
        let two = s.begin();
        s.reveal(two);
        s.show(two, vec![row(2), row(1)]);
        s.pick(one, row(1).row.id);
        assert!(s.outcome.is_none());
        s.navigate(Nav::Next);
        s.navigate(Nav::Commit);
        assert!(matches!(s.outcome.take(),Some(Choice::Item(id)) if id==row(1).row.id));
        s.pick(two, row(2).row.id);
        assert!(s.outcome.is_none());
    }

    #[test]
    fn rows_read_during_the_hold_open_the_moment_it_passes() {
        let mut s = State::default();
        let epoch = s.begin();
        s.show(epoch, vec![row(1)]);
        assert!(!s.view.open);
        s.reveal(epoch);
        assert!(s.view.open);
        // A tap claims the invocation; its rows never show.
        let tap = s.begin();
        s.show(tap, vec![row(2)]);
        assert!(s.tap(tap));
        s.reveal(tap);
        assert!(!s.view.open);
    }

    /// A tap pastes; a hold opens the picker, which stays open once the
    /// keys come up and moves on Paste Last's key alone; Escape closes it.
    #[test]
    fn a_tap_pastes_and_a_hold_opens_a_picker_that_stays_until_escape() {
        use crate::app_settings::RuntimeHotkey;
        use crate::audio::CaptureInstant;
        use crate::paste::gesture::{Effect, Gesture};
        use crate::suppression::InputEvent;
        let binding = RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(9),
        };
        let at = |ms| CaptureInstant::from_nanos(ms * 1_000_000u64);
        let up = InputEvent::Key {
            code: 9,
            down: false,
            flags: 0,
        };
        let key = |code, trigger_key, repeat| Key {
            code,
            repeat,
            trigger: false,
            trigger_key,
            other_shortcut: false,
        };
        let mut hold = Gesture::default();
        let mut s = State::default();

        let tap = s.begin();
        hold.press(binding, at(0), true);
        assert_eq!(hold.edge(up, at(120)), Effect::PasteLatest);
        assert!(s.tap(tap));

        let epoch = s.begin();
        s.show(epoch, vec![row(3), row(2), row(1)]);
        hold.press(binding, at(1_000), true);
        assert_eq!(hold.tick(at(1_199)), Effect::Wait);
        assert_eq!(hold.tick(at(1_200)), Effect::Browse);
        s.reveal(epoch);
        assert_eq!(hold.edge(up, at(1_400)), Effect::Wait);
        assert!(s.view.open && s.armed);

        let open = s.view.open;
        for (k, expected) in [
            (key(9, true, false), KeyUse::Nav(Nav::Next)),
            (key(9, true, true), KeyUse::Swallow),
            (key(125, false, false), KeyUse::Nav(Nav::Next)),
            (key(126, false, false), KeyUse::Nav(Nav::Previous)),
        ] {
            let used = key_use(k, open);
            assert_eq!(used, expected);
            if let KeyUse::Nav(nav) = used {
                assert!(s.navigate(nav));
            }
        }
        assert_eq!(s.view.cursor, 1);
        assert!(s.view.open);
        assert_eq!(
            key_use(key(36, false, false), open),
            KeyUse::Nav(Nav::Commit)
        );
        assert_eq!(key_use(key(0, false, false), open), KeyUse::Close);

        assert_eq!(
            key_use(key(53, false, false), open),
            KeyUse::Nav(Nav::Cancel)
        );
        s.navigate(Nav::Cancel);
        assert!(!s.view.open);
        assert!(matches!(s.outcome.take(), Some(Choice::Cancel)));
    }
}
