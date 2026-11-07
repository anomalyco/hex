//! Whether macOS's own screenshot shortcuts would take Hex's.
//!
//! macOS matches its symbolic hot keys (Keyboard › Keyboard Shortcuts ›
//! Screenshots) in the WindowServer, before any session tap sees the key,
//! so while one of them is on with Hex's chord, the system screenshot wins.
//! They live in `com.apple.symbolichotkeys`'s `AppleSymbolicHotKeys`, one
//! entry per ID; an ID with no entry is on with its default chord.

use crate::app_settings::RuntimeHotkey;

/// One symbolic hot key as `AppleSymbolicHotKeys` holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) enabled: bool,
    /// The key code and the modifier flags (CGEventFlags' bits), when the
    /// entry names them.
    pub(crate) chord: Option<(u16, u64)>,
}

const SHIFT: u64 = 0x2_0000;
const CONTROL: u64 = 0x4_0000;
const OPTION: u64 = 0x8_0000;
const COMMAND: u64 = 0x10_0000;
const MODIFIERS: u64 = SHIFT | CONTROL | OPTION | COMMAND;

/// macOS's screenshot shortcuts and their default chords: ⌘⇧3, ⌃⌘⇧3,
/// ⌘⇧4, ⌃⌘⇧4, and ⌘⇧5.
const SCREENSHOT_KEYS: [(u32, u16, u64); 5] = [
    (28, 20, COMMAND | SHIFT),
    (29, 20, CONTROL | COMMAND | SHIFT),
    (30, 21, COMMAND | SHIFT),
    (31, 21, CONTROL | COMMAND | SHIFT),
    (184, 23, COMMAND | SHIFT),
];

/// The ID of the macOS screenshot shortcut that takes `hotkey`, if one is
/// on with its chord. `entry` reads one ID's saved entry.
pub(crate) fn taken_by(hotkey: RuntimeHotkey, entry: impl Fn(u32) -> Option<Entry>) -> Option<u32> {
    let code = hotkey.key_code?;
    let wanted = flags(hotkey);
    SCREENSHOT_KEYS
        .into_iter()
        .find(|&(id, default_code, default_flags)| {
            let saved = entry(id).unwrap_or(Entry {
                enabled: true,
                chord: None,
            });
            let (key, mods) = saved.chord.unwrap_or((default_code, default_flags));
            saved.enabled && key == code && mods & MODIFIERS == wanted
        })
        .map(|(id, ..)| id)
}

fn flags(hotkey: RuntimeHotkey) -> u64 {
    let m = hotkey.modifiers;
    [
        (m.shift.is_some(), SHIFT),
        (m.control.is_some(), CONTROL),
        (m.option.is_some(), OPTION),
        (m.command.is_some(), COMMAND),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |all, (_, bit)| all | bit)
}

/// Whether a macOS screenshot shortcut is on with `hotkey`'s chord, read
/// from the saved preferences now.
pub(crate) fn takes(hotkey: RuntimeHotkey) -> bool {
    let saved = read();
    taken_by(hotkey, |id| {
        saved.iter().find(|(i, _)| *i == id).map(|(_, e)| *e)
    })
    .is_some()
}

/// The saved screenshot entries.
fn read() -> Vec<(u32, Entry)> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUserDefaults};

    let Some(domain) = NSUserDefaults::standardUserDefaults()
        .persistentDomainForName(&NSString::from_str("com.apple.symbolichotkeys"))
    else {
        return Vec::new();
    };
    let Some(keys) = domain.objectForKey(&NSString::from_str("AppleSymbolicHotKeys")) else {
        return Vec::new();
    };
    let Some(keys) = keys.downcast_ref::<NSDictionary>() else {
        return Vec::new();
    };
    let field = |dict: &NSDictionary, name: &str| -> Option<Retained<AnyObject>> {
        let key = NSString::from_str(name);
        let key: &AnyObject = &key;
        dict.objectForKey(key)
    };
    let number = |object: &AnyObject| object.downcast_ref::<NSNumber>().map(|n| n.integerValue());
    SCREENSHOT_KEYS
        .into_iter()
        .filter_map(|(id, ..)| {
            let entry = field(keys, &id.to_string())?;
            let entry = entry.downcast_ref::<NSDictionary>()?;
            let enabled = field(entry, "enabled")
                .and_then(|on| number(&on))
                .is_none_or(|on| on != 0);
            let chord = field(entry, "value").and_then(|value| {
                let value = value.downcast_ref::<NSDictionary>()?;
                let parameters = field(value, "parameters")?;
                let parameters = parameters.downcast_ref::<NSArray>()?;
                if parameters.count() < 3 {
                    return None;
                }
                let code = number(&parameters.objectAtIndex(1))?;
                let mods = number(&parameters.objectAtIndex(2))?;
                Some((u16::try_from(code).ok()?, u64::try_from(mods).ok()?))
            });
            Some((id, Entry { enabled, chord }))
        })
        .collect()
}

/// Opens Keyboard › Keyboard Shortcuts in System Settings, where
/// Screenshots' shortcuts are turned off.
pub(crate) fn open_settings() {
    let opened = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.Keyboard-Settings.extension")
        .spawn();
    if let Err(error) = opened {
        tracing::warn!(%error, "could not open Keyboard settings");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeyBinding;

    fn command_shift_4() -> RuntimeHotkey {
        HotkeyBinding::screenshot_default().runtime()
    }

    #[test]
    fn an_unsaved_shortcut_is_on_with_its_default() {
        assert_eq!(taken_by(command_shift_4(), |_| None), Some(30));
    }

    #[test]
    fn a_turned_off_shortcut_takes_nothing() {
        let off = |id| {
            (id == 30).then_some(Entry {
                enabled: false,
                chord: Some((21, 0x12_0000)),
            })
        };
        assert_eq!(taken_by(command_shift_4(), off), None);
    }

    #[test]
    fn a_moved_shortcut_takes_its_new_chord() {
        // ⌘⇧5 rebound onto ⌘⇧4, with ⌘⇧4 itself turned off.
        let saved = |id| match id {
            30 => Some(Entry {
                enabled: false,
                chord: None,
            }),
            184 => Some(Entry {
                enabled: true,
                chord: Some((21, 0x12_0000 | 0x80_0000)),
            }),
            _ => None,
        };
        assert_eq!(taken_by(command_shift_4(), saved), Some(184));
    }

    #[test]
    fn another_chord_is_free() {
        let mut hotkey = command_shift_4();
        hotkey.modifiers.option = hotkey.modifiers.command;
        assert_eq!(taken_by(hotkey, |_| None), None);
    }
}
