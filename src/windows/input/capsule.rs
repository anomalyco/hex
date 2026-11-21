//! The finished capture capsule's keys on Windows, read from the keyboard
//! hook: a Rune overlay never takes the keyboard here (its window is a
//! no-activate tool window, and Rune hands an overlay the keyboard on macOS
//! and Wayland only), so the search and the suggestions hear what Hex's macOS
//! tap hands them (`capture_host::list_key`, `suppression::list_tap`).
//!
//! As on the Mac, nothing here consumes a key a shortcut of Hex's claimed, no
//! modifier is ever kept from the app, and a key the capsule took keeps its
//! release with it. A key that edits nothing, and one the app already holds,
//! stay the app's.

use std::sync::Mutex;
use std::sync::atomic::Ordering;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ToUnicodeEx, VK_BACK, VK_CAPITAL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
    VK_INSERT, VK_LEFT, VK_LSHIFT, VK_NEXT, VK_PACKET, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RSHIFT,
    VK_SHIFT, VK_TAB, VK_UP,
};

use super::Disposition;
use super::disposition::Handled;
use super::modifiers::ModifierState;
use crate::capture_host::{CapsuleKey, capsule_key, list_open, suggesting, takes_text};

/// Which of the capsule's states takes keys now: the typed search takes every
/// key it edits with (`CapsuleKeys::List`), the suggestions only what their
/// keys say (`CapsuleKeys::Suggest`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Search,
    Suggest,
}

/// What a key types in the foreground layout.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Typed {
    /// The character it types.
    Text(String),
    /// A dead key: it types nothing on its own, and the layout is never asked
    /// to compose its accent.
    Dead,
}

/// What the capsule may take of a press right now, as its grab and Hex's own
/// shortcuts decide it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Opening {
    /// One of Hex's shortcuts took the key, so it stays that shortcut's.
    claimed: bool,
    /// The mode taking keys, or `None` while the capsule takes none.
    mode: Option<Mode>,
    /// The capsule can take text: the open search, or suggestions the pointer
    /// moved onto (`capture_host::takes_text`). Only then is the layout asked
    /// what a key types, since asking moves its state.
    text: bool,
}

/// What the capsule does with a key press. `capture_host::list_key` reads the
/// macOS tap's events into the same three.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Tap {
    /// Not the capsule's: the app in front keeps it.
    Pass,
    /// The capsule's, with no key to hand it: a dead key, an autorepeat, or a
    /// release the capsule owed.
    Swallow,
    /// The capsule's, and it hears this.
    Key(CapsuleKey),
}

impl Tap {
    /// The capsule's reading of `vk` pressed with `shift` while `mode` shows:
    /// `typed` says what the key types in the foreground layout, and is asked
    /// only when the key isn't one of the named ones.
    fn read(
        mode: Mode,
        vk: u32,
        scan: u32,
        shift: bool,
        typed: impl FnOnce() -> Option<Typed>,
    ) -> Tap {
        let tap = match vk as u16 {
            VK_ESCAPE => Tap::Key(CapsuleKey::Escape),
            VK_BACK => Tap::Key(CapsuleKey::Backspace),
            VK_UP => Tap::Key(CapsuleKey::Up),
            VK_DOWN => Tap::Key(CapsuleKey::Down),
            VK_RETURN => Tap::Key(CapsuleKey::Return { shift }),
            VK_TAB => Tap::Key(CapsuleKey::Tab { shift }),
            // The rest of the keys a caret takes: Delete, Left and Right, Home
            // and End, Page Up and Page Down, Insert. The capsule has no key for
            // them, but the app behind must not get them while the search has
            // the field. Over the suggestions they pass, as below.
            VK_DELETE | VK_LEFT | VK_RIGHT | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_INSERT => {
                Tap::Swallow
            }
            // Another input method's character (SendInput's
            // `KEYEVENTF_UNICODE`, an on-screen or handwriting keyboard) rides
            // in the scan code with no layout to read: it is text by definition.
            VK_PACKET => match char::from_u32(u32::from(scan as u16)) {
                Some(text) if !text.is_control() => Tap::Key(CapsuleKey::Text(text.into())),
                _ => Tap::Pass,
            },
            _ => match typed() {
                Some(Typed::Text(text)) if text.chars().next().is_some_and(|c| !c.is_control()) => {
                    Tap::Key(CapsuleKey::Text(text))
                }
                // A dead key types nothing here: the layout is read without
                // changing its state, so no accent is composed for it. The
                // search takes it with nothing rather than leaving half a
                // composition with the app.
                Some(Typed::Dead) => Tap::Swallow,
                // Anything else that types nothing edits nothing — Caps Lock,
                // Num Lock, the media and browser keys, Print Screen, Sleep, the
                // Apps key, a function key, a control character. The app keeps
                // every one of them.
                _ => Tap::Pass,
            },
        };
        match tap {
            // The suggestions take only what their keys say (text, once the
            // pointer moved onto them), so a dead key goes on to the app there,
            // as it does on the Mac.
            Tap::Swallow if mode == Mode::Suggest => Tap::Pass,
            tap => tap,
        }
    }
}

/// The keys the capsule took and the keys the app holds, Shift's state, and
/// Caps Lock's. A taken key's release stays the capsule's, as the macOS tap's
/// taken bits do, and a key the app already has down keeps its repeats and
/// release there, so the capsule taking keys under it never leaves it stuck.
struct Keys {
    taken: [bool; 256],
    /// Keys the app got a press of and hasn't seen released.
    passed: [bool; 256],
    /// Shift's last state as the capsule hears it: down while either key is.
    shift: bool,
    left_shift: bool,
    right_shift: bool,
    /// Caps Lock's toggle, read once and then kept from the presses the hook
    /// sees of its key; `None` until something needs it (a character, or Caps
    /// Lock's own press).
    caps: Option<bool>,
    /// Caps Lock's own key is down, so only its press flips the toggle.
    caps_down: bool,
}

/// A virtual key's bit in a table, if it has one.
fn slot(bits: &[bool; 256], vk: u32) -> Option<&bool> {
    usize::try_from(vk).ok().and_then(|vk| bits.get(vk))
}

fn slot_mut(bits: &mut [bool; 256], vk: u32) -> Option<&mut bool> {
    usize::try_from(vk).ok().and_then(|vk| bits.get_mut(vk))
}

impl Keys {
    const fn new() -> Self {
        Self {
            taken: [false; 256],
            passed: [false; 256],
            shift: false,
            left_shift: false,
            right_shift: false,
            caps: None,
            caps_down: false,
        }
    }

    /// Whether the capsule took `vk`'s press, which it hasn't let go of yet.
    fn holds(&self, vk: u32) -> bool {
        slot(&self.taken, vk).copied().unwrap_or(false)
    }

    /// Keeps `vk`'s press, so its release is the capsule's too.
    fn keep(&mut self, vk: u32) {
        if let Some(taken) = slot_mut(&mut self.taken, vk) {
            *taken = true;
        }
    }

    /// Whether the capsule holds `vk`'s press, letting go of it: its release
    /// belongs to the capsule then.
    fn release(&mut self, vk: u32) -> bool {
        slot_mut(&mut self.taken, vk).is_some_and(|taken| std::mem::replace(taken, false))
    }

    /// Whether the app got `vk`'s press and hasn't seen its release.
    fn held_by_app(&self, vk: u32) -> bool {
        slot(&self.passed, vk).copied().unwrap_or(false)
    }

    /// Remembers that the app got `vk`'s press, so its autorepeats and release
    /// stay the app's.
    fn hold_for_app(&mut self, vk: u32) {
        if let Some(passed) = slot_mut(&mut self.passed, vk) {
            *passed = true;
        }
    }

    /// Whether the app got `vk`'s press, letting go of it: its release is the
    /// app's then.
    fn release_for_app(&mut self, vk: u32) -> bool {
        slot_mut(&mut self.passed, vk).is_some_and(|passed| std::mem::replace(passed, false))
    }

    /// Follows Shift, whose presses and releases the capsule hears while it
    /// takes keys: the list's "always send" chord. Left and right are tracked
    /// apart, so letting go of one while the other is held keeps it down.
    fn follow_shift(&mut self, vk: u32, down: bool, takes: bool) {
        match vk as u16 {
            VK_LSHIFT => self.left_shift = down,
            VK_RSHIFT => self.right_shift = down,
            _ => {
                self.left_shift = down;
                self.right_shift = down;
            }
        }
        let shift = self.left_shift || self.right_shift;
        if shift != self.shift {
            self.shift = shift;
            if takes {
                capsule_key(CapsuleKey::Shift(shift));
            }
        }
    }

    /// Follows Caps Lock's own key: the toggle flips once, on the press.
    fn follow_caps(&mut self, down: bool) {
        if down
            && !self.caps_down
            && let Some(caps) = self.caps.as_mut()
        {
            *caps = !*caps;
        }
        self.caps_down = down;
    }

    /// Caps Lock's toggle, which a letter's case needs. `GetKeyState` is the
    /// only Win32 source of a toggle (`GetAsyncKeyState` has none), and this
    /// thread's queue can lag the hook, so it is read once — the system's
    /// toggle at that moment — and kept from then on: Caps Lock only changes
    /// from its own key, which the hook sees.
    fn caps_lock(&mut self) -> bool {
        if let Some(caps) = self.caps {
            return caps;
        }
        // SAFETY: a plain read of this thread's key state; Caps Lock's low bit
        // is its toggle.
        let caps = unsafe { GetKeyState(i32::from(VK_CAPITAL)) } & 1 != 0;
        self.caps = Some(caps);
        caps
    }

    /// One key event of `vk`, `scan` its scan code: whether the hook keeps it
    /// from the app in front. `opening` says what the capsule may take right
    /// now, `held` the modifiers as the event leaves them, and `hear` hands the
    /// capsule a key, answering whether its grab takes that key.
    fn decide(
        &mut self,
        vk: u32,
        scan: u32,
        down: bool,
        opening: Opening,
        held: impl FnOnce() -> ModifierState,
        hear: impl FnOnce(CapsuleKey) -> bool,
    ) -> bool {
        let kept = match self.reading(vk, scan, down, opening, held) {
            Tap::Pass => false,
            Tap::Swallow => true,
            Tap::Key(key) => hear(key),
        };
        if down {
            if kept {
                self.keep(vk);
            } else if !opening.claimed {
                // The app keeps this press: remembered until its release, so a
                // key held while the capsule starts taking keys under it keeps
                // its autorepeats and its release with the app.
                self.hold_for_app(vk);
            }
        }
        kept
    }

    /// What the capsule does with one key event before any key is handed over.
    fn reading(
        &mut self,
        vk: u32,
        scan: u32,
        down: bool,
        opening: Opening,
        held: impl FnOnce() -> ModifierState,
    ) -> Tap {
        if vk == u32::from(VK_CAPITAL) {
            // Seeded before the flip, so the press that starts a toggle is
            // counted once it is read.
            let _ = self.caps_lock();
            self.follow_caps(down);
        }
        if ModifierState::is_modifier(vk) {
            // No modifier is ever kept from the app: a swallowed one would leave
            // a chord stuck down. Its release lets go of what its press recorded.
            if !down {
                self.release_for_app(vk);
            }
            if matches!(vk as u16, VK_SHIFT | VK_LSHIFT | VK_RSHIFT) {
                self.follow_shift(vk, down, opening.mode.is_some());
            }
            return Tap::Pass;
        }
        if !down {
            // A release is the capsule's only when its press was; one the app
            // got is the app's, so nothing stays held down there.
            if self.release_for_app(vk) {
                return Tap::Pass;
            }
            return if self.release(vk) {
                Tap::Swallow
            } else {
                Tap::Pass
            };
        }
        // A held key's autorepeats never switch sides mid-press, whatever the
        // capsule's keys say now: the app's stay the app's, the capsule's stay
        // the capsule's.
        if self.held_by_app(vk) {
            return Tap::Pass;
        }
        if self.holds(vk) {
            return Tap::Swallow;
        }
        if opening.claimed {
            // A shortcut's own key: it stays the shortcut's, as the macOS tap
            // lets its hotkeys through.
            return Tap::Pass;
        }
        let Some(mode) = opening.mode else {
            return Tap::Pass;
        };
        let held = held();
        if chord(&held) {
            // Ctrl, Alt and the Windows key make a shortcut's chord, not typing;
            // AltGr arrives as Ctrl and Alt, so its characters stay the layout's
            // for the app in front.
            return Tap::Pass;
        }
        let shift = held.left_shift || held.right_shift;
        if opening.text {
            let caps = self.caps_lock();
            Tap::read(mode, vk, scan, shift, || typed(vk, scan, shift, caps))
        } else {
            // Nothing here reads what the key types, and asking a layout moves
            // its state: a key that goes on to the app is never asked about.
            Tap::read(mode, vk, scan, shift, || None)
        }
    }
}

static KEYS: Mutex<Keys> = Mutex::new(Keys::new());

/// The keyboard hook's event once Hex's own shortcuts have had it: a shortcut
/// keeps its key, and the capture capsule takes what else it may.
pub(super) fn settle(handled: Handled, vk: u32, scan: u32, down: bool) -> Handled {
    let claimed = handled.disposition != Disposition::Pass || handled.triggered();
    let mode = mode();
    let opening = Opening {
        claimed,
        mode,
        text: mode.is_some() && takes_text(),
    };
    let mut keys = KEYS.lock().unwrap_or_else(|error| error.into_inner());
    // The grab answers for the key itself; one it refuses is remembered as the
    // app's until its release, like every other press the hook lets through.
    if keys.decide(vk, scan, down, opening, ModifierState::current, capsule_key) {
        Handled::new(Disposition::Suppress, false)
    } else {
        handled
    }
}

/// A mouse button going down, `message` its hook message: a press anywhere may
/// fold the capsule's list or search, and the click still reaches whatever is
/// under it, as the macOS tap's press does.
pub(super) fn press(message: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };
    if matches!(
        message,
        WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_XBUTTONDOWN
    ) && (list_open() || suggesting())
    {
        capsule_key(CapsuleKey::Press);
    }
}

/// The mode taking keys now, if any: its list or suggestions are open, and
/// Annotate is on, no shortcut is being recorded, and Hex isn't paused, as
/// `suppression::list_takes_keys` has it on the Mac.
fn mode() -> Option<Mode> {
    if !crate::app_settings::capture_on()
        || super::CAPTURE_INHIBIT.load(Ordering::Relaxed)
        || crate::app_settings::shortcuts_suspended()
    {
        return None;
    }
    if list_open() {
        Some(Mode::Search)
    } else if suggesting() {
        Some(Mode::Suggest)
    } else {
        None
    }
}

/// Whether `held` makes a shortcut's chord rather than typing.
fn chord(held: &ModifierState) -> bool {
    held.left_control
        || held.right_control
        || held.left_alt
        || held.right_alt
        || held.left_windows
        || held.right_windows
}

/// What `vk` types in the layout the window with keyboard focus uses, read
/// without changing the layout's state: `None` when the key types nothing
/// there, and a dead key as `Dead` (its accent is never composed, since nothing
/// changes the state it would need).
fn typed(vk: u32, scan: u32, shift: bool, caps: bool) -> Option<Typed> {
    let layout = crate::keyboard_input_sources::foreground_layout();
    let mut state = [0u8; 256];
    // Only a character's own state: Shift and Caps Lock's toggle. Ctrl, Alt and
    // the Windows key make a chord and never get here, and their bits stay clear
    // so an AltGr level is never read by accident.
    let shift = if shift { 0x80 } else { 0 };
    for key in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT] {
        state[usize::from(key)] = shift;
    }
    state[usize::from(VK_CAPITAL)] = u8::from(caps);
    let mut text = [0u16; 8];
    // SAFETY: `text` holds eight UTF-16 units, `state` the keyboard state
    // `ToUnicodeEx` reads, and `layout` a handle from `GetKeyboardLayout`.
    // 0x4: never change the layout's state, so a key that goes on to the app and
    // a dead key the capsule takes both leave its dead-key state alone.
    let length = unsafe {
        ToUnicodeEx(
            vk,
            scan,
            state.as_ptr(),
            text.as_mut_ptr(),
            text.len() as i32,
            0x4,
            layout,
        )
    };
    match length {
        0 => None,
        -1 => Some(Typed::Dead),
        length => {
            let length = usize::try_from(length).ok()?;
            Some(Typed::Text(String::from_utf16_lossy(text.get(..length)?)))
        }
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_APPS, VK_BROWSER_BACK, VK_BROWSER_FORWARD, VK_BROWSER_HOME, VK_BROWSER_REFRESH, VK_F5,
        VK_LAUNCH_MAIL, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
        VK_MEDIA_STOP, VK_NUMLOCK, VK_SCROLL, VK_SLEEP, VK_SNAPSHOT, VK_VOLUME_DOWN,
        VK_VOLUME_MUTE, VK_VOLUME_UP,
    };

    use super::*;

    /// The openings the hook can meet: the search, the suggestions with the
    /// pointer on them (they take text), the plain suggestions, and nothing
    /// open.
    const SEARCH: Opening = Opening {
        claimed: false,
        mode: Some(Mode::Search),
        text: true,
    };
    const SUGGEST: Opening = Opening {
        claimed: false,
        mode: Some(Mode::Suggest),
        text: false,
    };
    const CLOSED: Opening = Opening {
        claimed: false,
        mode: None,
        text: false,
    };

    fn nothing() -> Option<Typed> {
        None
    }

    fn types(text: &'static str) -> impl FnOnce() -> Option<Typed> {
        move || Some(Typed::Text(text.into()))
    }

    fn dead() -> Option<Typed> {
        Some(Typed::Dead)
    }

    fn plain() -> ModifierState {
        ModifierState::default()
    }

    /// A grab that refuses every key, as the plain suggestions do.
    fn takes_nothing(_: CapsuleKey) -> bool {
        false
    }

    /// A grab no event here should ask: nothing is handed over.
    fn asks_nothing(key: CapsuleKey) -> bool {
        panic!("nothing should be handed over, but {key:?} was");
    }

    /// Each named key reads as its own, in the search and in the suggestions.
    #[test]
    fn the_named_keys_read_as_their_keys() {
        for mode in [Mode::Search, Mode::Suggest] {
            for (vk, key) in [
                (VK_ESCAPE, CapsuleKey::Escape),
                (VK_BACK, CapsuleKey::Backspace),
                (VK_UP, CapsuleKey::Up),
                (VK_DOWN, CapsuleKey::Down),
                (VK_RETURN, CapsuleKey::Return { shift: false }),
                (VK_TAB, CapsuleKey::Tab { shift: false }),
            ] {
                assert_eq!(
                    Tap::read(mode, u32::from(vk), 0, false, nothing),
                    Tap::Key(key),
                    "vk {vk:#x} in {mode:?}"
                );
            }
        }
        assert_eq!(
            Tap::read(Mode::Search, u32::from(VK_RETURN), 0, true, nothing),
            Tap::Key(CapsuleKey::Return { shift: true })
        );
        assert_eq!(
            Tap::read(Mode::Search, u32::from(VK_TAB), 0, true, nothing),
            Tap::Key(CapsuleKey::Tab { shift: true })
        );
    }

    /// A key that types a character is text, in the search and over the
    /// suggestions alike; the capsule's own keys decide what the suggestions do
    /// with it (`capture_host::takes`).
    #[test]
    fn a_key_that_types_is_text() {
        let key = 0x41_u32;
        assert_eq!(
            Tap::read(Mode::Search, key, 0, false, types("a")),
            Tap::Key(CapsuleKey::Text("a".into()))
        );
        assert_eq!(
            Tap::read(Mode::Suggest, key, 0, true, types("A")),
            Tap::Key(CapsuleKey::Text("A".into()))
        );
        assert_eq!(
            Tap::read(Mode::Search, key, 0, false, types("é")),
            Tap::Key(CapsuleKey::Text("é".into())),
            "a composed character arrives whole"
        );
        assert_eq!(
            Tap::read(Mode::Search, key, 0, false, types(" ")),
            Tap::Key(CapsuleKey::Text(" ".into()))
        );
    }

    /// The search keeps the rest of a caret's keys from the app; over the
    /// suggestions they pass, as they always have.
    #[test]
    fn the_search_keeps_the_caret_keys_from_the_app() {
        for vk in [
            VK_DELETE, VK_LEFT, VK_RIGHT, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_INSERT,
        ] {
            assert_eq!(
                Tap::read(Mode::Search, u32::from(vk), 0, false, nothing),
                Tap::Swallow,
                "vk {vk:#x} in the search"
            );
            assert_eq!(
                Tap::read(Mode::Suggest, u32::from(vk), 0, false, nothing),
                Tap::Pass,
                "vk {vk:#x} over the suggestions"
            );
        }
    }

    /// A key the app got keeps its autorepeats and its release there, even
    /// after the capsule starts taking keys under it.
    #[test]
    fn a_key_the_app_got_stays_the_apps_until_it_comes_up() {
        let mut keys = Keys::new();
        let back = u32::from(VK_BACK);
        assert!(!keys.decide(back, 0, true, CLOSED, plain, asks_nothing));
        assert!(keys.held_by_app(back), "the app has it down");
        assert!(
            !keys.decide(back, 0, true, SEARCH, plain, asks_nothing),
            "its autorepeat stays the app's"
        );
        assert!(
            !keys.decide(back, 0, false, SEARCH, plain, asks_nothing),
            "and so does its release"
        );
        assert!(!keys.held_by_app(back));
        assert!(
            keys.decide(back, 0, true, SEARCH, plain, |_| true),
            "a fresh press is the capsule's"
        );
        assert!(
            keys.decide(back, 0, false, SEARCH, plain, asks_nothing),
            "and its release stays the capsule's"
        );
    }

    /// A key the app got while the suggestions showed keeps passing once the
    /// search opens under it: its repeats and release never reach the capsule.
    #[test]
    fn a_key_the_app_got_over_the_suggestions_stays_the_apps() {
        let mut keys = Keys::new();
        // A caret key passes over the plain suggestions, and the app has it.
        let left = u32::from(VK_LEFT);
        assert!(!keys.decide(left, 0, true, SUGGEST, plain, asks_nothing));
        assert!(keys.held_by_app(left), "the app has it down");
        assert!(
            !keys.decide(left, 0, true, SEARCH, plain, asks_nothing),
            "its autorepeat passes after the search opens"
        );
        assert!(
            !keys.decide(left, 0, false, SEARCH, plain, asks_nothing),
            "and its release is the app's"
        );
        assert!(!keys.held_by_app(left));
        // A key the grab refuses over the suggestions is the app's the same way.
        let back = u32::from(VK_BACK);
        assert!(!keys.decide(back, 0, true, SUGGEST, plain, takes_nothing));
        assert!(keys.held_by_app(back));
        assert!(!keys.decide(back, 0, false, SEARCH, plain, asks_nothing));
        assert!(!keys.held_by_app(back));
    }

    /// A dead key is the search's to take with nothing, and the app's over the
    /// suggestions.
    #[test]
    fn a_dead_key_is_taken_with_nothing_only_in_the_search() {
        let key = 0xDE_u32;
        assert_eq!(Tap::read(Mode::Search, key, 0, false, dead), Tap::Swallow);
        assert_eq!(Tap::read(Mode::Suggest, key, 0, false, dead), Tap::Pass);
    }

    /// Every key that edits nothing goes on to the app, even in the search:
    /// Caps Lock, Num Lock, Scroll Lock, Print Screen, Sleep, the Apps key, a
    /// function key, the media and browser keys, and a control character.
    #[test]
    fn keys_that_edit_nothing_pass_in_the_search() {
        let keys = [
            VK_CAPITAL,
            VK_NUMLOCK,
            VK_SCROLL,
            VK_SNAPSHOT,
            VK_SLEEP,
            VK_APPS,
            VK_F5,
            VK_VOLUME_MUTE,
            VK_VOLUME_DOWN,
            VK_VOLUME_UP,
            VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP,
            VK_MEDIA_PLAY_PAUSE,
            VK_BROWSER_BACK,
            VK_BROWSER_FORWARD,
            VK_BROWSER_REFRESH,
            VK_BROWSER_HOME,
            VK_LAUNCH_MAIL,
        ];
        for vk in keys {
            assert_eq!(
                Tap::read(Mode::Search, u32::from(vk), 0, false, nothing),
                Tap::Pass,
                "vk {vk:#x}"
            );
        }
        assert_eq!(
            Tap::read(Mode::Search, 0x41, 0, false, types("\u{7f}")),
            Tap::Pass,
            "a control character edits nothing"
        );
    }

    /// Another input method's character rides in a packet key's scan code, and
    /// anything that is not a character goes on to the app.
    #[test]
    fn a_packet_key_carries_its_character() {
        assert_eq!(
            Tap::read(
                Mode::Search,
                u32::from(VK_PACKET),
                u32::from('é'),
                false,
                nothing
            ),
            Tap::Key(CapsuleKey::Text("é".into()))
        );
        assert_eq!(
            Tap::read(Mode::Search, u32::from(VK_PACKET), 0x07, false, nothing),
            Tap::Pass,
            "a control character is no text"
        );
    }

    /// Shift's press and release are followed until both keys are up, and no
    /// modifier is ever kept from the app.
    #[test]
    fn shift_is_followed_until_both_keys_are_up() {
        let mut keys = Keys::new();
        let (left, right) = (u32::from(VK_LSHIFT), u32::from(VK_RSHIFT));
        assert!(!keys.decide(left, 0, true, CLOSED, plain, asks_nothing));
        assert!(!keys.decide(right, 0, true, CLOSED, plain, asks_nothing));
        assert!(keys.shift);
        assert!(!keys.decide(left, 0, false, CLOSED, plain, asks_nothing));
        assert!(keys.shift, "the right Shift is still down");
        assert!(!keys.decide(right, 0, false, CLOSED, plain, asks_nothing));
        assert!(!keys.shift);
    }

    /// Caps Lock's toggle flips on its press alone, and stays unread until it
    /// is first needed.
    #[test]
    fn caps_lock_flips_on_its_press_alone() {
        let mut keys = Keys::new();
        keys.follow_caps(true);
        assert_eq!(keys.caps, None, "nothing is read before it is needed");
        keys.caps = Some(false);
        keys.follow_caps(false);
        keys.follow_caps(true);
        assert_eq!(keys.caps, Some(true));
        keys.follow_caps(true);
        assert_eq!(keys.caps, Some(true), "an autorepeat flips nothing");
        keys.follow_caps(false);
        keys.follow_caps(true);
        assert_eq!(keys.caps, Some(false));
    }

    /// A release is the capsule's only when its press was.
    #[test]
    fn a_taken_key_keeps_its_release() {
        let mut keys = Keys::new();
        let back = u32::from(VK_BACK);
        assert!(!keys.decide(back, 0, false, SEARCH, plain, asks_nothing));
        keys.keep(back);
        assert!(
            keys.decide(back, 0, true, SEARCH, plain, asks_nothing),
            "an autorepeat hears nothing"
        );
        assert!(
            keys.decide(back, 0, false, SEARCH, plain, asks_nothing),
            "its release is the capsule's"
        );
        assert!(!keys.decide(back, 0, false, SEARCH, plain, asks_nothing));
    }

    /// Ctrl, Alt and the Windows key make a chord, not typing.
    #[test]
    fn a_chord_passes() {
        let mut keys = Keys::new();
        for held in [
            ModifierState {
                left_control: true,
                ..Default::default()
            },
            ModifierState {
                right_alt: true,
                ..Default::default()
            },
            ModifierState {
                left_windows: true,
                ..Default::default()
            },
        ] {
            assert!(!keys.decide(0x41, 0, true, SEARCH, || held, asks_nothing));
        }
    }
}
