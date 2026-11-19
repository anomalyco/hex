//! The calendar reminder capsule, and the pill row every panel capsule
//! shares: a status dot, two truncated lines, and its actions.
use super::*;

/// "Starts in 1 min", "Starting now", or "Started 2 min ago".
pub(super) fn starts_label(start_ms: i64, now_ms: i64) -> String {
    let minutes = crate::calendar::minutes_until(start_ms, now_ms);
    if minutes > 0 {
        tr_fill("Starts in {} min", &minutes.to_string())
    } else {
        let ago = (now_ms - start_ms) / 60_000;
        if ago < 1 {
            tr("Starting now").to_owned()
        } else {
            tr_fill("Started {} min ago", &ago.to_string())
        }
    }
}

/// The calendar reminder: Record, or Join & record when the invitation has
/// a call link. With Always record set for its service, it says so and
/// offers Join and Don't record instead.
pub(super) fn reminder(
    p: Paint,
    meeting: &CalendarMeeting,
    automatic: bool,
    now_ms: i64,
) -> (rune::Color, String, String, Vec<El>) {
    let when = starts_label(meeting.start_ms, now_ms);
    let has_link = meeting.link.is_some();
    let send_meeting = |command: fn(CalendarMeeting) -> ControllerCommand| {
        move |c: &mut Capsule| {
            let Some(Kind::Reminder { meeting, .. }) = &c.kind else {
                return;
            };
            let command = command(meeting.clone());
            c.send(command);
            c.close();
        }
    };
    if automatic {
        let mut actions = vec![button(
            p,
            "dont-record",
            tr("Don't record"),
            false,
            send_meeting(ControllerCommand::DismissReminder),
        )];
        if has_link {
            actions.push(button(
                p,
                "join",
                tr("Join"),
                true,
                send_meeting(ControllerCommand::JoinMeeting),
            ));
        }
        return (
            p.accent(),
            meeting.display_title(),
            tr_fill("{} · Hex records automatically", &when),
            actions,
        );
    }
    let mut actions = vec![button(
        p,
        "not-now",
        tr("Not now"),
        false,
        send_meeting(ControllerCommand::DismissReminder),
    )];
    if has_link {
        actions.push(button(
            p,
            "join-record",
            tr("Join & record"),
            false,
            send_meeting(|meeting| ControllerCommand::RecordMeeting {
                meeting,
                join: true,
            }),
        ));
    }
    actions.push(button(
        p,
        "record",
        tr("Record"),
        true,
        send_meeting(|meeting| ControllerCommand::RecordMeeting {
            meeting,
            join: false,
        }),
    ));
    let subtitle = match &meeting.link {
        Some(link) => format!("{when} · {}", link.provider.label()),
        None => when,
    };
    (p.accent(), meeting.display_title(), subtitle, actions)
}

/// The panel capsule (reminder, offer, call ended): status dot, two
/// truncated lines, and its actions, in a rounded rectangle. It drags to
/// another corner like the recording capsule.
pub(super) fn pill(
    p: Paint,
    dot: rune::Color,
    title: &str,
    subtitle: &str,
    actions: Vec<El>,
) -> El {
    // Only the body takes the pointer, not the window's margin around it.
    let body = pill_body(p, status_dot(dot), title, subtitle)
        .w_full()
        .child(row().key("actions").gap(8.0).children(actions))
        .hit()
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .child(body)
}

/// The panel capsule's corners, concentric with its buttons' (`radius.md`)
/// across the [`ACTIONS_INSET`] between them and the edge.
fn panel_radius(p: Paint) -> f32 {
    p.k.radius.md + ACTIONS_INSET
}

/// The panel capsule's body: its dot and two truncated lines in a rounded
/// rectangle.
fn pill_body(p: Paint, dot: El, title: &str, subtitle: &str) -> El {
    let c = &p.k.c;
    // A status named by its title, announced when the capsule's title
    // changes; the subtitle stays readable inside it.
    row()
        .key("pill")
        .role("status")
        .label(title)
        .h_full()
        .gap(12.0)
        .pl(16.0)
        .pr(ACTIONS_INSET)
        .center_y()
        .radius(panel_radius(p))
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(dot)
        .child(
            col()
                .key("lines")
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(title.to_owned())
                        .key("title")
                        .truncate()
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .a11y_hidden(),
                )
                .children((!subtitle.is_empty()).then(|| {
                    text(subtitle.to_owned())
                        .key("subtitle")
                        .truncate()
                        .font(p.k.ty.small)
                        .color(p.faint())
                })),
        )
}
