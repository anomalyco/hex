//! Shortcut keys a Labs feature allows, and the repair that puts a saved
//! shortcut on one back while the feature is off: mouse buttons (Labs ›
//! Mouse shortcuts, macOS), Menu, Pause, or Scroll Lock (Labs › Special
//! keys alone, Linux), a navigation key alone (the same Lab,
//! Windows), the ISO section key (Labs › § key alone, Linux
//! and macOS), the microphone key (Labs › Microphone key alone, macOS),
//! shortcuts without a key (Labs › Double-tap Option, Command, or
//! Globe, macOS), and the persisted double-tap mode those need.

use super::hotkeys::{ISO_SECTION_KEY_CODE, MICROPHONE_KEY_CODE};
use super::{AppSettings, HotkeyBinding, HotkeyKey, MAX_EXTRA_DICTATION_SHORTCUTS};

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

    /// The § key, left of 1 on ISO keyboards, which Labs › § key
    /// alone lets a shortcut use on its own (issue 1050). macOS knows it by
    /// virtual key code too, because a dead-key layout labels it `^`, `´`,
    /// or `` ` `` rather than §.
    pub fn is_section(&self) -> bool {
        !self.is_mouse()
            && (Self::section_name(&self.label)
                || (cfg!(target_os = "macos") && self.code == ISO_SECTION_KEY_CODE))
    }

    /// Whether `name` names the § key: `§` or `section`.
    pub fn section_name(name: &str) -> bool {
        let name = name.trim();
        name == "§" || name.eq_ignore_ascii_case("section")
    }

    /// The microphone (dictation) key in a laptop keyboard's top row, which
    /// Labs › Microphone key alone lets a macOS shortcut use on its own. It
    /// has no character, so the code is the only name it has.
    pub fn is_microphone(&self) -> bool {
        cfg!(target_os = "macos") && !self.is_mouse() && self.code == MICROPHONE_KEY_CODE
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

    /// Whether `name` names Caps Lock: `capslock`, `caps_lock`, or
    /// `Caps Lock`.
    pub fn caps_lock_name(name: &str) -> bool {
        name.trim()
            .replace([' ', '_'], "")
            .eq_ignore_ascii_case("capslock")
    }

    /// Whether `label` names a function key (`F1`–`F20`, any case).
    pub fn is_function_name(label: &str) -> bool {
        label
            .strip_prefix(['F', 'f'])
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=20).contains(&number))
    }
}

impl HotkeyBinding {
    /// A navigation key alone (`HotkeyKey::is_navigation`).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn is_lone_navigation_key(&self) -> bool {
        self.modifiers.is_empty() && self.key.as_ref().is_some_and(HotkeyKey::is_navigation)
    }

    /// The § key alone (`HotkeyKey::is_section`).
    pub fn is_lone_section_key(&self) -> bool {
        self.modifiers.is_empty() && self.key.as_ref().is_some_and(HotkeyKey::is_section)
    }

    /// A binding whose key is the microphone key (`HotkeyKey::is_microphone`),
    /// with or without modifiers.
    pub fn uses_microphone_key(&self) -> bool {
        self.key.as_ref().is_some_and(HotkeyKey::is_microphone)
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

    /// Whether Labs › § key alone is on: on Linux and macOS a shortcut
    /// may be the ISO section key, left of 1, with no modifier.
    pub fn section_key_allowed(&self) -> bool {
        cfg!(any(target_os = "linux", target_os = "macos"))
            && self.labs.section_key_alone
            && crate::labs::Feature::SectionKeyAlone.shipped()
    }

    /// Whether Labs › Microphone key alone is on: on macOS a shortcut may be
    /// the microphone (dictation) key in a laptop keyboard's top row with no
    /// modifier.
    pub fn microphone_key_allowed(&self) -> bool {
        cfg!(target_os = "macos")
            && self.labs.microphone_key
            && crate::labs::Feature::MicrophoneKeyAlone.shipped()
    }

    /// Whether Labs › Double-tap Option, Command, or Globe is on: Double-tap
    /// shortcut mode accepts a dictation shortcut without a key (macOS).
    pub fn double_tap_modifiers_allowed(&self) -> bool {
        cfg!(target_os = "macos")
            && self.labs.double_tap_modifiers
            && crate::labs::Feature::DoubleTapModifiers.shipped()
    }

    /// Clears a double-tap-only choice a shortcut can't support: the lock
    /// is off, or a lone modifier is not allowed by its Labs switch. With
    /// the lab off, a modifier-only binding falls back exactly as a fresh
    /// load does (`shortcut_mode` stays Hold, `double_tap_lock` stays on).
    /// Each Labs › More dictation shortcuts row folds the same way.
    pub(crate) fn normalize_double_tap_settings(&mut self) {
        let modifiers_allowed = self.double_tap_modifiers_allowed();
        if !self.double_tap_lock || (self.dictation_hotkey.key.is_none() && !modifiers_allowed) {
            self.double_tap_only = false;
        }
        for row in &mut self.extra_dictation_hotkeys {
            row.mode = row
                .mode
                .for_binding(row.binding.as_ref(), modifiers_allowed);
        }
    }

    /// With Labs › § key alone off, a Linux or macOS shortcut on the
    /// ISO section key alone goes back to its default (an optional one is
    /// unbound), so the listener never registers one. The section key with a
    /// modifier is unaffected.
    pub(crate) fn repair_section_key(&mut self) {
        if self.section_key_allowed() || !cfg!(any(target_os = "linux", target_os = "macos")) {
            return;
        }
        if self.reset_shortcuts(HotkeyBinding::is_lone_section_key) {
            tracing::info!(
                "reset section-key shortcuts without a modifier: Labs › § key alone is off"
            );
        }
    }

    /// With Labs › Microphone key alone off, a macOS shortcut on the
    /// microphone key, with or without a modifier, goes back to its default
    /// (an optional one is unbound), so the listener never registers one.
    pub(crate) fn repair_microphone_key(&mut self) {
        if self.microphone_key_allowed() || !cfg!(target_os = "macos") {
            return;
        }
        if self.reset_shortcuts(HotkeyBinding::uses_microphone_key) {
            tracing::info!("reset microphone-key shortcuts: Labs › Microphone key alone is off");
        }
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

    /// Labs › More dictation shortcuts keeps at most
    /// [`MAX_EXTRA_DICTATION_SHORTCUTS`] rows, and each row's mode folds to
    /// one its binding can trigger (double-tap only needs a key, or Labs ›
    /// Double-tap Option, Command, or Globe). The rows stay saved while the Lab is
    /// off.
    pub(crate) fn repair_extra_dictation_shortcuts(&mut self) {
        self.extra_dictation_hotkeys
            .truncate(MAX_EXTRA_DICTATION_SHORTCUTS);
        let modifiers_allowed = self.double_tap_modifiers_allowed();
        for row in &mut self.extra_dictation_hotkeys {
            row.mode = row
                .mode
                .for_binding(row.binding.as_ref(), modifiers_allowed);
        }
    }

    /// Clears a saved Dictate without processing or Paste last dictation
    /// shortcut that shares the dictation shortcut's chord. Neither is
    /// filtered at runtime the way the Lab shortcuts' `active_*` accessors
    /// are ([`AppSettings::hotkey_in_use`]), so a shared chord would start
    /// both captures.
    pub(crate) fn clear_shortcuts_sharing_dictation(&mut self) {
        let dictation = self.dictation_hotkey.clone();
        clear_shared_shortcut(
            &dictation,
            &mut self.raw_dictation_hotkey,
            "Dictate without processing",
        );
        clear_shared_shortcut(
            &dictation,
            &mut self.paste_last_hotkey,
            "Paste last dictation",
        );
    }
}

/// Clears `binding` when it shares `dictation`'s chord, naming it in the log.
fn clear_shared_shortcut(
    dictation: &HotkeyBinding,
    binding: &mut Option<HotkeyBinding>,
    name: &str,
) {
    if !binding
        .as_ref()
        .is_some_and(|saved| saved.overlaps(dictation))
    {
        return;
    }
    tracing::warn!("cleared a shortcut that shares the dictation shortcut: {name}");
    *binding = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeyModifiers;

    fn menu() -> HotkeyBinding {
        HotkeyBinding {
            modifiers: Default::default(),
            key: Some(HotkeyKey::named("menu")),
        }
    }

    #[test]
    fn double_tap_only_on_a_lone_modifier_needs_the_lab() {
        let binding = |key: Option<HotkeyKey>| HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key,
        };
        let mut settings = AppSettings {
            dictation_hotkey: binding(None),
            double_tap_only: true,
            ..Default::default()
        };
        settings.normalize_double_tap_settings();
        assert!(!settings.double_tap_only);
        settings.labs.double_tap_modifiers = true;
        settings.double_tap_only = true;
        settings.normalize_double_tap_settings();
        // Modifier-only dictation exists on macOS.
        assert_eq!(
            settings.double_tap_only,
            settings.double_tap_modifiers_allowed()
        );
        // A keyed binding keeps the choice with the lab off.
        settings.labs.double_tap_modifiers = false;
        settings.dictation_hotkey = binding(Some(HotkeyKey::named("space")));
        settings.double_tap_only = true;
        settings.normalize_double_tap_settings();
        assert!(settings.double_tap_only);
    }

    #[test]
    fn turning_the_double_tap_modifiers_lab_off_falls_back_as_a_fresh_load() {
        let mut settings = AppSettings {
            dictation_hotkey: HotkeyBinding {
                modifiers: HotkeyModifiers::option(),
                key: None,
            },
            double_tap_only: true,
            ..Default::default()
        };
        settings.labs.double_tap_modifiers = true;
        settings.normalize_double_tap_settings();
        crate::labs::Feature::DoubleTapModifiers.set(&mut settings, false);
        assert!(!settings.double_tap_only);
        assert!(settings.double_tap_lock, "the lock stays on");
        assert_eq!(
            settings.shortcut_mode,
            crate::dictation::ShortcutMode::Hold,
            "the mode returns to Hold or double-tap"
        );
    }

    #[test]
    fn caps_lock_names_are_not_labs_gated_special_or_navigation_keys() {
        for name in ["capslock", "caps_lock", "Caps Lock", " caps lock "] {
            assert!(HotkeyKey::caps_lock_name(name), "{name}");
            let key = HotkeyKey::named(name);
            assert_eq!(key.label, "Caps Lock");
            assert!(!key.is_special());
            assert!(!key.is_navigation());
        }
        assert!(!HotkeyKey::caps_lock_name("scroll lock"));
    }

    #[test]
    fn special_keys_lab_off_keeps_caps_lock_and_repairs_only_windows_navigation() {
        for name in ["capslock", "insert", "home", "end", "pageup", "pagedown"] {
            let binding = HotkeyBinding {
                modifiers: Default::default(),
                key: Some(HotkeyKey::named(name)),
            };
            let mut settings = AppSettings {
                dictation_hotkey: binding.clone(),
                raw_dictation_hotkey: Some(binding.clone()),
                ..Default::default()
            };
            settings.labs.standalone_special_keys = true;
            settings.repair_special_keys();
            assert_eq!(settings.dictation_hotkey, binding);
            crate::labs::Feature::StandaloneSpecialKeys.set(&mut settings, false);
            settings.repair_special_keys();
            if cfg!(target_os = "windows") && name != "capslock" {
                assert_eq!(
                    settings.dictation_hotkey,
                    HotkeyBinding::default(),
                    "{name}"
                );
                assert_eq!(settings.raw_dictation_hotkey, None, "{name}");
            } else {
                assert_eq!(settings.dictation_hotkey, binding, "{name}");
                assert_eq!(settings.raw_dictation_hotkey, Some(binding), "{name}");
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn caps_lock_survives_save_and_load_with_the_special_keys_lab_off() {
        let sequence = crate::app_settings::SETTINGS_TEMP_SEQUENCE
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "hex-caps-lock-settings-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        for name in ["capslock", "insert", "home", "end", "pageup", "pagedown"] {
            let binding = HotkeyBinding {
                modifiers: Default::default(),
                key: Some(HotkeyKey::named(name)),
            };
            let settings = AppSettings {
                dictation_hotkey: binding.clone(),
                ..Default::default()
            };
            settings.write_to(&path).unwrap();
            let loaded = AppSettings::load_from(&path).unwrap();
            assert!(!loaded.labs.standalone_special_keys);
            if name == "capslock" {
                assert_eq!(loaded.dictation_hotkey, binding);
            } else {
                assert_eq!(loaded.dictation_hotkey, HotkeyBinding::default(), "{name}");
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
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

    /// Issue 1050: off, a saved § shortcut without a modifier goes back to
    /// its default on Linux and macOS; § with a modifier, and § with the
    /// toggle on, stay.
    #[test]
    fn a_lone_section_key_goes_back_to_its_default_while_its_labs_toggle_is_off() {
        let section = HotkeyBinding {
            modifiers: Default::default(),
            key: Some(HotkeyKey::named("section")),
        };
        assert!(section.is_lone_section_key());
        assert_eq!(HotkeyKey::named("section").label, "§");
        assert!(HotkeyKey::named("§").is_section());
        assert!(!HotkeyKey::named("s").is_section());
        let with_control = HotkeyBinding {
            modifiers: super::super::HotkeyModifiers::control_option(),
            key: Some(HotkeyKey::named("§")),
        };
        assert!(!with_control.is_lone_section_key());
        let mut settings = AppSettings {
            dictation_hotkey: section.clone(),
            raw_dictation_hotkey: Some(with_control.clone()),
            ..Default::default()
        };
        let mut on = settings.clone();
        on.labs.section_key_alone = true;
        on.repair_section_key();
        assert_eq!(on.dictation_hotkey, section);
        settings.repair_section_key();
        assert_eq!(settings.raw_dictation_hotkey, Some(with_control));
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(on.section_key_allowed());
            assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        } else {
            assert!(!on.section_key_allowed());
            assert_eq!(settings.dictation_hotkey, section);
        }
    }

    /// macOS labels the ISO section key by what it types: a dead-key layout
    /// gives `^` for key code 10, and the section-key rules must still see
    /// it. Elsewhere only the label names the section key.
    #[test]
    fn a_dead_key_label_is_the_section_key_only_on_macos() {
        let dead_key = HotkeyBinding {
            modifiers: Default::default(),
            key: Some(HotkeyKey {
                code: ISO_SECTION_KEY_CODE,
                label: "^".into(),
            }),
        };
        assert_eq!(dead_key.is_lone_section_key(), cfg!(target_os = "macos"));
        let mut settings = AppSettings {
            dictation_hotkey: dead_key.clone(),
            ..Default::default()
        };
        settings.repair_section_key();
        if cfg!(target_os = "macos") {
            assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        } else {
            assert_eq!(settings.dictation_hotkey, dead_key);
        }
        settings.labs.section_key_alone = true;
        settings.dictation_hotkey = dead_key.clone();
        settings.repair_section_key();
        assert_eq!(settings.dictation_hotkey, dead_key);
    }

    /// Issue 181: off, a saved microphone-key shortcut goes back to its
    /// default on macOS, with or without a modifier; with the toggle on,
    /// either stays.
    #[test]
    fn a_microphone_key_shortcut_goes_back_to_its_default_while_its_labs_toggle_is_off() {
        let microphone = HotkeyBinding {
            modifiers: Default::default(),
            key: Some(HotkeyKey {
                code: MICROPHONE_KEY_CODE,
                label: "Mic".into(),
            }),
        };
        assert_eq!(microphone.uses_microphone_key(), cfg!(target_os = "macos"));
        assert!(
            !HotkeyKey::named("f5").is_microphone(),
            "F5 is not the microphone key"
        );
        let with_control = HotkeyBinding {
            modifiers: super::super::HotkeyModifiers::control_option(),
            key: Some(HotkeyKey {
                code: MICROPHONE_KEY_CODE,
                label: "Mic".into(),
            }),
        };
        assert_eq!(
            with_control.uses_microphone_key(),
            cfg!(target_os = "macos")
        );
        let mut settings = AppSettings {
            dictation_hotkey: microphone.clone(),
            raw_dictation_hotkey: Some(with_control.clone()),
            cancel_hotkey: microphone.clone(),
            screenshots: crate::app_settings::ScreenshotSettings {
                hotkey: with_control.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut on = settings.clone();
        on.labs.microphone_key = true;
        on.repair_microphone_key();
        assert_eq!(on.dictation_hotkey, microphone);
        assert_eq!(on.raw_dictation_hotkey, Some(with_control.clone()));
        assert_eq!(on.cancel_hotkey, microphone);
        assert_eq!(on.screenshots.hotkey, with_control.clone());
        settings.repair_microphone_key();
        if cfg!(target_os = "macos") {
            assert!(on.microphone_key_allowed());
            assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
            assert_eq!(settings.raw_dictation_hotkey, None);
            assert_eq!(settings.cancel_hotkey, HotkeyBinding::cancel_default());
            assert_eq!(
                settings.screenshots.hotkey,
                HotkeyBinding::screenshot_default()
            );
        } else {
            assert!(!on.microphone_key_allowed());
            assert_eq!(settings.dictation_hotkey, microphone);
            assert_eq!(settings.cancel_hotkey, microphone);
            assert_eq!(settings.screenshots.hotkey, with_control);
            assert_eq!(settings.raw_dictation_hotkey, Some(with_control));
        }
    }
}
