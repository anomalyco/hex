//! The key a virtual key code records, and the key a typed name records, for
//! the macOS window's shortcut recorder (`super::host::ChordCapture`).
//!
//! Split from the window's host to keep that file under the source cap.

use crate::app_settings::HotkeyKey;
use crate::desktop::i18n::tr;

/// The key a virtual key code records, and its label. AppKit gives no
/// characters for a dead key (`^`, `´`, `` ` ``), so `layout_label` names it
/// from the active layout instead.
pub(crate) fn hotkey_key(
    code: u16,
    characters: Option<&str>,
    layout_label: impl FnOnce() -> Option<String>,
) -> Result<HotkeyKey, &'static str> {
    let special = match code {
        49 => Some("Space"),
        48 => Some("Tab"),
        36 => Some("Return"),
        51 => Some(tr("Delete")),
        126 => Some("Up"),
        125 => Some("Down"),
        123 => Some(tr("Left")),
        124 => Some(tr("Right")),
        116 => Some("Page Up"),
        121 => Some("Page Down"),
        115 => Some("Home"),
        119 => Some("End"),
        117 => Some("Forward Delete"),
        114 => Some("Help"),
        122 => Some("F1"),
        120 => Some("F2"),
        99 => Some("F3"),
        118 => Some("F4"),
        96 => Some("F5"),
        97 => Some("F6"),
        98 => Some("F7"),
        100 => Some("F8"),
        101 => Some("F9"),
        109 => Some("F10"),
        103 => Some("F11"),
        111 => Some("F12"),
        105 => Some("F13"),
        107 => Some("F14"),
        113 => Some("F15"),
        106 => Some("F16"),
        64 => Some("F17"),
        79 => Some("F18"),
        80 => Some("F19"),
        90 => Some("F20"),
        176 => Some("Mic"),
        _ => None,
    };
    if let Some(label) = special {
        return Ok(HotkeyKey {
            code,
            label: label.into(),
        });
    }
    let character = match single_key_character(characters) {
        Some(character) => Some(character),
        None => layout_label().and_then(|label| single_key_character(Some(label.as_str()))),
    };
    match character {
        Some(character) => Ok(HotkeyKey {
            code,
            label: character.to_uppercase().collect(),
        }),
        None => Err(tr("Unsupported key")),
    }
}

/// The one visible character `text` holds, if it is exactly one non-control
/// character.
fn single_key_character(text: Option<&str>) -> Option<char> {
    let mut characters = text.unwrap_or_default().chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if !character.is_control() => Some(character),
        _ => None,
    }
}

/// The key a typed name records, with the label recording it would show.
pub(crate) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    let normalized: String = name
        .chars()
        .filter(|character| !matches!(character, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect();
    if let Some(number) = normalized.strip_prefix("mouse") {
        return number
            .parse::<u16>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(HotkeyKey::mouse)
            .ok_or(tr("Unsupported key"));
    }
    let code = match normalized.as_str() {
        "space" => Some(49),
        "tab" => Some(48),
        "return" | "enter" => Some(36),
        "delete" | "backspace" => Some(51),
        "up" => Some(126),
        "down" => Some(125),
        "left" => Some(123),
        "right" => Some(124),
        "pageup" => Some(116),
        "pagedown" => Some(121),
        "home" => Some(115),
        "end" => Some(119),
        "forwarddelete" => Some(117),
        "help" => Some(114),
        "mic" | "microphone" => Some(176),
        function => function
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .and_then(|number| {
                const CODES: [u16; 20] = [
                    122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64,
                    79, 80, 90,
                ];
                CODES.get(usize::from(number).checked_sub(1)?).copied()
            }),
    };
    if let Some(code) = code {
        return hotkey_key(code, None, || None);
    }
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => {
            let character = character.to_lowercase().next().unwrap_or(character);
            // A dead key (`^`, `´`, `` ` ``) names itself on the active
            // layout even though it types nothing until the next key.
            let code = crate::keyboard::key_code_for(character)
                .ok()
                .or_else(|| crate::keyboard::key_code_for_base_label(character))
                .ok_or(tr("Unsupported key"))?;
            hotkey_key(code, Some(&character.to_string()), || None)
        }
        _ => Err(tr("Unsupported key")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One visible character is a key; nothing, several, or a control
    /// character names no label.
    #[test]
    fn a_key_names_one_visible_character() {
        assert_eq!(single_key_character(Some("^")), Some('^'));
        assert_eq!(single_key_character(Some("ab")), None);
        assert_eq!(single_key_character(Some("\u{0}")), None);
        assert_eq!(single_key_character(None), None);
    }
}
