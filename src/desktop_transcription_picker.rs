use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, relative, rgb, rgba,
};

use crate::desktop_ui::{
    CANVAS, FAINT, LINE, MUTED, SURFACE, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT,
    error_message,
};
use crate::transcription_models::{
    ModelChoice, ModelDefinition, ModelRuntime, TranscriptionModelId, TranscriptionSelection,
    language_name,
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
    },
    Preparing {
        label: String,
        progress: Option<TranscriptionPickerProgress>,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum TranscriptionPickerProgress {
    Downloading(f32),
    Loading(f32),
}

#[derive(Clone)]
pub(crate) struct TranscriptionPickerView {
    pub(crate) error: Option<String>,
    pub(crate) language: String,
    pub(crate) models: Vec<TranscriptionPickerModel>,
}

pub(crate) trait TranscriptionPickerDelegate: Sized + 'static {
    fn cancel_transcription_preparation(&mut self);
    fn choose_transcription_model(
        &mut self,
        model: TranscriptionModelId,
        language: String,
        cx: &mut Context<Self>,
    );
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
            div()
                .id(("transcription-language", index))
                .h(px(34.0))
                .px_3()
                .flex()
                .items_center()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
                .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                .hover(|row| row.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                .child(*name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_transcription_language(code.clone(), cx);
                }))
        });
    let model_cards = view.models.into_iter().enumerate().map(|(index, model)| {
        let language = view.language.clone();
        let definition = model.choice.model;
        let (state_label, action, active, preparing, progress) = match model.status {
            TranscriptionPickerStatus::Active => ("Active".into(), "", true, false, None),
            TranscriptionPickerStatus::Available { installed } => (
                model.choice.recommendation.label().into(),
                if installed {
                    "Installed"
                } else if matches!(definition.runtime, ModelRuntime::AppleSpeech) {
                    "Use"
                } else {
                    "Download"
                },
                false,
                false,
                None,
            ),
            TranscriptionPickerStatus::Preparing { label, progress } => {
                (label, "Cancel", false, true, progress)
            }
        };
        let metadata = if definition.coverage == language_name(&language) {
            format!("{} · {}", definition.quality_context, definition.timestamps)
        } else {
            format!(
                "{} · {} · {}",
                definition.coverage, definition.quality_context, definition.timestamps
            )
        };
        div()
            .id(("transcription-model", index))
            .w_full()
            .p_4()
            .mb_3()
            .rounded_sm()
            .border_1()
            .border_color(if active { rgb(TEXT_SOFT) } else { rgb(LINE) })
            .bg(rgb(SURFACE))
            .hover(|card| card.bg(rgb(SURFACE_HOVER)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(definition.name),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(SURFACE_SELECTED))
                            .text_size(px(10.0))
                            .text_color(rgb(TEXT_SOFT))
                            .child(state_label),
                    ),
            )
            .child(
                div()
                    .pt_3()
                    .flex()
                    .gap_2()
                    .child(model_metric(
                        definition.realtime_context.to_uppercase(),
                        definition.realtime,
                    ))
                    .child(model_metric("SIZE", definition.size_label()))
                    .child(model_metric(
                        "ERROR RATE · LOWER IS BETTER",
                        definition.quality,
                    )),
            )
            .when_some(progress, |card, progress| {
                let indicator = match progress {
                    TranscriptionPickerProgress::Downloading(progress) => div()
                        .h_full()
                        .w(relative(progress))
                        .rounded_sm()
                        .bg(rgb(TEXT_SOFT)),
                    TranscriptionPickerProgress::Loading(progress) => div()
                        .ml(relative(progress * 0.75))
                        .h_full()
                        .w(relative(0.25))
                        .rounded_sm()
                        .bg(rgb(TEXT_SOFT)),
                };
                card.child(
                    div().pt_3().child(
                        div()
                            .h(px(3.0))
                            .w_full()
                            .rounded_sm()
                            .bg(rgb(CANVAS))
                            .child(indicator),
                    ),
                )
            })
            .child(
                div()
                    .pt_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(10.0))
                    .text_color(rgb(FAINT))
                    .child(metadata)
                    .child(
                        div()
                            .when(preparing, |status| {
                                status
                                    .text_color(rgb(TEXT_SOFT))
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(action),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if preparing {
                    this.cancel_transcription_preparation();
                } else if !active {
                    this.choose_transcription_model(definition.id, language.clone(), cx);
                }
                cx.notify();
            }))
    });

    div()
        .id("transcription-picker-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(0x000000bb))
        .on_click(cx.listener(|this, _, _, cx| {
            this.dismiss_transcription_picker(cx);
        }))
        .child(
            div()
                .id("transcription-picker")
                .w(px(780.0))
                .h(px(520.0))
                .flex()
                .flex_col()
                .rounded_lg()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CANVAS))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px_6()
                        .py_5()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Choose local transcription"),
                        )
                        .child(
                            div()
                                .pt_1()
                                .text_size(px(12.0))
                                .text_color(rgb(MUTED))
                                .child(
                                    "Select what you speak; HEX recommends the best supported models.",
                                ),
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
                                .w(px(220.0))
                                .h_full()
                                .p_4()
                                .overflow_y_scroll()
                                .border_r_1()
                                .border_color(rgb(LINE))
                                .children(language_rows),
                        )
                        .child(
                            div()
                                .id("transcription-model-list")
                                .flex_1()
                                .h_full()
                                .p_5()
                                .overflow_y_scroll()
                                .children(model_cards)
                                .when_some(view.error, |list, error| {
                                    list.child(error_message(
                                        "Model could not be installed.",
                                        error,
                                    ))
                                }),
                        ),
                ),
        )
        .into_any_element()
}

fn model_metric(label: impl IntoElement, value: impl IntoElement) -> gpui::Div {
    div()
        .flex_1()
        .h(px(54.0))
        .px_3()
        .py_2()
        .rounded_sm()
        .bg(rgb(CANVAS))
        .child(div().text_size(px(9.0)).text_color(rgb(FAINT)).child(label))
        .child(
            div()
                .pt_1()
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(value),
        )
}
