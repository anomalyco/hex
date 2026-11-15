//! Labs › Screenshots' card while on: its shortcut, where shots go, and a note
//! while macOS's own screenshot shortcut takes the same keys first.

use rune::{El, col, row, text};

use super::super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, folder_label, icon_button, note,
};
use super::super::general::keys_button;
use super::super::{ShortcutTarget, Ui};
use crate::desktop::file_dialog::{Choice, Dialog, Pending, Target};
use crate::desktop::i18n::{tr, tr_named};

/// The saves-folder chooser's answer, polled from the window: the folder
/// picked becomes `screenshots.folder` at once, and a chooser that couldn't
/// open says why under the row.
#[derive(Default)]
pub(in crate::rune_ui) struct FolderChoice {
    /// The open dialog, while one is.
    pending: Option<Pending>,
    /// Why the last chooser failed, until another is opened.
    error: Option<String>,
}

pub(super) fn body(ui: &Ui, p: Paint, system_takes_shortcut: bool) -> El {
    let folder = ui.settings.screenshots.folder();
    let label = folder_label(&folder);
    let reveal_at = folder.clone();
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
            row()
                .key("controls")
                .gap(8.0)
                .center_y()
                .child(button(
                    p,
                    "reveal-screenshot-folder",
                    tr("Reveal"),
                    ButtonKind::Secondary,
                    move |_, _| reveal_folder(&reveal_at),
                ))
                .child(button(
                    p,
                    "choose-screenshot-folder",
                    &label,
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| choose_folder(ui),
                )),
        ));
    if let Some(error) = &ui.screenshot_folder.error {
        body = body.child(note("screenshot-folder-error", error, p.danger()));
    } else if let Some(error) = crate::screenshots::save_error() {
        body = body.child(note(
            "screenshot-save-error",
            &tr_named(
                "Couldn't save to {folder}: {error}",
                &[("folder", label.as_str()), ("error", error.as_str())],
            ),
            p.danger(),
        ));
    }
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

/// The saves-folder chooser, one at a time, starting where shots go now.
fn choose_folder(ui: &mut Ui) {
    if ui.screenshot_folder.pending.is_some() {
        return;
    }
    ui.screenshot_folder.error = None;
    let waker = ui.modes.waker.clone();
    let wake = move || {
        if let Some(waker) = &waker {
            waker();
        }
    };
    ui.screenshot_folder.pending = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Screenshots folder").into(),
            target: Target::Folder,
            directory: Some(ui.settings.screenshots.folder()),
        },
        wake,
    ));
}

/// The chooser's answer, polled from the window: a picked folder is saved
/// and applied live, a cancel changes nothing, and a chooser that couldn't
/// open shows why. True when the card shows something new.
pub(in crate::rune_ui) fn poll(ui: &mut Ui) -> bool {
    let Some(choice) = ui
        .screenshot_folder
        .pending
        .as_ref()
        .and_then(Pending::poll)
    else {
        return false;
    };
    ui.screenshot_folder.pending = None;
    apply_chosen_folder(ui, choice)
}

/// What a chooser's answer does to the card and its settings.
pub(in crate::rune_ui) fn apply_chosen_folder(ui: &mut Ui, choice: Choice) -> bool {
    match choice {
        Ok(Some(folder)) => {
            ui.screenshot_folder.error = None;
            // A save that failed into the old folder doesn't speak for
            // the new one.
            crate::screenshots::clear_save_error();
            ui.update(|settings| settings.screenshots.folder = Some(folder));
            true
        }
        Ok(None) => false,
        Err(error) => {
            ui.screenshot_folder.error = Some(error);
            true
        }
    }
}

/// Shows the saves folder in Finder, making it first as a save would.
fn reveal_folder(folder: &std::path::Path) {
    if let Err(error) = std::fs::create_dir_all(folder) {
        tracing::warn!(%error, folder = %folder.display(), "could not make the screenshots folder");
        return;
    }
    crate::meeting::open_folder(folder);
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
