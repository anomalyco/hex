use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, relative, rgb, rgba,
};

use crate::desktop_ui::{
    ACCENT, CANVAS, FAINT, LINE, MUTED, PANEL_RADIUS, SIDEBAR, SURFACE, SURFACE_HOVER,
    SURFACE_SELECTED, TEXT, TEXT_SOFT, error_message,
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
                .rounded(px(6.0))
                .text_size(px(13.0))
                .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
                .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                .when(!selected, |row| {
                    row.hover(|row| row.bg(rgb(0x2a2a2a)).text_color(rgb(TEXT_SOFT)))
                })
                .child(*name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_transcription_language(code.clone(), cx);
                }))
        });
    let model_cards = view.models.into_iter().enumerate().map(|(index, model)| {
        let language = view.language.clone();
        let definition = model.choice.model;
        let (state_label, action, active, preparing, progress, installed) = match model.status {
            TranscriptionPickerStatus::Active => ("Active".into(), None, true, false, None, true),
            TranscriptionPickerStatus::Available { installed } => (
                model.choice.recommendation.label().into(),
                Some(if installed { "Use" } else { "Download" }),
                false,
                false,
                None,
                installed,
            ),
            TranscriptionPickerStatus::Preparing { label, progress } => {
                (label, Some("Cancel"), false, true, progress, false)
            }
        };
        let mut metadata = if definition.coverage == language_name(&language) {
            format!("{} · {}", definition.quality_context, definition.timestamps)
        } else {
            format!(
                "{} · {} · {}",
                definition.coverage, definition.quality_context, definition.timestamps
            )
        };
        if installed && !active {
            metadata.push_str(" · Installed");
        }
        let badge = div()
            .h(px(22.0))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(6.0))
            .text_size(px(10.0))
            .map(|badge| {
                if active {
                    badge
                        .bg(rgb(ACCENT))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                } else if preparing {
                    badge.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT_SOFT))
                } else {
                    badge
                        .border_1()
                        .border_color(rgb(LINE))
                        .text_color(rgb(MUTED))
                }
            })
            .child(state_label);
        div()
            .id(("transcription-model", index))
            .w_full()
            .p_4()
            .mb_3()
            .rounded(px(PANEL_RADIUS))
            .border_1()
            .border_color(if active { rgb(TEXT_SOFT) } else { rgb(LINE) })
            .bg(rgb(SURFACE))
            .when(!active, |card| {
                card.cursor_pointer()
                    .hover(|card| card.bg(rgb(SURFACE_HOVER)))
            })
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
                    .child(badge),
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
            .when(definition.id == TranscriptionModelId::ParakeetV3, |card| {
                card.child(
                    div()
                        .pt_3()
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED))
                        .child(
                            "Detects language from audio, not the selected language. \
                             For language-guided transcription, try Whisper.",
                        ),
                )
            })
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
                    .min_h(px(26.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(FAINT))
                            .child(metadata),
                    )
                    .when_some(action, |footer, action| {
                        footer.child(
                            div()
                                .h(px(26.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(rgb(LINE))
                                .bg(rgb(CANVAS))
                                .text_size(px(11.0))
                                .text_color(rgb(TEXT_SOFT))
                                .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT)))
                                .child(action),
                        )
                    }),
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
        .occlude()
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
                .rounded(px(PANEL_RADIUS + 2.0))
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CANVAS))
                .overflow_hidden()
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
