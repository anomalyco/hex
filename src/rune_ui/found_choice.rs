//! Onboarding's model choice, shown only when the hub-cache scan found a
//! compatible model (`found_models`): Hex's recommended model, marked
//! Recommended and the default (Return picks it), or one already on this
//! computer. Nothing is picked for the person, and without a found model
//! the step is exactly as before.

use rune::{El, col, text};
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
    !model_ready && !ui.found_models.is_empty()
}

/// The found file that is exactly the recommended model, if any.
fn recommended_on_disk(ui: &Ui) -> Option<&Found> {
    ui.found_models
        .iter()
        .find(|found| found.catalog == Some(ui.settings.transcription.model))
}

/// The two options under the setup card.
pub(super) fn choice(ui: &Ui, p: Paint) -> El {
    let model = transcription_models::definition(ui.settings.transcription.model);
    let on_disk = recommended_on_disk(ui).is_some();
    let detail = format!(
        "{} · {}\n{}",
        model.name,
        model.size_label(),
        if on_disk {
            tr("Already downloaded, no download needed")
        } else {
            tr("Tested by Hex for quality and reliability")
        }
    );
    let recommended = icon_row_tagged(
        p,
        "choice-recommended",
        &glyphs::SPARKLE,
        tr("Hex's recommended model"),
        Some(Badge::new(tr("Recommended")).build(p.k)),
        Some(&detail),
        button(
            p,
            "use-recommended",
            if on_disk { tr("Use") } else { tr("Download") },
            ButtonKind::Primary,
            |ui: &mut Ui, _| choose_recommended(ui),
        )
        .default_button(),
    );
    let mut found_rows: Vec<El> = Vec::new();
    for (index, found) in ui.found_models.iter().enumerate() {
        found_rows.push(found_row(ui, p, index, found));
    }
    col()
        .key("model-choice")
        .gap(16.0)
        .child(card(p, "recommended-card", vec![recommended]))
        .child(
            col()
                .key("found")
                .gap(8.0)
                .child(
                    text(tr("Use a model already on this computer"))
                        .key("found-title")
                        .font(p.k.ty.body_medium)
                        .color(p.text()),
                )
                .child(card(p, "found-card", found_rows))
                .child(
                    text(tr(
                        "Compatible doesn't mean accurate: a model that loads can still transcribe poorly.",
                    ))
                    .key("found-note")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
                ),
        )
}

fn found_row(ui: &Ui, p: Paint, index: usize, found: &Found) -> El {
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
        tr("Not tested by Hex")
    });
    let tag = (found.catalog == Some(ui.settings.transcription.model))
        .then(|| Badge::new(tr("Recommended")).build(p.k));
    icon_row_tagged(
        p,
        &format!("found-{index}"),
        &glyphs::SPARKLE,
        name,
        tag,
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
    ui.onboarding_choice = None;
    super::onboarding::input(ui, Input::Grant(SetupItem::Model));
}

/// A catalog match prepares that catalog model (Hex verifies the file in
/// place instead of downloading); any other file is hashed and registered
/// first (`found::adopt`), then prepared.
fn choose_found(ui: &mut Ui, index: usize) {
    let Some(found) = ui.found_models.get(index).cloned() else {
        return;
    };
    match found.catalog {
        Some(model) => {
            let language = transcription_models::definition(model)
                .language_for(&ui.settings.transcription.language)
                .to_owned();
            ui.onboarding_choice = Some(TranscriptionSelection { model, language });
            super::onboarding::input(ui, Input::Grant(SetupItem::Model));
        }
        None => super::found::adopt(ui, found, true),
    }
}
