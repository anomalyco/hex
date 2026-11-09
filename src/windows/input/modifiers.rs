//! The modifier keys the Windows hook tracks, and whether a chord's
//! modifiers alone are down.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use crate::app_settings::ModifierSide;

#[derive(Default)]
pub(super) struct ModifierState {
    pub(super) left_control: bool,
    pub(super) right_control: bool,
    pub(super) left_windows: bool,
    pub(super) right_windows: bool,
    pub(super) left_alt: bool,
    pub(super) right_alt: bool,
    pub(super) left_shift: bool,
    pub(super) right_shift: bool,
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
        }
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
