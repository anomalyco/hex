//! The modifier keys the Windows hook tracks, and whether a chord's
//! modifiers alone are down.

use std::sync::atomic::{AtomicU8, Ordering};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use crate::app_settings::ModifierSide;

#[derive(Clone, Copy, Default)]
pub(super) struct ModifierState {
    pub(super) left_control: bool,
    pub(super) right_control: bool,
    pub(super) left_windows: bool,
    pub(super) right_windows: bool,
    pub(super) left_alt: bool,
    pub(super) right_alt: bool,
    pub(super) left_shift: bool,
    pub(super) right_shift: bool,
    /// This view dropped AltGr's synthesized Left Ctrl (`without_altgr`).
    pub(super) altgr: bool,
}

impl ModifierState {
    pub(super) fn read(key_down: impl Fn(u16) -> bool) -> Self {
        Self {
            left_control: key_down(VK_LCONTROL),
            right_control: key_down(VK_RCONTROL),
            left_windows: key_down(VK_LWIN),
            right_windows: key_down(VK_RWIN),
            left_alt: key_down(VK_LMENU),
            right_alt: key_down(VK_RMENU),
            left_shift: key_down(VK_LSHIFT),
            right_shift: key_down(VK_RSHIFT),
            altgr: false,
        }
    }

    /// The held modifiers as the keys are now, with AltGr's synthesized
    /// Left Ctrl dropped.
    pub(super) fn current() -> Self {
        Self::read(key_down).without_altgr(altgr_only())
    }

    /// Whether `key` is one of the modifier keys the hook tracks.
    pub(super) fn is_modifier(key: u32) -> bool {
        matches!(
            key as u16,
            VK_LCONTROL
                | VK_RCONTROL
                | VK_CONTROL
                | VK_LWIN
                | VK_RWIN
                | VK_LMENU
                | VK_RMENU
                | VK_MENU
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_SHIFT
        )
    }

    pub(super) fn update(&mut self, key: u32, is_down: bool) -> bool {
        match key as u16 {
            VK_LCONTROL => self.left_control = is_down,
            VK_RCONTROL => self.right_control = is_down,
            VK_CONTROL => {
                self.left_control = is_down;
                self.right_control = false;
            }
            VK_LWIN => self.left_windows = is_down,
            VK_RWIN => self.right_windows = is_down,
            VK_LMENU => self.left_alt = is_down,
            VK_RMENU => self.right_alt = is_down,
            VK_MENU => {
                self.left_alt = is_down;
                self.right_alt = false;
            }
            VK_LSHIFT => self.left_shift = is_down,
            VK_RSHIFT => self.right_shift = is_down,
            VK_SHIFT => {
                self.left_shift = is_down;
                self.right_shift = false;
            }
            _ => return false,
        }
        true
    }

    /// Exactly the chord's modifier keys are down: a side alone, both sides
    /// only for `Both`, and either side (or both) for `Either`.
    pub(super) fn matches(&self, binding: &super::WindowsHotkey) -> bool {
        side_matches(binding.control, self.left_control, self.right_control)
            && side_matches(binding.windows, self.left_windows, self.right_windows)
            && side_matches(binding.alt, self.left_alt, self.right_alt)
            && side_matches(binding.shift, self.left_shift, self.right_shift)
    }

    pub(super) fn any_required_down(&self, binding: &super::WindowsHotkey) -> bool {
        (binding.control.is_some() && (self.left_control || self.right_control))
            || (binding.windows.is_some() && (self.left_windows || self.right_windows))
            || (binding.alt.is_some() && (self.left_alt || self.right_alt))
            || (binding.shift.is_some() && (self.left_shift || self.right_shift))
    }

    /// The held modifiers with AltGr's synthesized Left Ctrl dropped, which
    /// the async key state reports as a real Ctrl (`note_left_control`).
    pub(super) fn without_altgr(mut self, altgr_only: bool) -> Self {
        if altgr_only {
            self.left_control = false;
        }
        self.altgr = altgr_only;
        self
    }

    /// Whether the chord names Ctrl but AltGr's synthesized Left Ctrl is the
    /// only one down, and its key types a character: the layout types that
    /// character instead of pressing the chord (#1964).
    fn altgr_types_a_character(&self, binding: &super::WindowsHotkey) -> bool {
        self.altgr
            && !self.right_control
            && binding.control.is_some()
            && binding.key.as_deref().is_some_and(types_character)
    }

    /// Whether the chord's modifiers are down. `self` is the masked view
    /// (`read.without_altgr`); `raw` is the same read with AltGr's Ctrl still
    /// counted, so a chord with an F-key or navigation key sees what it
    /// always has (AltGr+F9 is Ctrl+Alt+F9, never Alt+F9) while a
    /// modifier-only chord also sees the masked view, so `right-option` fires
    /// on AltGr beside the `control+option` a recording made of AltGr before
    /// it was ignored. A chord naming Ctrl with a key that types a character
    /// never matches on AltGr: the layout types that character, and the key
    /// must reach the app (#1964).
    pub(super) fn holds(&self, binding: &super::WindowsHotkey, raw: &Self) -> bool {
        if binding.key.is_some() {
            if self.altgr_types_a_character(binding) {
                return false;
            }
            return raw.matches(binding);
        }
        self.matches(binding) || raw.matches(binding)
    }

    /// [`ModifierState::holds`] for "a modifier the chord requires is still
    /// down": one that a discarded chord cannot re-press. AltGr's Ctrl is not
    /// one for a chord it never presses (`altgr_types_a_character`), so a
    /// discarded typing chord cannot stay dirty while AltGr is held.
    pub(super) fn holds_any_required(&self, binding: &super::WindowsHotkey, raw: &Self) -> bool {
        if binding.key.is_some() {
            if self.altgr_types_a_character(binding) {
                return false;
            }
            return raw.any_required_down(binding);
        }
        self.any_required_down(binding) || raw.any_required_down(binding)
    }
}

/// Whether a shortcut's key types a character on the active layout: a letter,
/// a digit, or one of the punctuation keys the input layer names `oem …`.
/// AltGr with one of these types its character instead of pressing a chord.
fn types_character(key: &str) -> bool {
    let name = key.trim();
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        // A key named by the character it types (`v`, `7`, `;`, `§`).
        (Some(_), None) => true,
        _ => name.to_ascii_lowercase().starts_with("oem "),
    }
}

fn side_matches(side: Option<ModifierSide>, left: bool, right: bool) -> bool {
    match side {
        None => !left && !right,
        Some(ModifierSide::Either) => left || right,
        Some(ModifierSide::Left) => left && !right,
        Some(ModifierSide::Right) => right && !left,
        Some(ModifierSide::Both) => left && right,
    }
}

pub(super) fn key_down(key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
}

/// AltGr's mark on its synthesized Left Ctrl's scan code: 0x21D where a
/// real Left Ctrl's is 0x1D. Windows' keyboard layouts send it just before
/// the Right Alt, and `GetKeyState` and `GetAsyncKeyState` both report it as
/// a real Ctrl. Undocumented, as every implementation that needs it notes.
const ALTGR_SCAN_CODE: u32 = 0x200;

/// Which Left Ctrl a hook event is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeftControl {
    /// Not a Left Ctrl.
    Other,
    /// A real Left Ctrl press.
    Real,
    /// AltGr's synthesized Left Ctrl.
    AltGr,
}

impl LeftControl {
    pub(super) fn of(vk: u32, scan_code: u32) -> Self {
        if vk != u32::from(VK_LCONTROL) {
            Self::Other
        } else if scan_code & ALTGR_SCAN_CODE != 0 {
            Self::AltGr
        } else {
            Self::Real
        }
    }
}

/// The Left Ctrls the process's hooks have seen down: `ALTGR_DOWN` for
/// AltGr's synthesized one, `REAL_DOWN` for a real one. The listener's hook
/// and the shortcut recorder's run separately (the recorder's even runs with
/// no listener), so the state is shared, and one atomic means no reader sees
/// half an update.
static LEFT_CONTROL: AtomicU8 = AtomicU8::new(0);

const ALTGR_DOWN: u8 = 1;
const REAL_DOWN: u8 = 2;

/// Records a Left Ctrl event. Every keyboard hook calls this first, before
/// anything else acts on the event.
pub(super) fn note_left_control(control: LeftControl, is_down: bool) {
    // The key state is read before this event is applied, so it settles a
    // release that landed on another desktop and cannot undo this event.
    let left_control_down = key_down(VK_LCONTROL);
    let mut bits = LEFT_CONTROL.load(Ordering::Relaxed);
    loop {
        let next = next_left_control(bits, control, is_down, left_control_down);
        match LEFT_CONTROL.compare_exchange(bits, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(current) => bits = current,
        }
    }
}

/// The Left Ctrl bits after one hook event. A key state that says Left Ctrl
/// is up vouches for neither bit: a real Left Ctrl never reports up, and
/// AltGr's is down only while Right Alt is, so both bits are a release that
/// landed on another desktop and never reached a hook. Settling first means
/// the event's own bit is applied on top and cannot be cleared by a read
/// that lags it.
fn next_left_control(bits: u8, control: LeftControl, is_down: bool, left_control_down: bool) -> u8 {
    let bits = if left_control_down { bits } else { 0 };
    match control {
        LeftControl::Other => bits,
        LeftControl::Real => {
            if is_down {
                bits | REAL_DOWN
            } else {
                bits & !REAL_DOWN
            }
        }
        // AltGr's Ctrl-up keeps its bit: the key state still reports that
        // Ctrl down while this event is handled, so clearing it here would
        // let a recorder poll in the gap count it as a real Ctrl. The next
        // event's settle clears it, once the key state says up.
        LeftControl::AltGr if is_down => bits | ALTGR_DOWN,
        LeftControl::AltGr => bits,
    }
}

/// Whether the Left Ctrl the async key state reports is AltGr's alone: with
/// a real Left Ctrl down as well, it is a Ctrl after all.
pub(super) fn altgr_only() -> bool {
    LEFT_CONTROL.load(Ordering::Relaxed) == ALTGR_DOWN
}

/// The sided modifier keys, left then right for each modifier.
const SIDE_KEYS: [u16; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LWIN,
    VK_RWIN,
    VK_LMENU,
    VK_RMENU,
    VK_LSHIFT,
    VK_RSHIFT,
];

/// When each sided modifier key went down, in hook time (ms), for chords
/// that hold both sides of one modifier.
#[derive(Default)]
pub(super) struct SideDownTimes([Option<u32>; 8]);

impl SideDownTimes {
    pub(super) fn update(&mut self, vk: u32, is_down: bool, time: u32) {
        // A side-less modifier code reads as the left key, as `ModifierState`.
        let vk = match vk as u16 {
            VK_CONTROL => VK_LCONTROL,
            VK_MENU => VK_LMENU,
            VK_SHIFT => VK_LSHIFT,
            vk => vk,
        };
        if let Some(index) = SIDE_KEYS.iter().position(|&key| key == vk) {
            if is_down {
                // Auto-repeat keeps the first press.
                self.0[index].get_or_insert(time);
            } else {
                self.0[index] = None;
            }
        }
    }

    /// Forgets sides the asynchronous key state now reports up, after a
    /// release the hook never saw: a fresh press must not pair with a stale
    /// one in `together`.
    pub(super) fn forget_released(&mut self, key_held: impl Fn(u16) -> bool) {
        for (index, &key) in SIDE_KEYS.iter().enumerate() {
            if self.0[index].is_some() && !key_held(key) {
                self.0[index] = None;
            }
        }
    }

    /// Both keys of each `Both` modifier went down within
    /// `BOTH_SIDES_WINDOW`, in either order; true for every other chord.
    pub(super) fn together(&self, binding: &super::WindowsHotkey) -> bool {
        let window = crate::dictation::BOTH_SIDES_WINDOW.as_millis() as u32;
        [binding.control, binding.windows, binding.alt, binding.shift]
            .into_iter()
            .enumerate()
            .filter(|(_, side)| *side == Some(ModifierSide::Both))
            .all(
                |(index, _)| match (self.0[2 * index], self.0[2 * index + 1]) {
                    (Some(left), Some(right)) => {
                        left.wrapping_sub(right).min(right.wrapping_sub(left)) <= window
                    }
                    _ => false,
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_control_bits_follow_the_hooks_and_heal_a_missed_release() {
        // AltGr press: its synthesized Ctrl, before any real one.
        let pressed = next_left_control(0, LeftControl::AltGr, true, false);
        assert_eq!(pressed, ALTGR_DOWN);

        // A real Left Ctrl held first, then AltGr: both are down, so it is
        // not AltGr's alone, and `right-option` must not fire.
        let real = next_left_control(0, LeftControl::Real, true, false);
        assert_eq!(real, REAL_DOWN);
        let both = next_left_control(real, LeftControl::AltGr, true, true);
        assert_eq!(both, REAL_DOWN | ALTGR_DOWN);
        assert_ne!(both, ALTGR_DOWN);

        // A real Ctrl release that landed on another desktop left
        // `REAL_DOWN` behind; the next AltGr press sees Left Ctrl up and
        // settles it, so Right Alt alone works again.
        let healed = next_left_control(REAL_DOWN, LeftControl::AltGr, true, false);
        assert_eq!(healed, ALTGR_DOWN);

        // Both hooks note the same event, so the second changes nothing.
        assert_eq!(
            next_left_control(healed, LeftControl::AltGr, true, true),
            healed
        );
        assert_eq!(
            next_left_control(both, LeftControl::AltGr, true, true),
            both
        );

        // AltGr released: its bit stays until a later event settles it, so a
        // recorder poll in the gap (Left Ctrl still reported down) cannot
        // count that Ctrl as a real one. The next event's settle clears it.
        assert_eq!(
            next_left_control(ALTGR_DOWN, LeftControl::AltGr, false, true),
            ALTGR_DOWN
        );
        assert_eq!(
            next_left_control(ALTGR_DOWN, LeftControl::Other, true, false),
            0
        );
        // A release whose key state already reads up settles at once.
        assert_eq!(
            next_left_control(ALTGR_DOWN, LeftControl::AltGr, false, false),
            0
        );
        // A real Left Ctrl pressed right after AltGr release: its own settle
        // reads Left Ctrl up (the event is not applied yet), clears AltGr,
        // and then sets REAL, so the real Ctrl is never masked.
        assert_eq!(
            next_left_control(ALTGR_DOWN, LeftControl::Real, true, false),
            REAL_DOWN
        );
        // Any other event settles a stale bit too, and keeps a live one.
        assert_eq!(
            next_left_control(REAL_DOWN, LeftControl::Other, true, false),
            0
        );
        assert_eq!(
            next_left_control(REAL_DOWN, LeftControl::Other, true, true),
            REAL_DOWN
        );
    }

    #[test]
    fn a_lost_release_forgets_sides_now_up() {
        let both_alt = super::super::WindowsHotkey {
            control: None,
            windows: None,
            alt: Some(ModifierSide::Both),
            shift: None,
            key: None,
        };
        let mut times = SideDownTimes::default();
        times.update(u32::from(VK_LMENU), true, 100);
        times.update(u32::from(VK_RMENU), true, 120);
        assert!(times.together(&both_alt));

        // The right side's release was lost: reading it up forgets it, and a
        // fresh press on that side no longer pairs with the stale one.
        times.forget_released(|key| key == VK_LMENU);
        assert!(!times.together(&both_alt));
        times.update(u32::from(VK_LMENU), true, 130);
        assert!(!times.together(&both_alt), "the left side is still down");
        times.update(u32::from(VK_RMENU), true, 140);
        assert!(times.together(&both_alt), "a new pair within the window");
    }

    #[test]
    fn a_typing_key_is_a_letter_a_digit_or_punctuation() {
        for name in [
            "a",
            "v",
            "7",
            ";",
            "§",
            "oem 1",
            "oem plus",
            "oem minus",
            "oem 102",
        ] {
            assert!(types_character(name), "{name}");
        }
        for name in [
            "space", "enter", "escape", "f9", "num 1", "mouse1", "pageup",
        ] {
            assert!(!types_character(name), "{name}");
        }
    }
}
