//! Hex's own GNOME custom keyboard shortcuts, for GNOME without a
//! GlobalShortcuts portal (GNOME 46 and older, #781): entries in GNOME
//! Settings › Keyboard › Custom Shortcuts that run `hex` commands. Hex owns
//! only the entries under its own paths (`…/custom-keybindings/hex-*/`) and
//! leaves every other custom shortcut alone. GNOME runs a custom shortcut on
//! the key press only, so dictation toggles there.

use color_eyre::eyre::Result;

use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::linux::gsettings::{gsettings, string_list, string_list_text, string_text};

const LIST_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
/// Every entry Hex may write, by path name.
const ENTRIES: [&str; 3] = ["hex-dictation", "hex-paste-last", "hex-annotate"];

/// One custom shortcut: its path name, its name in GNOME Settings, the `hex`
/// arguments it runs, and its GTK accelerator.
#[derive(Debug, PartialEq)]
struct Entry {
    id: &'static str,
    name: &'static str,
    arguments: &'static str,
    accelerator: String,
}

/// Writes Hex's entries for `settings`' shortcuts, starting `hex`, and
/// removes Hex's entries it no longer has a shortcut for. An entry already
/// listed keeps its key, which the person may have changed in GNOME Settings.
pub(crate) fn install(settings: &AppSettings, hex: &std::path::Path) -> Result<()> {
    let entries = entries(settings);
    let listed = string_list(&gsettings(&["get", LIST_SCHEMA, "custom-keybindings"])?);
    let command_path = hex.to_string_lossy();
    for entry in &entries {
        let path = format!("{ENTRY_SCHEMA}:{BASE}{}/", entry.id);
        let command = format!("{} {}", shell_word(&command_path), entry.arguments);
        gsettings(&["set", &path, "name", &string_text(entry.name)])?;
        gsettings(&["set", &path, "command", &string_text(&command)])?;
        if !listed.contains(&format!("{BASE}{}/", entry.id)) {
            gsettings(&["set", &path, "binding", &string_text(&entry.accelerator)])?;
        }
    }
    let kept = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();
    write_list(&kept)
}

/// Removes every entry Hex wrote.
pub(crate) fn remove() -> Result<()> {
    write_list(&[])
}

/// Lists Hex's entries `kept` (and every entry that isn't Hex's) in GNOME's
/// custom shortcuts, and resets Hex's others.
fn write_list(kept: &[&str]) -> Result<()> {
    let listed = string_list(&gsettings(&["get", LIST_SCHEMA, "custom-keybindings"])?);
    let ours = |path: &str| {
        ENTRIES
            .iter()
            .find(|id| path == format!("{BASE}{id}/"))
            .copied()
    };
    for id in ENTRIES.iter().filter(|id| !kept.contains(id)) {
        if listed.iter().any(|path| ours(path) == Some(id)) {
            gsettings(&["reset-recursively", &format!("{ENTRY_SCHEMA}:{BASE}{id}/")])?;
        }
    }
    let mut paths = listed
        .iter()
        .filter(|path| ours(path).is_none())
        .cloned()
        .collect::<Vec<_>>();
    paths.extend(kept.iter().map(|id| format!("{BASE}{id}/")));
    let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
    gsettings(&[
        "set",
        LIST_SCHEMA,
        "custom-keybindings",
        &string_list_text(&paths),
    ])?;
    Ok(())
}

/// Hex's entries for the shortcuts `settings` records. A shortcut GNOME
/// can't bind (modifiers alone, a mouse button) gets none.
fn entries(settings: &AppSettings) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut add = |id, name, arguments, binding: Option<&HotkeyBinding>| {
        if let Some(accelerator) = binding.and_then(accelerator) {
            entries.push(Entry {
                id,
                name,
                arguments,
                accelerator,
            });
        }
    };
    add(
        "hex-dictation",
        "Hex: dictation",
        "dictation toggle",
        Some(&settings.dictation_hotkey),
    );
    add(
        "hex-paste-last",
        "Hex: paste last dictation",
        "paste-last",
        settings.paste_last_hotkey.as_ref(),
    );
    #[cfg(hex_capture)]
    if settings.capture.enabled {
        add(
            "hex-annotate",
            "Hex: annotate",
            "annotate toggle",
            Some(&settings.capture.hotkey),
        );
    }
    entries
}

/// `binding` as a GTK accelerator (`<Control><Alt>space`), when GNOME can
/// bind it.
fn accelerator(binding: &HotkeyBinding) -> Option<String> {
    let key = binding.key.as_ref().filter(|key| !key.is_mouse())?;
    let mut accelerator = String::new();
    let modifiers = &binding.modifiers;
    for (held, name) in [
        (modifiers.control.is_some(), "<Control>"),
        (modifiers.option.is_some(), "<Alt>"),
        (modifiers.shift.is_some(), "<Shift>"),
        (modifiers.command.is_some(), "<Super>"),
    ] {
        if held {
            accelerator.push_str(name);
        }
    }
    accelerator.push_str(&keysym(&key.name()));
    Some(accelerator)
}

/// A key's lowercase name as the X keysym GTK reads.
fn keysym(name: &str) -> String {
    let named = match name {
        "return" | "enter" => "Return",
        "tab" => "Tab",
        "escape" => "Escape",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "print" => "Print",
        "pause" => "Pause",
        _ => "",
    };
    if !named.is_empty() {
        return named.into();
    }
    match name.strip_prefix('f') {
        Some(number) if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) => {
            format!("F{number}")
        }
        _ => name.into(),
    }
}

/// `word` as one argument for GNOME, which splits the command as a shell
/// would.
fn shell_word(word: &str) -> String {
    if word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        return word.into();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{HotkeyKey, HotkeyModifiers};

    fn chord(modifiers: HotkeyModifiers, key: &str) -> HotkeyBinding {
        HotkeyBinding {
            modifiers,
            key: Some(HotkeyKey::named(key)),
        }
    }

    #[test]
    fn shortcuts_read_as_gtk_accelerators() {
        assert_eq!(
            accelerator(&chord(HotkeyModifiers::control(), "space")).as_deref(),
            Some("<Control>space")
        );
        assert_eq!(
            accelerator(&chord(HotkeyModifiers::control_option(), "v")).as_deref(),
            Some("<Control><Alt>v")
        );
        assert_eq!(
            accelerator(&chord(HotkeyModifiers::default(), "f9")).as_deref(),
            Some("F9")
        );
        let modifiers_only = HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: None,
        };
        assert_eq!(accelerator(&modifiers_only), None);
    }

    #[test]
    fn dictation_toggles_and_paths_quote() {
        let settings = AppSettings {
            dictation_hotkey: chord(HotkeyModifiers::control(), "space"),
            paste_last_hotkey: None,
            ..AppSettings::default()
        };
        let entries = entries(&settings);
        assert_eq!(entries[0].arguments, "dictation toggle");
        assert!(entries.iter().all(|entry| entry.id != "hex-paste-last"));
        assert_eq!(
            shell_word("/home/a/.local/bin/hex"),
            "/home/a/.local/bin/hex"
        );
        assert_eq!(shell_word("/home/a b/hex"), "'/home/a b/hex'");
    }
}
