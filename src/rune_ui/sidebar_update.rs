//! The sidebar's update: Restart to update, Download update, or a channel
//! switch, and under Restart to update what the staged dev build changed.

use super::*;

const SHOWN: usize = 3;

/// An available update, as a secondary button with a dot.
pub(super) fn control(ui: &Ui, p: Paint) -> Option<El> {
    let action = match ui.view.as_ref().map(|view| &view.update_status) {
        Some(DesktopUpdateStatus::ReadyToRestart) => Some((
            tr("Restart to update").to_owned(),
            DesktopAction::RestartIntoUpdate,
        )),
        Some(DesktopUpdateStatus::Available) => {
            Some((tr("Download update").to_owned(), DesktopAction::OpenUpdate))
        }
        Some(DesktopUpdateStatus::SwitchAvailable { channel, version }) => Some((
            crate::updater::switch_label(channel, version),
            DesktopAction::ConfirmUpdateSwitch,
        )),
        _ => None,
    };
    // An available update is a state, not the screen's main action.
    action.map(|(label, action)| {
        row()
            .key("sidebar-update-control")
            .gap(8.0)
            .pl(6.0)
            .center_y()
            .child(controls::status_dot(p.accent()))
            .child(controls::button(
                p,
                "sidebar-update",
                &label,
                controls::ButtonKind::Secondary,
                move |ui: &mut Ui, _| ui.send(Request::Desktop(action.clone())),
            ))
    })
}

/// What the staged dev build changed, while Restart to update shows.
pub(super) fn notes(ui: &Ui, p: Paint) -> Option<El> {
    ui.view
        .as_ref()
        .filter(|view| view.update_status == DesktopUpdateStatus::ReadyToRestart)
        .and_then(|view| view.update_notes.as_ref())
        .and_then(|notes| list(p, notes))
}

/// The first few changes and What's new, which opens the build's entry in
/// the dev changelog.
fn list(p: Paint, notes: &crate::desktop::host::UpdateNotes) -> Option<El> {
    if notes.items.is_empty() {
        return None;
    }
    let c = &p.k.c;
    let url = notes.url.clone();
    let link = row()
        .key("whats-new")
        .h(FOOTER_LINE)
        .px(6.0)
        .center_y()
        .self_start()
        .radius(p.k.radius.sm)
        .role("link")
        .label(tr("What's new"))
        .hover(c.hover)
        .press(c.pressed)
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
        .on_press(move |ui: &mut Ui| {
            if ui.live()
                && let Err(error) = crate::desktop::opencode_catalog::open_url(&url)
            {
                ui.error = Some(format!("{error:#}"));
            }
        })
        .child(
            text(tr("What's new"))
                .key("label")
                .font(p.k.ty.small)
                .color(c.text_muted)
                .truncate(),
        );
    let items = notes
        .items
        .iter()
        .take(SHOWN)
        .enumerate()
        .map(|(index, item)| {
            text(format!("· {item}"))
                .key(format!("item{index}"))
                .font(p.k.ty.small)
                .color(p.faint())
                .truncate()
                .px(6.0)
        });
    Some(
        col()
            .key("sidebar-update-notes")
            .mt(6.0)
            .gap(2.0)
            .child(link)
            .children(items),
    )
}
