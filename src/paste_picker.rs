//! Labs › Paste picker (`labs.paste_picker`, macOS): tap Paste last dictation
//! to paste the latest dictation at once; hold it (`HOLD_THRESHOLD`) to open a
//! non-activating list of the last [`MAX_ITEMS`] pasted dictations at the text
//! caret (falling back to the pointer).
//!
//! While open:
//! - Repeated presses of the shortcut's key (or `V`) cycle through recent
//!   dictations, and `↑` / `↓` step up and down.
//! - Holding `⇧` with `↑` / `↓` (or `V` / clicking a row), or pressing `Space`,
//!   picks multiple dictations in selection order. When the shortcut itself
//!   includes `⇧` (such as the default `⌥⇧V`), keeping the non-Shift modifier
//!   (`⌥`) held keeps the picker open when `⇧` lifts, and `⇧` begins
//!   multi-selecting once released and pressed again.
//! - Releasing the held modifier(s), pressing `Return`, or clicking a row
//!   pastes at the original focus; `Escape` or clicking outside dismisses
//!   without pasting.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::app_settings::{HotkeyModifiers, RuntimeHotkey, SHIFT_KEY_MASK, modifiers_from_flags};
use crate::audio::CaptureInstant;

/// Maximum number of recent pasted dictations shown in the picker.
pub const MAX_ITEMS: usize = 8;

/// Hold duration before Paste last dictation opens the picker instead of
/// pasting on release. Matches dictation's 300 ms intentional-hold threshold.
pub const HOLD_THRESHOLD: Duration = Duration::from_millis(300);

const ESCAPE_KEY_CODE: u16 = 53;
const RETURN_KEY_CODE: u16 = 36;
const KEYPAD_ENTER_KEY_CODE: u16 = 76;
const SPACE_KEY_CODE: u16 = 49;
const DELETE_KEY_CODE: u16 = 51;
const UP_ARROW_KEY_CODE: u16 = 126;
const DOWN_ARROW_KEY_CODE: u16 = 125;

/// One recent pasted dictation offered in the picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickerItem {
    pub id: u64,
    pub text: String,
    pub timestamp_ms: u64,
}

/// Global Quartz bounds (points from the primary display's top-left, y down)
/// of the text caret, or a zero-width point at the mouse cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Immutable projection of the picker rendered by `rune_ui::paste_picker`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerSnapshot {
    pub open: bool,
    pub items: Vec<PickerItem>,
    pub cursor: usize,
    /// Indices into `items` in the exact order they will be joined and pasted.
    /// Empty means single-selection at `cursor`.
    pub selected: Vec<usize>,
    pub now_ms: u64,
    pub anchor: Option<Anchor>,
}

impl PickerSnapshot {
    /// 1-based paste position when multi-selecting (`selected` is non-empty).
    pub fn selection_order(&self, index: usize) -> Option<usize> {
        (!self.selected.is_empty())
            .then(|| self.selected.iter().position(|&picked| picked == index))
            .flatten()
            .map(|pos| pos + 1)
    }

    /// Whether `index` will be pasted on commit.
    pub fn is_chosen(&self, index: usize) -> bool {
        if self.selected.is_empty() {
            index < self.items.len() && self.cursor == index
        } else {
            self.selected.contains(&index)
        }
    }

    /// Texts to paste on commit, in paste order.
    pub fn chosen_texts(&self) -> Vec<&str> {
        if self.items.is_empty() {
            return Vec::new();
        }
        if self.selected.is_empty() {
            self.items
                .get(self.cursor)
                .map(|item| vec![item.text.as_str()])
                .unwrap_or_default()
        } else {
            self.selected
                .iter()
                .filter_map(|&idx| self.items.get(idx).map(|item| item.text.as_str()))
                .collect()
        }
    }
}

/// What the session event tap should do with a key press while the picker is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerKeyDisposition {
    /// Picker is closed; pass the event on normally.
    Pass,
    /// Picker handled (or suppressed) the key; swallow it before the app and
    /// before the listener's hotkey state machines see it.
    Swallow,
}

/// Outcome observed by the listener while stepping a Paste last press/hold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HoldOutcome {
    None,
    /// Released before `HOLD_THRESHOLD`: run normal Paste last immediately.
    TapPaste,
    /// Hold reached `HOLD_THRESHOLD`: open the picker with recent history.
    OpenPicker {
        binding: RuntimeHotkey,
    },
    /// Picker committed one or more items (already joined on macOS).
    Commit(Vec<String>),
}

#[derive(Clone, Debug)]
struct OpenPicker {
    items: Vec<PickerItem>,
    cursor: usize,
    selected: Vec<usize>,
    now_ms: u64,
    anchor: Anchor,
    trigger_key_code: Option<u16>,
    v_key_code: Option<u16>,
    hold_modifiers: HotkeyModifiers,
    initial_trigger_held: bool,
    initial_shift_held: bool,
}

impl OpenPicker {
    fn new(
        binding: RuntimeHotkey,
        v_key_code: Option<u16>,
        items: Vec<PickerItem>,
        now_ms: u64,
        anchor: Anchor,
    ) -> Self {
        Self {
            items,
            cursor: 0,
            selected: Vec::new(),
            now_ms,
            anchor,
            trigger_key_code: binding.key_code,
            v_key_code,
            hold_modifiers: hold_modifiers_for(binding.modifiers),
            initial_trigger_held: binding.key_code.is_some(),
            initial_shift_held: binding.modifiers.shift.is_some(),
        }
    }

    fn snapshot(&self) -> PickerSnapshot {
        PickerSnapshot {
            open: true,
            items: self.items.clone(),
            cursor: self.cursor,
            selected: self.selected.clone(),
            now_ms: self.now_ms,
            anchor: Some(self.anchor),
        }
    }

    fn shift_active(&self, flags: u64) -> bool {
        (flags & SHIFT_KEY_MASK != 0) && !self.initial_shift_held
    }

    fn is_cycle_key(&self, code: u16) -> bool {
        self.trigger_key_code == Some(code) || self.v_key_code == Some(code)
    }

    fn step_arrow(&mut self, delta: isize, shift: bool) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let max = self.items.len() as isize - 1;
        let next = (self.cursor as isize + delta).clamp(0, max) as usize;
        if next == self.cursor {
            if shift && !self.selected.contains(&self.cursor) {
                self.selected.push(self.cursor);
                return true;
            }
            return false;
        }
        if shift {
            self.extend_shift(self.cursor, next);
        }
        self.cursor = next;
        true
    }

    fn step_cycle(&mut self, shift: bool) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let next = (self.cursor + 1) % self.items.len();
        if next == self.cursor {
            return false;
        }
        if shift {
            self.extend_shift(self.cursor, next);
        }
        self.cursor = next;
        true
    }

    fn extend_shift(&mut self, from: usize, to: usize) {
        // Reversing back onto the previous item in `selected` pops the tip,
        // matching standard list Shift-selection.
        if self.selected.len() >= 2
            && self.selected.last() == Some(&from)
            && self.selected.get(self.selected.len() - 2) == Some(&to)
        {
            self.selected.pop();
            return;
        }
        if !self.selected.contains(&from) {
            self.selected.push(from);
        }
        if !self.selected.contains(&to) {
            self.selected.push(to);
        }
    }

    fn toggle_at(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        if let Some(pos) = self.selected.iter().position(|&picked| picked == index) {
            self.selected.remove(pos);
        } else {
            self.selected.push(index);
        }
        true
    }

    fn click_row(&mut self, index: usize, shift: bool) -> Option<Vec<String>> {
        if index >= self.items.len() {
            return None;
        }
        if shift {
            if self.selected.is_empty() && index != self.cursor {
                if self.cursor <= index {
                    self.selected.extend(self.cursor..=index);
                } else {
                    self.selected.extend((index..=self.cursor).rev());
                }
            } else {
                self.toggle_at(index);
            }
            self.cursor = index;
            None
        } else {
            self.cursor = index;
            self.selected.clear();
            Some(vec![self.items[index].text.clone()])
        }
    }

    fn commit_texts(&self) -> Vec<String> {
        self.snapshot()
            .chosen_texts()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

/// For chords that include both Shift and another modifier (like `⌥⇧V`), the
/// non-Shift modifier(s) keep the picker open so lifting `⇧` never dismisses
/// it prematurely and re-pressing `⇧` can multi-select.
fn hold_modifiers_for(modifiers: HotkeyModifiers) -> HotkeyModifiers {
    let without_shift = HotkeyModifiers {
        shift: None,
        ..modifiers
    };
    if without_shift.is_empty() {
        modifiers
    } else {
        without_shift
    }
}

/// Pre-open hold tracker owned by the listener loop (`recognition`).
#[derive(Default)]
pub struct PendingHold {
    active: Option<(RuntimeHotkey, CaptureInstant)>,
}

impl PendingHold {
    pub fn begin(&mut self, binding: RuntimeHotkey, pressed_at: CaptureInstant) {
        self.active = Some((binding, pressed_at));
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn cancel(&mut self) {
        self.active = None;
    }

    /// Steps the pending hold on a key/modifier edge before `HOLD_THRESHOLD`.
    pub fn on_key_or_flags(
        &mut self,
        code_and_down: Option<(u16, bool)>,
        flags: Option<u64>,
        at: CaptureInstant,
    ) -> HoldOutcome {
        let Some((binding, pressed_at)) = self.active else {
            return HoldOutcome::None;
        };
        if at.duration_since(pressed_at) >= HOLD_THRESHOLD {
            self.active = None;
            return HoldOutcome::OpenPicker { binding };
        }
        if let Some((code, down)) = code_and_down {
            if !down && binding.key_code == Some(code) {
                self.active = None;
                return HoldOutcome::TapPaste;
            }
            if down && binding.key_code != Some(code) {
                // Any other key (including Escape) cancels the hold without pasting.
                self.active = None;
                return HoldOutcome::None;
            }
        }
        if let Some(flags) = flags
            && !binding.modifiers.is_empty()
            && !binding.required_modifiers_down(flags)
        {
            self.active = None;
            return HoldOutcome::TapPaste;
        }
        HoldOutcome::None
    }

    /// Checks whether a still-held shortcut has crossed `HOLD_THRESHOLD`.
    pub fn poll(&mut self, now: CaptureInstant) -> HoldOutcome {
        let Some((binding, pressed_at)) = self.active else {
            return HoldOutcome::None;
        };
        if now.duration_since(pressed_at) >= HOLD_THRESHOLD {
            self.active = None;
            HoldOutcome::OpenPicker { binding }
        } else {
            HoldOutcome::None
        }
    }
}

type Watcher = Box<dyn Fn(&PickerSnapshot) + Send + Sync>;

struct GlobalPicker {
    open: Option<OpenPicker>,
    overlay_rect: Option<[f64; 4]>,
    commits: VecDeque<Vec<String>>,
    watchers: Vec<Watcher>,
}

static IS_OPEN: AtomicBool = AtomicBool::new(false);

fn global() -> &'static Mutex<GlobalPicker> {
    static GLOBAL: OnceLock<Mutex<GlobalPicker>> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        Mutex::new(GlobalPicker {
            open: None,
            overlay_rect: None,
            commits: VecDeque::new(),
            watchers: Vec::new(),
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, GlobalPicker> {
    global()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn notify(watchers: &[Watcher], snapshot: &PickerSnapshot) {
    for watcher in watchers {
        watcher(snapshot);
    }
}

/// Fast lock-free check used on the session event tap's hot path.
pub fn is_open() -> bool {
    IS_OPEN.load(Ordering::Acquire)
}

/// Subscribes an overlay view to picker snapshots; immediately delivers the
/// current snapshot if the picker is already open.
pub fn watch(f: impl Fn(&PickerSnapshot) + Send + Sync + 'static) {
    let snapshot = {
        let mut state = lock();
        let current = state.open.as_ref().map(OpenPicker::snapshot);
        state.watchers.push(Box::new(f));
        current
    };
    if let Some(snapshot) = snapshot {
        let state = lock();
        if let Some(last) = state.watchers.last() {
            last(&snapshot);
        }
    }
}

/// Records the shown overlay's global Quartz rect (`[x, y, width, height]`) so
/// outside clicks dismiss the picker while inside clicks reach Rune's rows.
pub fn set_overlay_rect(rect: Option<[f64; 4]>) {
    lock().overlay_rect = rect;
}

/// Opens the picker with `items` anchored at `anchor`.
pub fn open(
    binding: RuntimeHotkey,
    v_key_code: Option<u16>,
    items: Vec<PickerItem>,
    now_ms: u64,
    anchor: Anchor,
) {
    let snapshot = {
        let mut state = lock();
        let picker = OpenPicker::new(binding, v_key_code, items, now_ms, anchor);
        let snapshot = picker.snapshot();
        state.open = Some(picker);
        IS_OPEN.store(true, Ordering::Release);
        snapshot
    };
    let state = lock();
    notify(&state.watchers, &snapshot);
}

/// Closes the picker without pasting.
pub fn close() {
    if !IS_OPEN.swap(false, Ordering::AcqRel) {
        let mut state = lock();
        if state.open.take().is_none() {
            return;
        }
    } else {
        let mut state = lock();
        state.open = None;
        state.overlay_rect = None;
    }
    let closed = PickerSnapshot::default();
    let state = lock();
    notify(&state.watchers, &closed);
}

/// Drains the next committed selection (if any) for the listener to paste.
pub fn take_commit() -> Option<Vec<String>> {
    lock().commits.pop_front()
}

/// Handles a row press inside the Rune overlay (`shift` toggles multi-select;
/// plain click commits that row).
pub fn click_row(index: usize, shift: bool) {
    let mut snapshot = None;
    let mut closed = false;
    {
        let mut state = lock();
        let Some(picker) = state.open.as_mut() else {
            return;
        };
        if let Some(committed) = picker.click_row(index, shift) {
            if !committed.is_empty() {
                state.commits.push_back(committed);
            }
            state.open = None;
            state.overlay_rect = None;
            IS_OPEN.store(false, Ordering::Release);
            closed = true;
        } else {
            snapshot = Some(picker.snapshot());
        }
    }
    let state = lock();
    if closed {
        notify(&state.watchers, &PickerSnapshot::default());
    } else if let Some(snapshot) = snapshot {
        notify(&state.watchers, &snapshot);
    }
}

/// Commits the current selection (for example from the footer action button).
pub fn commit_current() {
    let closed = {
        let mut state = lock();
        let Some(picker) = state.open.take() else {
            return;
        };
        let committed = picker.commit_texts();
        if !committed.is_empty() {
            state.commits.push_back(committed);
        }
        state.overlay_rect = None;
        IS_OPEN.store(false, Ordering::Release);
        true
    };
    if closed {
        let state = lock();
        notify(&state.watchers, &PickerSnapshot::default());
    }
}

/// Observes a modifier-flag change on the session tap while the picker is open.
pub fn tap_flags(flags: u64) {
    if !is_open() {
        return;
    }
    let mut closed = false;
    {
        let mut state = lock();
        let Some(picker) = state.open.as_mut() else {
            return;
        };
        if flags & SHIFT_KEY_MASK == 0 {
            picker.initial_shift_held = false;
        }
        if !picker.hold_modifiers.is_empty()
            && !modifiers_from_flags(flags).contains(picker.hold_modifiers)
        {
            let committed = picker.commit_texts();
            if !committed.is_empty() {
                state.commits.push_back(committed);
            }
            state.open = None;
            state.overlay_rect = None;
            IS_OPEN.store(false, Ordering::Release);
            closed = true;
        }
    }
    if closed {
        let state = lock();
        notify(&state.watchers, &PickerSnapshot::default());
    }
}

/// Observes a key release on the session tap so the initial trigger hold ends
/// cleanly before subsequent `V` presses cycle rows.
pub fn tap_key_up(code: u16) {
    if !is_open() {
        return;
    }
    let mut state = lock();
    if let Some(picker) = state.open.as_mut()
        && picker.trigger_key_code == Some(code)
    {
        picker.initial_trigger_held = false;
    }
}

/// Handles a key-down on the session tap while the picker is open.
pub fn tap_key_down(code: u16, flags: u64, autorepeat: bool) -> PickerKeyDisposition {
    if !is_open() {
        return PickerKeyDisposition::Pass;
    }
    let mut next_snapshot = None;
    let mut closed = false;
    {
        let mut state = lock();
        let Some(picker) = state.open.as_mut() else {
            return PickerKeyDisposition::Pass;
        };
        let shift = picker.shift_active(flags);
        match code {
            ESCAPE_KEY_CODE => {
                state.open = None;
                state.overlay_rect = None;
                IS_OPEN.store(false, Ordering::Release);
                closed = true;
            }
            RETURN_KEY_CODE | KEYPAD_ENTER_KEY_CODE => {
                let committed = picker.commit_texts();
                if !committed.is_empty() {
                    state.commits.push_back(committed);
                }
                state.open = None;
                state.overlay_rect = None;
                IS_OPEN.store(false, Ordering::Release);
                closed = true;
            }
            UP_ARROW_KEY_CODE => {
                if picker.step_arrow(-1, shift) {
                    next_snapshot = Some(picker.snapshot());
                }
            }
            DOWN_ARROW_KEY_CODE => {
                if picker.step_arrow(1, shift) {
                    next_snapshot = Some(picker.snapshot());
                }
            }
            SPACE_KEY_CODE => {
                let cursor = picker.cursor;
                if picker.toggle_at(cursor) {
                    next_snapshot = Some(picker.snapshot());
                }
            }
            DELETE_KEY_CODE => {
                if !picker.selected.is_empty() {
                    picker.selected.clear();
                    next_snapshot = Some(picker.snapshot());
                }
            }
            // The opening press's autorepeat never cycles.
            other
                if picker.is_cycle_key(other)
                    && !(autorepeat && picker.initial_trigger_held)
                    && picker.step_cycle(shift) =>
            {
                next_snapshot = Some(picker.snapshot());
            }
            _ => {}
        }
    }
    let state = lock();
    if closed {
        notify(&state.watchers, &PickerSnapshot::default());
    } else if let Some(snapshot) = next_snapshot {
        notify(&state.watchers, &snapshot);
    }
    PickerKeyDisposition::Swallow
}

/// Dismisses the picker when the user clicks outside its overlay bounds; clicks
/// inside pass through to Rune's non-activating overlay.
pub fn tap_mouse_down(x: f64, y: f64) {
    if !is_open() {
        return;
    }
    let outside = {
        let state = lock();
        state
            .overlay_rect
            .is_none_or(|[rx, ry, rw, rh]| x < rx || x > rx + rw || y < ry || y > ry + rh)
    };
    if outside {
        close();
    }
}

/// Resolves the caret bounds of the frontmost app, falling back to the current
/// pointer location in global Quartz coordinates.
#[cfg(target_os = "macos")]
pub fn resolve_anchor() -> Anchor {
    if let Some(caret) =
        crate::accessibility::focused_caret_rect(crate::dictation::context::frontmost_pid())
    {
        return Anchor {
            x: caret.x,
            y: caret.y,
            width: caret.width,
            height: caret.height,
        };
    }
    let [x, y] = pointer_location();
    Anchor {
        x,
        y,
        width: 0.0,
        height: 16.0,
    }
}

#[cfg(target_os = "macos")]
fn pointer_location() -> [f64; 2] {
    use core_graphics_types::geometry::CGPoint;
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    // SAFETY: a null source creates an event carrying the current pointer; it
    // is released immediately after reading its location.
    unsafe {
        let event = CGEventCreate(std::ptr::null_mut());
        if event.is_null() {
            return [0.0, 0.0];
        }
        let at = CGEventGetLocation(event);
        CFRelease(event);
        [at.x, at.y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{HotkeyBinding, ModifierSide, OPTION_KEY_MASK};

    fn sample_items(count: usize) -> Vec<PickerItem> {
        (0..count)
            .map(|idx| PickerItem {
                id: (count - idx) as u64,
                text: format!("Dictation {}", idx + 1),
                timestamp_ms: 1_000_000 - (idx as u64) * 30_000,
            })
            .collect()
    }

    fn default_anchor() -> Anchor {
        Anchor {
            x: 320.0,
            y: 440.0,
            width: 1.0,
            height: 18.0,
        }
    }

    #[test]
    fn release_before_hold_threshold_pastes_immediately_and_hold_opens_picker() {
        let mut binding = HotkeyBinding::paste_last_default().runtime();
        binding.key_code = Some(9);
        let mut hold = PendingHold::default();
        let start = CaptureInstant::from_nanos(1_000_000_000);

        hold.begin(binding, start);
        assert_eq!(
            hold.poll(CaptureInstant::from_nanos(1_120_000_000)),
            HoldOutcome::None
        );
        // Key lift at 65 ms is an immediate tap paste.
        assert_eq!(
            hold.on_key_or_flags(
                Some((9, false)),
                Some(OPTION_KEY_MASK | SHIFT_KEY_MASK),
                CaptureInstant::from_nanos(1_065_000_000),
            ),
            HoldOutcome::TapPaste
        );
        assert!(!hold.is_active());

        // Holding to 300 ms opens the picker.
        hold.begin(binding, start);
        assert_eq!(
            hold.poll(CaptureInstant::from_nanos(1_300_000_000)),
            HoldOutcome::OpenPicker { binding }
        );
        assert!(!hold.is_active());
    }

    #[test]
    fn unrelated_key_during_pending_hold_cancels_without_pasting() {
        let mut binding = HotkeyBinding::paste_last_default().runtime();
        binding.key_code = Some(9);
        let mut hold = PendingHold::default();
        let start = CaptureInstant::from_nanos(1_000_000_000);
        hold.begin(binding, start);

        assert_eq!(
            hold.on_key_or_flags(
                Some((ESCAPE_KEY_CODE, true)),
                Some(OPTION_KEY_MASK | SHIFT_KEY_MASK),
                CaptureInstant::from_nanos(1_100_000_000),
            ),
            HoldOutcome::None
        );
        assert!(!hold.is_active());
    }

    #[test]
    fn option_shift_v_ignores_initial_shift_for_multi_select_until_shift_re_pressed() {
        let binding = RuntimeHotkey {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key_code: Some(9),
        };
        let mut picker = OpenPicker::new(
            binding,
            Some(9),
            sample_items(4),
            1_000_000,
            default_anchor(),
        );

        // Holding ⌥ only keeps the picker open when ⇧ lifts.
        assert_eq!(
            picker.hold_modifiers,
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            }
        );

        // Initial autorepeats of V before V lifts do not advance selection.
        assert!(picker.initial_trigger_held);

        // While initial ⇧ is still down from ⌥⇧V, ↓ moves cursor without multi-selecting.
        assert!(!picker.shift_active(OPTION_KEY_MASK | SHIFT_KEY_MASK));
        assert!(picker.step_arrow(1, picker.shift_active(OPTION_KEY_MASK | SHIFT_KEY_MASK)));
        assert_eq!(picker.cursor, 1);
        assert!(picker.selected.is_empty());

        // Once ⇧ lifts and is pressed again, ⇧+↓ multi-selects in order.
        picker.initial_shift_held = false;
        assert!(picker.shift_active(OPTION_KEY_MASK | SHIFT_KEY_MASK));
        assert!(picker.step_arrow(1, true));
        assert_eq!(picker.cursor, 2);
        assert_eq!(picker.selected, vec![1, 2]);
        assert_eq!(
            picker.commit_texts(),
            vec!["Dictation 2".to_string(), "Dictation 3".to_string()]
        );

        // Reversing with ⇧+↑ pops the tip cleanly.
        assert!(picker.step_arrow(-1, true));
        assert_eq!(picker.cursor, 1);
        assert_eq!(picker.selected, vec![1]);
    }

    #[test]
    fn repeated_v_cycles_and_shift_up_selects_older_to_newer_in_order() {
        let binding = RuntimeHotkey {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key_code: Some(9),
        };
        let mut picker = OpenPicker::new(
            binding,
            Some(9),
            sample_items(3),
            1_000_000,
            default_anchor(),
        );
        assert!(picker.step_cycle(false));
        assert_eq!(picker.cursor, 1);
        assert!(picker.step_cycle(false));
        assert_eq!(picker.cursor, 2);
        // Wraps back to 0 on another V press.
        assert!(picker.step_cycle(false));
        assert_eq!(picker.cursor, 0);

        // Move to oldest (2), then ⇧+↑ twice to select 2 -> 1 -> 0 in chronological order!
        picker.cursor = 2;
        assert!(picker.step_arrow(-1, true));
        assert!(picker.step_arrow(-1, true));
        assert_eq!(picker.selected, vec![2, 1, 0]);
        assert_eq!(
            picker.commit_texts(),
            vec![
                "Dictation 3".to_string(),
                "Dictation 2".to_string(),
                "Dictation 1".to_string(),
            ]
        );
    }
}
