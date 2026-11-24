//! The OS's rules for a shortcut, the ones Settings applies when recording
//! one; shared with `hex settings set`. Kept out of `rune_ui.rs` so its
//! source stays under the file cap.

use super::host;
use super::host::ShortcutTarget;
use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeySlot};

/// The key `name` names on the active keyboard layout ("v", "space", "f5").
#[cfg(target_os = "macos")]
pub(crate) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    super::hotkey_keys::hotkey_key_named(name)
}

/// The key `name` names: Linux and Windows bind keys by name, and the
/// shortcut's rule ([`shortcut_rule`]) checks it can register.
#[cfg(not(target_os = "macos"))]
pub(crate) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    Ok(HotkeyKey::named(name))
}

/// The OS's shortcut rules for `hex settings set`.
pub(crate) fn shortcut_rules() -> crate::control::settings::ShortcutRules {
    crate::control::settings::ShortcutRules {
        key_named: |name| hotkey_key_named(name).map_err(str::to_owned),
        rule: |slot, binding| shortcut_rule(slot, binding).map_err(str::to_owned),
    }
}

/// The one mapping from a window's `ShortcutTarget` to its `HotkeySlot`.
pub(super) fn slot_of(target: ShortcutTarget) -> HotkeySlot {
    match target {
        ShortcutTarget::Dictation => HotkeySlot::Dictation,
        ShortcutTarget::PasteLast => HotkeySlot::PasteLast,
        ShortcutTarget::RawDictation => HotkeySlot::RawDictation,
        #[cfg(target_os = "linux")]
        ShortcutTarget::TypedText => HotkeySlot::TypedText,
        ShortcutTarget::Cancel => HotkeySlot::Cancel,
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => HotkeySlot::Capture,
        #[cfg(all(hex_capture, target_os = "macos"))]
        ShortcutTarget::CaptureReplay => HotkeySlot::CaptureReplay,
        ShortcutTarget::SecondDictation => HotkeySlot::SecondDictation,
        ShortcutTarget::ModelSwitch => HotkeySlot::ModelSwitch,
        #[cfg(target_os = "macos")]
        ShortcutTarget::ExtraDictation(index) => HotkeySlot::ExtraDictation(index),
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        ShortcutTarget::TerminalPaste => HotkeySlot::TerminalPaste,
        #[cfg(target_os = "macos")]
        ShortcutTarget::Screenshot => HotkeySlot::Screenshot,
        #[cfg(target_os = "macos")]
        ShortcutTarget::ScreenshotRecord => HotkeySlot::ScreenshotRecord,
    }
}

pub(crate) fn shortcut_rule(slot: HotkeySlot, binding: &HotkeyBinding) -> Result<(), &'static str> {
    let target = match slot {
        HotkeySlot::Dictation => ShortcutTarget::Dictation,
        HotkeySlot::PasteLast => ShortcutTarget::PasteLast,
        HotkeySlot::RawDictation => ShortcutTarget::RawDictation,
        HotkeySlot::Cancel => ShortcutTarget::Cancel,
        #[cfg(hex_capture)]
        HotkeySlot::Capture => ShortcutTarget::Capture,
        #[cfg(all(hex_capture, target_os = "macos"))]
        HotkeySlot::CaptureReplay => ShortcutTarget::CaptureReplay,
        #[cfg(all(hex_capture, not(target_os = "macos")))]
        HotkeySlot::CaptureReplay => ShortcutTarget::Capture,
        // Labs › Second dictation shortcut: its own shortcut's rules on every
        // OS, so Windows and Linux require a key and macOS keeps accepting a
        // shortcut without one.
        HotkeySlot::SecondDictation => ShortcutTarget::SecondDictation,
        // Labs › Model switch shortcut: its own shortcut's rules on every
        // OS. Every OS matches it as a key press, so the recorder requires a
        // key on macOS too.
        HotkeySlot::ModelSwitch => ShortcutTarget::ModelSwitch,
        // Labs › More dictation shortcuts (macOS): each row records under its
        // own shortcut's rules where they exist, the dictation shortcut's
        // everywhere else.
        #[cfg(target_os = "macos")]
        HotkeySlot::ExtraDictation(index) => ShortcutTarget::ExtraDictation(index),
        #[cfg(not(target_os = "macos"))]
        HotkeySlot::ExtraDictation(_) => ShortcutTarget::Dictation,
        #[cfg(target_os = "macos")]
        HotkeySlot::Screenshot => ShortcutTarget::Screenshot,
        #[cfg(target_os = "macos")]
        HotkeySlot::ScreenshotRecord => ShortcutTarget::ScreenshotRecord,
        // Transform typed text is Linux-only. A saved binding meets the
        // dictation shortcut's rules everywhere else, including macOS, where
        // it is never registered.
        HotkeySlot::TypedText => ShortcutTarget::Dictation,
        // Screenshots are macOS-only; elsewhere a saved one meets the
        // dictation shortcut's rules.
        #[cfg(not(target_os = "macos"))]
        HotkeySlot::Screenshot | HotkeySlot::ScreenshotRecord => ShortcutTarget::Dictation,
        // Labs › Terminal paste shortcut: its own hook rules where it exists,
        // and the dictation shortcut's everywhere else, where it never runs.
        #[cfg(target_os = "macos")]
        HotkeySlot::TerminalPaste => ShortcutTarget::SecondDictation,
        #[cfg(not(target_os = "macos"))]
        HotkeySlot::TerminalPaste => ShortcutTarget::RawDictation,
    };
    host::validate_hotkey(target, binding)
}
