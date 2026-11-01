//! Onboarding's model choice, shown only when the hub-cache scan found a
//! compatible model (`found_models`). Hex's recommended model is the default
//! (Return picks it): its exact copy already on this computer when the cache
//! holds one, with Download instead beside it, else its download. Other
//! models on this computer sit behind "Use a model already on this
//! computer": known catalog models first, then unverified ones. Nothing is
//! picked for the person, and without a found model the step is exactly as
//! before.

use rune::{El, col, row, text};
use rune_kit::{Badge, glyphs};

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, Paint, button, card, icon_row_tagged};
use crate::desktop::i18n::{tr, tr_named};
use crate::desktop::onboarding::{Input, SetupItem};
use crate::transcription::found_models::Found;
use crate::transcription::models::{self as transcription_models, TranscriptionSelection};

/// Whether the model step offers the choice: something compatible was found
/// and the model isn't ready yet.
pub(super) fn offered(ui: &Ui, model_ready: bool) -> bool {
    !model_ready && !ui.found.models.is_empty()
}

/// The found file that is exactly the recommended model, if any.
fn recommended_on_disk(ui: &Ui) -> Option<&Found> {
    ui.found
        .models
        .iter()
        .find(|found| found.catalog == Some(ui.settings.transcription.model))
}

/// The recommended model, then the models already on this computer behind
/// a secondary button. The primary action is the recommended copy already
/// here when the cache holds it exactly (Download instead stays possible),
/// else downloading it.
pub(super) fn choice(ui: &Ui, p: Paint) -> El {
    let model = transcription_models::definition(ui.settings.transcription.model);
    let on_disk = recommended_on_disk(ui);
    let detail = format!(
        "{} · {}\n{}",
        model.name,
        model.size_label(),
        match on_disk {
            Some(found) => tr_named(
                "Already on this computer: {source}",
                &[("source", &found.source())],
            ),
            None => tr("Tested by Hex for quality and reliability").to_owned(),
        }
    );
    let recommended = icon_row_tagged(
        p,
        "choice-recommended",
        &glyphs::SPARKLE,
        tr("Hex's recommended model"),
        Some(Badge::new(tr("Recommended")).build(p.k)),
        Some(&detail),
        row(),
    );
    let others: Vec<El> = ui
        .found
        .models
        .iter()
        .enumerate()
        .filter(|(_, found)| found.catalog != Some(ui.settings.transcription.model))
        .map(|(index, found)| found_row(p, index, found))
        .collect();
    let mut actions = row().key("choice-actions").gap(8.0).center_y();
    // With only the recommended copy found, there is nothing else to show.
    if !others.is_empty() {
        actions = actions.child(button(
            p,
            "show-found",
            tr("Use a model already on this computer"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.found.expanded = !ui.found.expanded,
        ));
    }
    actions = actions.child(rune::spacer());
    actions = if on_disk.is_some() {
        actions
            .child(button(
                p,
                "download-instead",
                tr("Download instead"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    crate::transcription::found_models::decline(ui.settings.transcription.model);
                    choose_recommended(ui);
                },
            ))
            .child(
                button(
                    p,
                    "use-recommended",
                    tr("Use the copy already on this computer"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| {
                        crate::transcription::found_models::accept(ui.settings.transcription.model);
                        choose_recommended(ui);
                    },
                )
                .default_button(),
            )
    } else {
        actions.child(
            button(
                p,
                "download-recommended",
                tr("Download recommended model"),
                ButtonKind::Primary,
                |ui: &mut Ui, _| choose_recommended(ui),
            )
            .default_button(),
        )
    };
    let mut page = col()
        .key("model-choice")
        .gap(16.0)
        .child(card(p, "recommended-card", vec![recommended]))
        .child(actions);
    if ui.found.expanded && !others.is_empty() {
        page = page.child(
            col()
                .key("found")
                .gap(8.0)
                .child(card(p, "found-card", others))
                .child(
                    text(tr(
                        "Compatible doesn't mean accurate: a model that loads can still transcribe poorly.",
                    ))
                    .key("found-note")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
                ),
        );
    }
    page
}

/// A found file: exact catalog matches (first, from the scan's order) are
/// known models; any other is unverified and not benchmarked.
fn found_row(p: Paint, index: usize, found: &Found) -> El {
    let catalog = found.catalog.map(transcription_models::definition);
    let name = catalog.map_or(found.name.as_str(), |model| model.name);
    let mut detail = tr_named(
        "{revision} · {compression} · {source}",
        &[
            ("revision", found.short_revision()),
            ("compression", found.compression),
            ("source", &found.repository),
        ],
    );
    detail.push('\n');
    detail.push_str(if catalog.is_some() {
        tr("Already downloaded, no download needed")
    } else {
        tr("Not verified by Hex · Not benchmarked")
    });
    let tag = if catalog.is_some() {
        Badge::new(tr("Known model")).build(p.k)
    } else {
        Badge::new(tr("Not tested by Hex")).build(p.k)
    };
    icon_row_tagged(
        p,
        &format!("found-{index}"),
        &glyphs::SPARKLE,
        name,
        Some(tag),
        Some(&detail),
        button(
            p,
            &format!("use-found-{index}"),
            tr("Use"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| choose_found(ui, index),
        ),
    )
}

fn choose_recommended(ui: &mut Ui) {
    ui.found.choice = None;
    super::onboarding::input(ui, Input::Grant(SetupItem::Model));
}

/// A catalog match prepares that catalog model (Hex verifies the file in
/// place instead of downloading); any other file is hashed and registered
/// first (`found::adopt`), then prepared.
fn choose_found(ui: &mut Ui, index: usize) {
    let Some(found) = ui.found.models.get(index).cloned() else {
        return;
    };
    match found.catalog {
        Some(model) => {
            crate::transcription::found_models::accept(model);
            let language = transcription_models::definition(model)
                .language_for(&ui.settings.transcription.language)
                .to_owned();
            ui.found.choice = Some(TranscriptionSelection { model, language });
            super::onboarding::input(ui, Input::Grant(SetupItem::Model));
        }
        None => super::found::adopt(ui, found, true),
    }
}
