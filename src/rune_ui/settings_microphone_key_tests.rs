//! Issue 181: any shortcut on the microphone (dictation) key needs Labs ›
//! Microphone key alone on macOS, with or without a modifier. macOS's
//! recorder records the chord and `validate` refuses it before saving, with
//! the same "Unsupported key" the key gave before.
//!
//! Split from `settings.rs` to keep that file under the source cap.

use super::*;
use crate::app_settings::MICROPHONE_KEY_CODE;

fn microphone() -> HotkeyKey {
    HotkeyKey {
        code: MICROPHONE_KEY_CODE,
        label: "Mic".into(),
    }
}

#[test]
fn the_microphone_key_needs_its_labs_toggle() {
    let alone = HotkeyBinding {
        modifiers: HotkeyModifiers::default(),
        key: Some(microphone()),
    };
    let with_control = HotkeyBinding {
        modifiers: HotkeyModifiers::control_option(),
        key: Some(microphone()),
    };
    let mut settings = AppSettings::default();
    for target in [
        ShortcutTarget::Dictation,
        ShortcutTarget::Cancel,
        ShortcutTarget::Screenshot,
    ] {
        for binding in [&alone, &with_control] {
            assert_eq!(
                validate(&settings, target, binding),
                Err(tr("Unsupported key")),
                "{target:?}"
            );
        }
    }
    settings.labs.microphone_key = true;
    for target in [
        ShortcutTarget::Dictation,
        ShortcutTarget::Cancel,
        ShortcutTarget::Screenshot,
    ] {
        for binding in [&alone, &with_control] {
            assert_eq!(validate(&settings, target, binding), Ok(()), "{target:?}");
        }
    }
    assert_eq!(alone.keycaps(), ["Mic"]);
}
