//! Labs › YOLO mode's card while on: how long a
//! dictation must be for Return to follow its paste, and the apps it sends
//! in, chosen with the Modes pane's floating application picker.

use rune::{El, col, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use super::super::modes::{self, PickerTarget};
use crate::app_settings::YOLO_MODE_SECONDS;
use crate::desktop::i18n::{tr, tr_fill};

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
    col()
        .key("yolo-mode")
        .gap(6.0)
        .child(
            row()
                .key("yolo-mode-row")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(label(p, tr("Longer than")))
                .child(longer_than(ui, p, layer)),
        )
        .child(only_in(ui, p))
}

const PICKER_ANCHOR: &str = "open-yolo-mode-picker";

fn label(p: Paint, words: &str) -> El {
    text(words.to_owned())
        .key("label")
        .font(DESCRIPTION)
        .color(p.muted())
}

/// The shortest dictation that sends: 1, 2, 3, 5, or 10 seconds.
fn longer_than(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let labels: Vec<String> = YOLO_MODE_SECONDS
        .iter()
        .map(|seconds| tr_fill("{} s", &seconds.to_string()))
        .collect();
    let selected = YOLO_MODE_SECONDS
        .iter()
        .position(|seconds| *seconds == ui.settings.labs.yolo_mode_seconds)
        .unwrap_or(1);
    super::super::general::quiet_select(
        ui,
        p,
        super::super::Menu::Pick("yolo-mode"),
        "yolo-mode-seconds".to_owned(),
        tr("Longer than"),
        labels,
        selected,
        |ui, index| {
            let seconds = YOLO_MODE_SECONDS[index];
            ui.update(|settings| settings.labs.yolo_mode_seconds = seconds);
        },
        layer,
    )
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
