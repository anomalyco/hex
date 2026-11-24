//! Labs › Model switch shortcut in General: the key, and the other model
//! and language it swaps to.

use super::*;

/// The lab's rows: one recorder and the other model's speech row, shown only
/// while Labs › Model switch shortcut is on.
pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let mut rows = shortcut_rows(
        ui,
        p,
        "model-switch-shortcut",
        &KEYBOARD,
        tr("Model switch shortcut"),
        ShortcutTarget::ModelSwitch,
        Some(tr("Swaps dictation to the other model, and back")),
    );
    rows.push(other_model_row(ui, p, layer).into());
    rows
}

/// The other model and language, chosen like a keyboard-language mapping's:
/// an installed model, then a language it supports.
fn other_model_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    super::super::speech_model::setting(
        ui,
        p,
        super::super::speech_model::SpeechRow {
            key: "model-switch-other",
            model_key: "model-switch-other-model",
            language_key: "model-switch-other-language",
            title: tr("Other model"),
            description: Some(tr("The model and language the shortcut switches to")),
            choice: ui.settings.model_switch_other.clone(),
            empty_label: tr("None"),
            model_menu: Menu::Pick("model-switch-other-model"),
            language_menu: Menu::Pick("model-switch-other-language"),
        },
        |model, fallback| model.language_for(fallback).to_owned(),
        |ui, choice| {
            ui.update(|settings| settings.model_switch_other = choice);
        },
        layer,
    )
}
