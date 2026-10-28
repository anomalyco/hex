//! The Voice Action pane: the opt-in, its shortcut, and the OpenCode model
//! that turns spoken instructions into paste-ready text. Enabling checks the
//! saved shortcut against the others first.

use rune::El;

use super::ShortcutTarget;
use super::Ui;
use super::controls::{Paint, card, described_row, folding, note, page, section, toggle_row};
use super::models::{self, Target};
use super::settings::{cancel_capture, shortcut_control};
use crate::app_settings::AppSettings;
use crate::desktop_i18n::tr;
use crate::opencode_catalog::OpenCodeFeature;

fn description(enabled: bool) -> &'static str {
    if enabled {
        tr("Use the shortcut below with OpenCode to run spoken instructions")
    } else {
        tr("Off by default. Its saved shortcut is not reserved until you enable Voice Action.")
    }
}

/// Turns Voice Action on or off; enabling fails while its saved shortcut
/// collides with another, and leaves the shortcut as it is.
fn set_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled
        && super::settings::conflicts(settings, ShortcutTarget::VoiceAction, &settings.edit_hotkey)
    {
        return Err(tr(
            "The Voice Action shortcut is already in use. Change it below or change the other shortcut before enabling Voice Action.",
        ));
    }
    settings.voice_action.enabled = enabled;
    Ok(())
}

fn toggle(ui: &mut Ui) {
    cancel_capture(ui);
    let enabled = !ui.settings.voice_action.enabled;
    let mut candidate = ui.drafted();
    match set_enabled(&mut candidate, enabled) {
        Ok(()) => {
            if ui.commit(candidate) {
                ui.voice_action_error = None;
            } else {
                ui.voice_action_error = ui.error.clone();
            }
        }
        Err(error) => ui.voice_action_error = Some(error.to_string()),
    }
}

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let enabled = ui.settings.voice_action.enabled;
    // The switch stays reachable without OpenCode; it says why it won't work.
    let reason = (!ui.models.checking())
        .then(|| models::unavailable_reason(ui, OpenCodeFeature::VoiceAction))
        .flatten();
    let mut content = page("content");
    let enable = vec![
        toggle_row(
            p,
            "voice-action-enabled",
            tr("Enable Voice Action"),
            reason.unwrap_or_else(|| description(enabled)),
            enabled,
            toggle,
        )
        .into(),
        folding(
            "voice-action-error-fold",
            ui.voice_action_error.as_ref().map(|error| {
                note(
                    "voice-action-error",
                    &format!("{} {error}", tr("Could not update Voice Action:")),
                    p.danger(),
                )
            }),
        ),
    ];
    content = content.child(card(p, "enable", enable));
    if ui.records_shortcuts() {
        let mut rows = vec![described_row(
            p,
            "voice-action-shortcut",
            tr("Shortcut"),
            Some(
                super::settings::granted(ui, p, ShortcutTarget::VoiceAction).unwrap_or_else(|| {
                    let purpose = if enabled {
                        tr("Hold to speak; selected text is included automatically")
                    } else {
                        tr("Saved for when Voice Action is enabled")
                    };
                    (purpose.to_string(), p.muted())
                }),
            ),
            shortcut_control(ui, p, ShortcutTarget::VoiceAction),
        )];
        rows.extend(super::settings::refused_note(
            ui,
            p,
            ShortcutTarget::VoiceAction,
        ));
        content = content.child(section(p, "capture", tr("Capture"), rows));
    }
    if enabled {
        let rows = match models::unavailable(ui, p, OpenCodeFeature::VoiceAction) {
            Some(notice) => vec![notice],
            None => models::rows(ui, p, Target::VoiceAction, layer),
        };
        content = content.child(section(p, "processing", tr("Processing"), rows));
    }
    super::scroll_body("scroll", content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeyBinding;

    #[test]
    fn voice_action_defaults_off_and_explains_its_inactive_shortcut() {
        let settings = AppSettings::default();
        assert!(!settings.voice_action.enabled);
        let off = description(settings.voice_action.enabled);
        assert!(off.contains("Off by default"));
        assert!(off.contains("not reserved"));

        let on = description(true);
        assert!(on.contains("OpenCode"));
        assert!(!on.contains("not reserved"));
    }

    #[test]
    fn voice_action_opt_in_rejects_active_conflicts_without_rebinding() {
        let defaults = AppSettings::default();
        let mut active = vec![
            defaults.dictation_hotkey,
            defaults.paste_last_hotkey.unwrap(),
        ];
        if cfg!(target_os = "macos") && crate::DEVELOPER_FEATURES_ENABLED {
            active.push(HotkeyBinding::paste_meeting_default());
        }
        for binding in active {
            let mut settings = AppSettings {
                edit_hotkey: binding,
                ..AppSettings::default()
            };
            let before = serde_json::to_value(&settings).unwrap();
            let error = set_enabled(&mut settings, true).unwrap_err();

            assert!(error.contains("already in use"));
            assert!(error.contains("Change it below"));
            assert_eq!(serde_json::to_value(&settings).unwrap(), before);
        }
    }

    #[test]
    fn voice_action_toggle_preserves_configuration_and_can_always_disable() {
        let mut settings: AppSettings = serde_json::from_value(serde_json::json!({
            "voice_action": {
                "model": "example/saved-model",
                "variant": "high",
                "deadline_seconds": 45
            },
            "edit_hotkey": {
                "modifiers": { "control": "left" },
                "key": null
            }
        }))
        .unwrap();
        assert!(!settings.voice_action.enabled);
        let mut expected = serde_json::to_value(&settings).unwrap();
        for enabled in [true, false] {
            set_enabled(&mut settings, enabled).unwrap();
            expected["voice_action"]["enabled"] = serde_json::json!(enabled);
            assert_eq!(serde_json::to_value(&settings).unwrap(), expected);
        }

        set_enabled(&mut settings, true).unwrap();
        settings.dictation_hotkey = settings.edit_hotkey.clone();
        set_enabled(&mut settings, false).unwrap();
        assert!(!settings.voice_action.enabled);
    }
}
