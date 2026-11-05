//! The Annotate pane (macOS and Linux): Scrawl's captures inside Hex. The
//! opt-in with Screen Recording while it's missing and the capture
//! shortcut, then what happens after an annotation (how long it's kept and
//! its folder), and folded away the transcriber, demos, and the replay Lab.
//! Scrawl's own app keeps the shortcut while it runs until "Use Hex for
//! annotations" asks it to quit.

use rune::{El, col, row};
use scrawl_capture::retention::{Policy, Preset};

use super::controls::{
    ButtonKind, CardRow, Choice, METRICS, Paint, button, card, folding, icon_described_row,
    icon_row, icon_toggle_row, note, page, section, segmented, select, setting_row, step_row,
    sub_row,
};
use super::general::{advanced_open, disclosure, disclosure_link, keys_button};
use super::icons::{CLOCK, FOLDER, KEYBOARD, WAVE};
use super::lists::permission_control;
use super::platform::{PlatformRequest, ScrawlRequest, ScrawlView};
use super::settings::cancel_capture;
use super::sidebar::SCRAWL_ICON;
use super::{Menu, Pane, ShortcutTarget, Ui};
use crate::app_settings::{AppSettings, CaptureTranscriber};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::desktop::onboarding::{PermissionAction, PermissionKind};

/// The switch's second line: the whole gesture with the bound shortcut.
fn description(ui: &Ui, enabled: bool) -> String {
    if enabled {
        let separator = if cfg!(target_os = "macos") { "" } else { "+" };
        tr_fill(
            "Tap {}, talk and draw; tap again and it's copied",
            &ui.settings.capture.hotkey.keycaps().join(separator),
        )
    } else {
        tr("Off. Hex records nothing and keeps no shortcut until you turn this on.").into()
    }
}

/// Turns Annotate on or off; turning it on fails while its shortcut
/// collides with another, and leaves the shortcuts as they are.
fn set_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled
        && super::settings::conflicts(settings, ShortcutTarget::Capture, &settings.capture.hotkey)
    {
        return Err(tr(
            "The capture shortcut is already in use. Change it below before turning captures on.",
        ));
    }
    settings.capture.enabled = enabled;
    Ok(())
}

pub(super) fn toggle(ui: &mut Ui) {
    if crate::policy::Policy::current().locks(crate::policy::Lock::Annotate) {
        return;
    }
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
    match (policy.media_days, policy.bundle_days) {
        (Some(video), Some(annotation)) => tr_named(
            "Video goes after {video} days; the annotation after {annotation} days",
            &[
                ("video", &video.to_string()),
                ("annotation", &annotation.to_string()),
            ],
        ),
        (Some(video), None) => tr_fill(
            "Video goes after {} days; the annotation stays",
            &video.to_string(),
        ),
        (None, Some(annotation)) => {
            tr_fill("Annotations go after {} days", &annotation.to_string())
        }
        (None, None) => tr("Annotations stay until you remove them").into(),
    }
}

/// The captures folder, with the home folder as `~`.
fn folder_label(ui: &Ui) -> String {
    let folder = ui.settings.capture.captures_dir();
    crate::app_paths::home_dir()
        .and_then(|home| {
            folder
                .strip_prefix(home)
                .ok()
                .map(|relative| format!("~/{}", relative.display()))
        })
        .unwrap_or_else(|| folder.display().to_string())
}

/// The steps in order, each with its keys where it has any: one key taps
/// to start and stop and is held to draw; the capsule's pen draws without
/// it; Esc stops drawing.
fn how_it_works_steps(ui: &Ui, p: Paint) -> Vec<El> {
    let shortcut = ui.settings.capture.hotkey.keycaps();
    let keys = |key: &str, parts: &[String]| Some(super::controls::keycaps(p, key, parts));
    // Wayland tells Hex nothing of Shift outside its own windows.
    let draw_how = if cfg!(not(target_os = "linux")) {
        tr(
            "While it records, hold the shortcut and move the pointer; no click needed. Shift draws too",
        )
    } else {
        tr("While it records, hold the shortcut and move the pointer; no click needed")
    };
    vec![
        step_row(
            p,
            "how-start",
            1,
            tr("Start"),
            keys("how-start-keys", &shortcut),
            tr("Tap the shortcut to start recording; talk as you go"),
        ),
        step_row(
            p,
            "how-draw",
            2,
            tr("Draw"),
            keys("how-draw-keys", &shortcut),
            draw_how,
        ),
        step_row(
            p,
            "how-pen",
            3,
            tr("Pen"),
            keys("how-pen-keys", &["Esc".to_string()]),
            tr(
                "Or click the pen on the bar, then drag to draw; click it again or press Esc to stop",
            ),
        ),
        step_row(
            p,
            "how-stop",
            4,
            tr("Stop"),
            keys("how-stop-keys", &shortcut),
            tr("Tap the shortcut again: the annotation is copied, ready to paste"),
        ),
        step_row(
            p,
            "how-send",
            5,
            tr("Send"),
            None,
            tr("Paste it anywhere, or click Send to… on the capsule to pick a session"),
        ),
    ]
}

/// A fold's link, and under it, while open, its card.
fn fold(p: Paint, key: &str, link: El, rows: Option<Vec<El>>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(link)
        .children(rows.map(|rows| card(p, "card", rows)))
}

/// How it works, closed until its link is pressed.
fn how_it_works(ui: &Ui, p: Paint, view: &ScrawlView) -> El {
    let link = disclosure_link(
        p,
        "how-toggle",
        tr("How it works"),
        view.how_it_works,
        |ui: &mut Ui| {
            if let Some(view) = ui.platform.scrawl.as_mut() {
                view.how_it_works = !view.how_it_works;
            }
        },
    );
    fold(
        p,
        "how",
        link,
        view.how_it_works.then(|| how_it_works_steps(ui, p)),
    )
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
        tr("Hex leaves the Annotate shortcut to Scrawl until you switch")
    };
    card(
        p,
        "scrawl-running",
        vec![setting_row(
            p,
            "scrawl-running-row",
            tr("Scrawl is running"),
            Some(description),
            button(
                p,
                "use-hex-for-captures",
                tr("Use Hex for annotations"),
                ButtonKind::Primary,
                |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::QuitScrawl),
            ),
        )],
    )
}

/// The switch, Screen Recording while Annotate can't record yet, and the
/// shortcut.
fn top_rows(ui: &Ui, p: Paint, view: &ScrawlView) -> Vec<CardRow> {
    let enabled = ui.settings.capture.enabled;
    let switch = if crate::policy::Policy::current().locks(crate::policy::Lock::Annotate) {
        super::controls::managed_row(p, "capture-enabled", Some(&SCRAWL_ICON), tr("Annotate"))
    } else {
        icon_toggle_row(
            p,
            "capture-enabled",
            &SCRAWL_ICON,
            tr("Annotate"),
            description(ui, enabled).as_str(),
            enabled,
            toggle,
        )
    };
    let purpose = if enabled {
        tr("Hold while recording to draw")
    } else {
        tr("Saved for when Annotate is on")
    };
    vec![
        switch.into(),
        folding(
            "capture-error-fold",
            view.enable_error.as_ref().map(|error| {
                note(
                    "capture-error",
                    &format!("{} {error}", tr("Could not update Annotate:")),
                    p.danger(),
                )
            }),
        ),
        folding("screen-recording-fold", screen_recording_row(ui, p, view)),
        icon_described_row(
            p,
            "capture-shortcut",
            &KEYBOARD,
            tr("Shortcut"),
            Some((purpose.to_string(), p.muted())),
            keys_button(ui, p, ShortcutTarget::Capture),
        )
        .into(),
    ]
}

/// Screen Recording, only while Annotate is on and can't record the screen.
/// Linux asks through the desktop's screen-cast portal when a capture
/// starts, so it has none.
fn screen_recording_row(ui: &Ui, p: Paint, view: &ScrawlView) -> Option<El> {
    if !ui.settings.capture.enabled || !cfg!(target_os = "macos") {
        return None;
    }
    if view.screen_needs_restart {
        // macOS lets a running app use the grant only after it relaunches.
        return Some(sub_row(setting_row(
            p,
            "screen-recording",
            tr("Screen Recording"),
            Some(tr("Turns on when Hex restarts")),
            button(
                p,
                "screen-recording-restart",
                tr("Restart Hex"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.ask(PlatformRequest::Permission(PermissionAction::Restart)),
            ),
        )));
    }
    if view.screen_recording {
        return None;
    }
    let stale = ui.platform.stale(PermissionKind::ScreenRecording);
    Some(sub_row(setting_row(
        p,
        "screen-recording",
        tr("Screen Recording"),
        Some(if stale {
            tr("An older Hex entry can keep this off")
        } else {
            tr("Hex can't see your screen yet")
        }),
        permission_control(
            p,
            "screen-recording-allow",
            ui.platform
                .control_for(PermissionKind::ScreenRecording, false),
            ButtonKind::Secondary,
            move |ui: &mut Ui| {
                let action = if stale {
                    PermissionAction::ResetAndPrompt(PermissionKind::ScreenRecording)
                } else {
                    PermissionAction::ShowGuide(PermissionKind::ScreenRecording)
                };
                ui.ask(PlatformRequest::Permission(action))
            },
        ),
    )))
}

/// Keep, then the folder.
fn after_rows(ui: &Ui, p: Paint, view: &ScrawlView, layer: &mut Vec<El>) -> Vec<El> {
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
    let folder = folder_label(ui);
    let mut rows = vec![
        icon_row(
            p,
            "capture-retention",
            &CLOCK,
            tr("Keep"),
            Some(kept.as_str()),
            select(
                p,
                "capture-retention-menu",
                tr("Keep"),
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
        icon_row(
            p,
            "capture-folder",
            &FOLDER,
            tr("Folder"),
            Some(folder.as_str()),
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
                    tr("Change…"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::ChooseFolder),
                )),
        ),
    ];
    if let Some(error) = &view.folder_error {
        rows.push(note("capture-folder-error", error, p.danger()));
    }
    rows
}

/// The transcriber, showing Hex in screen recordings for demos, and the
/// replay Lab: settings most people never change. macOS only.
fn more_rows(ui: &Ui, p: Paint) -> Vec<El> {
    let mut rows = vec![transcriber_row(ui, p), recordings_row(ui, p)];
    #[cfg(target_os = "macos")]
    if crate::labs::Feature::AnnotateReplay.on_this_os()
        && crate::labs::Feature::AnnotateReplay.on(&ui.settings)
    {
        let mut control = row()
            .key("capture-replay-shortcut-control")
            .gap(4.0)
            .center_y()
            .child(keys_button(ui, p, ShortcutTarget::CaptureReplay));
        if ui.settings.capture.replay_hotkey.is_some() {
            control = control.child(super::controls::icon_button(
                p,
                "clear-capture-replay-shortcut",
                &rune_theme_opencode::icons::TRASH,
                tr("Clear shortcut"),
                |ui: &mut Ui| {
                    ui.update(|settings| settings.capture.replay_hotkey = None);
                },
            ));
        }
        use super::lists::{Tone, tone_tag};
        rows.push(super::controls::icon_row_tagged(
            p,
            "capture-replay-shortcut",
            &super::icons::REFRESH,
            tr("Replay last 15 s"),
            Some(tone_tag(p, "lab", tr("Lab"), Tone::Accent)),
            Some(tr("Press to save the last 15 s; hold to narrate over it")),
            control,
        ));
    }
    rows
}

/// Lets screen recordings, captures included, show Hex's overlays, for
/// showing someone how captures work.
fn recordings_row(ui: &Ui, p: Paint) -> El {
    let on = ui.settings.capture.show_overlays_in_recordings;
    icon_toggle_row(
        p,
        "capture-show-in-recordings",
        &rune_kit::glyphs::EYE,
        tr("Show Hex in screen recordings"),
        if on {
            tr("Recordings show the capsule, ink, and HUD")
        } else {
            tr("The capsule and ink stay out of recordings")
        },
        on,
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
    icon_row(
        p,
        "capture-transcriber",
        &WAVE,
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

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let view = ui.platform.scrawl.clone().unwrap_or_default();
    let enabled = ui.settings.capture.enabled;
    let mut content = page("content").child(card(p, "enable", top_rows(ui, p, &view)));
    if !enabled {
        return super::scroll_body(p.k, "scroll", content);
    }
    if view.scrawl_running {
        content = content.child(scrawl_running(p, &view));
    }
    content = content.child(how_it_works(ui, p, &view)).child(section(
        p,
        "after",
        tr("After an annotation"),
        after_rows(ui, p, &view, layer),
    ));
    // Apple's speech recognizer, keeping Hex out of recordings, and replay
    // are macOS's; Linux transcribes with Hex's model.
    if cfg!(target_os = "macos") {
        let link = disclosure(
            ui,
            p,
            "more-toggle",
            tr("Transcriber, demos and replay"),
            Pane::Annotate,
        );
        let open = advanced_open(ui, Pane::Annotate);
        content = content.child(fold(p, "more", link, open.then(|| more_rows(ui, p))));
    }
    super::scroll_body(p.k, "scroll", content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn captures_default_on_and_refuse_a_shortcut_in_use() {
        let settings = AppSettings::default();
        assert!(settings.capture.enabled);
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
        let mut rt = crate::rune_ui::offscreen::runtime(
            state,
            crate::rune_ui::view,
            (980.0, 760.0),
            env,
            text,
        );
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
    fn granted_screen_recording_hides_its_row() {
        let rt = pane("mac-scrawl");
        assert!(rt.find("screen-recording").is_none());
        assert!(rt.find(ALLOW).is_none());
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
        rt.settle_all();
        rt.frame(0.0);
        assert!(rt.find(ALLOW).is_none(), "the row folds away once granted");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rare_settings_stay_folded_until_opened() {
        let mut rt = pane("mac-scrawl");
        assert!(rt.find("capture-transcriber").is_none());
        tap(&mut rt, "more-toggle");
        assert!(rt.find("capture-transcriber").is_some());
        assert!(rt.find("capture-show-in-recordings").is_some());
    }

    #[test]
    fn how_it_works_opens_from_its_link() {
        let mut rt = pane("mac-scrawl");
        assert!(rt.find("how-start").is_none());
        tap(&mut rt, "how-toggle");
        assert!(rt.find("how-start").is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_grant_this_hex_cant_use_yet_offers_a_restart() {
        let mut rt = pane("mac-scrawl-restart");
        assert!(
            rt.find(ALLOW).is_none(),
            "no Allow over a switch already on"
        );
        tap(&mut rt, "screen-recording-restart");
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(PermissionAction::Restart)]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stale_screen_recording_offers_fix_it_and_resets_permission() {
        let mut rt = pane("mac-scrawl-permission");
        rt.state
            .platform
            .note_stale(PermissionKind::ScreenRecording);
        rt.invalidate();
        rt.settle_all();
        rt.frame(0.0);
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/dot")).is_some(),
            "stale keeps the waiting dot"
        );
        tap(&mut rt, ALLOW);
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(
                PermissionAction::ResetAndPrompt(PermissionKind::ScreenRecording)
            )]
        );
        assert!(!rt.state.platform.stale(PermissionKind::ScreenRecording));
    }

    #[test]
    fn opening_the_pane_reads_screen_recording_again() {
        let mut rt = pane("mac-scrawl");
        tap(&mut rt, "nav-general");
        tap(&mut rt, "nav-labs");
        tap(&mut rt, "nav-annotate");
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
