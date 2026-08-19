//! Shared OpenCode processing card for contextual dictation modes.
//!
//! Native roots own model catalogs, field controls, persistence, and runtime
//! projection. This module owns the common enablement contract, expanded-card
//! composition, and unavailable/retry presentation.

use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px, rgb};

use crate::desktop_mode_list::ModeTarget;
use crate::desktop_ui::{
    FAINT, LINE, NEGATIVE, SURFACE, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT,
    compact_panel, compact_panel_header, disclosure_button, header_button, toggle, tr,
};

pub(crate) struct ModeProcessingUnavailableView {
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) error: Option<String>,
    pub(crate) retry_label: Option<&'static str>,
    pub(crate) setup_label: Option<&'static str>,
}

pub(crate) struct ModeProcessingView {
    pub(crate) target: ModeTarget,
    pub(crate) enabled: bool,
    pub(crate) toggle_position: f32,
    pub(crate) can_toggle: bool,
    pub(crate) settings: Option<AnyElement>,
    pub(crate) unavailable: Option<ModeProcessingUnavailableView>,
}

pub(crate) struct ModeVariantPickerView {
    pub(crate) target: ModeTarget,
    pub(crate) variants: Vec<String>,
    pub(crate) selected: Option<String>,
    pub(crate) open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModeProcessingAction {
    SetEnabled {
        target: ModeTarget,
        enabled: bool,
    },
    ToggleVariantPicker {
        target: ModeTarget,
    },
    SetVariant {
        target: ModeTarget,
        variant: Option<String>,
    },
    RetryOpenCode,
    OpenOpenCodeSetup,
}

pub(crate) trait ModeProcessingDelegate: Sized + 'static {
    fn handle_mode_processing_action(
        &mut self,
        action: ModeProcessingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

pub(crate) fn render_mode_variant_picker<T: ModeProcessingDelegate>(
    view: ModeVariantPickerView,
    cx: &mut Context<T>,
) -> AnyElement {
    let target = view.target;
    let selected_label = view.selected.as_deref().unwrap_or("Default").to_string();
    let choices = std::iter::once(None)
        .chain(view.variants.into_iter().map(Some))
        .collect::<Vec<_>>();
    let list = view.open.then(|| {
        div()
            .mt_1()
            .p_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .children(choices.into_iter().enumerate().map(|(index, variant)| {
                let selected = view.selected == variant;
                let label = variant.clone().unwrap_or_else(|| tr("Default").to_string());
                div()
                    .id(("mode-model-variant", index))
                    .w_full()
                    .h(px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.0))
                    .text_color(rgb(if selected { TEXT } else { TEXT_SOFT }))
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.handle_mode_processing_action(
                            ModeProcessingAction::SetVariant {
                                target,
                                variant: variant.clone(),
                            },
                            window,
                            cx,
                        );
                    }))
            }))
    });
    div()
        .w(px(160.0))
        .flex_none()
        .child(
            disclosure_button(selected_label)
                .id(SharedString::from(format!(
                    "mode-model-variant-trigger-{}",
                    target.id_fragment()
                )))
                .w(px(160.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.handle_mode_processing_action(
                        ModeProcessingAction::ToggleVariantPicker { target },
                        window,
                        cx,
                    );
                })),
        )
        .when_some(list, |control, list| control.child(list))
        .into_any_element()
}

pub(crate) fn render_mode_processing<T: ModeProcessingDelegate>(
    view: ModeProcessingView,
    cx: &mut Context<T>,
) -> AnyElement {
    let target_id = view.target.id_fragment();
    let target = view.target;
    let enabled = view.enabled;
    let toggle_control = div()
        .id(SharedString::from(format!(
            "mode-processing-toggle-{target_id}"
        )))
        .h(px(28.0))
        .flex()
        .items_center()
        .child(toggle(view.toggle_position))
        .when(!view.can_toggle, |control| control.opacity(0.42))
        .when(view.can_toggle, |control| {
            control.on_click(cx.listener(move |this, _, window, cx| {
                this.handle_mode_processing_action(
                    ModeProcessingAction::SetEnabled {
                        target,
                        enabled: !enabled,
                    },
                    window,
                    cx,
                );
            }))
        })
        .into_any_element();
    let unavailable = view
        .unavailable
        .map(|unavailable| render_unavailable(unavailable, cx));

    compact_panel()
        .child(
            compact_panel_header(tr("OpenCode transformation"), Some(toggle_control))
                .when(!view.enabled && unavailable.is_none(), |header| {
                    header.border_b_0()
                }),
        )
        .when_some(view.settings, |panel, settings| {
            panel.child(div().px_3().pb_3().child(settings))
        })
        .when_some(unavailable, |panel, unavailable| panel.child(unavailable))
        .into_any_element()
}

fn render_unavailable<T: ModeProcessingDelegate>(
    view: ModeProcessingUnavailableView,
    cx: &mut Context<T>,
) -> AnyElement {
    let retry = view.retry_label.map(|label| {
        header_button(tr(label))
            .id("retry-mode-opencode")
            .on_click(cx.listener(|this, _, window, cx| {
                this.handle_mode_processing_action(ModeProcessingAction::RetryOpenCode, window, cx);
            }))
    });
    let setup = view.setup_label.map(|label| {
        header_button(tr(label))
            .id("open-mode-opencode-setup")
            .on_click(cx.listener(|this, _, window, cx| {
                this.handle_mode_processing_action(
                    ModeProcessingAction::OpenOpenCodeSetup,
                    window,
                    cx,
                );
            }))
    });
    let show_actions = retry.is_some() || setup.is_some();
    let is_error = view.error.is_some();
    let message = view
        .error
        .unwrap_or_else(|| tr(view.description).to_string());

    div()
        .px_3()
        .py_3()
        .flex()
        .items_start()
        .justify_between()
        .gap_4()
        .border_t_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_SOFT))
                        .child(tr(view.title)),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .line_height(px(16.0))
                        .text_color(rgb(if is_error { NEGATIVE } else { FAINT }))
                        .child(message),
                ),
        )
        .when(show_actions, |notice| {
            notice.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(retry)
                    .children(setup),
            )
        })
        .into_any_element()
}
