//! The Scrawl pane (the `capture` experiment, macOS and Linux): captures
//! inside Hex. The opt-in, the capture shortcut,
//! the send-to list's shortcut, Screen Recording, the transcriber, the captures folder and how long
//! captures are kept, and Scrawl's own app, which keeps the shortcut while
//! it runs until "Use Hex for captures" asks it to quit.

use rune::{El, row};
use scrawl_capture::retention::{Policy, Preset};

use super::controls::{
    ButtonKind, Choice, Paint, card, described_row, folding, note, page, section, segmented,
    select, setting_row, toggle_row,
};
use super::lists::{PermissionControl, permission_control};
use super::platform::{PlatformRequest, ScrawlRequest, ScrawlView};
use super::settings::{cancel_capture, shortcut_control};
use super::{Menu, ShortcutTarget, Ui};
use crate::app_settings::{AppSettings, CaptureTranscriber};
use crate::desktop_i18n::{tr, tr_fill};
use crate::onboarding::{PermissionAction, PermissionKind};

fn description(enabled: bool) -> &'static str {
    if enabled {
        tr("Record your screen and voice with the shortcut below")
    } else {
        tr("Off by default. Hex records nothing and keeps no shortcut until you turn this on.")
    }
}

/// Turns captures on or off; turning them on fails while the capture
/// shortcut or the send-to list's collides with another, and leaves the
/// shortcuts as they are.
fn set_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled
        && super::settings::conflicts(settings, ShortcutTarget::Capture, &settings.capture.hotkey)
    {
        return Err(tr(
            "The capture shortcut is already in use. Change it below before turning captures on.",
        ));
    }
    if enabled
        && super::settings::conflicts(
            settings,
            ShortcutTarget::CaptureList,
            &settings.capture.list_hotkey,
        )
    {
        return Err(tr(
            "The send-to list shortcut is already in use. Change it below before turning captures on.",
        ));
    }
    settings.capture.enabled = enabled;
    Ok(())
}

fn toggle(ui: &mut Ui) {
    cancel_capture(ui);
    let enabled = !ui.settings.capture.enabled;
    let mut candidate = ui.drafted();
    let error = match set_enabled(&mut candidate, enabled) {
        Ok(()) if ui.commit(candidate) => None,
        Ok(()) => ui.error.clone(),
        Err(error) => Some(error.to_string()),
    };
    if let Some(view) = ui.platform.scrawl.as_mut() {
        view.enable_error = error;
    }
}

fn transcriber_description(transcriber: CaptureTranscriber) -> &'static str {
    match transcriber {
        CaptureTranscriber::Parakeet => {
            tr("The model Hex dictates with, with a time for every word")
        }
        CaptureTranscriber::Apple => tr("Apple's speech recognizer, as Scrawl uses"),
    }
}

/// What the Keep menu calls `policy`: one of Scrawl's presets, or the days
/// imported from Scrawl.
fn retention_label(policy: Policy) -> String {
    match Preset::ALL
        .into_iter()
        .find(|preset| preset.policy() == policy)
    {
        Some(Preset::Week) => tr("Video for 7 days").into(),
        Some(Preset::Month) => tr("Video for 30 days").into(),
        Some(Preset::Forever) => tr("Forever").into(),
        None => match policy.media_days {
            Some(days) => tr_fill("Video for {} days", &days.to_string()),
            None => tr("Forever").into(),
        },
    }
}

fn retention_description(policy: Policy) -> String {
    match policy.bundle_days {
        Some(days) => tr_fill(
            "Video and audio go first; captures are removed after {} days",
            &days.to_string(),
        ),
        None if policy.media_days.is_some() => {
            tr("Video and audio go first; captures stay until you remove them").into()
        }
        None => tr("Captures stay until you remove them").into(),
    }
}

/// The captures folder, with the home folder as `~`.
fn folder_label(ui: &Ui) -> String {
    let folder = ui.settings.capture.captures_dir();
    dirs::home_dir()
        .and_then(|home| {
            folder
                .strip_prefix(home)
                .ok()
                .map(|relative| format!("~/{}", relative.display()))
        })
        .unwrap_or_else(|| folder.display().to_string())
}

fn scrawl_request(ui: &mut Ui, request: ScrawlRequest) {
    ui.ask(PlatformRequest::Scrawl(request));
}

/// While Scrawl's own app runs it keeps the shortcut; only this card's
/// button asks it to quit.
fn scrawl_running(p: Paint, view: &ScrawlView) -> El {
    let description = if view.quitting_scrawl {
        tr("Waiting for Scrawl to quit…")
    } else {
        tr("Hex leaves the capture shortcut to Scrawl until you switch")
    };
    card(
        p,
        "scrawl-running",
        vec![setting_row(
            p,
            "scrawl-running-row",
            tr("Scrawl is running"),
            Some(description),
            super::controls::button(
                p,
                "use-hex-for-captures",
                tr("Use Hex for captures"),
                ButtonKind::Primary,
                |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::QuitScrawl),
            ),
        )],
    )
}

fn shortcut_rows(ui: &Ui, p: Paint, view: &ScrawlView) -> Vec<El> {
    let enabled = ui.settings.capture.enabled;
    let purpose = if enabled {
        tr("Press to start and again to stop; Esc cancels")
    } else {
        tr("Saved for when captures are on")
    };
    let mut rows = vec![described_row(
        p,
        "capture-shortcut",
        tr("Shortcut"),
        Some((purpose.to_string(), p.muted())),
        shortcut_control(ui, p, ShortcutTarget::Capture),
    )];
    let list_purpose = if enabled {
        tr("Opens the list of sessions while a capture is showing or waiting to be sent")
    } else {
        tr("Saved for when captures are on")
    };
    rows.push(described_row(
        p,
        "capture-list-shortcut",
        tr("Send-to list"),
        Some((list_purpose.to_string(), p.muted())),
        shortcut_control(ui, p, ShortcutTarget::CaptureList),
    ));
    // Linux asks through the desktop's screen-cast portal when a capture
    // starts, and Wayland can't keep an overlay out of a recording.
    if enabled && cfg!(target_os = "macos") {
        rows.push(setting_row(
            p,
            "screen-recording",
            tr("Screen Recording"),
            Some(if view.screen_recording {
                tr("Lets a capture record your screen")
            } else {
                tr("Required to record your screen. macOS asks at your first capture, or allow it here.")
            }),
            permission_control(
                p,
                "screen-recording-allow",
                if view.screen_recording {
                    PermissionControl::Allowed
                } else if ui.platform.asked(PermissionKind::ScreenRecording) {
                    PermissionControl::Waiting
                } else {
                    PermissionControl::Allow
                },
                ButtonKind::Secondary,
                |ui: &mut Ui| {
                    ui.ask(PlatformRequest::Permission(PermissionAction::ShowGuide(
                        PermissionKind::ScreenRecording,
                    )))
                },
            ),
        ));
        rows.push(recordings_row(ui, p));
    }
    rows
}

/// Lets screen recordings, captures included, show Hex's overlays, for
/// showing someone how captures work.
fn recordings_row(ui: &Ui, p: Paint) -> El {
    toggle_row(
        p,
        "capture-show-in-recordings",
        tr("Show in screen recordings"),
        tr("Lets recordings, captures included, show Hex's capsule, ink, and HUD, for demos"),
        ui.settings.capture.show_overlays_in_recordings,
        |ui: &mut Ui| {
            ui.update(|settings| {
                settings.capture.show_overlays_in_recordings =
                    !settings.capture.show_overlays_in_recordings;
            });
        },
    )
}

fn transcriber_row(ui: &Ui, p: Paint) -> El {
    let transcriber = ui.settings.capture.transcriber;
    let selected = CaptureTranscriber::ALL
        .iter()
        .position(|choice| *choice == transcriber)
        .unwrap_or(0);
    setting_row(
        p,
        "capture-transcriber",
        tr("Transcriber"),
        Some(transcriber_description(transcriber)),
        segmented(
            p,
            "capture-transcriber-choice",
            &["Parakeet", "Apple"],
            selected,
            |ui: &mut Ui, index| {
                let transcriber = CaptureTranscriber::ALL[index];
                ui.update(|settings| settings.capture.transcriber = transcriber);
            },
        ),
    )
}

fn storage_rows(ui: &Ui, p: Paint, view: &ScrawlView, layer: &mut Vec<El>) -> Vec<El> {
    use super::controls::button;
    let policy = ui.settings.capture.retention();
    let mut choices: Vec<Choice> = Preset::ALL
        .into_iter()
        .map(|preset| Choice {
            label: retention_label(preset.policy()),
            detail: None,
            selected: preset.policy() == policy,
            choose: Box::new(move |ui: &mut Ui| {
                ui.open = None;
                ui.update(|settings| settings.capture.set_retention(preset.policy()));
            }),
        })
        .collect();
    if !choices.iter().any(|choice| choice.selected) {
        choices.push(Choice {
            label: retention_label(policy),
            detail: None,
            selected: true,
            choose: Box::new(|ui: &mut Ui| ui.open = None),
        });
    }
    let kept = retention_description(policy);
    let mut rows = vec![
        setting_row(
            p,
            "capture-folder",
            tr("Captures folder"),
            Some(tr("Shared with Scrawl, so both list the same captures")),
            row()
                .key("controls")
                .gap(8.0)
                .center_y()
                .child(button(
                    p,
                    "reveal-capture-folder",
                    tr("Reveal"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::RevealFolder),
                ))
                .child(button(
                    p,
                    "choose-capture-folder",
                    &folder_label(ui),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::ChooseFolder),
                )),
        ),
        setting_row(
            p,
            "capture-retention",
            tr("Keep"),
            Some(kept.as_str()),
            select(
                p,
                "capture-retention-menu",
                &retention_label(policy),
                ui.open == Some(Menu::CaptureRetention),
                choices,
                |ui: &mut Ui| {
                    ui.open =
                        (ui.open != Some(Menu::CaptureRetention)).then_some(Menu::CaptureRetention);
                },
                layer,
            ),
        ),
    ];
    if let Some(error) = &view.folder_error {
        rows.push(note("capture-folder-error", error, p.danger()));
    }
    rows
}

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let view = ui.platform.scrawl.clone().unwrap_or_default();
    let enabled = ui.settings.capture.enabled;
    let mut content = page("content");
    content = content.child(card(
        p,
        "enable",
        vec![
            toggle_row(
                p,
                "capture-enabled",
                tr("Capture with Hex"),
                description(enabled),
                enabled,
                toggle,
            )
            .into(),
            folding(
                "capture-error-fold",
                view.enable_error.as_ref().map(|error| {
                    note(
                        "capture-error",
                        &format!("{} {error}", tr("Could not update captures:")),
                        p.danger(),
                    )
                }),
            ),
        ],
    ));
    if enabled && view.scrawl_running {
        content = content.child(scrawl_running(p, &view));
    }
    content = content.child(section(
        p,
        "capture",
        tr("Capture"),
        shortcut_rows(ui, p, &view),
    ));
    // Apple's speech recognizer is macOS's; Linux transcribes with Hex's model.
    if enabled && cfg!(target_os = "macos") {
        content = content.child(section(
            p,
            "transcription",
            tr("Transcription"),
            vec![transcriber_row(ui, p)],
        ));
    }
    if enabled {
        content = content.child(section(
            p,
            "storage",
            tr("Storage"),
            storage_rows(ui, p, &view, layer),
        ));
    }
    super::scroll_body("scroll", content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn captures_default_off_and_refuse_a_shortcut_in_use() {
        let settings = AppSettings::default();
        assert!(!settings.capture.enabled);
        let mut taken = AppSettings::default();
        taken.capture.hotkey = taken.dictation_hotkey.clone();
        let before = serde_json::to_value(&taken).unwrap();
        assert!(
            set_enabled(&mut taken, true)
                .unwrap_err()
                .contains("already in use")
        );
        assert_eq!(serde_json::to_value(&taken).unwrap(), before);
        // Turning captures off always works.
        set_enabled(&mut taken, false).unwrap();
        // The send-to list's ⌥⇧V is Paste Last's default: rebind one first.
        let mut list = AppSettings::default();
        assert!(
            set_enabled(&mut list, true)
                .unwrap_err()
                .contains("send-to list")
        );
        list.paste_last_hotkey = Some(crate::app_settings::HotkeyBinding {
            modifiers: crate::app_settings::HotkeyModifiers::option_command(),
            key: Some(crate::app_settings::HotkeyKey::named("v")),
        });
        set_enabled(&mut list, true).unwrap();
        assert!(list.capture.enabled);
    }

    /// The Scrawl pane drawn from `scene`, laid out headlessly.
    fn pane(scene: &str) -> rune::Runtime<Ui> {
        use crate::rune_ui::{SCENES, Service};
        let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        let (_, apply) = SCENES.iter().find(|(name, _)| *name == scene).unwrap();
        apply(&mut state);
        let env = rune::Env {
            system_dark: true,
            scale: 1.0,
        };
        let text =
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let mut rt =
            rune::Runtime::with_text(state, crate::rune_ui::view, (980.0, 760.0), env, text);
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    fn tap(rt: &mut rune::Runtime<Ui>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn asked(rt: &rune::Runtime<Ui>) -> &[PlatformRequest] {
        match &rt.state.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => sent,
            _ => panic!("scenes record platform requests"),
        }
    }

    #[cfg(target_os = "macos")]
    const ALLOW: &str = "screen-recording-allow";

    #[cfg(target_os = "macos")]
    #[test]
    fn granted_screen_recording_shows_allowed_and_asks_nothing() {
        let mut rt = pane("mac-scrawl");
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/ink/check"))
                .is_some(),
            "already granted reads Allowed"
        );
        tap(&mut rt, ALLOW);
        assert!(asked(&rt).is_empty(), "Allowed is not a button");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn screen_recording_allow_opens_the_permission_guide() {
        let mut rt = pane("mac-scrawl-permission");
        tap(&mut rt, ALLOW);
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(PermissionAction::ShowGuide(
                PermissionKind::ScreenRecording
            ))]
        );
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/dot")).is_some(),
            "waits with a dot"
        );
        // The fixture grants on the second ask.
        tap(&mut rt, ALLOW);
        assert!(rt.state.platform.scrawl.as_ref().unwrap().screen_recording);
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/ink/check"))
                .is_some()
        );
    }

    #[test]
    fn opening_the_pane_reads_screen_recording_again() {
        let mut rt = pane("mac-scrawl");
        tap(&mut rt, "nav-general");
        tap(&mut rt, "nav-scrawl");
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Scrawl(ScrawlRequest::Refresh)]
        );
    }

    #[test]
    fn retention_names_presets_and_imported_days() {
        assert_eq!(retention_label(Preset::Week.policy()), "Video for 7 days");
        assert_eq!(retention_label(Preset::Forever.policy()), "Forever");
        let imported = Policy {
            media_days: Some(3),
            bundle_days: Some(30),
        };
        assert_eq!(retention_label(imported), "Video for 3 days");
        assert!(retention_description(imported).contains("30 days"));
    }
}
