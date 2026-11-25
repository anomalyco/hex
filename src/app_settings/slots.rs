//! The single descriptor table for Hex's shortcut slots (`HotkeySlot`) and
//! the shared platform/Labs acceptance rules every surface (the Settings
//! recorder, `hex settings set`, and load-time repair) enforces.

use super::hotkeys::hotkey_conflicts;
use super::{AppSettings, HotkeyBinding, HotkeyKey};

/// A shortcut Settings and `hex settings` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeySlot {
    Dictation,
    PasteLast,
    RawDictation,
    /// Labs › Transform typed text's shortcut (Linux).
    TypedText,
    #[cfg(hex_capture)]
    Capture,
    /// Labs › Annotate last 15 seconds shortcut (macOS 27+).
    #[cfg(hex_capture)]
    CaptureReplay,
    /// Labs › Second dictation shortcut.
    SecondDictation,
    /// Labs › Model switch shortcut.
    ModelSwitch,
    /// Labs › More dictation shortcuts (macOS): row `index`'s binding.
    ExtraDictation(usize),
    /// Labs › Terminal paste shortcut (Windows, Linux).
    TerminalPaste,
    /// Toggle mode's cancel shortcut.
    Cancel,
    /// Labs › Screenshots (macOS).
    Screenshot,
    /// Labs › Screenshots' record shortcut (macOS, with Annotate).
    ScreenshotRecord,
}

/// Metadata and field accessors for one named `HotkeySlot`.
pub struct SlotDescriptor {
    pub slot: HotkeySlot,
    /// The `hex settings` key for this slot.
    #[cfg_attr(not(test), allow(dead_code))]
    pub key: &'static str,
    pub get: fn(&AppSettings) -> Option<&HotkeyBinding>,
    pub set: fn(&mut AppSettings, HotkeyBinding),
    /// Resets the slot to its default (or unbinds an optional slot with none).
    pub reset: fn(&mut AppSettings),
    /// Unbinds the slot on plain ⌫ while recording in Settings, when allowed.
    pub clear: Option<fn(&mut AppSettings)>,
    /// Whether this slot's binding counts as in use against `candidate_slot`.
    pub reserved_against: fn(&AppSettings, HotkeySlot) -> bool,
}

impl HotkeySlot {
    /// Every named shortcut slot (all slots except the dynamic
    /// `ExtraDictation(index)` rows).
    pub const NAMED: &'static [SlotDescriptor] = &[
        SlotDescriptor {
            slot: Self::Dictation,
            key: "dictation_hotkey",
            get: |s| Some(&s.dictation_hotkey),
            set: |s, b| s.dictation_hotkey = b,
            reset: |s| s.dictation_hotkey = HotkeyBinding::default(),
            clear: None,
            reserved_against: |_, _| true,
        },
        SlotDescriptor {
            slot: Self::PasteLast,
            key: "paste_last_hotkey",
            get: |s| s.paste_last_hotkey.as_ref(),
            set: |s, b| s.paste_last_hotkey = Some(b),
            reset: |s| s.paste_last_hotkey = Some(HotkeyBinding::paste_last_default()),
            clear: Some(|s| s.paste_last_hotkey = None),
            reserved_against: |_, _| true,
        },
        SlotDescriptor {
            slot: Self::Cancel,
            key: "cancel_hotkey",
            get: |s| Some(&s.cancel_hotkey),
            set: |s, b| s.cancel_hotkey = b,
            reset: |s| s.cancel_hotkey = HotkeyBinding::cancel_default(),
            clear: None,
            reserved_against: |_, _| true,
        },
        SlotDescriptor {
            slot: Self::RawDictation,
            key: "raw_dictation_hotkey",
            get: |s| s.raw_dictation_hotkey.as_ref(),
            set: |s, b| s.raw_dictation_hotkey = Some(b),
            reset: |s| s.raw_dictation_hotkey = None,
            clear: Some(|s| s.raw_dictation_hotkey = None),
            reserved_against: |_, _| true,
        },
        SlotDescriptor {
            slot: Self::SecondDictation,
            key: "second_dictation_hotkey",
            get: |s| s.second_dictation_hotkey.as_ref(),
            set: |s, b| s.second_dictation_hotkey = Some(b),
            reset: |s| s.second_dictation_hotkey = None,
            clear: Some(|s| s.second_dictation_hotkey = None),
            // Linux and Windows register a key, so a keyless binding never
            // runs there and takes no key from another shortcut.
            reserved_against: |s, _| {
                s.labs.second_dictation_shortcut
                    && s.second_dictation_hotkey
                        .as_ref()
                        .is_some_and(|b| cfg!(target_os = "macos") || b.key.is_some())
            },
        },
        SlotDescriptor {
            slot: Self::ModelSwitch,
            key: "model_switch_hotkey",
            get: |s| s.model_switch_hotkey.as_ref(),
            set: |s, b| s.model_switch_hotkey = Some(b),
            reset: |s| s.model_switch_hotkey = None,
            clear: Some(|s| s.model_switch_hotkey = None),
            // Every OS matches it as a key press, so a keyless binding never
            // runs and takes no key from another shortcut.
            reserved_against: |s, _| {
                s.labs.model_switch_shortcut
                    && s.model_switch_other.is_some()
                    && s.model_switch_hotkey
                        .as_ref()
                        .is_some_and(|b| b.key.is_some())
            },
        },
        SlotDescriptor {
            slot: Self::TypedText,
            key: "typed_text_hotkey",
            get: |s| s.typed_text_hotkey.as_ref(),
            set: |s, b| s.typed_text_hotkey = Some(b),
            reset: |s| s.typed_text_hotkey = None,
            clear: Some(|s| s.typed_text_hotkey = None),
            reserved_against: |s, _| cfg!(target_os = "linux") && s.labs.typed_text,
        },
        SlotDescriptor {
            slot: Self::TerminalPaste,
            key: "terminal_paste_hotkey",
            get: |s| s.terminal_paste_hotkey.as_ref(),
            set: |s, b| s.terminal_paste_hotkey = Some(b),
            reset: |s| s.terminal_paste_hotkey = None,
            clear: Some(|s| s.terminal_paste_hotkey = None),
            reserved_against: |s, _| {
                cfg!(any(target_os = "windows", target_os = "linux"))
                    && s.labs.terminal_paste_shortcut
            },
        },
        SlotDescriptor {
            slot: Self::Screenshot,
            key: "screenshots.hotkey",
            get: |s| Some(&s.screenshots.hotkey),
            set: |s, b| s.screenshots.hotkey = b,
            reset: |s| s.screenshots.hotkey = HotkeyBinding::screenshot_default(),
            clear: None,
            reserved_against: |s, _| cfg!(target_os = "macos") && s.labs.screenshots,
        },
        SlotDescriptor {
            slot: Self::ScreenshotRecord,
            key: "screenshots.record_hotkey",
            get: |s| s.screenshots.record_hotkey.as_ref(),
            set: |s, b| s.screenshots.record_hotkey = Some(b),
            reset: |s| s.screenshots.record_hotkey = None,
            clear: None,
            // A saved record shortcut and Annotate's own capture shortcuts
            // never share a key while Labs › Screenshots is on, even with
            // Annotate off; for every other shortcut it counts only while
            // screenshot recording is available.
            reserved_against: |s, candidate| {
                cfg!(target_os = "macos")
                    && if candidate.is_annotate_capture() {
                        s.labs.screenshots
                    } else {
                        s.screenshot_recording_available()
                    }
            },
        },
        #[cfg(hex_capture)]
        SlotDescriptor {
            slot: Self::Capture,
            key: "capture.hotkey",
            get: |s| Some(&s.capture.hotkey),
            set: |s, b| s.capture.hotkey = b,
            reset: |s| s.capture.hotkey = HotkeyBinding::capture_default(),
            clear: None,
            // The record shortcut counts Annotate's capture shortcuts always
            // on macOS, even with Annotate off, so turning Annotate on never
            // collides with a saved record shortcut.
            reserved_against: |s, candidate| {
                s.capture_on() || (cfg!(target_os = "macos") && candidate == Self::ScreenshotRecord)
            },
        },
        #[cfg(hex_capture)]
        SlotDescriptor {
            slot: Self::CaptureReplay,
            key: "capture.replay_hotkey",
            get: |s| s.capture.replay_hotkey.as_ref(),
            set: |s, b| s.capture.replay_hotkey = Some(b),
            reset: |s| s.capture.replay_hotkey = None,
            clear: None,
            reserved_against: |s, candidate| {
                cfg!(target_os = "macos")
                    && ((s.capture_on() && s.labs.annotate_replay)
                        || candidate == Self::ScreenshotRecord)
            },
        },
    ];

    fn descriptor(self) -> Option<&'static SlotDescriptor> {
        Self::NAMED.iter().find(|desc| desc.slot == self)
    }

    /// Every slot in `settings`: all named slots followed by each configured
    /// `ExtraDictation` row.
    pub fn all_in(settings: &AppSettings) -> impl Iterator<Item = Self> + '_ {
        Self::NAMED
            .iter()
            .map(|desc| desc.slot)
            .chain((0..settings.extra_dictation_hotkeys.len()).map(Self::ExtraDictation))
    }

    /// One of Annotate's own capture shortcuts: its capture, and its 15 s
    /// replay.
    pub fn is_annotate_capture(self) -> bool {
        #[cfg(hex_capture)]
        {
            matches!(self, Self::Capture | Self::CaptureReplay)
        }
        #[cfg(not(hex_capture))]
        {
            let _ = self;
            false
        }
    }

    /// The current binding saved in this slot, if bound.
    pub fn get(self, settings: &AppSettings) -> Option<&HotkeyBinding> {
        match self {
            Self::ExtraDictation(index) => settings
                .extra_dictation_hotkeys
                .get(index)?
                .binding
                .as_ref(),
            _ => (self.descriptor()?.get)(settings),
        }
    }

    /// Writes `binding` into this slot.
    pub fn set(self, settings: &mut AppSettings, binding: HotkeyBinding) {
        match self {
            Self::ExtraDictation(index) => {
                // A row removed while its recorder commits keeps the rest.
                if let Some(row) = settings.extra_dictation_hotkeys.get_mut(index) {
                    row.binding = Some(binding);
                }
            }
            _ => {
                if let Some(desc) = self.descriptor() {
                    (desc.set)(settings, binding);
                }
            }
        }
    }

    /// Resets this slot to its default binding (or unbinds it when optional
    /// without a default).
    pub fn reset(self, settings: &mut AppSettings) {
        match self {
            Self::ExtraDictation(index) => {
                if let Some(row) = settings.extra_dictation_hotkeys.get_mut(index) {
                    row.binding = None;
                }
            }
            _ => {
                if let Some(desc) = self.descriptor() {
                    (desc.reset)(settings);
                }
            }
        }
    }

    /// Whether plain ⌫ while recording in Settings turns this shortcut off.
    pub fn can_turn_off(self) -> bool {
        self.descriptor().is_some_and(|desc| desc.clear.is_some())
    }

    /// Turns off a shortcut `can_turn_off` allows; others keep their binding.
    pub fn turn_off(self, settings: &mut AppSettings) {
        if let Some(clear) = self.descriptor().and_then(|desc| desc.clear) {
            clear(settings);
        }
    }

    /// Whether `self` counts as an active/reserved shortcut when checking
    /// conflicts for `candidate_slot`.
    fn reserved_against(self, settings: &AppSettings, candidate_slot: Self) -> bool {
        match self {
            Self::ExtraDictation(_) => {
                cfg!(target_os = "macos") && settings.labs.more_dictation_shortcuts
            }
            _ => self
                .descriptor()
                .is_some_and(|desc| (desc.reserved_against)(settings, candidate_slot)),
        }
    }
}

impl AppSettings {
    /// Whether `binding` overlaps a shortcut `slot` must not share: every
    /// other slot reserved against `slot` (each active Labs › More dictation
    /// shortcuts row included). The two capture shortcuts never share a key,
    /// on or off, since turning captures on can't pull them apart. A saved
    /// Screenshots record shortcut and Annotate's capture shortcuts never
    /// share a key while the Lab is on, whether Annotate is on or off.
    pub fn hotkey_in_use(&self, slot: HotkeySlot, binding: &HotkeyBinding) -> bool {
        let others = HotkeySlot::all_in(self)
            .filter(|&other| other != slot && other.reserved_against(self, slot))
            .filter_map(|other| other.get(self).cloned());
        hotkey_conflicts(binding, others)
    }

    /// Puts back every shortcut `matches`: required slots to their defaults,
    /// optional ones unbound. Returns whether any changed.
    pub(super) fn reset_shortcuts(&mut self, matches: impl Fn(&HotkeyBinding) -> bool) -> bool {
        let slots: Vec<HotkeySlot> = HotkeySlot::all_in(self).collect();
        let mut reset = false;
        for slot in slots {
            if slot.get(self).is_some_and(&matches) {
                slot.reset(self);
                reset = true;
            }
        }
        reset
    }

    /// A shortcut saved as a bare left click swallowed every click, so its
    /// owner couldn't change it in Settings. Such a shortcut goes back to its
    /// default (or is cleared) when settings load.
    pub(super) fn repair_bare_primary_clicks(&mut self) {
        let slots: Vec<HotkeySlot> = HotkeySlot::all_in(self).collect();
        for slot in slots {
            if slot
                .get(self)
                .is_some_and(HotkeyBinding::is_bare_primary_click)
            {
                tracing::warn!(?slot, "reset a bare left-click shortcut");
                slot.reset(self);
            }
        }
    }
}

/// Why a candidate shortcut binding is refused by the shared platform/Labs
/// acceptance rules, independent of the OS registrability check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutRefusal {
    BarePrimaryClick,
    BareErase,
    NeedsMouseShortcutsLab,
    NeedsSectionKeyLab,
    NeedsMicrophoneKeyLab,
    MicrophoneKeyOnScreenshot,
    NeedsNavigationKeyLab { key_name: String },
    NeedsSpecialKeysLab { key_name: String, alone: bool },
    NeedsModifierForKey { key_name: String },
    InUse,
}

/// Failure from [`validate_shortcut`]: either a shared policy/conflict refusal
/// or an OS-level registrability refusal from the host rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutValidationError<E> {
    Policy(ShortcutRefusal),
    Host(E),
}

/// Checks every platform- and Labs-gated acceptance rule for saving `binding`
/// in `slot`, before the OS-specific registrability check.
pub fn check_shortcut_policy(
    settings: &AppSettings,
    slot: HotkeySlot,
    binding: &HotkeyBinding,
) -> Result<(), ShortcutRefusal> {
    // A bare left click would swallow every click, this window's included.
    if binding.is_bare_primary_click() {
        return Err(ShortcutRefusal::BarePrimaryClick);
    }
    // Plain ⌫ turns an optional shortcut off rather than binding (`record`).
    // Windows' cancel key may be any key alone, as it always could.
    if binding.is_bare_erase() && !(cfg!(target_os = "windows") && slot == HotkeySlot::Cancel) {
        return Err(ShortcutRefusal::BareErase);
    }
    // Mouse buttons need Labs › Mouse shortcuts on macOS.
    if cfg!(target_os = "macos")
        && !settings.labs.mouse_shortcuts
        && binding.key.as_ref().is_some_and(HotkeyKey::is_mouse)
    {
        return Err(ShortcutRefusal::NeedsMouseShortcutsLab);
    }
    // The ISO section key alone needs Labs › § key alone (Linux, macOS).
    if cfg!(any(target_os = "linux", target_os = "macos"))
        && binding.is_lone_section_key()
        && !settings.section_key_allowed()
    {
        return Err(ShortcutRefusal::NeedsSectionKeyLab);
    }
    // Any shortcut on the microphone key needs Labs › Microphone key alone
    // (macOS); a modifier doesn't excuse it.
    if cfg!(target_os = "macos")
        && binding.uses_microphone_key()
        && !settings.microphone_key_allowed()
    {
        return Err(ShortcutRefusal::NeedsMicrophoneKeyLab);
    }
    // macOS reads the microphone key at Hex's own session tap, before the
    // annotated-session tap Screenshots' keys go through, so a shot never sees
    // it however the labs are set.
    if cfg!(target_os = "macos")
        && binding.uses_microphone_key()
        && matches!(slot, HotkeySlot::Screenshot | HotkeySlot::ScreenshotRecord)
    {
        return Err(ShortcutRefusal::MicrophoneKeyOnScreenshot);
    }
    // Windows: a navigation key alone needs Labs › Special keys alone.
    if cfg!(target_os = "windows")
        && binding.is_lone_navigation_key()
        && !settings.special_keys_allowed()
    {
        let key_name = binding
            .key
            .as_ref()
            .map(|key| key.label.to_lowercase().replace(' ', ""))
            .unwrap_or_default();
        return Err(ShortcutRefusal::NeedsNavigationKeyLab { key_name });
    }
    // Menu, Pause, and Scroll Lock: on Linux they need Labs › Special keys
    // alone; Windows accepts them alone; elsewhere one alone still needs a
    // modifier.
    if !cfg!(target_os = "windows")
        && let Some(key) = binding.key.as_ref().filter(|key| key.is_special())
        && !settings.special_keys_allowed()
    {
        let key_name = key.label.to_lowercase().replace(' ', "");
        if cfg!(target_os = "linux") {
            return Err(ShortcutRefusal::NeedsSpecialKeysLab {
                key_name,
                alone: binding.modifiers.is_empty(),
            });
        }
        if binding.modifiers.is_empty() {
            return Err(ShortcutRefusal::NeedsModifierForKey { key_name });
        }
    }
    Ok(())
}

/// Validates `binding` for `slot` against the shared policy rules, the host's
/// OS registrability rule, and active-shortcut conflicts.
pub fn validate_shortcut<E>(
    policy_settings: &AppSettings,
    conflict_settings: &AppSettings,
    slot: HotkeySlot,
    binding: &HotkeyBinding,
    host_rule: impl FnOnce(HotkeySlot, &HotkeyBinding) -> Result<(), E>,
) -> Result<(), ShortcutValidationError<E>> {
    check_shortcut_policy(policy_settings, slot, binding)
        .map_err(ShortcutValidationError::Policy)?;
    host_rule(slot, binding).map_err(ShortcutValidationError::Host)?;
    if conflict_settings.hotkey_in_use(slot, binding) {
        return Err(ShortcutValidationError::Policy(ShortcutRefusal::InUse));
    }
    Ok(())
}
