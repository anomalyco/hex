//! What a shortcut's desktop granted it: the Wayland shortcuts portal's
//! trigger, and the note when GNOME keeps the key for itself.

use rune::El;

use super::super::controls::{Paint, note};
use super::super::{ShortcutTarget, Ui};
use super::binding;
use crate::desktop::host::{DesktopGrantedShortcut, DesktopShortcuts};
use crate::desktop::i18n::{tr, tr_fill};

/// What a shortcuts portal granted for `target`, since the desktop decides
/// which trigger Hex actually gets.
pub(in crate::rune_ui) fn granted(
    ui: &Ui,
    p: Paint,
    target: ShortcutTarget,
) -> Option<(String, rune::Color)> {
    Some(match portal_grant(ui, target)? {
        DesktopGrantedShortcut {
            trigger: Some(trigger),
            ..
        } => (tr_fill("Your desktop granted {}", trigger), p.muted()),
        DesktopGrantedShortcut {
            trigger: None,
            unconfirmed: true,
            ..
        } => (
            tr("Your desktop assigns this key itself, so Hex can't confirm it").to_string(),
            p.muted(),
        ),
        DesktopGrantedShortcut { trigger: None, .. } => (
            tr("Your desktop did not grant this shortcut").to_string(),
            p.danger(),
        ),
    })
}

pub(in crate::rune_ui) fn portal_grant(
    ui: &Ui,
    target: ShortcutTarget,
) -> Option<&DesktopGrantedShortcut> {
    let Some(DesktopShortcuts::Portal(granted)) = ui.shortcuts() else {
        return None;
    };
    let name = match target {
        ShortcutTarget::Dictation => "Dictation",
        ShortcutTarget::PasteLast => "Paste last dictation",
        ShortcutTarget::RawDictation => "Dictate without processing",
        #[cfg(target_os = "linux")]
        ShortcutTarget::TypedText => "Transform typed text",
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        ShortcutTarget::TerminalPaste => "Terminal paste shortcut",
        // The portal never holds the cancel key.
        ShortcutTarget::Cancel => return None,
        // The portal lists Annotate's shortcut under that name.
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "Annotate",
        #[cfg(all(hex_capture, target_os = "macos"))]
        ShortcutTarget::CaptureReplay => return None,
        // Only Linux lists this one under its own name; macOS and Windows
        // have no shortcuts portal.
        #[cfg(target_os = "linux")]
        ShortcutTarget::SecondDictation => "Second dictation shortcut",
        #[cfg(not(target_os = "linux"))]
        ShortcutTarget::SecondDictation => return None,
        #[cfg(target_os = "macos")]
        ShortcutTarget::Screenshot
        | ShortcutTarget::ScreenshotRecord
        | ShortcutTarget::ExtraDictation(_) => return None,
    };
    granted.iter().find(|shortcut| shortcut.name == name)
}

/// Keys GNOME keeps for itself by default (window menu, input sources, app
/// switching, the overview, screenshots, …), normalized as [`chord`] writes
/// them. A portal shortcut on one is listed in GNOME Settings but never
/// reaches Hex (#723).
const GNOME_KEYS: &[&str] = &[
    "alt+space",
    "super+space",
    "shift+super+space",
    "alt+tab",
    "alt+shift+tab",
    "super+tab",
    "alt+f1",
    "alt+f2",
    "alt+f4",
    "alt+f7",
    "alt+f8",
    "alt+f10",
    "super+a",
    "super+h",
    "super+l",
    "super+s",
    "super+v",
    "super+up",
    "super+down",
    "super+left",
    "super+right",
    "print",
    "alt+print",
    "shift+print",
    "alt+ctrl+delete",
];

/// `keys` (`Ctrl+Alt+Space`, `Super+D`) lowercased with its modifiers in
/// one order, so the same chord always reads the same.
fn chord(keys: &str) -> String {
    let mut parts = keys
        .split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "control" => "ctrl".to_string(),
            "meta" | "logo" => "super".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>();
    let key = parts.pop().unwrap_or_default();
    parts.sort();
    parts.push(key);
    parts.join("+")
}

pub(super) fn gnome_keeps(keys: &str) -> bool {
    GNOME_KEYS.contains(&chord(keys).as_str())
}

#[cfg(target_os = "linux")]
pub(in crate::rune_ui) fn on_gnome() -> bool {
    crate::linux::portal::gnome()
}

#[cfg(not(target_os = "linux"))]
pub(in crate::rune_ui) fn on_gnome() -> bool {
    false
}

/// On GNOME, the note under `target`'s row when its key, the one GNOME
/// granted or else the one Hex asks for, is one GNOME keeps for itself.
pub(in crate::rune_ui) fn gnome_clash_note(
    ui: &Ui,
    p: Paint,
    target: ShortcutTarget,
) -> Option<El> {
    if !on_gnome() {
        return None;
    }
    let keys = match portal_grant(ui, target) {
        Some(DesktopGrantedShortcut {
            trigger: Some(trigger),
            ..
        }) => trigger.clone(),
        _ => binding(&ui.settings, target)?.keycaps().join("+"),
    };
    gnome_keeps(&keys).then(|| {
        note(
            "gnome-clash",
            &tr_fill(
                "GNOME uses {} itself, so it never reaches Hex. Record another shortcut, or change it in GNOME Settings › Keyboard › Keyboard Shortcuts.",
                &keys,
            ),
            p.danger(),
        )
    })
}
