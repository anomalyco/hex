//! Labs › More dictation shortcuts (macOS): up to four extra dictation
//! shortcuts, each with its own binding and activation mode. Off, the saved
//! rows stay as they are but no shortcut registers or matches.

use super::*;

/// The most extra dictation shortcuts one settings file holds.
pub const MAX_EXTRA_DICTATION_SHORTCUTS: usize = 4;

/// How an extra dictation shortcut triggers, in place of the saved
/// `shortcut_mode`, `double_tap_lock`, and `double_tap_only` the dictation
/// shortcut reads.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtraDictationMode {
    /// Hold to talk, or double-tap to talk hands-free.
    #[default]
    HoldOrDoubleTap,
    /// Hold to talk, let go to paste.
    Hold,
    /// Tap to talk hands-free, tap again to paste.
    Tap,
    /// Double-tap to talk hands-free, tap again to paste. Needs a binding
    /// with a key, or with Labs › Double-tap on a lone modifier, one without
    /// a key; otherwise it folds to [`Self::HoldOrDoubleTap`].
    DoubleTap,
}

impl ExtraDictationMode {
    /// Every choice, in the order Settings shows them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const ALL: [Self; 4] = [
        Self::HoldOrDoubleTap,
        Self::Hold,
        Self::Tap,
        Self::DoubleTap,
    ];

    /// The shortcut machine's switches: toggle, double-tap enabled, and
    /// double-tap only, exactly as `Trigger::apply` saves the dictation
    /// shortcut's (`rune_ui::shortcut_mode`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub const fn switches(self) -> (bool, bool, bool) {
        match self {
            Self::HoldOrDoubleTap => (false, true, false),
            Self::Hold => (false, false, false),
            Self::Tap => (true, true, false),
            Self::DoubleTap => (false, true, true),
        }
    }

    /// `self`, folded to a mode its binding can trigger: double-tap only
    /// needs a shortcut with a key or, with `modifiers_allowed` (Labs ›
    /// Double-tap on a lone modifier), a shortcut without one, as
    /// `normalize_double_tap_settings` folds the dictation shortcut's.
    pub fn for_binding(self, binding: Option<&HotkeyBinding>, modifiers_allowed: bool) -> Self {
        if self == Self::DoubleTap
            && !modifiers_allowed
            && binding.is_none_or(|binding| binding.key.is_none())
        {
            Self::HoldOrDoubleTap
        } else {
            self
        }
    }
}

/// One extra dictation shortcut: its binding and how it triggers. Unbound by
/// default and ignored while its Labs toggle is off (macOS).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ExtraDictationHotkey {
    /// The shortcut; `None` is unbound.
    pub binding: Option<HotkeyBinding>,
    pub mode: ExtraDictationMode,
}

impl AppSettings {
    /// Row `index`'s binding while it takes effect: macOS, Labs › More
    /// dictation shortcuts on, bound, and sharing no key with another active
    /// shortcut. Otherwise Hex registers nothing for it.
    pub fn active_extra_dictation_hotkey(&self, index: usize) -> Option<&HotkeyBinding> {
        if !cfg!(target_os = "macos") || !self.labs.more_dictation_shortcuts {
            return None;
        }
        self.extra_dictation_hotkeys
            .get(index)?
            .binding
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::ExtraDictation(index), binding))
    }

    /// Row `index`'s activation mode, folded to one its binding can trigger;
    /// a row that isn't there reads as the default.
    pub fn extra_dictation_mode(&self, index: usize) -> ExtraDictationMode {
        self.extra_dictation_hotkeys
            .get(index)
            .map_or_else(ExtraDictationMode::default, |row| {
                row.mode
                    .for_binding(row.binding.as_ref(), self.double_tap_modifiers_allowed())
            })
    }

    /// Adds an unbound row; `None` while there are already
    /// [`MAX_EXTRA_DICTATION_SHORTCUTS`].
    pub fn add_extra_dictation_hotkey(&mut self) -> Option<usize> {
        if self.extra_dictation_hotkeys.len() >= MAX_EXTRA_DICTATION_SHORTCUTS {
            return None;
        }
        self.extra_dictation_hotkeys
            .push(ExtraDictationHotkey::default());
        Some(self.extra_dictation_hotkeys.len() - 1)
    }

    /// Removes row `index`; false when it isn't there.
    pub fn remove_extra_dictation_hotkey(&mut self, index: usize) -> bool {
        if index >= self.extra_dictation_hotkeys.len() {
            return false;
        }
        self.extra_dictation_hotkeys.remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyed() -> HotkeyBinding {
        HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Either),
                option: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(HotkeyKey::named("f5")),
        }
    }

    fn modifier_only() -> HotkeyBinding {
        HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: None,
        }
    }

    /// Each mode saves the switches `Trigger::apply` saves for the same
    /// choice of the dictation shortcut's mode.
    #[test]
    fn every_mode_saves_its_trigger_switches() {
        assert_eq!(
            ExtraDictationMode::HoldOrDoubleTap.switches(),
            (false, true, false)
        );
        assert_eq!(ExtraDictationMode::Hold.switches(), (false, false, false));
        assert_eq!(ExtraDictationMode::Tap.switches(), (true, true, false));
        assert_eq!(
            ExtraDictationMode::DoubleTap.switches(),
            (false, true, true)
        );
        assert_eq!(
            ExtraDictationMode::default(),
            ExtraDictationMode::HoldOrDoubleTap
        );
        assert_eq!(ExtraDictationMode::ALL.len(), 4);
    }

    /// Double-tap only needs a binding with a key, or Labs › Double-tap on a
    /// lone modifier; without either it folds to hold-or-double-tap, as the
    /// dictation shortcut's mode does.
    #[test]
    fn double_tap_folds_away_without_a_key_or_the_lab() {
        for mode in ExtraDictationMode::ALL {
            assert_eq!(mode.for_binding(Some(&keyed()), false), mode);
            assert_eq!(mode.for_binding(Some(&keyed()), true), mode);
        }
        assert_eq!(
            ExtraDictationMode::DoubleTap.for_binding(Some(&modifier_only()), false),
            ExtraDictationMode::HoldOrDoubleTap
        );
        assert_eq!(
            ExtraDictationMode::DoubleTap.for_binding(None, false),
            ExtraDictationMode::HoldOrDoubleTap
        );
        // Labs › Double-tap on a lone modifier keeps Double-tap on a shortcut
        // without a key, as it does the dictation shortcut's; an unbound row
        // keeps the choice too, and folds it once it gets a lone modifier
        // with the lab off.
        assert_eq!(
            ExtraDictationMode::DoubleTap.for_binding(Some(&modifier_only()), true),
            ExtraDictationMode::DoubleTap
        );
        assert_eq!(
            ExtraDictationMode::DoubleTap.for_binding(None, true),
            ExtraDictationMode::DoubleTap
        );
    }

    /// The row reads its mode the way the dictation shortcut does: a lone
    /// modifier keeps Double-tap exactly while the lab allows it.
    #[test]
    fn a_rows_double_tap_follows_the_lone_modifier_lab() {
        let mut settings = AppSettings::default();
        let index = settings.add_extra_dictation_hotkey().unwrap();
        settings.extra_dictation_hotkeys[index] = ExtraDictationHotkey {
            binding: Some(modifier_only()),
            mode: ExtraDictationMode::DoubleTap,
        };
        assert_eq!(
            settings.extra_dictation_mode(index),
            if settings.double_tap_modifiers_allowed() {
                ExtraDictationMode::DoubleTap
            } else {
                ExtraDictationMode::HoldOrDoubleTap
            }
        );
        // Loading folds the same way, as it folds a saved double-tap-only
        // start.
        settings.repair_extra_dictation_shortcuts();
        assert_eq!(
            settings.extra_dictation_hotkeys[index].mode,
            ExtraDictationMode::DoubleTap.for_binding(
                Some(&modifier_only()),
                settings.double_tap_modifiers_allowed()
            )
        );
        // Turning the lab off folds the saved mode; a keyed row keeps its
        // own.
        settings.extra_dictation_hotkeys[index].mode = ExtraDictationMode::DoubleTap;
        settings.labs.double_tap_modifiers = false;
        settings.normalize_double_tap_settings();
        assert_eq!(
            settings.extra_dictation_hotkeys[index].mode,
            ExtraDictationMode::HoldOrDoubleTap
        );
        settings.extra_dictation_hotkeys[index].binding = Some(keyed());
        settings.extra_dictation_hotkeys[index].mode = ExtraDictationMode::DoubleTap;
        settings.normalize_double_tap_settings();
        assert_eq!(
            settings.extra_dictation_hotkeys[index].mode,
            ExtraDictationMode::DoubleTap
        );
    }

    /// The rows' modes read back folded, and a missing row reads as the
    /// default.
    #[test]
    fn a_rows_mode_reads_back_folded() {
        let mut settings = AppSettings::default();
        assert_eq!(
            settings.extra_dictation_mode(0),
            ExtraDictationMode::HoldOrDoubleTap
        );
        let index = settings.add_extra_dictation_hotkey().unwrap();
        settings.extra_dictation_hotkeys[index].mode = ExtraDictationMode::DoubleTap;
        assert_eq!(
            settings.extra_dictation_mode(index),
            ExtraDictationMode::HoldOrDoubleTap
        );
        settings.extra_dictation_hotkeys[index].binding = Some(keyed());
        assert_eq!(
            settings.extra_dictation_mode(index),
            ExtraDictationMode::DoubleTap
        );
    }

    /// Labs › More dictation shortcuts: a row's binding takes effect only on
    /// macOS with the Lab on, never sharing a key with another shortcut, and
    /// the other shortcuts' checks count the active rows (issue 627's rule).
    #[test]
    fn an_extra_shortcut_needs_its_lab_and_its_own_keys() {
        let binding = keyed();
        let mut settings = AppSettings::default();
        let index = settings.add_extra_dictation_hotkey().unwrap();
        settings.extra_dictation_hotkeys[index].binding = Some(binding.clone());
        assert!(!settings.labs.more_dictation_shortcuts);
        assert_eq!(settings.active_extra_dictation_hotkey(index), None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));

        settings.labs.more_dictation_shortcuts = true;
        let active = settings.active_extra_dictation_hotkey(index);
        if cfg!(target_os = "macos") {
            assert_eq!(active, Some(&binding));
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));
            assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &binding));
        } else {
            assert_eq!(active, None);
        }

        // The dictation shortcut's own keys are refused, and a saved overlap
        // is dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::ExtraDictation(index), &dictation));
        settings.extra_dictation_hotkeys[index].binding = Some(dictation);
        assert_eq!(settings.active_extra_dictation_hotkey(index), None);
    }

    /// A second active row can't reuse the first one's keys on macOS, and
    /// each row may still use its own.
    #[test]
    fn active_rows_do_not_share_a_key() {
        let binding = keyed();
        let mut settings = AppSettings::default();
        let first = settings.add_extra_dictation_hotkey().unwrap();
        let second = settings.add_extra_dictation_hotkey().unwrap();
        settings.extra_dictation_hotkeys[first].binding = Some(binding.clone());
        settings.extra_dictation_hotkeys[second].binding = Some(binding.clone());
        settings.labs.more_dictation_shortcuts = true;
        if cfg!(target_os = "macos") {
            assert!(settings.hotkey_in_use(HotkeySlot::ExtraDictation(first), &binding));
            assert_eq!(settings.active_extra_dictation_hotkey(first), None);
            assert_eq!(settings.active_extra_dictation_hotkey(second), None);
        }
        settings.extra_dictation_hotkeys[second].binding = None;
        let unique = HotkeyBinding {
            modifiers: HotkeyModifiers::control_option(),
            key: Some(HotkeyKey::named("f6")),
        };
        settings.extra_dictation_hotkeys[first].binding = Some(unique.clone());
        assert!(!settings.hotkey_in_use(HotkeySlot::ExtraDictation(first), &unique));
        if cfg!(target_os = "macos") {
            assert_eq!(settings.active_extra_dictation_hotkey(first), Some(&unique));
        }
    }

    /// At most four rows; removing one frees its place.
    #[test]
    fn adding_stops_at_the_maximum_and_removing_frees_a_place() {
        let mut settings = AppSettings::default();
        for index in 0..MAX_EXTRA_DICTATION_SHORTCUTS {
            assert_eq!(settings.add_extra_dictation_hotkey(), Some(index));
        }
        assert_eq!(settings.add_extra_dictation_hotkey(), None);
        assert_eq!(
            settings.extra_dictation_hotkeys.len(),
            MAX_EXTRA_DICTATION_SHORTCUTS
        );
        assert!(!settings.remove_extra_dictation_hotkey(MAX_EXTRA_DICTATION_SHORTCUTS));
        assert!(settings.remove_extra_dictation_hotkey(1));
        assert_eq!(
            settings.extra_dictation_hotkeys.len(),
            MAX_EXTRA_DICTATION_SHORTCUTS - 1
        );
        assert_eq!(
            settings.add_extra_dictation_hotkey(),
            Some(MAX_EXTRA_DICTATION_SHORTCUTS - 1)
        );
    }

    /// The rows round-trip through settings.json, and turning the Lab off
    /// keeps them saved.
    #[test]
    fn extra_rows_round_trip_and_survive_their_lab_turning_off() {
        let mut settings = AppSettings::default();
        let index = settings.add_extra_dictation_hotkey().unwrap();
        settings.extra_dictation_hotkeys[index] = ExtraDictationHotkey {
            binding: Some(keyed()),
            mode: ExtraDictationMode::Tap,
        };
        settings.labs.more_dictation_shortcuts = true;
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            saved["extra_dictation_hotkeys"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            saved["extra_dictation_hotkeys"][0]["mode"],
            serde_json::Value::String("tap".into())
        );

        let mut loaded: AppSettings = serde_json::from_value(saved).unwrap();
        assert_eq!(
            loaded.extra_dictation_hotkeys,
            settings.extra_dictation_hotkeys
        );
        loaded.labs.more_dictation_shortcuts = false;
        assert_eq!(
            loaded.extra_dictation_hotkeys, settings.extra_dictation_hotkeys,
            "turning the Lab off keeps the saved rows"
        );
    }

    /// Loading keeps at most four rows and folds each row's mode to one its
    /// binding can trigger.
    #[test]
    fn loading_truncates_and_folds_the_rows() {
        let mut settings = AppSettings::default();
        for _ in 0..MAX_EXTRA_DICTATION_SHORTCUTS + 2 {
            settings.extra_dictation_hotkeys.push(ExtraDictationHotkey {
                binding: Some(modifier_only()),
                mode: ExtraDictationMode::DoubleTap,
            });
        }
        settings.repair_extra_dictation_shortcuts();
        assert_eq!(
            settings.extra_dictation_hotkeys.len(),
            MAX_EXTRA_DICTATION_SHORTCUTS
        );
        assert!(
            settings
                .extra_dictation_hotkeys
                .iter()
                .all(|row| row.mode == ExtraDictationMode::HoldOrDoubleTap)
        );
    }

    /// Turning Labs › Mouse shortcuts off unbinds an extra shortcut on a
    /// mouse button, as it does the dictation shortcut's; the rows stay.
    #[test]
    fn the_mouse_repair_unbinds_extra_mouse_shortcuts() {
        let mouse = HotkeyBinding {
            modifiers: HotkeyModifiers::control_option(),
            key: HotkeyKey::mouse(3),
        };
        let mut settings = AppSettings::default();
        settings.add_extra_dictation_hotkey();
        settings.extra_dictation_hotkeys[0].binding = Some(mouse.clone());
        settings.labs.mouse_shortcuts = true;
        settings.repair_mouse_shortcuts();
        assert_eq!(
            settings.extra_dictation_hotkeys[0].binding,
            Some(mouse.clone())
        );
        settings.labs.mouse_shortcuts = false;
        settings.repair_mouse_shortcuts();
        assert_eq!(settings.extra_dictation_hotkeys.len(), 1);
        assert_eq!(settings.extra_dictation_hotkeys[0].binding, None);
    }
}
