//! General's always-visible basics and per-window Advanced fold.
use super::*;
use crate::rune_ui::controls::section;
use crate::rune_ui::{platform_settings as os, scroll_body};

pub(in crate::rune_ui) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let mut content = controls::page("content")
        .children(os::permissions_section(ui, p))
        .children(os::error_section(ui, p));
    if ui.platform.caps.listener_control {
        content = content.child(s::listener_card(ui, p).1);
    }
    if !ui.records_shortcuts() {
        content = content.child(s::desktop_commands(ui, p));
    }
    content = content
        .children(basics(ui, p, layer))
        .child(advanced_toggle(ui, p, Pane::General));
    if advanced_open(ui, Pane::General) {
        for (key, title, rows) in [
            ("more-dictation", tr("Dictation"), dictation(ui, p, layer)),
            ("output", tr("Output"), output(ui, p, layer)),
            ("application", tr("Application"), application(ui, p, layer)),
        ] {
            if !rows.is_empty() {
                content = content.child(section(p, key, title, rows));
            }
        }
    }
    scroll_body(p.k, "scroll", content)
}

/// Toggle's cancel shortcut. A shortcuts portal would hold its key from
/// every app for as long as Hex runs, so on Wayland it says to bind
/// `hex dictation cancel` in the desktop instead. Its description comes back
/// for search too.
pub(super) fn cancel_rows(ui: &Ui, p: Paint) -> (&'static str, Vec<CardRow>) {
    if matches!(
        ui.shortcuts(),
        Some(crate::desktop::host::DesktopShortcuts::Portal(_))
    ) {
        let description =
            tr("Bind hex dictation cancel to a key in your desktop's keyboard shortcuts");
        return (
            description,
            vec![
                icon_row(
                    p,
                    "cancel-shortcut",
                    &CIRCLE_X,
                    tr("Cancel"),
                    Some(description),
                    row().key("cancel-shortcut-control"),
                )
                .into(),
            ],
        );
    }
    let description = tr("Discards the recording");
    (
        description,
        shortcut_rows(
            ui,
            p,
            "cancel-shortcut",
            &CIRCLE_X,
            tr("Cancel"),
            ShortcutTarget::Cancel,
            Some(description),
        ),
    )
}
