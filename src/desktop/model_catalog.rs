//! The shared speech-model browser: the full runtime catalog with install
//! state, a single-select language filter, download/verify/load progress,
//! and uninstall. Both port shells (Windows and Linux) render this dialog;
//! installing keeps the current dictation language when the model supports
//! it, otherwise falls back through automatic detection.

use gpui::{
    AnyElement, Bounds, Context, FontWeight, Pixels, canvas, div, prelude::*, px, relative, rgb,
    rgba,
};

use crate::desktop_host::DesktopTranscriptionSnapshot;
use crate::desktop_transcription_picker::{
    TranscriptionPickerDelegate, TranscriptionPickerProgress, TranscriptionPickerStatus,
};
use crate::desktop_ui::{
    CANVAS, DIALOG_STROKE, DIVIDER, FAINT, LINE, MUTED, OVERLAY_PANEL, OVERLAY_SMOKE, PANEL_RADIUS,
    SURFACE, SURFACE_HOVER, TEXT, TEXT_ON_ACCENT, accent_color, disclosure_button,
    dropdown_backdrop, dropdown_item, dropdown_panel_with_width, error_message, header_button, tr,
    tr_fill,
};

/// The catalog dialog's per-shell state and chrome. The picker delegate
/// supplies choose/cancel/dismiss; this adds the language filter and the
/// platform-specific dialog details.
pub(crate) trait ModelCatalogDelegate: TranscriptionPickerDelegate {
    fn catalog_language_filter(&self) -> Option<String>;
    fn set_catalog_language_filter(&mut self, filter: Option<String>);
    fn catalog_filter_dropdown_open(&self) -> bool;
    fn set_catalog_filter_dropdown_open(&mut self, open: bool);
    fn catalog_filter_dropdown_bounds(&self) -> Option<Bounds<Pixels>>;
    fn set_catalog_filter_dropdown_bounds(&mut self, bounds: Bounds<Pixels>);
    fn report_transcription_error(&mut self, error: String);

    /// Vertical space reserved above the dialog backdrop (a custom caption
    /// bar on Windows; nothing on system-titled windows).
    fn dialog_top_inset() -> Pixels {
        px(0.0)
    }

    /// The content of the per-row uninstall button.
    fn uninstall_control() -> AnyElement {
        div()
            .text_size(px(12.0))
            .child(tr("Remove"))
            .into_any_element()
    }

    /// The content of the dialog's close button.
    fn close_control() -> AnyElement {
        div()
            .text_size(px(12.0))
            .child(tr("Close"))
            .into_any_element()
    }
}

/// The browser's initial language filter: the dictation language, or no
/// filter when dictation detects the language automatically.
pub(crate) fn catalog_filter_for_language(language: &str) -> Option<String> {
    (language != crate::transcription_models::AUTO_LANGUAGE).then(|| language.to_string())
}

fn filtered_model_catalog(
    language_filter: Option<&str>,
) -> Vec<&'static crate::transcription_models::ModelDefinition> {
    crate::transcription_models::available_models()
        .into_iter()
        .filter(|model| language_filter.is_none_or(|language| model.supports_language(language)))
        .collect()
}

fn catalog_filter_label(filter: Option<&str>) -> String {
    match filter {
        None => tr("All languages").to_string(),
        Some(language) => crate::transcription_models::language_name(language).to_string(),
    }
}

pub(crate) fn render_model_catalog_filter_dropdown<D: ModelCatalogDelegate>(
    view: &D,
    viewport_height: Pixels,
    cx: &mut Context<D>,
) -> AnyElement {
    let Some(bounds) = view.catalog_filter_dropdown_bounds() else {
        return div().into_any_element();
    };
    let selected_language = view.catalog_language_filter();
    let mut languages: Vec<(&str, &str)> = crate::transcription_models::LANGUAGES
        .iter()
        .filter(|(code, _)| *code != crate::transcription_models::AUTO_LANGUAGE)
        .copied()
        .collect();
    languages.sort_by_key(|(_, name)| *name);
    let choices = std::iter::once((None, tr("All languages").to_string())).chain(
        languages
            .into_iter()
            .map(|(code, name)| (Some(code.to_string()), name.to_string())),
    );
    let items = choices.enumerate().map(|(index, (code, name))| {
        let selected = code == selected_language;
        dropdown_item(("model-catalog-filter-option", index), name, selected).on_click(cx.listener(
            move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_catalog_language_filter(code.clone());
                this.set_catalog_filter_dropdown_open(false);
                cx.notify();
            },
        ))
    });
    let panel_rows = crate::transcription_models::LANGUAGES.len();
    dropdown_backdrop("model-catalog-filter-backdrop")
        .on_click(cx.listener(|this, _, _, cx| {
            this.set_catalog_filter_dropdown_open(false);
            cx.notify();
        }))
        .child(
            dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(230.0))
                .id("model-catalog-filter-dropdown")
                .overflow_y_scroll()
                .on_click(|_, _, cx| cx.stop_propagation())
                .children(items),
        )
        .into_any_element()
}

pub(crate) fn render_model_catalog<D: ModelCatalogDelegate>(
    view: &D,
    transcription: &DesktopTranscriptionSnapshot,
    viewport_width: Pixels,
    viewport_height: Pixels,
    cx: &mut Context<D>,
) -> AnyElement {
    let selection = transcription.selection.clone();
    let recommendation_language = view.catalog_language_filter();
    let catalog = filtered_model_catalog(recommendation_language.as_deref());
    let filter_label = catalog_filter_label(view.catalog_language_filter().as_deref());
    let dialog_width = px(640.0).min(viewport_width - px(40.0));
    let dialog_height = px(640.0).min(viewport_height - D::dialog_top_inset() - px(32.0));
    let mut entries: Vec<(usize, &'static crate::transcription_models::ModelDefinition)> =
        catalog.into_iter().enumerate().collect();
    let active_position = entries.iter().position(|(_, definition)| {
        definition.id == selection.model
            && crate::transcription_models::language_for_model(definition.id, &selection.language)
                .as_deref()
                .is_some_and(|language| {
                    crate::transcription_models::is_installed(definition, language)
                        && crate::transcription_models::is_verified(definition)
                })
    });
    let active_entry = active_position.map(|position| entries.remove(position));
    // One flat list with the active model pinned on top.
    let mut sections: Vec<AnyElement> = Vec::new();
    {
        let build_row = |index: usize,
                         definition: &'static crate::transcription_models::ModelDefinition|
         -> AnyElement {
            let install_language =
                crate::transcription_models::language_for_model(definition.id, &selection.language);
            let installed = install_language.as_deref().is_some_and(|language| {
                crate::transcription_models::is_installed(definition, language)
                    && crate::transcription_models::is_verified(definition)
            });
            let downloading = transcription.preparing == Some(definition.id);
            let another_preparing = transcription.preparing.is_some() && !downloading;
            let active = selection.model == definition.id && installed;
            let status = if downloading {
                let progress = definition.download_bytes().map_or(0.0, |bytes| {
                    (transcription.downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
                });
                let stage = transcription
                    .preparation_stage
                    .unwrap_or(crate::transcription_models::ModelPreparationStage::Downloading);
                let (label, progress) = match stage {
                    crate::transcription_models::ModelPreparationStage::Downloading => (
                        format!("{} {:.0}%", tr("Downloading"), progress * 100.0),
                        Some(TranscriptionPickerProgress::Downloading(progress)),
                    ),
                    crate::transcription_models::ModelPreparationStage::Verifying => {
                        (tr("Verifying model").into(), None)
                    }
                    crate::transcription_models::ModelPreparationStage::Loading => (
                        tr("Loading model").into(),
                        Some(TranscriptionPickerProgress::Loading(0.25)),
                    ),
                };
                TranscriptionPickerStatus::Preparing { label, progress }
            } else if active {
                TranscriptionPickerStatus::Active
            } else {
                TranscriptionPickerStatus::Available { installed }
            };
            let recommendation = recommendation_language.as_deref().and_then(|language| {
                crate::transcription_models::choices_for_runtime(language)
                    .iter()
                    .find(|recommended| recommended.model.id == definition.id)
                    .map(|recommended| tr(recommended.recommendation.label()).to_string())
            });
            let detail = format!("{} · {}", definition.coverage, definition.size_label());
            let compatibility_note = install_language.as_deref().and_then(|language| {
                (language != selection.language).then(|| {
                    tr_fill(
                        "Switches dictation to {}",
                        crate::transcription_models::language_name(language),
                    )
                })
            });
            let (control, progress_bar): (AnyElement, Option<AnyElement>) = match status {
                TranscriptionPickerStatus::Active => (
                    // Sized like header_button so the pill lines up with the
                    // Use/Download controls on other rows.
                    div()
                        .h(px(32.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .bg(rgb(accent_color()))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_ON_ACCENT))
                        .child(tr("Active"))
                        .into_any_element(),
                    None,
                ),
                TranscriptionPickerStatus::Preparing { label, progress } => (
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(MUTED))
                                .child(label),
                        )
                        .child(
                            header_button(tr("Cancel"))
                                .id(("model-catalog-cancel", index))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.cancel_transcription_preparation();
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                    progress.map(|progress| {
                        let indicator = match progress {
                            TranscriptionPickerProgress::Downloading(progress) => div()
                                .h_full()
                                .w(relative(progress))
                                .rounded_full()
                                .bg(rgb(accent_color())),
                            TranscriptionPickerProgress::Loading(progress) => div()
                                .ml(relative(progress * 0.75))
                                .h_full()
                                .w(relative(0.25))
                                .rounded_full()
                                .bg(rgb(accent_color())),
                        };
                        div()
                            .mt_2()
                            .h(px(3.0))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(CANVAS))
                            .child(indicator)
                            .into_any_element()
                    }),
                ),
                TranscriptionPickerStatus::Available { installed } => {
                    let button = header_button(if another_preparing {
                        tr("Preparing model")
                    } else if installed {
                        tr("Use")
                    } else {
                        tr("Download")
                    })
                    .id(("model-catalog-choose", index));
                    let button = if another_preparing {
                        button
                    } else {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(language) = install_language.clone() {
                                this.choose_transcription_model(definition.id, language, cx);
                            }
                            cx.notify();
                        }))
                    };
                    let uninstall = installed.then(|| {
                        header_button(D::uninstall_control())
                            .id(("model-catalog-uninstall", index))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if let Err(error) =
                                    crate::transcription_models::uninstall(definition)
                                {
                                    this.report_transcription_error(format!("{error:#}"));
                                }
                                cx.notify();
                            }))
                    });
                    (
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .children(uninstall)
                            .child(button)
                            .into_any_element(),
                        None,
                    )
                }
            };
            div()
                .w_full()
                .px_4()
                .py_3()
                .flex_none()
                .flex()
                .flex_col()
                .rounded(px(PANEL_RADIUS))
                .border_1()
                .border_color(if active {
                    rgb(accent_color())
                } else {
                    rgb(LINE)
                })
                .bg(rgb(SURFACE))
                .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .min_w(px(0.0))
                                .flex_1()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(TEXT))
                                                .child(definition.name),
                                        )
                                        .when_some(recommendation, |name, recommendation| {
                                            name.child(
                                                div()
                                                    .flex_none()
                                                    .text_size(px(10.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(accent_color()))
                                                    .child(recommendation),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .pt_1()
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(rgb(MUTED))
                                        .child(detail),
                                )
                                .when_some(compatibility_note, |column, note| {
                                    column.child(
                                        div()
                                            .pt_1()
                                            .truncate()
                                            .text_size(px(10.0))
                                            .text_color(rgb(FAINT))
                                            .child(note),
                                    )
                                }),
                        )
                        .child(div().flex_none().child(control)),
                )
                .children(progress_bar)
                .into_any_element()
        };
        sections.extend(active_entry.map(|(index, definition)| build_row(index, definition)));
        sections.extend(
            entries
                .iter()
                .map(|&(index, definition)| build_row(index, definition)),
        );
    }

    div()
        .id("model-catalog-backdrop")
        .absolute()
        .top(D::dialog_top_inset())
        .left_0()
        .right_0()
        .bottom_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(OVERLAY_SMOKE))
        .on_click(cx.listener(|this, _, _, cx| {
            this.dismiss_transcription_picker(cx);
        }))
        .child(
            div()
                .id("model-catalog")
                .w(dialog_width)
                .h(dialog_height)
                .flex()
                .flex_col()
                .rounded(px(8.0))
                .border_1()
                .border_color(rgb(DIALOG_STROKE))
                .bg(rgb(OVERLAY_PANEL))
                .shadow_2xl()
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px_5()
                        .py_4()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .border_b_1()
                        .border_color(rgb(DIVIDER))
                        .child(
                            div()
                                .min_w(px(0.0))
                                .flex_1()
                                .truncate()
                                .text_size(px(16.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(tr("Speech models")),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .relative()
                                        .child(
                                            disclosure_button(filter_label)
                                                .id("model-catalog-filter")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let open = this.catalog_filter_dropdown_open();
                                                    this.set_catalog_filter_dropdown_open(!open);
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            canvas(
                                                {
                                                    let entity = cx.entity();
                                                    move |bounds, _, cx| {
                                                        entity.update(cx, |this, _| {
                                                            this.set_catalog_filter_dropdown_bounds(
                                                                bounds,
                                                            );
                                                        });
                                                    }
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .w_full()
                                            .h(px(0.0)),
                                        ),
                                )
                                .child(
                                    header_button(D::close_control())
                                        .id("model-catalog-close")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.dismiss_transcription_picker(cx);
                                        })),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("model-catalog-list")
                        .flex_1()
                        .min_h(px(0.0))
                        .p_5()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .overflow_y_scroll()
                        .children(sections)
                        .when_some(transcription.error.clone(), |list, error| {
                            list.child(
                                div()
                                    .w_full()
                                    .child(error_message("Model could not be installed.", error)),
                            )
                        }),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::TranscriptionModelId;

    #[test]
    fn catalog_filter_keeps_only_models_supporting_the_selected_language() {
        assert_eq!(
            filtered_model_catalog(None).len(),
            crate::transcription_models::available_models().len()
        );

        let filtered = filtered_model_catalog(Some("pl"));

        assert!(filtered.len() < crate::transcription_models::available_models().len());
        assert!(filtered.iter().all(|model| model.supports_language("pl")));
        assert!(
            filtered
                .iter()
                .all(|model| model.id != TranscriptionModelId::ParakeetUnifiedEnglish)
        );
    }
}
