//! Small adapters for gpui-component's Select: a generic labeled option and
//! the state alias shells store on their views.

use gpui::SharedString;
use gpui_component::select::{SelectItem, SelectState};

#[derive(Clone)]
pub struct SelectOption<T: Clone> {
    pub label: SharedString,
    pub value: T,
}

impl<T: Clone> SelectOption<T> {
    pub fn new(label: impl Into<SharedString>, value: T) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

impl<T: Clone + PartialEq + 'static> SelectItem for SelectOption<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

/// A single-choice dropdown over labeled options; `None` models an
/// "all/unset" entry.
pub type OptionalChoiceState = SelectState<Vec<SelectOption<Option<String>>>>;
