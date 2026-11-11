//! Labs › Screenshots' card while on: its shortcut, where shots go, and a note
//! while macOS's own screenshot shortcut takes the same keys first.

use rune::{El, col, row, text};

use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button, icon_button};
use super::super::general::keys_button;
use super::super::{ShortcutTarget, Ui};
use crate::desktop::i18n::tr;

pub(super) fn body(ui: &Ui, p: Paint, system_takes_shortcut: bool) -> El {
    let folder = ui.settings.screenshots.folder();
    let folder = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut body = col()
        .key("screenshots")
        .gap(12.0)
        .child(fact(
            p,
            "screenshot-shortcut",
            tr("Shortcut"),
            keys_button(ui, p, ShortcutTarget::Screenshot),
        ))
        .child(fact(
            p,
            "screenshot-record-shortcut",
            tr("Record screen"),
            record_control(ui, p),
        ))
        .child(fact(
            p,
            "screenshot-folder",
            tr("Saves to"),
            text(folder).key("value").font(DESCRIPTION).color(p.text()),
        ));
    if system_takes_shortcut {
        body = body.child(
            col()
                .key("system-shortcut")
                .gap(8.0)
                .child(
                    text(tr("macOS takes this shortcut first. Turn off its screenshot shortcuts in Keyboard Shortcuts › Screenshots, or choose another"))
                        .key("copy")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                )
                .child(row().key("actions").child(button(
                    p,
                    "open-keyboard-settings",
                    tr("Open Keyboard Settings"),
                    ButtonKind::Secondary,
                    |_, _| open_keyboard_settings(),
                ))),
        );
    }
    body
}

/// The record shortcut, unbound until recorded, or why it can't record:
/// a recording is an Annotate capture. With Annotate off, a saved shortcut
/// still shows (and can be cleared) so the key it holds stays visible.
fn record_control(ui: &Ui, p: Paint) -> El {
    let saved = ui.settings.screenshots.record_hotkey.is_some();
    if !ui.settings.capture_on() {
        let mut control = row().key("control").gap(8.0).center_y().child(
            text(tr("Needs Annotate"))
                .key("needs-annotate")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
        if saved {
            control = control
                .child(keys_button(ui, p, ShortcutTarget::ScreenshotRecord))
                .child(clear_record_shortcut(p));
        }
        return control;
    }
    let mut control = row().key("control").gap(4.0).center_y().child(keys_button(
        ui,
        p,
        ShortcutTarget::ScreenshotRecord,
    ));
    if saved {
        control = control.child(clear_record_shortcut(p));
    }
    control
}

/// Clears the saved record shortcut, freeing the key it holds while
/// Annotate is off.
fn clear_record_shortcut(p: Paint) -> El {
    icon_button(
        p,
        "clear-screenshot-record-shortcut",
        &rune_theme_opencode::icons::TRASH,
        tr("Clear shortcut"),
        |ui: &mut Ui| {
            ui.update(|settings| settings.screenshots.record_hotkey = None);
        },
    )
}

fn fact(p: Paint, key: &str, label: &str, value: El) -> El {
    row()
        .key(key)
        .gap(8.0)
        .center_y()
        .child(
            text(label)
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted())
                .grow(1.0),
        )
        .child(value)
}

fn open_keyboard_settings() {
    crate::screenshots::system_shortcut::open_settings();
}
