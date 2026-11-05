//! Labs › Screenshots' card while on: its shortcut, where shots go, and a note
//! while macOS's own screenshot shortcut takes the same keys first.

use rune::{El, col, row, text};

use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button};
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
