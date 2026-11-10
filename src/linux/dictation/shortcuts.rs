//! The shortcuts a Linux listener registers for saved settings.

use super::*;

/// The shortcuts a listener registers for `settings`.
pub(super) fn shortcuts(settings: &AppSettings) -> (LinuxHotkey, HotkeyOptions) {
    let options = HotkeyOptions {
        double_tap: settings.double_tap_lock,
        // Double-tap-only needs the double-tap window and a non-modifier key.
        double_tap_only: settings.double_tap_lock
            && settings.double_tap_only
            && settings.dictation_hotkey.key.is_some(),
        toggle: settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
        // Labs › Tap or hold, off by default; it acts only in Toggle mode.
        tap_or_hold: settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle
            && crate::labs::Feature::TapOrHold.on(settings),
        paste_last: settings.paste_last_hotkey.as_ref().map(LinuxHotkey::from),
        raw_dictation: settings
            .raw_dictation_hotkey
            .as_ref()
            .map(LinuxHotkey::from),
        // Only while Labs › Transform typed text is on: off registers nothing.
        typed_text: settings.active_typed_text_hotkey().map(LinuxHotkey::from),
        terminal_paste: settings
            .active_terminal_paste_hotkey()
            .map(LinuxHotkey::from),
        // Labs › Second dictation shortcut: off registers nothing, and a
        // keyless saved binding never runs on Linux.
        second_dictation: settings
            .active_second_dictation_hotkey()
            .map(LinuxHotkey::from),
        capture: settings
            .capture
            .enabled
            .then(|| LinuxHotkey::from(&settings.capture.hotkey)),
        cancel: settings.active_cancel_hotkey().map(LinuxHotkey::from),
    };
    (LinuxHotkey::from(&settings.dictation_hotkey), options)
}
