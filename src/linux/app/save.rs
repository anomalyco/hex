//! Saving the host's settings and the small keys it syncs: the settings file
//! transaction, the microphone/launch preference, and what the GNOME custom
//! shortcuts were last written for.

use color_eyre::Result;

use crate::app_settings::AppSettings;

/// Saves one host-owned change on top of the current `settings.json`, so
/// edits the window saved meanwhile survive.
/// Annotate's shortcut, while Annotate is on.
pub(super) fn capture_shortcut(
    settings: &AppSettings,
) -> Option<&crate::app_settings::HotkeyBinding> {
    #[cfg(hex_capture)]
    if settings.capture.enabled {
        return Some(&settings.capture.hotkey);
    }
    let _ = settings;
    None
}

/// What the GNOME custom shortcuts were last synced for: a new key means an
/// entry was added, removed, or rebound. Typed text and the model switch are
/// included while their labs are on and their shortcuts are bound, so
/// toggling either resyncs.
pub(super) fn gnome_shortcuts_key(settings: &AppSettings, wanted: bool) -> String {
    format!(
        "{wanted} {:?} {:?} {:?} {:?} {:?}",
        settings.dictation_hotkey,
        settings.paste_last_hotkey,
        capture_shortcut(settings),
        settings.active_typed_text_hotkey(),
        settings.active_model_switch_hotkey()
    )
}

pub(super) fn settings_modified() -> Option<std::time::SystemTime> {
    let path = crate::app_settings::path().ok()?;
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub(super) fn save_with(change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings> {
    let mut settings = AppSettings::load()?;
    let base = settings.clone();
    change(&mut settings);
    base.update(&settings)
}

#[cfg(not(test))]
pub(super) fn save_listening(_: &AppSettings, enabled: bool) -> Result<AppSettings> {
    save_with(|settings| settings.enable_on_launch = enabled)
}

/// Tests never write the person's `settings.json`.
#[cfg(test)]
pub(super) fn save_listening(current: &AppSettings, enabled: bool) -> Result<AppSettings> {
    let mut settings = current.clone();
    settings.enable_on_launch = enabled;
    Ok(settings)
}
