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

/// Registers the shortcuts `settings` records in place of the ones that
/// ran before, `running`, once those are dropped: X11 grabs the same keys
/// again. A set the desktop refuses (a key the X11 keymap lacks, a chord
/// another client holds) puts `running` back, so dictation keeps working,
/// and the refusal shows as a failure; only failing to restore those fails.
pub(super) fn reregister<M>(
    settings: &AppSettings,
    running: &mut (LinuxHotkey, HotkeyOptions),
    start: impl Fn((LinuxHotkey, HotkeyOptions)) -> Result<M>,
    events: &EventLog,
) -> Result<M> {
    let wanted = shortcuts(settings);
    let error = match start(wanted.clone()) {
        Ok(monitor) => {
            *running = wanted;
            return Ok(monitor);
        }
        Err(error) => error,
    };
    let refused =
        format!("could not register the new shortcuts, so Hex kept the previous ones: {error:#}");
    let monitor = start(running.clone())
        .map_err(|restore| eyre!("{refused}; restoring them failed too: {restore:#}"))?;
    tracing::warn!("{refused}");
    events.dictation(DictationPhase::Failed(refused), "")?;
    Ok(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeyBinding;
    use std::cell::RefCell;

    fn saved(key: &str) -> AppSettings {
        AppSettings {
            dictation_hotkey: HotkeyBinding::from(&LinuxHotkey {
                key: key.into(),
                ..LinuxHotkey::default()
            }),
            ..AppSettings::default()
        }
    }

    fn event_log() -> (EventLog, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "hex-linux-shortcuts-{}-{:?}.ndjson",
            std::process::id(),
            thread::current().id()
        ));
        (EventLog::create(&path).unwrap(), path)
    }

    /// 05.2: a refused shortcut keeps the previous ones registered instead
    /// of stopping the listener, and says why; a later good one replaces
    /// them.
    #[test]
    fn a_refused_shortcut_keeps_the_previous_ones() {
        let (events, path) = event_log();
        let tried = RefCell::new(Vec::new());
        let start = |(hotkey, _): (LinuxHotkey, HotkeyOptions)| {
            tried.borrow_mut().push(hotkey.key.clone());
            if hotkey.key == "right" {
                Err(eyre!("held by another client"))
            } else {
                Ok(hotkey.key)
            }
        };
        let mut running = shortcuts(&saved("space"));

        let monitor = reregister(&saved("right"), &mut running, start, &events).unwrap();
        assert_eq!(monitor, "space");
        assert_eq!(running, shortcuts(&saved("space")));
        assert_eq!(*tried.borrow(), ["right", "space"]);
        events.flush().unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("held by another client")
        );

        let monitor = reregister(&saved("v"), &mut running, start, &events).unwrap();
        assert_eq!(monitor, "v");
        assert_eq!(running, shortcuts(&saved("v")));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failing_to_restore_the_previous_shortcuts_fails() {
        let (events, path) = event_log();
        let mut running = shortcuts(&saved("space"));
        let refuse = |_| -> Result<()> { Err(eyre!("no X server")) };
        assert!(reregister(&saved("right"), &mut running, refuse, &events).is_err());
        let _ = std::fs::remove_file(path);
    }
}
