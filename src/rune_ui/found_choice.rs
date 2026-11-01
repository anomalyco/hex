//! Onboarding's speech model list, shown only when the hub-cache scan found
//! a compatible model (`found_models`): one card of selectable rows, Hex's
//! recommended model first and selected, then the models already on this
//! computer. Picking a row starts it at once: ours downloads (or, when its
//! exact file is already here, is used in place), and a found model is used
//! where it is. Continue starts the selected row when none was picked.
//! Without a found model the step is exactly as before.

use rune::{El, Tx, row};
use rune_kit::{Badge, RadioItem};

use super::Ui;
use super::controls::{Paint, card};
use crate::desktop::i18n::{tr, tr_fill};
use crate::desktop::onboarding::{Input, ItemStatus, SetupItem};
use crate::transcription::found_models::Found;
use crate::transcription::models::{self as transcription_models, TranscriptionSelection};

/// Whether onboarding shows the list: something compatible was found and
/// the model isn't ready yet.
pub(super) fn offered(ui: &Ui, model: ItemStatus) -> bool {
    model != ItemStatus::Ready && !ui.found.models.is_empty()
}

/// Whether the list waits for its first pick, so Continue starts the
/// selected row.
pub(super) fn pending(ui: &Ui, model: ItemStatus) -> bool {
    offered(ui, model)
        && matches!(model, ItemStatus::Todo | ItemStatus::Failed)
        && ui.found.picked.is_none()
        && !ui.found.adopting()
}

/// One row of the list: Hex's recommended model, or a found file.
enum Entry<'a> {
    Recommended { on_disk: bool },
    Found(&'a Found),
}

fn entries(ui: &Ui) -> Vec<Entry<'_>> {
    let recommended = ui.settings.transcription.model;
    let on_disk = ui
        .found
        .models
        .iter()
        .any(|found| found.catalog == Some(recommended));
    std::iter::once(Entry::Recommended { on_disk })
        .chain(
            ui.found
                .models
                .iter()
                .filter(|found| found.catalog != Some(recommended))
                .map(Entry::Found),
        )
        .collect()
}

/// The list, one radio row per model.
pub(super) fn choice(ui: &Ui, p: Paint, model: ItemStatus) -> El {
    let selected = ui.found.picked.unwrap_or(0);
    let rows: Vec<El> = entries(ui)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| model_row(ui, p, index, &entry, index == selected, model))
        .collect();
    card(p, "model-list", rows)
}

fn model_row(
    ui: &Ui,
    p: Paint,
    index: usize,
    entry: &Entry<'_>,
    selected: bool,
    model: ItemStatus,
) -> El {
    let name = match entry {
        Entry::Recommended { .. } => {
            transcription_models::definition(ui.settings.transcription.model).name
        }
        Entry::Found(found) => found.catalog.map_or(found.name.as_str(), |id| {
            transcription_models::definition(id).name
        }),
    };
    let started = selected && ui.found.picked.is_some();
    let line = match (entry, started, model) {
        (_, true, ItemStatus::Failed) => tr("Couldn't prepare this model").to_owned(),
        (Entry::Recommended { on_disk: false }, true, ItemStatus::Downloading(progress)) => {
            tr_fill(
                "Downloading… {}%",
                &((progress * 100.0).round() as u32).to_string(),
            )
        }
        (_, true, _) if ui.found.adopting() || model != ItemStatus::Ready => {
            tr("Checking…").to_owned()
        }
        (Entry::Recommended { on_disk: false }, ..) => tr_fill(
            "{} download",
            &transcription_models::definition(ui.settings.transcription.model).size_label(),
        ),
        (Entry::Recommended { on_disk: true }, ..) => tr("On this computer").to_owned(),
        (Entry::Found(found), ..) if found.catalog.is_some() => tr("On this computer").to_owned(),
        (Entry::Found(_), ..) => tr("On this computer · not tested by Hex").to_owned(),
    };
    let radio = RadioItem::new(name, selected)
        .description(line)
        .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
            tx.instant();
            pick(ui, index);
        })
        .build(p.k)
        .key("radio")
        .grow(1.0);
    let tag = matches!(entry, Entry::Recommended { .. })
        .then(|| Badge::new(tr("Recommended")).build(p.k).key("recommended"));
    row()
        .key(format!("model{index}"))
        .min_h(super::controls::METRICS.row_h_described)
        .px(super::controls::METRICS.row_px)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(p.k.c.hover)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            tx.instant();
            pick(ui, index);
        })
        .child(radio)
        .children(tag)
}

/// Starts the selected row when the person continues without picking one.
pub(super) fn start_selected(ui: &mut Ui) {
    pick(ui, ui.found.picked.unwrap_or(0));
}

/// Picks row `index` and starts it. A newer pick supersedes one still
/// preparing, as the picker's does.
fn pick(ui: &mut Ui, index: usize) {
    let failed = ui
        .tour
        .as_ref()
        .is_some_and(|tour| tour.flow.view().model == ItemStatus::Failed);
    if (ui.found.picked == Some(index) && !failed) || ui.found.adopting() {
        return;
    }
    let recommended = ui.settings.transcription.model;
    let found = match entries(ui).get(index) {
        None => return,
        Some(Entry::Recommended { .. }) => None,
        Some(Entry::Found(found)) => Some((*found).clone()),
    };
    ui.found.picked = Some(index);
    ui.found.error = None;
    match found {
        None => {
            crate::transcription::found_models::accept(recommended);
            prepare(ui, ui.settings.transcription.clone());
        }
        Some(found) => match found.catalog {
            Some(model) => {
                crate::transcription::found_models::accept(model);
                let language = transcription_models::definition(model)
                    .language_for(&ui.settings.transcription.language)
                    .to_owned();
                prepare(ui, TranscriptionSelection { model, language });
            }
            // Hashed once and registered in place first, then prepared.
            None => super::found::adopt(ui, found, true),
        },
    }
}

/// Prepares `selection` for onboarding: through the tour the first time,
/// so its progress shows, and as a newer choice afterwards.
pub(super) fn prepare(ui: &mut Ui, selection: TranscriptionSelection) {
    let requested = ui.tour.as_ref().is_some_and(|tour| {
        !matches!(
            tour.flow.view().model,
            ItemStatus::Todo | ItemStatus::Failed
        )
    });
    if requested {
        ui.send(super::Request::ChooseModel {
            model: selection.model,
            language: selection.language,
        });
    } else {
        ui.found.choice = Some(selection);
        super::onboarding::input(ui, Input::Grant(SetupItem::Model));
    }
}
