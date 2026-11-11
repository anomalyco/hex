//! Shortcut text for `hex settings`: reading and writing the modifier-plus-key
//! form, and why a bare special key still needs its Labs toggle.

use crate::app_settings::{
    AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, HotkeySlot, ModifierSide,
};

use super::{ShortcutRules, invalid};

/// Applies the checks a recorded shortcut meets: Labs › Mouse shortcuts
/// where mouse buttons are recorded, the special-key rules, the OS's own
/// rule for `slot`, and no key shared with another active shortcut.
pub(super) fn check_shortcut(
    settings: &AppSettings,
    candidate: &AppSettings,
    name: &str,
    slot: HotkeySlot,
    binding: &HotkeyBinding,
    shortcuts: &ShortcutRules,
) -> Result<(), crate::control::ControlError> {
    if cfg!(target_os = "macos")
        && !candidate.labs.mouse_shortcuts
        && binding.key.as_ref().is_some_and(HotkeyKey::is_mouse)
    {
        return Err(invalid(
            name,
            "mouse buttons need Labs › Mouse shortcuts: hex labs on mouse-shortcuts",
        ));
    }
    if let Some(why) = special_key_refusal(candidate, binding) {
        return Err(invalid(name, &why));
    }
    (shortcuts.rule)(slot, binding).map_err(|why| invalid(name, &why))?;
    if settings.hotkey_in_use(slot, binding) {
        return Err(crate::control::ControlError::new(
            "shortcut-in-use",
            format!(
                "{} is already in use by another Hex shortcut",
                format_hotkey(binding)
            ),
        ));
    }
    Ok(())
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
            && !is_function_key(&key.label)
            && !key.is_mouse()
            && !HotkeyKey::special_name(&key.label)
            && !(cfg!(target_os = "windows") && HotkeyKey::navigation_name(&key.label))
            && !(cfg!(target_os = "linux") && HotkeyKey::section_name(&key.label))
        {
            return Err("add a modifier, such as option+".to_string() + &key.label.to_lowercase());
        }
    } else if modifiers.is_empty() {
        return Err("expected at least one modifier or key".into());
    }
    Ok(binding)
}

/// Why `binding` can't be saved for its Menu, Pause, or Scroll Lock key: on
/// Linux they need Labs › Special keys alone; Windows takes them alone, and
/// a navigation key alone needs the toggle there; elsewhere one alone still
/// needs a modifier.
pub(super) fn special_key_refusal(
    settings: &AppSettings,
    binding: &HotkeyBinding,
) -> Option<String> {
    // The § key alone needs Labs › Section key alone (Linux).
    if cfg!(target_os = "linux") && binding.is_lone_section_key() && !settings.section_key_allowed()
    {
        return Some("section needs Labs › Section key alone: hex labs on section-key".into());
    }
    if cfg!(target_os = "windows") {
        let key = binding.key.as_ref()?;
        return (binding.is_lone_navigation_key() && !settings.special_keys_allowed()).then(|| {
            format!(
                "{} alone needs Labs › Special keys alone: hex labs on special-keys",
                key.label.to_lowercase().replace(' ', "")
            )
        });
    }
    let key = binding.key.as_ref().filter(|key| key.is_special())?;
    if settings.special_keys_allowed() {
        return None;
    }
    let name = key.label.to_lowercase().replace(' ', "");
    if cfg!(target_os = "linux") {
        return Some(format!(
            "{name} needs Labs › Special keys alone: hex labs on special-keys"
        ));
    }
    binding
        .modifiers
        .is_empty()
        .then(|| format!("add a modifier, such as option+{name}"))
}

fn is_function_key(label: &str) -> bool {
    label
        .strip_prefix(['F', 'f'])
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
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
