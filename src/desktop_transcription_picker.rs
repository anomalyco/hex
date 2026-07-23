use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, relative, rgb, rgba,
};

use crate::desktop_ui::{
    CANVAS, FAINT, LINE, MUTED, SURFACE, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT,
    error_message,
};
use crate::transcription_models::TranscriptionModelId;

#[derive(Clone)]
pub(crate) struct TranscriptionPickerModel {
    pub(crate) action: String,
    pub(crate) active: bool,
    pub(crate) activation_progress: f32,
    pub(crate) downloading: bool,
    pub(crate) error_rate: &'static str,
    pub(crate) id: TranscriptionModelId,
    pub(crate) metadata: String,
    pub(crate) name: &'static str,
    pub(crate) progress: f32,
    pub(crate) realtime: &'static str,
    pub(crate) realtime_context: &'static str,
    pub(crate) show_download_progress: bool,
    pub(crate) show_loading_progress: bool,
    pub(crate) size: String,
    pub(crate) state_label: String,
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
        div()
            .id(("transcription-model", index))
            .w_full()
            .p_4()
            .mb_3()
            .rounded_sm()
            .border_1()
            .border_color(if model.active {
                rgb(TEXT_SOFT)
            } else {
                rgb(LINE)
            })
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
                            .child(model.name),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(SURFACE_SELECTED))
                            .text_size(px(10.0))
                            .text_color(rgb(TEXT_SOFT))
                            .child(model.state_label),
                    ),
            )
            .child(
                div()
                    .pt_3()
                    .flex()
                    .gap_2()
                    .child(model_metric(
                        model.realtime_context.to_uppercase(),
                        model.realtime,
                    ))
                    .child(model_metric("SIZE", model.size))
                    .child(model_metric(
                        "ERROR RATE · LOWER IS BETTER",
                        model.error_rate,
                    )),
            )
            .when(model.show_download_progress, |card| {
                card.child(
                    div().pt_3().child(
                        div()
                            .h(px(3.0))
                            .w_full()
                            .rounded_sm()
                            .bg(rgb(CANVAS))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(model.progress))
                                    .rounded_sm()
                                    .bg(rgb(TEXT_SOFT)),
                            ),
                    ),
                )
            })
            .when(model.show_loading_progress, |card| {
                card.child(
                    div().pt_3().child(
                        div()
                            .h(px(3.0))
                            .w_full()
                            .rounded_sm()
                            .bg(rgb(CANVAS))
                            .child(
                                div()
                                    .ml(relative(model.activation_progress * 0.75))
                                    .h_full()
                                    .w(relative(0.25))
                                    .rounded_sm()
                                    .bg(rgb(TEXT_SOFT)),
                            ),
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
                    .child(model.metadata)
                    .child(
                        div()
                            .when(model.downloading, |status| {
                                status
                                    .text_color(rgb(TEXT_SOFT))
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(model.action),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if model.downloading {
                    this.cancel_transcription_preparation();
                } else if !model.active {
                    this.choose_transcription_model(model.id, language.clone(), cx);
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
