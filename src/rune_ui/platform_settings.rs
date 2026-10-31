//! Rows only some OSes show, chosen by
//! [`Capabilities`](super::platform::Capabilities): macOS's permission
//! warnings, Meetings › Settings, Show Dock icon, and the Hex Microphone
//! driver row Voice Filter shows; Launch at login on macOS and Windows. Their
//! state is [`PlatformState`](super::platform::PlatformState)
//! data and their actions are [`PlatformRequest`]s.

use rune::{El, Icon, row, text};

use super::controls::{
    ButtonKind, Choice, DESCRIPTION, Paint, button, card, icon_row, icon_toggle_row, note, section,
    select, setting_row,
};
use super::lists::{PermissionControl, Tone, permission_control, tone_tag};
use super::platform::{CalendarRequest, MeetingRequest, NotesAvailability, Os, PlatformRequest};
use super::settings::toggle_menu;
use super::{Menu, Pane, ShortcutTarget, Ui, icons};
use crate::app_settings::{
    AppSettings, CALENDAR_LEAD_MINUTES, HotkeyBinding, KNOWN_MEETING_PROVIDERS, MeetingPolicy,
    MeetingProviderChoice, MeetingRetention, ModifierSide,
};
use crate::calendar::{CalendarAccess, CallProvider};
use crate::desktop_i18n::{tr, tr_fill, tr_named};
use crate::hex_microphone::DriverActionState;
use crate::onboarding::{PermissionAction, PermissionKind, PermissionWarning};

/// A permission's row key, its name as System Settings lists it, and what
/// Hex cannot do without it.
fn permission_copy(
    kind: PermissionKind,
    accessibility_renamed: bool,
) -> (&'static str, &'static str, &'static str) {
    match kind {
        PermissionKind::Microphone => (
            "permission-microphone",
            tr("Microphone"),
            tr("Hex cannot record dictation until microphone access is restored."),
        ),
        PermissionKind::Accessibility => (
            "permission-accessibility",
            accessibility_name(accessibility_renamed),
            tr("Hex cannot recognize the dictation shortcut or paste into other apps."),
        ),
        PermissionKind::SystemAudio => (
            "permission-system-audio",
            tr("Screen & System Audio Recording"),
            tr("Required to hear the other participants."),
        ),
        PermissionKind::ScreenRecording => (
            "permission-screen-recording",
            tr("Screen Recording"),
            tr("Lets Annotate record your screen"),
        ),
    }
}

/// What System Settings calls the Accessibility list; macOS 27 renamed it.
pub(super) fn accessibility_name(renamed: bool) -> &'static str {
    if renamed {
        tr("Device Control and Data Access")
    } else {
        tr("Accessibility")
    }
}

/// The missing permissions above everything else, while the tour is closed.
pub(super) fn permissions_section(ui: &Ui, p: Paint) -> Option<El> {
    if !ui.platform.caps.permissions || ui.tour.is_some() {
        return None;
    }
    let warnings = crate::onboarding::permission_warnings(ui.platform.permissions);
    if warnings.is_empty() {
        return None;
    }
    let rows = warnings
        .into_iter()
        .map(|warning: PermissionWarning| {
            let (key, name, description) =
                permission_copy(warning.kind, ui.platform.accessibility_renamed);
            if warning.action == PermissionAction::Restart {
                return setting_row(
                    p,
                    key,
                    name,
                    Some(tr("Turns on when Hex restarts")),
                    button(
                        p,
                        &format!("{key}-restart"),
                        tr("Restart Hex"),
                        ButtonKind::Secondary,
                        |ui: &mut Ui, _| {
                            ui.ask(PlatformRequest::Permission(PermissionAction::Restart))
                        },
                    ),
                );
            }
            let control = permission_control(
                p,
                &format!("{key}-allow"),
                if ui.platform.asked(warning.kind) {
                    PermissionControl::Waiting
                } else {
                    PermissionControl::Allow
                },
                ButtonKind::Secondary,
                move |ui: &mut Ui| ui.ask(PlatformRequest::Permission(warning.action)),
            );
            setting_row(p, key, name, Some(description), control)
        })
        .collect();
    Some(section(p, "permissions", tr("Permissions needed"), rows))
}

/// A save error where the OS has no listener card to show it in.
pub(super) fn error_section(ui: &Ui, p: Paint) -> Option<El> {
    if ui.platform.caps.listener_control {
        return None;
    }
    let error = ui.error.as_deref()?;
    Some(section(
        p,
        "save-error",
        tr("Status"),
        super::settings::error_notes(p, error).into(),
    ))
}

fn notes_description(availability: NotesAvailability) -> &'static str {
    match availability {
        NotesAvailability::Available => {
            tr("Meeting titles and short summaries, written on this Mac")
        }
        NotesAvailability::DeviceNotEligible => tr("This Mac does not support Apple Intelligence"),
        NotesAvailability::NotEnabled => {
            tr("Turn on Apple Intelligence in System Settings to use this")
        }
        NotesAvailability::ModelNotReady => tr("The Apple Intelligence model is still downloading"),
        NotesAvailability::UnsupportedSystem => tr("Requires macOS 26 or later"),
        NotesAvailability::Unknown => tr("Apple Intelligence is unavailable"),
    }
}

fn folder_label(ui: &Ui) -> String {
    let folder = ui.settings.meetings.resolved_output_folder();
    dirs::home_dir()
        .and_then(|home| {
            folder
                .strip_prefix(home)
                .ok()
                .map(|relative| format!("~/{}", relative.display()))
        })
        .unwrap_or_else(|| folder.display().to_string())
}

/// The Meetings (beta) switch: the opt-in card's only row, and the first row
/// of Meetings › Settings.
pub(super) fn meetings_toggle(ui: &Ui, p: Paint) -> El {
    icon_toggle_row(
        p,
        "meetings-enabled",
        Pane::Meetings.icon(),
        tr("Meetings (beta)"),
        tr("Offer to record calls and save local transcripts"),
        ui.settings.meetings.enabled,
        |ui: &mut Ui| {
            let enabled = !ui.settings.meetings.enabled;
            ui.update(|settings| settings.meetings.enabled = enabled);
        },
    )
}

/// Meetings › Settings: the opt-in (and the system-audio permission while
/// it's missing); once on, how long meetings are kept, Calendar and each detected call provider's Ask /
/// Always record / Never offer choice; behind Advanced the indicator,
/// folder, Apple Intelligence notes, and the granted permission.
pub(super) fn meetings_settings(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<El> {
    use super::general::{advanced_open, advanced_toggle};
    let meetings = &ui.settings.meetings;
    let mut rows = vec![meetings_toggle(ui, p)];
    let mut more = Vec::new();
    // Only macOS asks before Hex may hear the other participants.
    if meetings.enabled && ui.platform.caps.permissions {
        let permitted = ui.platform.system_audio;
        let system_audio = icon_row(
            p,
            "meeting-system-audio",
            &icons::SPEAKER,
            tr("Screen & System Audio Recording"),
            Some(if permitted {
                tr("Captures the other participants' audio")
            } else {
                tr("Required to hear the other participants.")
            }),
            permission_control(
                p,
                "meeting-system-audio-allow",
                if permitted {
                    PermissionControl::Allowed
                } else if ui.platform.asked(PermissionKind::SystemAudio) {
                    PermissionControl::Waiting
                } else {
                    PermissionControl::Allow
                },
                ButtonKind::Secondary,
                |ui: &mut Ui| {
                    // macOS prompts once; after that the switch is in
                    // System Settings, with the guide beside it.
                    ui.ask(PlatformRequest::SystemAudio);
                },
            ),
        );
        // A grant made here stays in place to show Allowed.
        if permitted && !ui.platform.asked(PermissionKind::SystemAudio) {
            more.push(system_audio);
        } else {
            rows.push(system_audio);
        }
    }
    if meetings.enabled {
        rows.push(retention_row(ui, p, layer));
        more.push(icon_toggle_row(
            p,
            "meeting-indicator",
            &icons::PILL,
            tr("Show recording indicator"),
            tr("Floating pill to copy live transcript turns or stop recording"),
            meetings.show_recording_indicator,
            |ui: &mut Ui| {
                let show = !ui.settings.meetings.show_recording_indicator;
                ui.update(|settings| settings.meetings.show_recording_indicator = show);
            },
        ));
        more.push(icon_row(
            p,
            "meeting-folder",
            &icons::FOLDER,
            tr("Transcript folder"),
            Some(tr("Finished meetings are saved here as Markdown")),
            row()
                .key("controls")
                .gap(8.0)
                .center_y()
                .child(button(
                    p,
                    "reveal-meeting-folder",
                    tr("Reveal"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.ask(PlatformRequest::Meeting(MeetingRequest::RevealFolder)),
                ))
                .child(button(
                    p,
                    "choose-meeting-folder",
                    &folder_label(ui),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.ask(PlatformRequest::Meeting(MeetingRequest::ChooseFolder)),
                )),
        ));
        if ui.platform.caps.os == Os::Macos {
            more.push(icon_toggle_row(
                p,
                "meeting-notes",
                &icons::SPARKLES,
                tr("Summarize with Apple Intelligence"),
                notes_description(ui.platform.notes),
                meetings.summarize_with_apple_intelligence,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.meetings.summarize_with_apple_intelligence;
                    ui.update(|settings| {
                        settings.meetings.summarize_with_apple_intelligence = enabled
                    });
                },
            ));
        }
        more.push(icon_toggle_row(
            p,
            "meeting-notes-opencode",
            &icons::SPARKLES,
            tr("Summarize with OpenCode"),
            if ui.platform.caps.os == Os::Macos {
                tr("When Apple Intelligence is off or unavailable. Sends the transcript to your OpenCode model")
            } else {
                tr("Meeting titles and short summaries. Sends the transcript to your OpenCode model")
            },
            meetings.summarize_with_opencode,
            |ui: &mut Ui| {
                let enabled = !ui.settings.meetings.summarize_with_opencode;
                ui.update(|settings| settings.meetings.summarize_with_opencode = enabled);
            },
        ));
        if let Some(error) = &ui.platform.meeting_folder_error {
            // A failed folder change shows where it happened, opening the fold.
            more.push(note("meeting-folder-error", error, p.danger()));
        }
    }
    let mut sections = vec![card(p, "meetings", rows)];
    if meetings.enabled {
        sections.extend(calendar_section(ui, p, layer));
        sections.push(detected_calls_section(ui, p, layer));
        sections.push(advanced_toggle(ui, p, Pane::Meetings));
        if advanced_open(ui, Pane::Meetings) || ui.platform.meeting_folder_error.is_some() {
            sections.push(card(p, "advanced", more));
        }
    }
    sections
}

/// How long finished meetings are kept before they move to the Trash.
fn retention_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let retention = ui.settings.meetings.retention;
    let control = super::general::pick_select(
        ui,
        p,
        "meeting-retention",
        MeetingRetention::ALL
            .iter()
            .map(|retention| tr(retention.label()))
            .collect(),
        MeetingRetention::ALL
            .iter()
            .position(|choice| *choice == retention)
            .unwrap_or_default(),
        |ui, index| {
            ui.update(|settings| settings.meetings.retention = MeetingRetention::ALL[index]);
        },
        layer,
    );
    icon_row(
        p,
        "meeting-retention",
        &icons::CLOCK,
        tr("Keep meetings"),
        (retention != MeetingRetention::Forever).then(|| tr("Older meetings move to the Trash")),
        control,
    )
}

static ZOOM_ICON: Icon = Icon {
    name: "hex-provider-zoom",
    view: 16.0,
    body: include_str!("../../assets/icons/providers/zoom.svg"),
};
static MEET_ICON: Icon = Icon {
    name: "hex-provider-meet",
    view: 16.0,
    body: include_str!("../../assets/icons/providers/meet.svg"),
};
static TEAMS_ICON: Icon = Icon {
    name: "hex-provider-teams",
    view: 16.0,
    body: include_str!("../../assets/icons/providers/teams.svg"),
};
static SLACK_ICON: Icon = Icon {
    name: "hex-provider-slack",
    view: 16.0,
    body: include_str!("../../assets/icons/providers/slack.svg"),
};
static WEBEX_ICON: Icon = Icon {
    name: "hex-provider-webex",
    view: 16.0,
    body: include_str!("../../assets/icons/providers/webex.svg"),
};

fn provider_icon(provider: CallProvider) -> &'static Icon {
    match provider {
        CallProvider::Zoom => &ZOOM_ICON,
        CallProvider::GoogleMeet => &MEET_ICON,
        CallProvider::Teams => &TEAMS_ICON,
        CallProvider::Slack => &SLACK_ICON,
        CallProvider::Webex => &WEBEX_ICON,
    }
}

/// Calendar reminders: Connect calendar (the only thing that asks macOS for
/// access), then the reminder switch, lead time, included calendars, and
/// the next few meetings.
fn calendar_section(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Option<El> {
    if !ui.platform.caps.calendar {
        return None;
    }
    let view = &ui.platform.calendar;
    let calendar = &ui.settings.meetings.calendar;
    let mut rows = Vec::new();
    if view.access.blocked() {
        rows.push(icon_row(
            p,
            "calendar-blocked",
            &icons::CALENDAR,
            tr("Calendar access is off"),
            Some(tr(
                "Allow Hex under Privacy & Security › Calendars to get meeting reminders",
            )),
            button(
                p,
                "calendar-privacy",
                tr("Open settings"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    ui.ask(PlatformRequest::Calendar(
                        CalendarRequest::OpenPrivacySettings,
                    ))
                },
            ),
        ));
    } else if !calendar.connected || view.access != CalendarAccess::FullAccess {
        rows.push(icon_row(
            p,
            "calendar-connect",
            &icons::CALENDAR,
            tr("Remind me before meetings"),
            Some(tr("Names each recording after its calendar event")),
            button(
                p,
                "calendar-connect-button",
                if view.connecting {
                    tr("Connecting…")
                } else {
                    tr("Connect calendar")
                },
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    if !ui.platform.calendar.connecting {
                        ui.ask(PlatformRequest::Calendar(CalendarRequest::Connect));
                    }
                },
            ),
        ));
    } else {
        rows.push(icon_toggle_row(
            p,
            "calendar-remind",
            &icons::BELL,
            tr("Remind me before meetings"),
            tr("Shows a reminder with Record and Join buttons"),
            calendar.remind,
            |ui: &mut Ui| {
                let remind = !ui.settings.meetings.calendar.remind;
                ui.update(|settings| settings.meetings.calendar.remind = remind);
            },
        ));
        if calendar.remind {
            let lead = calendar.lead().as_secs() / 60;
            let control = super::general::pick_select(
                ui,
                p,
                "calendar-lead",
                vec![tr("At start"), tr("1 min"), tr("2 min"), tr("5 min")],
                CALENDAR_LEAD_MINUTES
                    .iter()
                    .position(|minutes| u64::from(*minutes) == lead)
                    .unwrap_or(1),
                |ui, index| {
                    ui.update(|settings| {
                        settings.meetings.calendar.lead_minutes = CALENDAR_LEAD_MINUTES[index];
                    });
                },
                layer,
            );
            rows.push(icon_row(
                p,
                "calendar-lead",
                &icons::CLOCK,
                tr("Remind me"),
                None,
                control,
            ));
        }
        rows.push(calendars_row(ui, p, layer));
        if view.upcoming.is_empty() {
            rows.push(note(
                "calendar-none",
                tr("No meetings in the next two days"),
                p.muted(),
            ));
        }
        rows.extend(view.upcoming.iter().enumerate().map(|(index, meeting)| {
            let provider = meeting.provider.map(|provider| {
                row()
                    .key("provider")
                    .gap(6.0)
                    .center_y()
                    .label(provider.label())
                    .child(
                        rune::icon(provider_icon(provider), 16.0)
                            .key("icon")
                            .color(p.muted()),
                    )
                    .child(
                        text(provider.label())
                            .key("name")
                            .font(DESCRIPTION)
                            .color(p.muted()),
                    )
            });
            setting_row(
                p,
                &format!("upcoming{index}"),
                &meeting.title,
                Some(&meeting.when),
                provider.unwrap_or_else(|| row().key("provider")),
            )
        }));
    }
    if let Some(error) = &view.error {
        rows.push(note("calendar-error", error, p.danger()));
    }
    Some(section(p, "calendar", tr("Calendar"), rows))
}

/// Which calendars remind: all by default, each one switched in the menu.
fn calendars_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let calendar = &ui.settings.meetings.calendar;
    let calendars = &ui.platform.calendar.calendars;
    let included = calendars
        .iter()
        .filter(|info| calendar.includes(&info.id))
        .count();
    let label = if included == calendars.len() {
        tr("All calendars").to_string()
    } else {
        tr_named(
            "{included} of {total} calendars",
            &[
                ("included", &included.to_string()),
                ("total", &calendars.len().to_string()),
            ],
        )
    };
    let choices = calendars
        .iter()
        .map(|info| {
            let id = info.id.clone();
            Choice {
                label: if info.source.is_empty() {
                    info.title.clone()
                } else {
                    format!("{} · {}", info.title, info.source)
                },
                detail: None,
                selected: calendar.includes(&info.id),
                choose: Box::new(move |ui: &mut Ui| {
                    let id = id.clone();
                    ui.update(|settings| {
                        let excluded = &mut settings.meetings.calendar.excluded_calendars;
                        if let Some(index) = excluded.iter().position(|known| *known == id) {
                            excluded.remove(index);
                        } else {
                            excluded.push(id);
                        }
                    });
                }),
            }
        })
        .collect();
    setting_row(
        p,
        "calendar-calendars",
        tr("Calendars"),
        None,
        select(
            p,
            "calendar-calendars-control",
            &label,
            ui.open == Some(Menu::Calendars),
            choices,
            |ui: &mut Ui| toggle_menu(ui, Menu::Calendars),
            layer,
        ),
    )
}

/// The known services, then every other website or app the user chose for,
/// each with Ask / Always record / Never offer.
fn detected_calls_section(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let choices = &ui.settings.meetings.providers;
    let known = KNOWN_MEETING_PROVIDERS
        .iter()
        .map(|(key, label)| (key.to_string(), label.to_string(), tr(label).to_owned()));
    let others = choices
        .iter()
        .filter(|choice| {
            !KNOWN_MEETING_PROVIDERS
                .iter()
                .any(|(key, _)| *key == choice.key)
        })
        .map(|choice| {
            (
                choice.key.clone(),
                choice.label.clone(),
                choice.label.clone(),
            )
        });
    let labels = [tr("Ask"), tr("Always record"), tr("Never offer")];
    let rows = known
        .chain(others)
        .enumerate()
        .map(|(index, (key, label, shown))| {
            let policy = choices
                .iter()
                .find(|choice| choice.key == key)
                .map_or(MeetingPolicy::Ask, |choice| choice.policy);
            let selected = POLICIES.iter().position(|known| *known == policy);
            let control = super::general::quiet_select(
                ui,
                p,
                Menu::Call(index),
                format!("call{index}-policy-control"),
                labels.to_vec(),
                selected.unwrap_or(0),
                move |ui, index| {
                    let choice = MeetingProviderChoice {
                        key: key.clone(),
                        label: label.clone(),
                        policy: POLICIES[index],
                    };
                    ui.update(|settings| {
                        crate::app_settings::set_meeting_provider(&mut settings.meetings, choice);
                    });
                },
                layer,
            );
            setting_row(p, &format!("call{index}"), &shown, None, control)
        })
        .collect::<Vec<_>>();
    section(p, "detected-calls", tr("Detected calls"), rows)
}

/// In the order of the Detected calls segments.
const POLICIES: [MeetingPolicy; 3] = [
    MeetingPolicy::Ask,
    MeetingPolicy::AlwaysRecord,
    MeetingPolicy::NeverOffer,
];

/// The virtual microphone driver's row: install, update, or uninstall it.
pub(super) fn driver_row(ui: &Ui, p: Paint) -> El {
    let title = tr("Virtual microphone driver");
    let Some(status) = &ui.platform.hex_microphone else {
        return setting_row(
            p,
            "hex-mic-driver",
            title,
            Some(tr("Checking…")),
            row().key("none"),
        );
    };
    let action = |key: &str, label: &str, request: PlatformRequest| {
        button(
            p,
            key,
            label,
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| ui.ask(request.clone()),
        )
    };
    let install = || {
        action(
            "install",
            tr("Install"),
            PlatformRequest::InstallHexMicrophone,
        )
    };
    let uninstall = || {
        action(
            "uninstall",
            tr("Uninstall"),
            PlatformRequest::UninstallHexMicrophone,
        )
    };
    let state_tag = |label: &str, tone: Tone| tone_tag(p, "state", label, tone);
    let controls = row().key("controls").gap(8.0).center_y();
    let (description, controls) = match &status.action_state {
        DriverActionState::Installing => (
            tr("Installing driver and restarting Core Audio…").to_string(),
            controls.child(state_tag(tr("Installing…"), Tone::Neutral)),
        ),
        DriverActionState::Uninstalling => (
            tr("Removing driver and restarting Core Audio…").to_string(),
            controls.child(state_tag(tr("Uninstalling…"), Tone::Neutral)),
        ),
        state if status.needs_update() => {
            let installed = status.installed_version.as_deref().unwrap_or("1.0.0");
            (
                match state {
                    DriverActionState::Cancelled => tr("Update cancelled").into(),
                    DriverActionState::Failed(reason) => {
                        tr_fill("Could not update driver: {}", reason)
                    }
                    _ => tr_fill("Update driver to v{}", &status.bundled_version),
                },
                controls
                    .child(state_tag(
                        &tr_fill("v{} installed", installed),
                        Tone::Accent,
                    ))
                    .child(action(
                        "update",
                        tr("Update"),
                        PlatformRequest::InstallHexMicrophone,
                    ))
                    .child(uninstall()),
            )
        }
        state if status.driver_loaded => {
            let version = status
                .installed_version
                .as_deref()
                .unwrap_or(&status.bundled_version);
            (
                match state {
                    DriverActionState::Cancelled => tr("Uninstall cancelled").into(),
                    DriverActionState::Failed(reason) => {
                        tr_fill("Could not update driver: {}", reason)
                    }
                    _ if !status.active_apps.is_empty() => tr_fill(
                        "Streaming 48 kHz audio to {}",
                        &status.active_apps.join(", "),
                    ),
                    _ => {
                        tr("Choose Hex Microphone as your input in Slack, Zoom, or Discord").into()
                    }
                },
                controls
                    .child(state_tag(&tr_fill("Installed v{}", version), Tone::Success))
                    .child(uninstall()),
            )
        }
        DriverActionState::DoneUninstalled => (
            tr("Driver removed").into(),
            controls
                .child(state_tag(tr("Removed"), Tone::Neutral))
                .child(install()),
        ),
        DriverActionState::Cancelled => (
            tr("Installation cancelled").into(),
            controls
                .child(state_tag(tr("Cancelled"), Tone::Neutral))
                .child(install()),
        ),
        DriverActionState::Failed(reason) => (
            tr_fill("Could not install driver: {}", reason),
            controls
                .child(state_tag(tr("Failed"), Tone::Danger))
                .child(install()),
        ),
        _ => (
            tr("Install the CoreAudio driver to choose Hex Microphone in Slack, Zoom, or Discord")
                .into(),
            controls.child(install()),
        ),
    };
    setting_row(p, "hex-mic-driver", title, Some(&description), controls)
}

/// `target`'s modifier-only shortcut moved to `side`, unless that collides
/// with another shortcut.
pub(super) fn side_binding(
    settings: &AppSettings,
    target: ShortcutTarget,
    side: ModifierSide,
) -> Option<HotkeyBinding> {
    let mut candidate = super::settings::binding(settings, target)?.clone();
    candidate.standalone_side()?;
    candidate.set_standalone_side(side);
    (!super::settings::conflicts(settings, target, &candidate)).then_some(candidate)
}

pub(super) const SIDES: [ModifierSide; 3] = [
    ModifierSide::Left,
    ModifierSide::Either,
    ModifierSide::Right,
];

/// The side [`side_selector`] shows for `target`, if it shows one: the
/// keycaps beside it then leave the side to it.
pub(super) fn picked_side(ui: &Ui, target: ShortcutTarget) -> Option<ModifierSide> {
    if !ui.platform.caps.modifier_sides {
        return None;
    }
    super::settings::binding(&ui.settings, target)?.standalone_side()
}
