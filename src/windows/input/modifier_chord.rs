//! What a modifier key does while dictation listens.
//!
//! The shortcut's own modifiers complete a modifier-only hold, finish it on
//! release, and press it again for a locked or toggled capture. A modifier
//! the shortcut doesn't name joining a modifier-only hold is another
//! application's chord, exactly like another key: Shift while Left Alt
//! dictates (Windows' input-language switch), or Ctrl on the way to
//! Ctrl+Alt+V (#2278).

use super::{CallbackState, send_event};
use crate::dictation::DictationFlavour;

impl CallbackState {
    /// One modifier event, once the caller has handled a masked Alt or
    /// Windows release: `vk` is the event's virtual key and `is_down` its
    /// direction.
    pub(super) fn modifier_chord(&mut self, vk: u32, is_down: bool, time: u32) {
        // A modifier the shortcut doesn't name joining a modifier-only hold
        // is a chord meant for another application. Its press takes the hold
        // away (Labs › Hold through other keys keeps it, as for keys), and
        // Alt's release then passes through unmasked, since Hex swallowed
        // nothing the app would read as Alt alone — unless Hex later swallows
        // a key of this chord (Paste Last's V re-arms the mask).
        let joining_modifier =
            is_down && self.trigger_vk.is_none() && !self.binding.names_modifier(vk);
        if joining_modifier {
            self.mask_release = false;
            if self.capture.active
                && self.capture.flavour.normal()
                && !self
                    .capture
                    .holds_through_joining_keys(crate::labs::hold_through_keys(), time)
                && let Some(action) = self.capture.interrupt()
            {
                send_event(self, action, time);
            }
        } else if self.trigger_vk.is_none()
            && self.capture.locked
            && self.capture.flavour.normal()
            && !self.dirty
            && is_down
            && self.holds(&self.binding)
            && self.side_down.together(&self.binding)
        {
            // Only a modifier-only shortcut finishes on a modifier press. A
            // key chord such as Alt+Space finishes on its key, so resting a
            // finger on Alt never ends a locked or toggled capture.
            if let Some(action) =
                self.capture
                    .press(self.double_tap_enabled, self.double_tap_only, time)
            {
                self.dirty = true;
                send_event(self, action, time);
            }
        } else if self.capture.active && self.capture.flavour.normal() && !self.holds(&self.binding)
        {
            if let Some(action) = self.capture.release(self.double_tap_enabled, time) {
                self.dirty = self.holds_any_required(&self.binding);
                send_event(self, action, time);
            }
        } else if self.trigger_vk.is_none()
            && !self.capture.active
            && !self.capture.locked
            && !self.dirty
            && is_down
            && self.holds(&self.binding)
            && self.side_down.together(&self.binding)
        {
            self.capture.claim(DictationFlavour::Normal);
            if let Some(action) =
                self.capture
                    .press(self.double_tap_enabled, self.double_tap_only, time)
            {
                send_event(self, action, time);
            }
        }
        if is_down && self.binding.masks_release() && self.holds(&self.binding) {
            self.mask_release = true;
        }
        if self.dirty && !self.holds_any_required(&self.binding) {
            self.dirty = false;
        }
    }
}
