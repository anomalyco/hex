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

/// Registers `wanted` in place of the shortcuts that ran before,
/// `registered`. A set the desktop refuses (a key the X11 keymap lacks, a
/// chord another client holds) puts `registered` back, so dictation keeps
/// working, and returns why beside it; only failing to restore those fails.
pub(super) fn reregister<M>(
    wanted: (LinuxHotkey, HotkeyOptions),
    registered: &mut (LinuxHotkey, HotkeyOptions),
    start: impl Fn((LinuxHotkey, HotkeyOptions)) -> Result<M>,
) -> Result<(M, Option<String>)> {
    match start(wanted.clone()) {
        Ok(monitor) => {
            *registered = wanted;
            Ok((monitor, None))
        }
        Err(error) => {
            let refused = format!(
                "could not register the new shortcuts, so Hex kept the previous ones: {error:#}"
            );
            let monitor = start(registered.clone())
                .map_err(|restore| eyre!("{refused}; restoring them failed too: {restore:#}"))?;
            Ok((monitor, Some(refused)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn chord(key: &str) -> (LinuxHotkey, HotkeyOptions) {
        let hotkey = LinuxHotkey {
            key: key.into(),
            ..LinuxHotkey::default()
        };
        (hotkey, HotkeyOptions::default())
    }

    /// 05.2: a refused shortcut keeps the previous ones registered instead
    /// of stopping the listener, and a later good one replaces them.
    #[test]
    fn a_refused_shortcut_keeps_the_previous_ones() {
        let tried = RefCell::new(Vec::new());
        let start = |(hotkey, _): (LinuxHotkey, HotkeyOptions)| {
            tried.borrow_mut().push(hotkey.key.clone());
            if hotkey.key == "right" {
                Err(eyre!("held by another client"))
            } else {
                Ok(hotkey.key)
            }
        };
        let mut registered = chord("space");

        let (monitor, error) = reregister(chord("right"), &mut registered, start).unwrap();
        assert_eq!(monitor, "space");
        assert!(error.unwrap().contains("held by another client"));
        assert_eq!(registered, chord("space"));
        assert_eq!(*tried.borrow(), ["right", "space"]);

        let (monitor, error) = reregister(chord("v"), &mut registered, start).unwrap();
        assert_eq!((monitor.as_str(), error), ("v", None));
        assert_eq!(registered, chord("v"));
    }

    #[test]
    fn failing_to_restore_the_previous_shortcuts_fails() {
        let mut registered = chord("space");
        let refuse = |_| -> Result<()> { Err(eyre!("no X server")) };
        assert!(reregister(chord("right"), &mut registered, refuse).is_err());
    }
}
