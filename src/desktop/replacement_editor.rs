//! Shared text-replacement editor used by all three desktop shells.
//!
//! The component owns the paired text inputs and their GPUI presentation. The
//! native roots still own the meaning of a target, settings persistence, and
//! runtime projection, so sharing this editor does not merge their schemas.

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, IntoElement, SharedString,
    Subscription, Window, div, prelude::*, px, rgb,
};

use crate::desktop_mode_target::ModeTarget;
use crate::desktop_ui::{
    FAINT, LINE, MUTED, SURFACE_HOVER, TEXT_SOFT, compact_button, compact_header_plus_button,
    compact_panel, compact_panel_header, tr,
};
use crate::text_input::{Changed as TextChanged, TextInput};
use crate::text_replacements::TextReplacement;

/// One editable phrase/output pair and the subscriptions that project changes
/// back into its owning desktop root.
pub(crate) struct ReplacementEditorInput {
    matched_phrase: Entity<TextInput>,
    output: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

impl ReplacementEditorInput {
    pub(crate) fn new<T: 'static>(
        replacement: &TextReplacement,
        changed: fn(&mut T, &mut Context<T>),
        cx: &mut Context<T>,
    ) -> Self {
        let matched_phrase =
            cx.new(|cx| TextInput::new(cx, "e.g. open code", &replacement.matched_phrase));
        let output = cx.new(|cx| TextInput::new(cx, "e.g. OpenCode", &replacement.output));
        let matched_changed = cx.subscribe(&matched_phrase, move |this, _, _: &TextChanged, cx| {
            changed(this, cx);
        });
        let output_changed = cx.subscribe(&output, move |this, _, _: &TextChanged, cx| {
            changed(this, cx);
        });
        Self {
            matched_phrase,
            output,
            _subscriptions: vec![matched_changed, output_changed],
        }
    }

    pub(crate) fn value(&self, cx: &App) -> TextReplacement {
        TextReplacement {
            matched_phrase: self.matched_phrase.read(cx).text().to_string(),
            output: self.output.read(cx).text().to_string(),
        }
    }

    pub(crate) fn matched_phrase_focus(&self, cx: &App) -> FocusHandle {
        self.matched_phrase.focus_handle(cx)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplacementEditorAction {
    Add(ModeTarget),
    Remove { target: ModeTarget, index: usize },
}

pub(crate) trait ReplacementEditorDelegate: Sized + 'static {
    fn handle_replacement_editor_action(
        &mut self,
        action: ReplacementEditorAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

pub(crate) struct ReplacementEditorView<'a> {
    pub(crate) target: ModeTarget,
    pub(crate) title: &'static str,
    pub(crate) empty_message: &'static str,
    pub(crate) rows: &'a [ReplacementEditorInput],
}

pub(crate) fn render_replacement_editor<T: ReplacementEditorDelegate>(
    view: ReplacementEditorView<'_>,
    cx: &mut Context<T>,
) -> AnyElement {
    let target = view.target;
    let target_id = target.id_fragment();
    let row_count = view.rows.len();
    let rows = view
        .rows
        .iter()
        .enumerate()
        .map(|(index, inputs)| {
            let row_id = SharedString::from(format!("replacement-{target_id}-{index}"));
            let remove_id = SharedString::from(format!("remove-replacement-{target_id}-{index}"));
            div()
                .id(row_id)
                .w_full()
                .px_3()
                .py_2()
                .flex()
                .items_end()
                .gap_2()
                .when(index + 1 < row_count, |row| {
                    row.border_b_1().border_color(rgb(LINE))
                })
                .child(compact_field(
                    tr("Transcription"),
                    inputs.matched_phrase.clone(),
                ))
                .child(
                    div()
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .text_size(px(11.0))
                        .text_color(rgb(FAINT))
                        .child("→"),
                )
                .child(compact_field(tr("Output"), inputs.output.clone()))
                .child(
                    compact_button("×")
                        .id(remove_id)
                        .size(px(34.0))
                        .px_0()
                        .justify_center()
                        .flex_none()
                        .text_size(px(14.0))
                        .text_color(rgb(MUTED))
                        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.handle_replacement_editor_action(
                                ReplacementEditorAction::Remove { target, index },
                                window,
                                cx,
                            );
                        })),
                )
        })
        .collect::<Vec<_>>();

    let add_id = SharedString::from(format!("add-replacement-{target_id}"));
    let add = compact_header_plus_button()
        .id(add_id)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.handle_replacement_editor_action(ReplacementEditorAction::Add(target), window, cx);
        }))
        .into_any_element();

    compact_panel()
        .child(compact_panel_header(tr(view.title), Some(add)))
        .when(row_count == 0, |panel| {
            panel.child(
                div()
                    .px_3()
                    .py_3()
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(tr(view.empty_message)),
            )
        })
        .children(rows)
        .into_any_element()
}

fn compact_field(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(9.0)).text_color(rgb(FAINT)).child(label))
        .child(control)
}
