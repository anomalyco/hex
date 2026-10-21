use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, relative, rgb, rgba,
};

use crate::design_tokens::*;
use crate::desktop_i18n::{tr, tr_fill};
use crate::desktop_ui::{
    ButtonKind, Icon, Tone, error_panel, icon_button, menu_item, progress_track, small_button,
    status_dot, tag,
};
use crate::transcription_models::{
    ModelChoice, ModelDefinition, TranscriptionModelId, TranscriptionSelection, language_name,
};

pub(crate) fn transcription_selection_is_active(
    selection: &TranscriptionSelection,
    model: &ModelDefinition,
    language: &str,
    installed: bool,
) -> bool {
    installed && selection.model == model.id && selection.language == language
}

#[derive(Clone)]
pub(crate) struct TranscriptionPickerModel {
    pub(crate) choice: ModelChoice,
    pub(crate) status: TranscriptionPickerStatus,
}

#[derive(Clone)]
pub(crate) enum TranscriptionPickerStatus {
    Active,
    Available {
        installed: bool,
        deletion: ModelDeletion,
    },
    Preparing {
        label: String,
        progress: Option<TranscriptionPickerProgress>,
    },
}

/// Whether a downloaded model card offers deleting its files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModelDeletion {
    Unavailable,
    Offered,
    Confirming,
}

#[derive(Clone, Copy)]
pub(crate) enum TranscriptionPickerProgress {
    Downloading(f32),
    Loading(f32),
}

#[derive(Clone)]
pub(crate) struct TranscriptionPickerView {
    pub(crate) error: Option<String>,
    pub(crate) deletion_error: Option<String>,
    pub(crate) language: String,
    pub(crate) models: Vec<TranscriptionPickerModel>,
    pub(crate) open_progress: f32,
    pub(crate) language_progress: f32,
}

pub(crate) trait TranscriptionPickerDelegate: Sized + 'static {
    fn cancel_transcription_preparation(&mut self, model: TranscriptionModelId);
    fn choose_transcription_model(
        &mut self,
        model: TranscriptionModelId,
        language: String,
        cx: &mut Context<Self>,
    );
    /// The first call asks for confirmation; a repeated call deletes the model.
    /// Roots that never offer deletion keep this default.
    fn delete_transcription_model(
        &mut self,
        _model: TranscriptionModelId,
        _cx: &mut Context<Self>,
    ) {
    }
    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>);
    fn select_transcription_language(&mut self, language: String, cx: &mut Context<Self>);
}

pub(crate) fn render_transcription_picker<T: TranscriptionPickerDelegate>(
    view: TranscriptionPickerView,
    cx: &mut Context<T>,
) -> AnyElement {
    let selected_language = view.language.clone();
    let language_rows = crate::transcription_models::LANGUAGES
        .iter()
        .enumerate()
        .map(|(index, (code, name))| {
            let selected = *code == selected_language;
            let code = (*code).to_string();
            menu_item(selected)
                .id(("transcription-language", index))
                .text_body()
                .child(*name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_transcription_language(code.clone(), cx);
                }))
        });
    let model_cards = view.models.into_iter().enumerate().map(|(index, model)| {
        let language = view.language.clone();
        let definition = model.choice.model;
        let active = matches!(model.status, TranscriptionPickerStatus::Active);
        let preparing = matches!(model.status, TranscriptionPickerStatus::Preparing { .. });
        let mut deletion = ModelDeletion::Unavailable;
        let mut installed_inactive = false;
        let mut progress_bar = None;
        let (state, action) = match model.status {
            TranscriptionPickerStatus::Active => (Some(model_state(tr("Active"))), None),
            TranscriptionPickerStatus::Available {
                installed,
                deletion: offered,
            } => {
                deletion = offered;
                installed_inactive = installed;
                (
                    None,
                    Some(if installed { tr("Use") } else { tr("Download") }),
                )
            }
            TranscriptionPickerStatus::Preparing { label, progress } => (
                // A progress bar carries its label; the title row keeps only Cancel.
                match progress {
                    Some(progress) => {
                        progress_bar = Some((progress, label));
                        None
                    }
                    None => Some(model_state(label)),
                },
                Some(tr("Cancel")),
            ),
        };
        let metadata = definition
            .streams
            .then(|| tr(STREAMING_DESCRIPTION))
            .into_iter()
            .chain(
                (definition.coverage != language_name(&language)).then(|| tr(definition.coverage)),
            )
            .chain([tr(definition.quality_context), tr(definition.timestamps)])
            .chain(installed_inactive.then(|| tr("Installed")))
            .collect::<Vec<_>>()
            .join(" · ");
        // At most two descriptive tags: why to pick it, and whether it streams.
        // Progress and the active state stay on the trailing edge as text.
        let tags = [
            Some(model.choice.recommendation.label()),
            definition.streams.then_some("Streams"),
        ]
        .into_iter()
        .flatten()
        .map(|label| tag(Tone::Neutral, tr(label)));
        let actions = div()
            .h(px(CONTROL_HEIGHT_SMALL))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .children(state)
            .when_some(
                match deletion {
                    ModelDeletion::Unavailable => None,
                    ModelDeletion::Offered => Some(false),
                    ModelDeletion::Confirming => Some(true),
                },
                |actions, confirming| {
                    actions.child(
                        small_button(
                            if confirming {
                                ButtonKind::Danger
                            } else {
                                ButtonKind::Secondary
                            },
                            if confirming {
                                tr_fill("Delete {}?", &definition.size_label())
                            } else {
                                tr("Delete").into()
                            },
                        )
                        .id(("transcription-model-delete", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.delete_transcription_model(definition.id, cx);
                            cx.notify();
                        })),
                    )
                },
            )
            .when_some(action, |actions, action| {
                actions.child(small_button(ButtonKind::Secondary, action))
            });
        div()
            .id(("transcription-model", index))
            .w_full()
            .px_4()
            // The title row is a small control tall; its extra 3 px above the
            // title line makes up the top padding.
            .pt_2()
            .pb_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(RADIUS_PANEL)
            .border_1()
            .border_color(rgb(if active { ACCENT_BORDER } else { LINE }))
            .bg(rgb(if active { ACCENT_SURFACE } else { SURFACE }))
            .when(!active, |card| {
                card.cursor_pointer()
                    .hover(|card| card.bg(rgb(SURFACE_HOVER)))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(CONTROL_HEIGHT_SMALL))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_body()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(TEXT_PRIMARY))
                                            .child(definition.name),
                                    )
                                    .children(tags),
                            )
                            .child(actions),
                    )
                    .child(
                        div()
                            .text_caption()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(metadata),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_5()
                    .child(model_metric(tr("Speed"), tr(definition.realtime)))
                    .child(model_metric(tr("Size"), definition.size_label()))
                    .child(model_metric("WER", definition.quality)),
            )
            .when(definition.id == TranscriptionModelId::ParakeetV3, |card| {
                card.child(div().text_caption().text_color(rgb(TEXT_TERTIARY)).child(
                    "Detects language from audio, not the selected language. \
                             For language-guided transcription, try Whisper.",
                ))
            })
            .when_some(progress_bar, |card, (progress, label)| {
                let indicator = div();
                let indicator = match progress {
                    TranscriptionPickerProgress::Downloading(progress) => {
                        indicator.w(relative(progress))
                    }
                    TranscriptionPickerProgress::Loading(progress) => {
                        indicator.ml(relative(progress * 0.75)).w(relative(0.25))
                    }
                };
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(progress_track(indicator))
                        .child(
                            div()
                                .flex_none()
                                .text_caption()
                                .text_color(rgb(TEXT_SECONDARY))
                                .child(label),
                        ),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if preparing {
                    this.cancel_transcription_preparation(definition.id);
                } else if !active {
                    this.choose_transcription_model(definition.id, language.clone(), cx);
                }
                cx.notify();
            }))
    });

    let open_progress = view.open_progress.clamp(0.0, 1.0);
    let modal_offset = ((1.0 - open_progress) * PANE_ENTER_OFFSET * 64.0).round() / 64.0;
    let language_progress = view.language_progress.clamp(0.0, 1.0);

    div()
        .id("transcription-picker-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(SCRIM))
        .opacity(open_progress)
        .on_click(cx.listener(|this, _, _, cx| {
            this.dismiss_transcription_picker(cx);
        }))
        .child(
            div()
                .id("transcription-picker")
                .w(px(MODAL_WIDTH))
                .h(px(MODAL_HEIGHT))
                .mt(px(modal_offset * 2.0))
                .flex()
                .flex_col()
                .rounded(RADIUS_MODAL)
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CANVAS))
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .pl_5()
                        .pr_3()
                        .py_3()
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(div().text_headline().child(tr("Local transcription")))
                        .child(
                            icon_button(ButtonKind::Ghost, Icon::Close)
                                .id("close-transcription-picker")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_transcription_picker(cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .overflow_hidden()
                        .child(
                            div()
                                .id("transcription-language-list")
                                .w(px(SIDEBAR_WIDTH))
                                .h_full()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .overflow_y_scroll()
                                .bg(rgb(SIDEBAR))
                                .border_r_1()
                                .border_color(rgb(LINE))
                                .children(language_rows),
                        )
                        .child(
                            div()
                                .id("transcription-model-list")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .overflow_y_scroll()
                                .opacity(language_progress)
                                .when_some(view.error, |list, error| {
                                    list.child(error_panel(
                                        tr("Model could not be installed."),
                                        error,
                                    ))
                                })
                                .when_some(view.deletion_error, |list, error| {
                                    list.child(error_panel(
                                        tr("Model could not be deleted."),
                                        error,
                                    ))
                                })
                                .children(model_cards),
                        ),
                ),
        )
        .into_any_element()
}

/// Streaming needs a GPU device; every supported Mac has Metal.
#[cfg(target_os = "macos")]
const STREAMING_DESCRIPTION: &str = "Transcribes while you speak";
#[cfg(not(target_os = "macos"))]
const STREAMING_DESCRIPTION: &str = "Transcribes while you speak on a GPU";

/// A card's ongoing state on its trailing edge: an accent dot and a label.
fn model_state(label: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(status_dot(ACCENT))
        .child(
            div()
                .text_footnote()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_SECONDARY))
                .child(label),
        )
}

/// One inline statistic: a tertiary label beside its value.
fn model_metric(label: &'static str, value: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_baseline()
        .gap_1p5()
        .child(
            div()
                .text_caption()
                .text_color(rgb(TEXT_TERTIARY))
                .child(label),
        )
        .child(
            div()
                .text_footnote()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_SECONDARY))
                .child(value),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_language_is_a_distinct_model_selection() {
        let model =
            crate::transcription_models::definition(TranscriptionModelId::WhisperLargeV3Turbo);
        let selection = TranscriptionSelection {
            model: model.id,
            language: crate::transcription_models::AUTO_LANGUAGE.into(),
            recognition_hints: String::new(),
        };

        assert!(transcription_selection_is_active(
            &selection,
            model,
            crate::transcription_models::AUTO_LANGUAGE,
            true,
        ));
        assert!(!transcription_selection_is_active(
            &selection, model, "en", true,
        ));
    }
}
