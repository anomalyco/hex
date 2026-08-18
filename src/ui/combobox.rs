//! A single-choice dropdown built on gpui-component's DropdownButton and
//! PopupMenu. Stateless and controlled: the trigger shows the caller's
//! current value and items are rebuilt on every open, so app state stays
//! the single source of truth and no per-callsite popup plumbing exists.

use gpui::{App, ElementId, SharedString, Window, px};
use gpui_component::button::{Button, DropdownButton};
use gpui_component::menu::PopupMenuItem;

pub struct ComboItem {
    label: SharedString,
    checked: bool,
    disabled: bool,
    on_pick: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl ComboItem {
    pub fn new(
        label: impl Into<SharedString>,
        checked: bool,
        on_pick: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            checked,
            disabled: false,
            on_pick: Some(Box::new(on_pick)),
        }
    }

    /// A non-interactive line in the menu, e.g. an inline error.
    pub fn note(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            checked: false,
            disabled: true,
            on_pick: None,
        }
    }
}

/// Build the dropdown. `items` runs every time the menu opens, so choices
/// and check marks always reflect the state at that moment.
pub fn combobox<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    items: F,
) -> DropdownButton
where
    F: Fn(&mut Window, &mut App) -> Vec<ComboItem> + 'static,
{
    DropdownButton::new(id)
        .button(Button::new("trigger").label(label))
        .dropdown_menu(move |menu, window, cx| {
            items(window, cx).into_iter().fold(
                menu.min_w(px(220.0)).max_h(px(400.0)).scrollable(true),
                |menu, item| {
                    let popup_item = PopupMenuItem::new(item.label)
                        .checked(item.checked)
                        .disabled(item.disabled);
                    let popup_item = match item.on_pick {
                        Some(on_pick) => {
                            popup_item.on_click(move |_, window, cx| on_pick(window, cx))
                        }
                        None => popup_item,
                    };
                    menu.item(popup_item)
                },
            )
        })
}
