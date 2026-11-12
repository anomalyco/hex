//! Labs › More dictation shortcuts (macOS): General's numbered rows for up
//! to four extra dictation shortcuts, each with its recorder, its own
//! shortcut mode, and a Remove button, then the row that adds another.

use rune::El;

use super::super::icons::KEYBOARD;
use super::super::lists::small_button;
use super::super::shortcut_mode::Trigger;
use super::*;
use crate::app_settings::{HotkeyBinding, MAX_EXTRA_DICTATION_SHORTCUTS};

/// The select menus for the rows' shortcut-mode choices, in row order (four
/// rows at most).
const MODE_MENUS: [&str; MAX_EXTRA_DICTATION_SHORTCUTS] = [
    "extra-dictation-mode-1",
    "extra-dictation-mode-2",
    "extra-dictation-mode-3",
    "extra-dictation-mode-4",
];

/// The extra shortcuts' rows: one per saved row, then Add while there is
/// room for another.
pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let mut built = Vec::new();
    for index in 0..ui.settings.extra_dictation_hotkeys.len() {
        built.push(extra_row(ui, p, index, layer));
    }
    if ui.settings.extra_dictation_hotkeys.len() < MAX_EXTRA_DICTATION_SHORTCUTS {
        built.push(add_row(p));
    }
    built
}

/// Row `index`'s recorder, its shortcut mode, and the button that removes it.
fn extra_row(ui: &Ui, p: Paint, index: usize, layer: &mut Vec<El>) -> CardRow {
    let target = ShortcutTarget::ExtraDictation(index);
    let binding = ui.settings.extra_dictation_hotkeys[index].binding.as_ref();
    let control = row()
        .key(format!("extra-dictation-{index}-control"))
        .gap(4.0)
        .center_y()
        .child(keys_button(ui, p, target))
        .children(mode_select(ui, p, index, binding, layer))
        .child(icon_button(
            p,
            &format!("remove-extra-dictation-{index}"),
            &rune_theme_opencode::icons::TRASH,
            tr("Remove"),
            move |ui: &mut Ui| {
                // A recorder still listening for this row would outlive it.
                s::cancel_capture(ui);
                ui.update(|settings| {
                    let _ = settings.remove_extra_dictation_hotkey(index);
                });
            },
        ));
    icon_described_row(
        p,
        &format!("extra-dictation-{index}"),
        &KEYBOARD,
        &tr_fill("Extra shortcut {}", &(index + 1).to_string()),
        None,
        control,
    )
    .into()
}

/// Row `index`'s shortcut mode: the choices its binding can trigger, in the
/// dictation shortcut's order. Double-tap needs a key, or Labs › Double-tap
/// on a lone modifier, as it does there.
fn mode_select(
    ui: &Ui,
    p: Paint,
    index: usize,
    binding: Option<&HotkeyBinding>,
    layer: &mut Vec<El>,
) -> Option<El> {
    let menu = *MODE_MENUS.get(index)?;
    let trigger = Trigger::from_extra(ui.settings.extra_dictation_mode(index));
    let modifiers_allowed = ui.settings.double_tap_modifiers_allowed();
    let available: Vec<Trigger> = Trigger::ALL
        .into_iter()
        .filter(|choice| choice.unavailable_for(binding, modifiers_allowed).is_none())
        .collect();
    let labels: Vec<String> = available
        .iter()
        .map(|choice| choice.name().to_owned())
        .collect();
    let selected = available
        .iter()
        .position(|choice| *choice == trigger)
        .unwrap_or(0);
    Some(quiet_select(
        ui,
        p,
        Menu::Pick(menu),
        format!("{menu}-control"),
        tr("Shortcut mode"),
        labels,
        selected,
        move |ui: &mut Ui, choice| {
            let Some(trigger) = available.get(choice).copied() else {
                return;
            };
            ui.update(|settings| {
                if let Some(row) = settings.extra_dictation_hotkeys.get_mut(index) {
                    row.mode = trigger.to_extra();
                }
            });
        },
        layer,
    ))
}

/// The Add row, while there is room for another shortcut.
fn add_row(p: Paint) -> CardRow {
    icon_row(
        p,
        "add-extra-dictation",
        &KEYBOARD,
        tr("Add shortcut"),
        None,
        small_button(
            p,
            "add-extra-dictation-control",
            tr("Add"),
            |ui: &mut Ui, _| {
                ui.update(|settings| {
                    let _ = settings.add_extra_dictation_hotkey();
                });
            },
        ),
    )
    .into()
}
