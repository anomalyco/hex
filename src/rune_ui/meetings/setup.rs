//! Meetings' one setup screen, shown until Meetings is on: what it does,
//! the three things worth deciding first (System audio, what happens when
//! a call starts, Calendar), and Turn on Meetings as its one action. Each
//! row acts in place, so nothing waits for a second screen.

use rune::{El, col, row, text};

use super::super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, card, icon_row, icon_toggle_row,
};
use super::super::lists::{allowed, permission_control};
use super::super::platform::{CalendarRequest, PlatformRequest};
use super::super::{Ui, icons};
use crate::app_settings::{MeetingPolicy, MeetingRetention};
use crate::calendar::CalendarAccess;
use crate::desktop::i18n::{tr, tr_fill};
use crate::desktop::onboarding::{PermissionAction, PermissionKind};

/// The setup column: header, the setup card, Turn on Meetings, and where
/// transcripts are kept, centred in the pane.
pub(in crate::rune_ui) fn setup(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let header = col()
        .key("header")
        .center()
        .child(
            rune::el()
                .key("badge")
                .size(40.0, 40.0)
                .mb(METRICS.row_px)
                .radius(p.k.radius.lg)
                .fill(p.k.c.layer_01)
                .center()
                .child(
                    rune::icon(super::super::sidebar::Pane::Meetings.icon(), 20.0)
                        .key("icon")
                        .color(p.k.c.icon_faint),
                )
                .a11y_hidden(),
        )
        .child(
            text(tr("Transcribe your meetings"))
                .key("title")
                .role("heading")
                .font(p.display())
                .color(p.text()),
        )
        .child(
            text(tr(
                "Records your mic and call audio locally. No bots join your calls.",
            ))
            .key("hint")
            .mt(4.0)
            .font(DESCRIPTION)
            .color(p.muted())
            .align(rune::Align::Center)
            .wrap()
            .w(METRICS.setup_w),
        );
    col()
        .key("meetings-setup")
        .grow(1.0)
        .shrink()
        .center()
        .pb(METRICS.section_gap * 2.0)
        .child(
            col()
                .key("column")
                .w(METRICS.setup_w)
                .gap(METRICS.section_gap)
                .child(header)
                .child(card(p, "setup", rows(ui, p, layer)))
                .child(
                    col()
                        .key("foot")
                        .center()
                        .gap(METRICS.row_px)
                        .child(button(
                            p,
                            "meetings-turn-on",
                            tr("Turn on Meetings"),
                            ButtonKind::Primary,
                            |ui: &mut Ui, _| {
                                ui.update(|settings| settings.meetings.enabled = true);
                            },
                        ))
                        .child(kept_note(ui, p)),
                ),
        )
}

/// System audio where the OS asks for it, When a call starts, and Calendar
/// where calendars are read (with its reminder once connected).
fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<El> {
    let mut rows = Vec::new();
    if ui.platform.caps.permissions {
        rows.push(system_audio_row(ui, p));
    }
    rows.push(call_start_row(ui, p, layer));
    if ui.platform.caps.calendar {
        rows.extend(calendar_rows(ui, p));
    }
    rows
}

fn system_audio_row(ui: &Ui, p: Paint) -> El {
    let permitted = ui.platform.system_audio;
    let stale = !permitted && ui.platform.stale(PermissionKind::SystemAudio);
    // A grant made here animates to Allowed; one made before shows settled,
    // like Calendar's Connected.
    let control = if permitted && !ui.platform.asked(PermissionKind::SystemAudio) {
        allowed(p, "setup-system-audio-allowed", tr("Allowed"))
    } else {
        permission_control(
            p,
            "setup-system-audio-allow",
            ui.platform
                .control_for(PermissionKind::SystemAudio, permitted),
            ButtonKind::Secondary,
            move |ui: &mut Ui| {
                ui.ask(if stale {
                    PlatformRequest::Permission(PermissionAction::ResetAndPrompt(
                        PermissionKind::SystemAudio,
                    ))
                } else {
                    PlatformRequest::SystemAudio
                });
            },
        )
    };
    icon_row(
        p,
        "setup-system-audio",
        &icons::SPEAKER,
        tr("System audio"),
        Some(if permitted {
            tr("Captures the other participants' audio")
        } else {
            tr("Required to hear the other participants")
        }),
        control,
    )
}

/// The default answer for every call, the same one Settings › Calls edits.
fn call_start_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let current = MeetingPolicy::ALL
        .iter()
        .position(|policy| *policy == ui.settings.meetings.default_policy)
        .unwrap_or(0);
    let control = super::super::general::pick_select(
        ui,
        p,
        "setup-call-start",
        tr("When a call starts"),
        MeetingPolicy::ALL
            .iter()
            .map(|policy| tr(policy.label()))
            .collect(),
        current,
        |ui, index| {
            ui.update(|settings| {
                settings
                    .meetings
                    .set_default_policy(MeetingPolicy::ALL[index])
            });
        },
        layer,
    );
    icon_row(
        p,
        "setup-call-start",
        super::super::platform_settings::call_icon(),
        tr("When a call starts"),
        None,
        control,
    )
}

fn calendar_rows(ui: &Ui, p: Paint) -> Vec<El> {
    let view = &ui.platform.calendar;
    let calendar = &ui.settings.meetings.calendar;
    let connected = calendar.connected && view.access == CalendarAccess::FullAccess;
    let control = if connected {
        allowed(p, "setup-calendar-connected", tr("Connected"))
    } else if view.access.blocked() {
        button(
            p,
            "setup-calendar-privacy",
            tr("Open settings"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| {
                ui.ask(PlatformRequest::Calendar(
                    CalendarRequest::OpenPrivacySettings,
                ))
            },
        )
    } else {
        button(
            p,
            "setup-calendar-connect",
            if view.connecting {
                tr("Connecting…")
            } else {
                tr("Connect")
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, _| {
                if !ui.platform.calendar.connecting {
                    ui.ask(PlatformRequest::Calendar(CalendarRequest::Connect));
                }
            },
        )
    };
    let mut rows = vec![icon_row(
        p,
        "setup-calendar",
        &icons::CALENDAR,
        tr("Calendar"),
        Some(tr("Names meetings and adds attendees")),
        control,
    )];
    if connected {
        rows.push(icon_toggle_row(
            p,
            "setup-calendar-remind",
            &icons::BELL,
            tr("Remind me before meetings"),
            tr("Shows a reminder with Record and Join buttons"),
            calendar.remind,
            |ui: &mut Ui| {
                let remind = !ui.settings.meetings.calendar.remind;
                ui.update(|settings| settings.meetings.calendar.remind = remind);
            },
        ));
    }
    rows
}

/// Where transcripts stay and for how long, under the action.
pub(super) fn kept_note(ui: &Ui, p: Paint) -> El {
    let mac = ui.platform.caps.os == super::super::platform::Os::Macos;
    let kept = ui
        .settings
        .meetings
        .retention
        .unwrap_or(MeetingRetention::DEFAULT);
    let copy = match (mac, kept) {
        (true, MeetingRetention::Forever) => tr("Meetings stay on this Mac.").to_owned(),
        (false, MeetingRetention::Forever) => tr("Meetings stay on this computer.").to_owned(),
        (true, kept) => tr_fill(
            "Transcripts stay on this Mac for {}, then move to the Trash.",
            tr(kept.label()),
        ),
        (false, kept) => tr_fill(
            "Transcripts stay on this computer for {}, then move to the Trash.",
            tr(kept.label()),
        ),
    };
    row()
        .key("where-kept")
        .gap(6.0)
        .center_y()
        .child(
            rune::icon(&rune_theme_opencode::icons::LOCK, 12.0)
                .color(p.k.c.icon_faint)
                .a11y_hidden(),
        )
        .child(text(copy).font(p.k.ty.small).color(p.faint()))
}
