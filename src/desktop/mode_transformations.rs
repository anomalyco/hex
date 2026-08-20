//! Shared ordered transformation editor for contextual dictation modes.
//!
//! The renderer owns catalog presentation, drag/drop ordering, and typed
//! actions. Native roots retain their persisted mode schemas and decide how a
//! registered transformation is executed.

use gpui::{AnyElement, Context, Pixels, Point, Render, Window, div, prelude::*, px, rgb};

use crate::desktop_mode_list::ModeTarget;
use crate::desktop_ui::{
    FAINT, LINE, MUTED, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT, compact_button,
    compact_header_plus_button, compact_panel, compact_panel_header, tr,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransformationCatalogEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

pub(crate) struct ModeTransformationsView {
    pub(crate) target: ModeTarget,
    pub(crate) selected: Vec<String>,
    pub(crate) catalog: Vec<TransformationCatalogEntry>,
    pub(crate) picker_open: bool,
    pub(crate) workspace: Option<TransformationWorkspaceView>,
}

pub(crate) struct TransformationWorkspaceView {
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) error: Option<String>,
    pub(crate) action_label: Option<&'static str>,
    pub(crate) action: ModeTransformationsWorkspaceAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModeTransformationsWorkspaceAction {
    Initialize,
    Retry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModeTransformationsAction {
    TogglePicker,
    Add {
        target: ModeTarget,
        id: String,
    },
    Remove {
        target: ModeTarget,
        id: String,
    },
    Move {
        target: ModeTarget,
        id: String,
        target_index: usize,
    },
    Workspace(ModeTransformationsWorkspaceAction),
}

pub(crate) trait ModeTransformationsDelegate: Sized + 'static {
    fn handle_mode_transformations_action(
        &mut self,
        action: ModeTransformationsAction,
        cx: &mut Context<Self>,
    );
}

#[derive(Clone)]
struct TransformationDrag {
    target: ModeTarget,
    id: String,
    name: String,
    position: Point<Pixels>,
}

impl TransformationDrag {
    fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for TransformationDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.position.x - px(90.0))
            .pt(self.position.y - px(17.0))
            .child(
                div()
                    .w(px(180.0))
                    .h(px(34.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(rgb(0x505050))
                    .bg(rgb(SURFACE_SELECTED))
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT))
                    .child(self.name.clone()),
            )
    }
}

pub(crate) fn render_mode_transformations<T: ModeTransformationsDelegate>(
    view: ModeTransformationsView,
    cx: &mut Context<T>,
) -> AnyElement {
    let target = view.target;
    let selected = view.selected;
    let workspace = view.workspace;
    let selected_rows = selected
        .iter()
        .enumerate()
        .map(|(target_index, id)| {
            let name = view
                .catalog
                .iter()
                .find(|transformation| &transformation.id == id)
                .map_or_else(|| id.clone(), |transformation| transformation.name.clone());
            let drag = TransformationDrag {
                target,
                id: id.clone(),
                name: name.clone(),
                position: Point::default(),
            };
            let dragged_id = id.clone();
            let drop_id = id.clone();
            div()
                .id(("mode-transformation", target_index))
                .h(px(36.0))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .when(target_index + 1 < selected.len(), |row| {
                    row.border_b_1().border_color(rgb(LINE))
                })
                .can_drop(move |value, _, _| {
                    value
                        .downcast_ref::<TransformationDrag>()
                        .is_some_and(|drag| drag.target == target && drag.id != drop_id)
                })
                .on_drop(cx.listener(move |this, drag: &TransformationDrag, _, cx| {
                    if drag.target == target {
                        this.handle_mode_transformations_action(
                            ModeTransformationsAction::Move {
                                target,
                                id: drag.id.clone(),
                                target_index,
                            },
                            cx,
                        );
                    }
                }))
                .child(
                    div()
                        .id(("drag-mode-transformation", target_index))
                        .w(px(20.0))
                        .h_full()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(rgb(FAINT))
                        .child("⠿")
                        .on_drag(drag, |drag, position, _, cx| {
                            cx.new(|_| drag.clone().at(position))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(11.0))
                        .text_color(rgb(TEXT_SOFT))
                        .truncate()
                        .child(name),
                )
                .child(
                    compact_button("×")
                        .id(("remove-mode-transformation", target_index))
                        .size(px(28.0))
                        .px_0()
                        .justify_center()
                        .text_size(px(14.0))
                        .text_color(rgb(MUTED))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.handle_mode_transformations_action(
                                ModeTransformationsAction::Remove {
                                    target,
                                    id: dragged_id.clone(),
                                },
                                cx,
                            );
                        })),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

    let available_rows = if view.picker_open {
        view.catalog
            .iter()
            .enumerate()
            .filter(|(_, transformation)| !selected.contains(&transformation.id))
            .map(|(index, transformation)| {
                let id = transformation.id.clone();
                let name = transformation.name.clone();
                let description = transformation.description.clone();
                div()
                    .id(("available-mode-transformation", index))
                    .min_h(px(40.0))
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_SOFT))
                                    .child(name),
                            )
                            .when_some(description, |copy, description| {
                                copy.child(
                                    div()
                                        .pt_1()
                                        .text_size(px(10.0))
                                        .text_color(rgb(FAINT))
                                        .child(description),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(TEXT_SOFT))
                            .child(tr("Add")),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.handle_mode_transformations_action(
                            ModeTransformationsAction::Add {
                                target,
                                id: id.clone(),
                            },
                            cx,
                        );
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let available_empty = available_rows.is_empty();
    let catalog_empty = view.catalog.is_empty();
    let add = (!catalog_empty).then(|| {
        let button = if view.picker_open {
            compact_button(tr("Done"))
                .h(px(28.0))
                .mr(px(-7.0))
                .text_size(px(10.0))
        } else {
            compact_header_plus_button()
        };
        button
            .id("toggle-transformation-picker")
            .on_click(cx.listener(|this, _, _, cx| {
                this.handle_mode_transformations_action(
                    ModeTransformationsAction::TogglePicker,
                    cx,
                );
            }))
            .into_any_element()
    });

    compact_panel()
        .child(
            compact_panel_header(tr("Transformations"), add)
                .when(selected.is_empty() && !view.picker_open, |header| {
                    header.border_b_0()
                }),
        )
        .children(selected_rows)
        .when(view.picker_open && !catalog_empty, |panel| {
            panel
                .when(available_empty, |list| {
                    list.child(
                        div()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .px_3()
                            .py_3()
                            .text_size(px(10.0))
                            .text_color(rgb(MUTED))
                            .child(tr("Every registered transformation is already included.")),
                    )
                })
                .children(available_rows)
        })
        .when_some(workspace, |panel, workspace| {
            let action = workspace.action_label.map(|label| {
                let action = workspace.action;
                compact_button(tr(label))
                    .id("mode-transformations-workspace-action")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.handle_mode_transformations_action(
                            ModeTransformationsAction::Workspace(action),
                            cx,
                        );
                    }))
            });
            let is_error = workspace.error.is_some();
            let message = workspace
                .error
                .unwrap_or_else(|| tr(workspace.description).to_string());
            panel.child(
                div()
                    .px_3()
                    .py_3()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
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
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_SOFT))
                                    .child(tr(workspace.title)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .line_height(px(16.0))
                                    .text_color(rgb(if is_error {
                                        crate::desktop_ui::NEGATIVE
                                    } else {
                                        FAINT
                                    }))
                                    .child(message),
                            ),
                    )
                    .children(action),
            )
        })
        .into_any_element()
}

pub(crate) fn reorder_transformation(
    values: &mut Vec<String>,
    id: &str,
    target_index: usize,
) -> bool {
    let Some(from) = values.iter().position(|candidate| candidate == id) else {
        return false;
    };
    if from == target_index {
        return false;
    }
    let value = values.remove(from);
    let target = target_index.min(values.len());
    values.insert(target, value);
    true
}

#[cfg(test)]
mod tests {
    use super::reorder_transformation;

    #[test]
    fn transformations_reorder_in_both_directions() {
        let mut transformations = vec!["one".into(), "two".into(), "three".into()];

        assert!(reorder_transformation(&mut transformations, "one", 2));
        assert_eq!(transformations, ["two", "three", "one"]);
        assert!(reorder_transformation(&mut transformations, "one", 0));
        assert_eq!(transformations, ["one", "two", "three"]);
    }

    #[test]
    fn reorder_ignores_same_position_and_missing_ids() {
        let mut transformations = vec!["one".into(), "two".into()];

        assert!(!reorder_transformation(&mut transformations, "one", 0));
        assert!(!reorder_transformation(&mut transformations, "missing", 1));
        assert_eq!(transformations, ["one", "two"]);
    }
}
