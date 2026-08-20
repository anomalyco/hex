//! Shared basic mode editor: global fallback copy plus custom mode name,
//! application rules, and browser-host rules.
//!
//! Application selection is an explicit behavioral variant. Catalog-backed
//! roots provide choices and typed actions; free-form roots provide their own
//! process-rule input. The renderer never branches on an operating-system name.

use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, FontWeight, Image, MouseButton, SharedString, Window, div,
    prelude::*, px, rgb,
};

use crate::desktop_mode_list::{ModeTarget, render_application_icon};
use crate::desktop_ui::{
    CANVAS, CONTROL_HEIGHT, FAINT, LINE, MUTED, NEGATIVE, SURFACE, SURFACE_HOVER, SURFACE_SELECTED,
    TEXT, TEXT_SOFT, compact_button, compact_panel, compact_plus_button, settings_copy, tr,
};
use crate::text_input::TextInput;

#[derive(Clone)]
pub(crate) struct CatalogApplicationView {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) icon: Option<Arc<Image>>,
}

pub(crate) struct CatalogApplicationEditorView {
    pub(crate) selected: Vec<CatalogApplicationView>,
    pub(crate) available: Vec<CatalogApplicationView>,
    pub(crate) search: Entity<TextInput>,
    pub(crate) open: bool,
    pub(crate) highlighted: usize,
    pub(crate) status: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) choose_label: &'static str,
}

pub(crate) enum ModeApplicationEditorView {
    #[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
    Catalog(CatalogApplicationEditorView),
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Freeform {
        title: &'static str,
        description: &'static str,
        input: Entity<TextInput>,
    },
}

pub(crate) struct ModeWebsiteEditorView {
    pub(crate) input: Entity<TextInput>,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) enum ModeBasicsView {
    Global {
        title: &'static str,
        description: &'static str,
    },
    Custom {
        target: ModeTarget,
        name: Entity<TextInput>,
        applications: Box<ModeApplicationEditorView>,
        websites: Option<ModeWebsiteEditorView>,
        remove_mode: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModeBasicsAction {
    AddApplication { target: ModeTarget, name: String },
    RemoveApplication { target: ModeTarget, name: String },
    OpenApplicationPicker(ModeTarget),
    CloseApplicationPicker(ModeTarget),
    ChooseApplicationFile(ModeTarget),
    RemoveMode(ModeTarget),
}

pub(crate) trait ModeBasicsDelegate: Sized + 'static {
    fn handle_mode_basics_action(
        &mut self,
        action: ModeBasicsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

pub(crate) fn render_mode_basics<T: ModeBasicsDelegate>(
    view: ModeBasicsView,
    cx: &mut Context<T>,
) -> AnyElement {
    match view {
        ModeBasicsView::Global { title, description } => compact_panel()
            .child(
                div()
                    .w_full()
                    .min_h(px(64.0))
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(settings_copy(title, description)),
            )
            .into_any_element(),
        ModeBasicsView::Custom {
            target,
            name,
            applications,
            websites,
            remove_mode,
        } => {
            let target_id = target.id_fragment();
            let remove = remove_mode.then(|| {
                compact_button(tr("Remove mode"))
                    .id(SharedString::from(format!("remove-mode-{target_id}")))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.handle_mode_basics_action(
                            ModeBasicsAction::RemoveMode(target),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            });
            compact_panel()
                .child(
                    div()
                        .w_full()
                        .min_h(px(54.0))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(tr("Name")),
                        )
                        .child(
                            div()
                                .max_w(px(300.0))
                                .min_w(px(0.0))
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .child(div().flex_1().min_w(px(0.0)).child(name))
                                .when_some(remove, |row, remove| row.child(remove)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(render_application_editor(target, *applications, cx)),
                )
                .when_some(websites, |panel, websites| {
                    panel.child(rule_input_row(
                        websites.title,
                        websites.description,
                        websites.input,
                    ))
                })
                .into_any_element()
        }
    }
}

fn render_application_editor<T: ModeBasicsDelegate>(
    target: ModeTarget,
    view: ModeApplicationEditorView,
    cx: &mut Context<T>,
) -> AnyElement {
    match view {
        ModeApplicationEditorView::Catalog(view) => render_catalog_editor(target, view, cx),
        ModeApplicationEditorView::Freeform {
            title,
            description,
            input,
        } => rule_input_row(title, description, input),
    }
}

fn render_catalog_editor<T: ModeBasicsDelegate>(
    target: ModeTarget,
    view: CatalogApplicationEditorView,
    cx: &mut Context<T>,
) -> AnyElement {
    let target_id = target.id_fragment();
    let selected_rows = view
        .selected
        .into_iter()
        .enumerate()
        .map(|(index, application)| {
            let name = application.name;
            let remove_name = name.clone();
            div()
                .id(SharedString::from(format!(
                    "selected-application-{target_id}-{index}"
                )))
                .h(px(30.0))
                .px_1()
                .flex()
                .items_center()
                .gap_1()
                .rounded_sm()
                .bg(rgb(SURFACE_SELECTED))
                .border_1()
                .border_color(rgb(LINE))
                .child(render_application_icon(application.icon, &name, 20.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(TEXT_SOFT))
                        .child(name),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "remove-selected-application-{target_id}-{index}"
                        )))
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .text_size(px(14.0))
                        .text_color(rgb(FAINT))
                        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                        .child("×")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.handle_mode_basics_action(
                                ModeBasicsAction::RemoveApplication {
                                    target,
                                    name: remove_name.clone(),
                                },
                                window,
                                cx,
                            );
                        })),
                )
        })
        .collect::<Vec<_>>();
    let available_rows = view
        .available
        .into_iter()
        .enumerate()
        .map(|(index, application)| {
            let name = application.name;
            let add_name = name.clone();
            div()
                .id(SharedString::from(format!(
                    "available-application-{target_id}-{index}"
                )))
                .w_full()
                .px_2()
                .py_2()
                .flex()
                .items_center()
                .gap_3()
                .border_b_1()
                .border_color(rgb(LINE))
                .when(index == view.highlighted, |row| {
                    row.bg(rgb(SURFACE_SELECTED))
                })
                .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                .child(render_application_icon(application.icon, &name, 30.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_SOFT))
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(FAINT))
                                .overflow_hidden()
                                .child(application.location),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.handle_mode_basics_action(
                            ModeBasicsAction::AddApplication {
                                target,
                                name: add_name.clone(),
                            },
                            window,
                            cx,
                        );
                    }),
                )
        })
        .collect::<Vec<_>>();
    let tray = view.open.then(|| {
        div()
            .mt_2()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .child(view.search)
            .when_some(view.status, |tray, status| {
                tray.child(
                    div()
                        .px_2()
                        .py_3()
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(status),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "application-picker-results-{target_id}"
                    )))
                    .max_h(px(300.0))
                    .overflow_y_scroll()
                    .children(available_rows),
            )
            .child(
                div()
                    .pt_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "choose-application-file-{target_id}"
                            )))
                            .h(px(28.0))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_SOFT))
                            .hover(|button| button.bg(rgb(SURFACE_HOVER)))
                            .child(view.choose_label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.handle_mode_basics_action(
                                    ModeBasicsAction::ChooseApplicationFile(target),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "close-application-picker-{target_id}"
                            )))
                            .h(px(28.0))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .hover(|button| button.bg(rgb(SURFACE_HOVER)))
                            .child(tr("Done"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.handle_mode_basics_action(
                                    ModeBasicsAction::CloseApplicationPicker(target),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
    });

    div()
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .min_h(px(64.0))
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(div().flex_1().min_w(px(0.0)).child(settings_copy(
                    "Applications",
                    "Switch when an app is frontmost",
                )))
                .child(
                    div()
                        .max_w(px(300.0))
                        .min_w(px(0.0))
                        .min_h(px(CONTROL_HEIGHT))
                        .flex_1()
                        .p_1()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .children(selected_rows)
                        .child(
                            compact_plus_button()
                                .id(SharedString::from(format!(
                                    "open-application-picker-{target_id}"
                                )))
                                .size(px(30.0))
                                .border_1()
                                .border_color(rgb(LINE))
                                .bg(rgb(CANVAS))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.handle_mode_basics_action(
                                        ModeBasicsAction::OpenApplicationPicker(target),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                ),
        )
        .when_some(view.error, |picker, error| {
            picker.child(
                div()
                    .px_3()
                    .pb_2()
                    .text_size(px(10.0))
                    .text_color(rgb(NEGATIVE))
                    .child(error),
            )
        })
        .when_some(tray, |picker, tray| {
            picker.child(div().px_3().pb_3().child(tray))
        })
        .into_any_element()
}

fn rule_input_row(
    title: &'static str,
    description: &'static str,
    input: Entity<TextInput>,
) -> AnyElement {
    div()
        .w_full()
        .min_h(px(64.0))
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(settings_copy(title, description)),
        )
        .child(div().max_w(px(300.0)).min_w(px(0.0)).flex_1().child(input))
        .into_any_element()
}
