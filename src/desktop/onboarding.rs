//! The shared first-run experience: pick a dictation language, download
//! its recommended model with live progress, and start dictating. Both
//! port shells render this dialog on a fresh install (no persisted
//! settings); finishing or skipping persists settings so it never shows
//! again.

use gpui::{
    AnyElement, Bounds, Context, FontWeight, Pixels, canvas, div, prelude::*, px, relative, rgb,
    rgba,
};

use crate::desktop_host::DesktopTranscriptionSnapshot;
use crate::desktop_ui::{
    CANVAS, DIALOG_STROKE, FAINT, MUTED, OVERLAY_PANEL, OVERLAY_SMOKE, TEXT, TEXT_ON_ACCENT,
    accent_color, disclosure_button, dropdown_backdrop, dropdown_item, dropdown_panel_with_width,
    error_message, header_button, hotkey_keycaps, tr, tr_fill,
};
use crate::transcription_models::TranscriptionSelection;

/// The onboarding dialog's per-shell state: the pending selection (not
/// persisted until a download begins), the language dropdown, and what
/// finishing means on that shell.
pub(crate) trait OnboardingDelegate: Sized + 'static {
    fn onboarding_selection(&self) -> TranscriptionSelection;
    fn set_onboarding_selection(&mut self, selection: TranscriptionSelection);
    fn onboarding_language_dropdown_open(&self) -> bool;
    fn set_onboarding_language_dropdown_open(&mut self, open: bool);
    fn onboarding_language_dropdown_bounds(&self) -> Option<Bounds<Pixels>>;
    fn set_onboarding_language_dropdown_bounds(&mut self, bounds: Bounds<Pixels>);
    fn begin_onboarding_download(&mut self, cx: &mut Context<Self>);
    fn cancel_onboarding_download(&mut self);
    /// Persist settings and close the dialog; with no installed model the
    /// shell keeps its ordinary "Model required" affordances.
    fn finish_onboarding(&mut self, cx: &mut Context<Self>);

    /// Vertical space reserved above the dialog backdrop.
    fn onboarding_top_inset() -> Pixels {
        px(0.0)
    }
}

pub(crate) fn render_onboarding_language_dropdown<D: OnboardingDelegate>(
    view: &D,
    viewport_height: Pixels,
    cx: &mut Context<D>,
) -> AnyElement {
    let Some(bounds) = view.onboarding_language_dropdown_bounds() else {
        return div().into_any_element();
    };
    let selection = view.onboarding_selection();
    let items = crate::transcription_models::LANGUAGES
        .iter()
        .enumerate()
        .map(|(index, (code, name))| {
            let selected = selection.language == *code;
            let label = if *code == crate::transcription_models::AUTO_LANGUAGE {
                tr("Auto").to_string()
            } else {
                (*name).to_string()
            };
            let code = (*code).to_string();
            dropdown_item(("onboarding-language-option", index), label, selected).on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    // Re-picking the current language must not cancel a
                    // download in flight.
                    if this.onboarding_selection().language != code {
                        this.cancel_onboarding_download();
                        this.set_onboarding_selection(
                            crate::transcription_models::recommended_selection(&code),
                        );
                    }
                    this.set_onboarding_language_dropdown_open(false);
                    cx.notify();
                }),
            )
        });
    dropdown_backdrop("onboarding-language-backdrop")
        .on_click(cx.listener(|this, _, _, cx| {
            this.set_onboarding_language_dropdown_open(false);
            cx.notify();
        }))
        .child(
            dropdown_panel_with_width(
                bounds,
                viewport_height,
                crate::transcription_models::LANGUAGES.len(),
                px(260.0),
            )
            .id("onboarding-language-dropdown")
            .overflow_y_scroll()
            .on_click(|_, _, cx| cx.stop_propagation())
            .children(items),
        )
        .into_any_element()
}

pub(crate) fn render_onboarding<D: OnboardingDelegate>(
    view: &D,
    transcription: &DesktopTranscriptionSnapshot,
    shortcut: Vec<String>,
    viewport_width: Pixels,
    _viewport_height: Pixels,
    cx: &mut Context<D>,
) -> AnyElement {
    let selection = view.onboarding_selection();
    let definition = crate::transcription_models::definition(selection.model);
    let installed = crate::transcription_models::is_installed(definition, &selection.language)
        && crate::transcription_models::is_verified(definition);
    // Any in-flight preparation gates the actions, even one for a model the
    // user has since navigated away from; its progress is attributed to the
    // model actually preparing.
    let preparing_model = transcription.preparing;
    let language_label = if selection.language == crate::transcription_models::AUTO_LANGUAGE {
        tr("Auto").to_string()
    } else {
        crate::transcription_models::language_name(&selection.language).to_string()
    };
    let dialog_width = px(460.0).min(viewport_width - px(40.0));

    let progress = preparing_model.map(|preparing| {
        let preparing = crate::transcription_models::definition(preparing);
        let fraction = preparing.download_bytes().map_or(0.0, |bytes| {
            (transcription.downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
        });
        let stage = transcription
            .preparation_stage
            .unwrap_or(crate::transcription_models::ModelPreparationStage::Downloading);
        let label = match stage {
            crate::transcription_models::ModelPreparationStage::Downloading => {
                format!(
                    "{} · {} {:.0}%",
                    preparing.name,
                    tr("Downloading"),
                    fraction * 100.0
                )
            }
            crate::transcription_models::ModelPreparationStage::Verifying => {
                tr("Verifying model").into()
            }
            crate::transcription_models::ModelPreparationStage::Loading => {
                tr("Loading model").into()
            }
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child(label),
                    )
                    .child(
                        header_button(tr("Cancel"))
                            .id("onboarding-cancel-download")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.cancel_onboarding_download();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(3.0))
                    .w_full()
                    .rounded_full()
                    .bg(rgb(CANVAS))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction.max(0.02)))
                            .rounded_full()
                            .bg(rgb(accent_color())),
                    ),
            )
            .into_any_element()
    });

    let action: AnyElement = if preparing_model.is_some() {
        // No action while any preparation runs: starting would persist a
        // selection the completing worker is about to overwrite, and a new
        // download would be refused until the old worker is reaped.
        div().into_any_element()
    } else if installed {
        div()
            .id("onboarding-finish")
            .h(px(36.0))
            .px_5()
            .flex()
            .items_center()
            .rounded(px(4.0))
            .bg(rgb(accent_color()))
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(TEXT_ON_ACCENT))
            .cursor_pointer()
            .child(tr("Start dictating"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.finish_onboarding(cx);
            }))
            .into_any_element()
    } else {
        div()
            .id("onboarding-download")
            .h(px(36.0))
            .px_5()
            .flex()
            .items_center()
            .rounded(px(4.0))
            .bg(rgb(accent_color()))
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(TEXT_ON_ACCENT))
            .cursor_pointer()
            .child(tr_fill("Download {}", definition.name))
            .on_click(cx.listener(|this, _, _, cx| {
                this.begin_onboarding_download(cx);
                cx.notify();
            }))
            .into_any_element()
    };

    div()
        .id("onboarding-backdrop")
        .absolute()
        .top(D::onboarding_top_inset())
        .left_0()
        .right_0()
        .bottom_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(OVERLAY_SMOKE))
        .child(
            div()
                .id("onboarding-dialog")
                .w(dialog_width)
                .flex()
                .flex_col()
                .gap_4()
                .p_6()
                .rounded(px(8.0))
                .border_1()
                .border_color(rgb(DIALOG_STROKE))
                .bg(rgb(OVERLAY_PANEL))
                .shadow_2xl()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(tr("Welcome to HEX")),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .line_height(px(19.0))
                        .text_color(rgb(MUTED))
                        .child(tr(
                            "Dictate into any application. Audio never leaves this computer; one speech model download is all it takes.",
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(rgb(TEXT))
                                        .child(tr("Dictation language")),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(rgb(FAINT))
                                        .child(format!(
                                            "{} · {}",
                                            definition.name,
                                            definition.size_label()
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .child(
                                    disclosure_button(language_label)
                                        .id("onboarding-language")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let open =
                                                this.onboarding_language_dropdown_open();
                                            this.set_onboarding_language_dropdown_open(!open);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    canvas(
                                        {
                                            let entity = cx.entity();
                                            move |bounds, _, cx| {
                                                entity.update(cx, |this, _| {
                                                    this.set_onboarding_language_dropdown_bounds(
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
                        ),
                )
                .children(progress)
                .when_some(transcription.error.clone(), |dialog, error| {
                    dialog.child(error_message("Model could not be installed.", error))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(rgb(FAINT))
                                        .child(tr("Hold")),
                                )
                                .child(hotkey_keycaps(shortcut, 0.9))
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(rgb(FAINT))
                                        .child(tr("to dictate")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id("onboarding-skip")
                                        .text_size(px(12.0))
                                        .text_color(rgb(FAINT))
                                        .cursor_pointer()
                                        .hover(|link| link.text_color(rgb(MUTED)))
                                        .child(tr("Set up later"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.finish_onboarding(cx);
                                        })),
                                )
                                .child(action),
                        ),
                ),
        )
        .into_any_element()
}
