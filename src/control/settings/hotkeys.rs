//! Shortcut text for `hex settings`: reading and writing the modifier-plus-key
//! form, and why a bare special key still needs its Labs toggle.

use crate::app_settings::{
    AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, HotkeySlot, ModifierSide,
    ShortcutRefusal, ShortcutValidationError, validate_shortcut,
};

use super::{ShortcutRules, invalid};

/// Applies the checks a recorded shortcut meets: the shared platform/Labs
/// acceptance rules, the OS's own rule for `slot`, and no key shared with
/// another active shortcut.
pub(super) fn check_shortcut(
    settings: &AppSettings,
    candidate: &AppSettings,
    name: &str,
    slot: HotkeySlot,
    binding: &HotkeyBinding,
    shortcuts: &ShortcutRules,
) -> Result<(), crate::control::ControlError> {
    validate_shortcut(candidate, settings, slot, binding, shortcuts.rule).map_err(|error| {
        match error {
            ShortcutValidationError::Host(why) => invalid(name, &why),
            ShortcutValidationError::Policy(ShortcutRefusal::InUse) => {
                crate::control::ControlError::new(
                    "shortcut-in-use",
                    format!(
                        "{} is already in use by another Hex shortcut",
                        format_hotkey(binding)
                    ),
                )
            }
            ShortcutValidationError::Policy(refusal) => {
                invalid(name, &cli_refusal_message(refusal))
            }
        }
    })
}

fn cli_refusal_message(refusal: ShortcutRefusal) -> String {
    match refusal {
        ShortcutRefusal::BarePrimaryClick => {
            "mouse1 (left click) needs a modifier, such as option+mouse1".into()
        }
        ShortcutRefusal::BareErase => "add a modifier".into(),
        ShortcutRefusal::NeedsMouseShortcutsLab => {
            "mouse buttons need Labs › Mouse shortcuts: hex labs on mouse-shortcuts".into()
        }
        ShortcutRefusal::NeedsSectionKeyLab => {
            "section needs Labs › § key alone: hex labs on section-key".into()
        }
        ShortcutRefusal::NeedsMicrophoneKeyLab => {
            "the mic key needs Labs › Microphone key alone: hex labs on microphone-key".into()
        }
        ShortcutRefusal::MicrophoneKeyOnScreenshot => {
            "the mic key can't be the screenshot shortcut: macOS takes it before Hex sees it".into()
        }
        ShortcutRefusal::NeedsNavigationKeyLab { key_name } => {
            format!("{key_name} alone needs Labs › Special keys alone: hex labs on special-keys")
        }
        ShortcutRefusal::NeedsSpecialKeysLab { key_name, .. } => {
            format!("{key_name} needs Labs › Special keys alone: hex labs on special-keys")
        }
        ShortcutRefusal::NeedsModifierForKey { key_name } => {
            format!("add a modifier, such as option+{key_name}")
        }
        ShortcutRefusal::InUse => unreachable!("handled as shortcut-in-use"),
    }
}

/// Reads a shortcut such as `option`, `right-command`, `fn`, `f5`,
/// `left-option+right-option` (both Options), or `control+option+space`:
/// modifiers, then at most one key.
pub fn parse_hotkey(
    text: &str,
    key_named: fn(&str) -> Result<HotkeyKey, String>,
) -> Result<HotkeyBinding, String> {
    let mut modifiers = HotkeyModifiers::default();
    let mut key = None;
    let mut sided = false;
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err("expected modifiers and a key joined by +, such as option+space".into());
    }
    for part in parts {
        let lower = part.to_ascii_lowercase();
        let (side, name) = if let Some(rest) =
            lower.strip_prefix("left-").or(lower.strip_prefix("left"))
        {
            (Some(ModifierSide::Left), rest.to_string())
        } else if let Some(rest) = lower.strip_prefix("right-").or(lower.strip_prefix("right")) {
            (Some(ModifierSide::Right), rest.to_string())
        } else {
            (None, lower.clone())
        };
        let slot = match name.as_str() {
            "control" | "ctrl" => Some(&mut modifiers.control),
            "option" | "opt" | "alt" => Some(&mut modifiers.option),
            "shift" => Some(&mut modifiers.shift),
            "command" | "cmd" | "super" | "win" | "meta" => Some(&mut modifiers.command),
            _ => None,
        };
        match slot {
            Some(slot) => {
                sided |= side.is_some();
                *slot = Some(match (*slot, side) {
                    (None, side) => side.unwrap_or(ModifierSide::Either),
                    (Some(ModifierSide::Left), Some(ModifierSide::Right))
                    | (Some(ModifierSide::Right), Some(ModifierSide::Left)) => ModifierSide::Both,
                    _ => return Err(format!("{part} appears twice")),
                });
            }
            None if side.is_none() && matches!(name.as_str(), "fn" | "function" | "globe") => {
                if modifiers.function {
                    return Err(format!("{part} appears twice"));
                }
                modifiers.function = true;
            }
            None if side.is_some() && name.is_empty() => {
                // "left" or "right" alone is the arrow key.
                if key.is_some() {
                    return Err("a shortcut has at most one key".into());
                }
                key = Some(key_named(&lower)?);
            }
            None => {
                if key.is_some() {
                    return Err("a shortcut has at most one key".into());
                }
                key = Some(key_named(part)?);
            }
        }
    }
    let binding = HotkeyBinding { modifiers, key };
    if let Some(key) = &binding.key {
        if sided {
            return Err("left- and right- apply only to shortcuts without a key".into());
        }
        if modifiers.function {
            return Err("fn can't be combined with a key".into());
        }
        if binding.is_bare_primary_click() {
            return Err("mouse1 (left click) needs a modifier, such as option+mouse1".into());
        }
        // Menu, Pause, and Scroll Lock alone, and on Windows the navigation
        // keys alone, are checked against their Labs toggle when set
        // (`special_key_refusal`).
        if modifiers.is_empty()
            && !HotkeyKey::is_function_name(&key.label)
            && !key.is_mouse()
            && !HotkeyKey::special_name(&key.label)
            && !(cfg!(target_os = "windows") && HotkeyKey::navigation_name(&key.label))
            && !(cfg!(any(target_os = "linux", target_os = "macos")) && key.is_section())
            && !(cfg!(target_os = "macos") && key.is_microphone())
        {
            return Err("add a modifier, such as option+".to_string() + &key.label.to_lowercase());
        }
    } else if modifiers.is_empty() {
        return Err("expected at least one modifier or key".into());
    }
    Ok(binding)
}

/// The text `parse_hotkey` reads back: `left-option`, `option+shift+v`,
/// `left-option+right-option`.
pub fn format_hotkey(binding: &HotkeyBinding) -> String {
    let modifiers = binding.modifiers;
    let side = |side: Option<ModifierSide>, name: &str| match side {
        Some(ModifierSide::Left) => Some(format!("left-{name}")),
        Some(ModifierSide::Right) => Some(format!("right-{name}")),
        Some(ModifierSide::Either) => Some(name.to_string()),
        Some(ModifierSide::Both) => Some(format!("left-{name}+right-{name}")),
        None => None,
    };
    let mut parts: Vec<String> = [
        modifiers.function.then(|| "fn".to_string()),
        side(modifiers.control, "control"),
        side(modifiers.option, "option"),
        side(modifiers.shift, "shift"),
        side(modifiers.command, "command"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if let Some(key) = &binding.key {
        parts.push(key.label.to_lowercase().replace(' ', ""));
    }
    parts.join("+")
}
