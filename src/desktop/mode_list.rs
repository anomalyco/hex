//! Shared mode collection and selection list for desktop roots that implement
//! contextual dictation modes.
//!
//! Entries contain presentation data only. Native roots still own mode schemas,
//! matching semantics, persistence, and the detail editor behind each row.

use std::sync::Arc;

use gpui::{
    AnyElement, Context, FontWeight, Image, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Window, div, img, prelude::*, px, rgb,
};

use crate::desktop_ui::{
    FAINT, MUTED, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT, compact_panel, tr,
};

/// Root-relative identity shared by the mode list and its nested editors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModeTarget {
    Global,
    Mode(usize),
}

impl ModeTarget {
    pub(crate) fn id_fragment(self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Mode(index) => format!("mode-{index}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModeActivationKind {
    Application,
    Website,
}

#[derive(Clone)]
pub(crate) struct ModeActivation {
    label: String,
    icon: Option<Arc<Image>>,
    kind: ModeActivationKind,
}

impl ModeActivation {
    pub(crate) fn application(label: impl Into<String>, icon: Option<Arc<Image>>) -> Self {
        Self {
            label: label.into(),
            icon,
            kind: ModeActivationKind::Application,
        }
    }

    pub(crate) fn website(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            kind: ModeActivationKind::Website,
        }
    }
}

pub(crate) struct ModeListEntry {
    pub(crate) target: ModeTarget,
    pub(crate) title: String,
    pub(crate) empty_subtitle: &'static str,
    pub(crate) activations: Vec<ModeActivation>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ModeListAction {
    Select(ModeTarget),
    OpenContextMenu {
        target: ModeTarget,
        position: Point<Pixels>,
    },
}

pub(crate) trait ModeListDelegate: Sized + 'static {
    fn handle_mode_list_action(
        &mut self,
        action: ModeListAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

pub(crate) struct ModeListView {
    pub(crate) entries: Vec<ModeListEntry>,
    pub(crate) selected: ModeTarget,
    /// Whether custom rows expose a root-owned secondary-click action.
    pub(crate) secondary_action: bool,
}

pub(crate) fn render_mode_list<T: ModeListDelegate>(
    view: ModeListView,
    cx: &mut Context<T>,
) -> AnyElement {
    let rows = view
        .entries
        .into_iter()
        .map(|entry| {
            let target = entry.target;
            let selected = view.selected == target;
            let row_id = SharedString::from(format!("mode-list-{}", entry.target.id_fragment()));
            mode_row(entry, selected)
                .id(row_id)
                .when(
                    view.secondary_action && matches!(target, ModeTarget::Mode(_)),
                    |row| {
                        row.on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                this.handle_mode_list_action(
                                    ModeListAction::OpenContextMenu {
                                        target,
                                        position: event.position,
                                    },
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                    },
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.handle_mode_list_action(ModeListAction::Select(target), window, cx);
                }))
        })
        .collect::<Vec<_>>();

    compact_panel()
        .id("mode-list-card")
        .h_full()
        .overflow_y_scroll()
        .child(div().p_2().flex().flex_col().gap_1().children(rows))
        .into_any_element()
}

fn mode_row(entry: ModeListEntry, selected: bool) -> gpui::Div {
    let has_activations = !entry.activations.is_empty();
    div()
        .w_full()
        .min_h(px(52.0))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .items_start()
        .justify_center()
        .gap_1()
        .rounded(px(6.0))
        .when(selected, |row| {
            row.bg(rgb(SURFACE_SELECTED))
                .hover(|row| row.bg(rgb(SURFACE_HOVER)))
        })
        .when(!selected, |row| row.hover(|row| row.bg(rgb(SURFACE_HOVER))))
        .child(
            div()
                .w_full()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .truncate()
                .child(entry.title),
        )
        .when(has_activations, |row| {
            row.child(mode_activation_icons(&entry.activations))
        })
        .when(!has_activations, |row| {
            row.child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(if selected { TEXT_SOFT } else { FAINT }))
                    .truncate()
                    .child(tr(entry.empty_subtitle)),
            )
        })
}

fn mode_activation_icons(activations: &[ModeActivation]) -> AnyElement {
    let visible = activations.len().min(5);
    let mut icons = activations
        .iter()
        .take(visible)
        .map(mode_activation_icon)
        .collect::<Vec<_>>();
    if activations.len() > visible {
        icons.push(
            div()
                .h(px(18.0))
                .px_1()
                .flex()
                .items_center()
                .rounded(px(4.0))
                .bg(rgb(0x303030))
                .text_size(px(8.0))
                .text_color(rgb(MUTED))
                .child(format!("+{}", activations.len() - visible))
                .into_any_element(),
        );
    }
    div()
        .h(px(18.0))
        .flex()
        .items_center()
        .gap_1()
        .children(icons)
        .into_any_element()
}

fn mode_activation_icon(activation: &ModeActivation) -> AnyElement {
    if let Some(icon) = &activation.icon {
        return img(icon.clone())
            .size(px(18.0))
            .rounded(px(4.0))
            .into_any_element();
    }
    let initial = activation
        .label
        .chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    let website = activation.kind == ModeActivationKind::Website;
    div()
        .size(px(18.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .when(website, |icon| {
            icon.border_1()
                .border_color(rgb(0x444444))
                .bg(rgb(0xeeeeee))
                .text_color(rgb(0x202020))
        })
        .when(!website, |icon| {
            icon.bg(rgb(0x343434)).text_color(rgb(TEXT_SOFT))
        })
        .text_size(px(8.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
        .into_any_element()
}
