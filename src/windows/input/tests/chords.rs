//! A modifier the shortcut doesn't name joining a modifier-only hold.
//!
//! The Windows default is one key alone (Left Alt, #2278), so Alt-then-Ctrl
//! on the way to Paste Last's Ctrl+Alt+V, or Alt+Shift for Windows' own
//! input-language switch, is another application's chord from that press on.
//! It takes the hold away like any key joining it, Toggle must not lock the
//! cancelled press, and Alt's release is the app's: nothing was swallowed
//! that would read as a lone Alt tap, unless Hex later swallows a key of this
//! chord (Paste Last's V re-arms the mask).
//!
//! These drive the real hook state machine through `process_button`, whose
//! `key_held` lookup stands in for the physical keys a test cannot press.

use std::sync::mpsc::Receiver;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use super::super::modifiers::LeftControl;
use super::super::{
    CALLBACK_STATE, CaptureState, Disposition, HotkeyAction, HotkeyEvent, WindowsHotkey,
    process_button, virtual_key,
};
use super::{callback_state_value, state_lock};
use crate::app_settings::ModifierSide;

/// The physical keys a test holds, as `ModifierState::read` asks for them.
#[derive(Clone, Copy, Default)]
struct Keys {
    left_control: bool,
    right_control: bool,
    left_alt: bool,
    right_alt: bool,
    left_shift: bool,
    right_shift: bool,
    left_windows: bool,
    right_windows: bool,
}

impl Keys {
    /// What `GetAsyncKeyState` would answer; a side-less code is its left key,
    /// as `ModifierState::update` reads it.
    fn is_down(&self, key: u16) -> bool {
        match key {
            VK_LCONTROL | VK_CONTROL => self.left_control,
            VK_RCONTROL => self.right_control,
            VK_LMENU | VK_MENU => self.left_alt,
            VK_RMENU => self.right_alt,
            VK_LSHIFT | VK_SHIFT => self.left_shift,
            VK_RSHIFT => self.right_shift,
            VK_LWIN => self.left_windows,
            VK_RWIN => self.right_windows,
            _ => false,
        }
    }

    fn set(&mut self, key: u16, is_down: bool) {
        match key {
            VK_LCONTROL | VK_CONTROL => self.left_control = is_down,
            VK_RCONTROL => self.right_control = is_down,
            VK_LMENU | VK_MENU => self.left_alt = is_down,
            VK_RMENU => self.right_alt = is_down,
            VK_LSHIFT | VK_SHIFT => self.left_shift = is_down,
            VK_RSHIFT => self.right_shift = is_down,
            VK_LWIN => self.left_windows = is_down,
            VK_RWIN => self.right_windows = is_down,
            _ => {}
        }
    }
}

/// The hook as the listener installs it for one modifier-only binding, with
/// the keys a test holds and the events it sends.
struct Rig {
    keys: Keys,
    events: Receiver<HotkeyEvent>,
    at: u32,
}

impl Rig {
    fn keyless(binding: WindowsHotkey, paste_last: Option<WindowsHotkey>, toggle: bool) -> Self {
        let (mut state, events) = callback_state_value(binding, false);
        // A modifier-only binding has no key for the hook to watch.
        state.trigger_vk = None;
        if let Some(paste) = &paste_last {
            state.paste_last = Some(paste.clone());
            state.paste_trigger_vk = paste.key.as_deref().and_then(|key| virtual_key(key).ok());
        }
        state.capture = CaptureState {
            toggle,
            ..CaptureState::default()
        };
        *CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(state);
        Self {
            keys: Keys::default(),
            events,
            at: 1_000,
        }
    }

    fn down(&mut self, key: u16) -> Disposition {
        self.step(key, true)
    }

    fn up(&mut self, key: u16) -> Disposition {
        self.step(key, false)
    }

    fn step(&mut self, key: u16, is_down: bool) -> Disposition {
        self.at += 100;
        let keys = &self.keys;
        let handled = process_button(
            u32::from(key),
            LeftControl::Other,
            is_down,
            self.at,
            |held| keys.is_down(held),
        );
        self.keys.set(key, is_down);
        handled.disposition
    }

    fn next(&mut self) -> Option<HotkeyAction> {
        self.events.try_recv().ok().map(|event| event.action)
    }

    fn capture_locked(&self) -> bool {
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|state| state.capture.locked)
    }

    fn dirty(&self) -> bool {
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|state| state.dirty)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }
}

/// Left Alt (the default) does not survive Shift: the hold is Windows'
/// input-language switch, so it cancels, and Alt's release is the app's.
#[test]
fn a_modifier_outside_the_hold_cancels_it_without_a_mask() {
    let _state = state_lock();
    let mut rig = Rig::keyless(WindowsHotkey::default(), None, false);
    assert_eq!(rig.down(VK_LMENU), Disposition::Pass);
    assert_eq!(rig.next(), Some(HotkeyAction::Start));

    assert_eq!(
        rig.down(VK_LSHIFT),
        Disposition::Pass,
        "Shift reaches the app, so the layout switches"
    );
    assert_eq!(rig.next(), Some(HotkeyAction::Cancel));
    assert!(!rig.capture_locked(), "a cancelled hold locks nothing");

    // Nothing was swallowed in this chord, so the Alt release passes with no
    // VK 0xE8 mask key ahead of it.
    assert_eq!(rig.up(VK_LMENU), Disposition::Pass);
    assert_eq!(rig.next(), None, "a cancelled hold finishes nothing");
    assert!(!rig.dirty(), "the chord leaves the shortcut pressable");
}

/// Toggle mode must not lock the capture the joining modifier cancelled.
#[test]
fn a_joining_modifier_never_locks_a_toggle_capture() {
    let _state = state_lock();
    let mut rig = Rig::keyless(WindowsHotkey::default(), None, true);
    assert_eq!(rig.down(VK_LMENU), Disposition::Pass);
    assert_eq!(rig.next(), Some(HotkeyAction::Start));
    assert_eq!(rig.down(VK_LWIN), Disposition::Pass, "Win stays Windows'");
    assert_eq!(rig.next(), Some(HotkeyAction::Cancel));
    assert_eq!(rig.up(VK_LMENU), Disposition::Pass);
    assert_eq!(rig.next(), None);
    assert!(
        !rig.capture_locked(),
        "a cancelled Toggle press must not stay locked and recording"
    );
}

/// Left Alt on the way to Paste Last's Ctrl+Alt+V hands the hold over: the
/// dictation cancels and the paste still fires.
#[test]
fn ctrl_after_alt_leaves_paste_last_to_fire() {
    let _state = state_lock();
    let paste = WindowsHotkey {
        control: Some(ModifierSide::Either),
        windows: None,
        alt: Some(ModifierSide::Either),
        shift: None,
        key: Some("v".into()),
    };
    let mut rig = Rig::keyless(WindowsHotkey::default(), Some(paste), false);
    assert_eq!(rig.down(VK_LMENU), Disposition::Pass);
    assert_eq!(rig.next(), Some(HotkeyAction::Start));

    assert_eq!(rig.down(VK_LCONTROL), Disposition::Pass);
    assert_eq!(rig.next(), Some(HotkeyAction::Cancel));
    assert!(
        !rig.dirty(),
        "the paste's own chord must not read as a dirty press"
    );

    let v = u16::try_from(virtual_key("v").unwrap()).unwrap();
    assert_eq!(rig.down(v), Disposition::Suppress, "Hex's paste key");
    assert_eq!(rig.next(), Some(HotkeyAction::PasteLast));
    assert_eq!(rig.next(), None);
    rig.up(v);
    rig.up(VK_LCONTROL);
    // The V this chord swallowed makes Alt's release mask, so the app reads
    // a chord rather than a lone Alt tap (#1254).
    assert_eq!(rig.up(VK_LMENU), Disposition::Mask);
    assert_eq!(rig.next(), None, "no dictation finishes or pastes");
}

/// A modifier the chord names is part of the shortcut, in either order: it
/// never cancels the hold.
#[test]
fn modifiers_the_chord_names_never_cancel_it() {
    let _state = state_lock();
    let ctrl_alt = WindowsHotkey {
        control: Some(ModifierSide::Either),
        windows: None,
        alt: Some(ModifierSide::Either),
        shift: None,
        key: None,
    };

    // Ctrl, then Alt.
    let mut rig = Rig::keyless(ctrl_alt.clone(), None, false);
    assert_eq!(rig.down(VK_LCONTROL), Disposition::Pass);
    assert_eq!(rig.next(), None, "an incomplete chord presses nothing");
    rig.down(VK_LMENU);
    assert_eq!(rig.next(), Some(HotkeyAction::Start));
    rig.up(VK_LMENU);
    assert_eq!(rig.next(), Some(HotkeyAction::Finish));
    drop(rig);

    // Alt, then Ctrl.
    let mut rig = Rig::keyless(ctrl_alt, None, false);
    rig.down(VK_LMENU);
    assert_eq!(rig.next(), None, "an incomplete chord presses nothing");
    rig.down(VK_LCONTROL);
    assert_eq!(rig.next(), Some(HotkeyAction::Start));
    rig.up(VK_LCONTROL);
    assert_eq!(rig.next(), Some(HotkeyAction::Finish));
}
