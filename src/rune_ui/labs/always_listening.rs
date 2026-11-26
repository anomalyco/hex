//! Labs › Always listening's card while on: how long a pause finishes a
//! dictation.

use rune::{El, row, text};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use crate::app_settings::ALWAYS_LISTENING_PAUSE_SECONDS;
use crate::desktop::i18n::{tr, tr_fill};

pub(super) fn body(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let labels: Vec<String> = ALWAYS_LISTENING_PAUSE_SECONDS
        .iter()
        .map(|seconds| tr_fill("{} s", &seconds.to_string()))
        .collect();
    let selected = ALWAYS_LISTENING_PAUSE_SECONDS
        .iter()
        .position(|seconds| *seconds == ui.settings.labs.always_listening_pause_seconds)
        .unwrap_or(1);
    let select = super::super::general::quiet_select(
        ui,
        p,
        super::super::Menu::Pick("always-listening-pause"),
        "always-listening-pause-seconds".to_owned(),
        tr("Finish after a pause of"),
        labels,
        selected,
        |ui, index| {
            let seconds = ALWAYS_LISTENING_PAUSE_SECONDS[index];
            ui.update(|settings| settings.labs.always_listening_pause_seconds = seconds);
        },
        layer,
    );
    row()
        .key("always-listening-pause-row")
        .gap(8.0)
        .center_y()
        .flex_wrap()
        .child(
            text(tr("Finish after a pause of"))
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(select)
}
