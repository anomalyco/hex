//! Shortcut keys a Labs feature allows, and the repair that puts a saved
//! shortcut on one back while the feature is off: mouse buttons (Labs ›
//! Mouse shortcuts, macOS) and Menu, Pause, or Scroll Lock (Labs › Special
//! keys alone, Linux).

use super::{AppSettings, HotkeyBinding, HotkeyKey};

impl HotkeyKey {
    /// Menu, Pause, or Scroll Lock: keys that type nothing, which Labs ›
    /// Special keys alone lets a Linux shortcut use (issue 767).
    pub fn is_special(&self) -> bool {
        !self.is_mouse() && Self::special_name(&self.label)
    }

    /// Whether `name` names Menu, Pause, or Scroll Lock.
    pub fn special_name(name: &str) -> bool {
        is_scroll_lock(name)
            || matches!(name.trim().to_ascii_lowercase().as_str(), "menu" | "pause")
    }

    /// Insert, Home, End, Page Up, or Page Down: keys that type nothing,
    /// which Labs › Special keys alone lets a Windows shortcut use alone
    /// (issue 794).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn is_navigation(&self) -> bool {
        !self.is_mouse() && Self::navigation_name(&self.label)
    }

    /// Whether `name` names Insert, Home, End, Page Up, or Page Down.
    pub fn navigation_name(name: &str) -> bool {
        matches!(
            name.trim()
                .to_ascii_lowercase()
                .replace([' ', '_'], "")
                .as_str(),
            "insert" | "home" | "end" | "pageup" | "pagedown"
        )
    }
}

impl HotkeyBinding {
    /// A navigation key alone (`HotkeyKey::is_navigation`).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn is_lone_navigation_key(&self) -> bool {
        self.modifiers.is_empty() && self.key.as_ref().is_some_and(HotkeyKey::is_navigation)
    }
}

/// `scrolllock`, `scroll_lock`, or `Scroll Lock`.
pub(super) fn is_scroll_lock(name: &str) -> bool {
    name.trim()
        .replace([' ', '_'], "")
        .eq_ignore_ascii_case("scrolllock")
}

impl AppSettings {
    /// Whether Labs › Special keys alone is on: on Linux a shortcut may use
    /// Menu, Pause, or Scroll Lock; on Windows, which takes those anyway,
    /// a navigation key alone.
    pub fn special_keys_allowed(&self) -> bool {
        cfg!(any(target_os = "linux", target_os = "windows"))
            && self.labs.standalone_special_keys
            && crate::labs::Feature::StandaloneSpecialKeys.shipped()
    }

    /// With Labs › Mouse shortcuts off, a mouse-button shortcut (issue 209)
    /// goes back to its default and the window notes it once.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn repair_mouse_shortcuts(&mut self) {
        if self.labs.mouse_shortcuts {
            return;
        }
        if self.reset_shortcuts(|binding| binding.key.as_ref().is_some_and(HotkeyKey::is_mouse)) {
            self.mouse_shortcut_notice = true;
        }
        if self.mouse_shortcut_notice {
            tracing::info!("reset mouse-button shortcuts: Labs › Mouse shortcuts is off");
        }
    }

    /// With Labs › Special keys alone off, a Linux shortcut on Menu, Pause,
    /// or Scroll Lock goes back to its default (an optional one is unbound),
    /// so the listener never registers one. Other OSes keep theirs.
    pub(crate) fn repair_special_keys(&mut self) {
        if self.special_keys_allowed() {
            return;
        }
        if cfg!(target_os = "linux")
            && self
                .reset_shortcuts(|binding| binding.key.as_ref().is_some_and(HotkeyKey::is_special))
        {
            tracing::info!(
                "reset Menu, Pause, and Scroll Lock shortcuts: Labs › Special keys alone is off"
            );
        }
        // Windows: a navigation key alone needs the toggle.
        if cfg!(target_os = "windows")
            && self.reset_shortcuts(HotkeyBinding::is_lone_navigation_key)
        {
            tracing::info!(
                "reset navigation-key shortcuts without a modifier: Labs › Special keys alone is off"
            );
        }
    }

    /// Puts back every shortcut `matches`: dictation, Paste last dictation,
    /// and Annotate to their defaults, the optional ones unbound. Returns
    /// whether any changed.
    fn reset_shortcuts(&mut self, matches: impl Fn(&HotkeyBinding) -> bool) -> bool {
        let mut reset = false;
        if matches(&self.dictation_hotkey) {
            self.dictation_hotkey = HotkeyBinding::default();
            reset = true;
        }
        if self.paste_last_hotkey.as_ref().is_some_and(&matches) {
            self.paste_last_hotkey = Some(HotkeyBinding::paste_last_default());
            reset = true;
        }
        if self.raw_dictation_hotkey.as_ref().is_some_and(&matches) {
            self.raw_dictation_hotkey = None;
            reset = true;
        }
        if self.second_dictation_hotkey.as_ref().is_some_and(&matches) {
            self.second_dictation_hotkey = None;
            reset = true;
        }
        #[cfg(hex_capture)]
        if matches(&self.capture.hotkey) {
            self.capture.hotkey = HotkeyBinding::capture_default();
            reset = true;
        }
        reset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> HotkeyBinding {
        HotkeyBinding {
            modifiers: Default::default(),
            key: Some(HotkeyKey::named("menu")),
        }
    }

    #[test]
    fn menu_pause_and_scroll_lock_are_the_special_keys() {
        for name in ["menu", "Pause", "scrolllock", "scroll_lock", "Scroll Lock"] {
            assert!(HotkeyKey::special_name(name), "{name}");
            assert!(HotkeyKey::named(name).is_special(), "{name}");
        }
        assert_eq!(HotkeyKey::named("scrolllock").label, "Scroll Lock");
        for name in ["f9", "space", "m", "mouse4"] {
            assert!(!HotkeyKey::named(name).is_special(), "{name}");
        }
    }

    /// Off, a saved Menu shortcut goes back to its default on Linux (and
    /// stays elsewhere, where the toggle doesn't exist); on, it stays.
    #[test]
    fn special_key_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off() {
        let mut settings = AppSettings {
            dictation_hotkey: menu(),
            raw_dictation_hotkey: Some(menu()),
            ..Default::default()
        };
        let mut on = settings.clone();
        on.labs.standalone_special_keys = true;
        on.repair_special_keys();
        assert_eq!(on.dictation_hotkey, menu());
        assert_eq!(
            on.special_keys_allowed(),
            cfg!(any(target_os = "linux", target_os = "windows"))
        );
        settings.repair_special_keys();
        if cfg!(target_os = "linux") {
            assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
            assert_eq!(settings.raw_dictation_hotkey, None);
        } else {
            assert_eq!(settings.dictation_hotkey, menu());
        }
    }
}
