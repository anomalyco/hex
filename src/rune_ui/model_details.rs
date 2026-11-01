//! A catalog model's details in the Local transcription picker, for models
//! whose licence or provenance needs more than the card (Parakeet Ultra):
//! its languages, measured peak memory, the credit word for word with
//! source and licence links, the publisher's notice, and a folded table of
//! the publisher's own benchmarks, labelled as not measured in Hex.

use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::Ui;
use super::controls::Paint;
use crate::desktop::i18n::{tr, tr_named};
use crate::transcription::model_details::{ModelDetails, details};
use crate::transcription::models::{ModelDefinition, TranscriptionModelId, language_name};

/// Which card's details are open, and whether its benchmark table is.
#[derive(Default)]
pub(super) struct DetailsState {
    open: Option<TranscriptionModelId>,
    published: bool,
}

fn link(p: Paint, key: &str, label: &str, url: &'static str) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |ui: &mut Ui| {
            if ui.live()
                && let Err(error) = crate::desktop::opencode_catalog::open_url(url)
            {
                ui.error = Some(format!("{error:#}"));
            }
        })
        .build(p.k)
        .key(key)
}

/// The card's Details / Hide details button, for a model with details.
pub(super) fn toggle(ui: &Ui, p: Paint, model: &ModelDefinition) -> Option<El> {
    details(model.id)?;
    let id = model.id;
    let open = ui.picker.details.open == Some(id);
    Some(super::controls::button(
        p,
        "model-details",
        if open {
            tr("Hide details")
        } else {
            tr("Details")
        },
        super::controls::ButtonKind::Secondary,
        move |ui: &mut Ui, _| {
            let state = &mut ui.picker.details;
            state.published = false;
            state.open = (state.open != Some(id)).then_some(id);
        },
    ))
}

fn line(p: Paint, key: &str, copy: String) -> El {
    text(copy)
        .key(key)
        .font(p.k.ty.small)
        .color(p.muted())
        .wrap()
}

/// The open details under a card.
pub(super) fn section(ui: &Ui, p: Paint, model: &ModelDefinition) -> Option<El> {
    let details = details(model.id).filter(|_| ui.picker.details.open == Some(model.id))?;
    let languages = model
        .languages
        .iter()
        .map(|code| language_name(code))
        .collect::<Vec<_>>()
        .join(", ");
    let memory = match details.peak_memory {
        Some((memory, hardware)) => tr_named(
            "Peak memory: {memory} on {hardware}; not measured on other hardware",
            &[("memory", memory), ("hardware", hardware)],
        ),
        None => tr("Peak memory: not measured").to_owned(),
    };
    let mut section = col()
        .key("details")
        .gap(6.0)
        .pt(4.0)
        .child(line(
            p,
            "languages",
            tr_named("Languages: {languages}", &[("languages", &languages)]),
        ))
        .child(line(p, "memory", memory))
        .child(line(
            p,
            "experimental",
            tr("Experimental: Hex has run it only on Linux (Vulkan) so far.").to_owned(),
        ))
        .child(
            text(details.attribution)
                .key("attribution")
                .font(p.k.ty.small)
                .color(p.text())
                .wrap(),
        )
        .child(line(p, "notice", details.notice.to_owned()))
        .child(
            row()
                .key("links")
                .gap(4.0)
                .child(link(p, "source-link", tr("Source"), details.source_url))
                .child(link(p, "license-link", tr("License"), details.license_url)),
        );
    section = section.child(published(ui, p, details));
    Some(section)
}

/// The publisher's benchmarks, folded until asked for, with their
/// conditions beside them. They rank nothing in Hex.
fn published(ui: &Ui, p: Paint, details: &ModelDetails) -> El {
    let open = ui.picker.details.published;
    let label = tr("Published conversion benchmarks; not measured in Hex");
    let disclosure = Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .icon(if open {
            p.k.glyph.chevron_up
        } else {
            p.k.glyph.chevron_down
        })
        .expanded(open)
        .on_press(|ui: &mut Ui| ui.picker.details.published = !ui.picker.details.published)
        .build(p.k)
        .key("published-toggle");
    let mut column = col().key("published").gap(4.0).child(disclosure);
    if open {
        column = column.child(line(
            p,
            "conditions",
            details.published_conditions.to_owned(),
        ));
        for (code, rate) in details.published {
            column = column.child(
                row()
                    .key(format!("published-{code}"))
                    .gap(12.0)
                    .child(
                        text(language_name(code))
                            .key("language")
                            .font(p.k.ty.small)
                            .color(p.muted())
                            .w(140.0),
                    )
                    .child(
                        text(format!("{rate:.2}% WER"))
                            .key("rate")
                            .font(p.k.ty.small_medium)
                            .color(p.text()),
                    ),
            );
        }
    }
    column
}
