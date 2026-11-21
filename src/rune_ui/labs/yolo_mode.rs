//! Choose where YOLO mode sends with the Modes pane's application picker.

use rune::{El, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use super::super::modes::{self, PickerTarget};
use crate::desktop::i18n::tr;

pub(super) fn body(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    if ui.modes.application_picker_open && ui.modes.picker_target == PickerTarget::YoloMode {
        modes::application_picker(
            ui,
            p,
            &ui.settings.labs.yolo_mode_apps,
            PICKER_ANCHOR,
            layer,
        );
    }
    only_in(ui, p)
}

const PICKER_ANCHOR: &str = "open-yolo-mode-picker";

fn label(p: Paint, words: &str) -> El {
    text(words.to_owned())
        .key("label")
        .font(DESCRIPTION)
        .color(p.muted())
}

/// Only in: the chosen apps as chips, or Every app, then the add button.
fn only_in(ui: &Ui, p: Paint) -> El {
    let apps = &ui.settings.labs.yolo_mode_apps;
    let mut row = row()
        .key("yolo-mode-apps")
        .gap(4.0)
        .center_y()
        .flex_wrap()
        .child(label(p, tr("Only in")).mr(4.0));
    if apps.is_empty() {
        row = row.child(
            text(tr("Every app"))
                .key("every-app")
                .font(DESCRIPTION)
                .color(p.text()),
        );
    }
    for (index, name) in apps.iter().enumerate() {
        let remove = name.clone();
        row = row.child(modes::application_chip(
            ui,
            p,
            index,
            name,
            move |ui: &mut Ui| {
                let remove = remove.clone();
                ui.update(|settings| settings.labs.yolo_mode_apps.retain(|app| *app != remove));
            },
        ));
    }
    row.child(
        IconButton::new(&rune_theme_opencode::icons::PLUS)
            .variant(IconButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .label(tr("Add application"))
            .on_press(|ui: &mut Ui| modes::toggle_picker(ui, PickerTarget::YoloMode))
            .build(p.k)
            .key(PICKER_ANCHOR),
    )
}
