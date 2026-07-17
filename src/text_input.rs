use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    WrappedLine, actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

const SURFACE: u32 = 0x171717;
const LINE: u32 = 0x292929;
const TEXT: u32 = 0xeeeeee;
const MUTED: u32 = 0x858585;
const FOCUS: u32 = 0x5a86c8;
const SELECTION: u32 = 0x4776b866;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocumentStart,
        DocumentEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        SelectAll,
        Home,
        End,
        Enter,
        DeleteWordBackward,
        DeleteWordForward,
        DeleteToLineStart,
        Escape,
        Undo,
        Redo,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
    ]
);

/// Emitted after keyboard, clipboard, or input-method editing changes the text.
pub struct Changed;
pub struct Submitted;
pub struct Navigate(pub i32);
pub struct Dismissed;

/// Key bindings required by [`TextInput`].
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("up", Up, Some("TextInput")),
        KeyBinding::new("down", Down, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("shift-up", SelectUp, Some("TextInput")),
        KeyBinding::new("shift-down", SelectDown, Some("TextInput")),
        KeyBinding::new("alt-left", WordLeft, Some("TextInput")),
        KeyBinding::new("alt-right", WordRight, Some("TextInput")),
        KeyBinding::new("shift-alt-left", SelectWordLeft, Some("TextInput")),
        KeyBinding::new("shift-alt-right", SelectWordRight, Some("TextInput")),
        KeyBinding::new("cmd-left", LineStart, Some("TextInput")),
        KeyBinding::new("cmd-right", LineEnd, Some("TextInput")),
        KeyBinding::new("ctrl-a", LineStart, Some("TextInput")),
        KeyBinding::new("ctrl-e", LineEnd, Some("TextInput")),
        KeyBinding::new("ctrl-b", Left, Some("TextInput")),
        KeyBinding::new("ctrl-f", Right, Some("TextInput")),
        KeyBinding::new("ctrl-p", Up, Some("TextInput")),
        KeyBinding::new("ctrl-n", Down, Some("TextInput")),
        KeyBinding::new("ctrl-d", Delete, Some("TextInput")),
        KeyBinding::new("ctrl-h", Backspace, Some("TextInput")),
        KeyBinding::new("shift-cmd-left", SelectLineStart, Some("TextInput")),
        KeyBinding::new("shift-cmd-right", SelectLineEnd, Some("TextInput")),
        KeyBinding::new("cmd-up", DocumentStart, Some("TextInput")),
        KeyBinding::new("cmd-down", DocumentEnd, Some("TextInput")),
        KeyBinding::new("shift-cmd-up", SelectDocumentStart, Some("TextInput")),
        KeyBinding::new("shift-cmd-down", SelectDocumentEnd, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("enter", Enter, Some("TextInput")),
        KeyBinding::new("alt-backspace", DeleteWordBackward, Some("TextInput")),
        KeyBinding::new("alt-delete", DeleteWordForward, Some("TextInput")),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, Some("TextInput")),
        KeyBinding::new("escape", Escape, Some("TextInput")),
        KeyBinding::new("cmd-z", Undo, Some("TextInput")),
        KeyBinding::new("shift-cmd-z", Redo, Some("TextInput")),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("TextInput")),
    ]
}

/// A native GPUI text input with single-line and wrapped multiline modes.
pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_multiline_layout: Option<MultilineLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    scroll_y: Pixels,
    mouse_selection: Option<MouseSelection>,
    preferred_x: Option<Pixels>,
    multiline: bool,
    picker: bool,
    undo_stack: Vec<EditState>,
    redo_stack: Vec<EditState>,
}

#[derive(Clone)]
struct EditState {
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

#[derive(Clone)]
enum MouseSelection {
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
}

#[derive(Clone)]
struct MultilineLayout {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
}

impl MultilineLayout {
    fn size(&self) -> gpui::Size<Pixels> {
        self.lines
            .iter()
            .fold(size(px(0.), px(0.)), |mut size, line| {
                let line_size = line.size(self.line_height);
                size.width = size.width.max(line_size.width);
                size.height += line_size.height;
                size
            })
    }

    fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let mut origin = point(px(0.), px(0.));
        let mut line_start = 0;
        for line in &self.lines {
            let line_end = line_start + line.len();
            if index <= line_end {
                return line
                    .position_for_index(index.saturating_sub(line_start), self.line_height)
                    .map(|position| origin + position);
            }
            origin.y += line.size(self.line_height).height;
            line_start = line_end + 1;
        }
        Some(origin)
    }

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let mut origin = point(px(0.), px(0.));
        let mut line_start = 0;
        for (index, line) in self.lines.iter().enumerate() {
            let bottom = origin.y + line.size(self.line_height).height;
            if position.y < bottom || index + 1 == self.lines.len() {
                return line_start
                    + line
                        .closest_index_for_position(position - origin, self.line_height)
                        .unwrap_or_else(|index| index);
            }
            origin.y = bottom;
            line_start += line.len() + 1;
        }
        line_start.saturating_sub(1)
    }
}

impl TextInput {
    pub fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        initial: impl AsRef<str>,
    ) -> Self {
        Self::with_mode(cx, placeholder, initial.as_ref(), false, false)
    }

    pub fn picker(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        initial: impl AsRef<str>,
    ) -> Self {
        Self::with_mode(cx, placeholder, initial.as_ref(), false, true)
    }

    pub fn multiline(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        initial: impl AsRef<str>,
    ) -> Self {
        Self::with_mode(cx, placeholder, initial.as_ref(), true, false)
    }

    fn with_mode(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        initial: &str,
        multiline: bool,
        picker: bool,
    ) -> Self {
        let content: SharedString = normalize(initial, multiline).into();
        let cursor = content.len();
        Self {
            focus_handle: cx.focus_handle(),
            content,
            placeholder: placeholder.into(),
            selected_range: cursor..cursor,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_multiline_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            scroll_y: px(0.),
            mouse_selection: None,
            preferred_x: None,
            multiline,
            picker,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        self.content.as_ref()
    }

    pub fn set_text(&mut self, text: impl AsRef<str>, cx: &mut Context<Self>) {
        let text = normalize(text.as_ref(), self.multiline);
        if self.content.as_ref() == text {
            return;
        }
        let cursor = text.len();
        self.content = text.into();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.preferred_x = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_vertically(-1.0, false, cx);
        } else if self.picker {
            cx.emit(Navigate(-1));
        } else {
            self.move_to(0, cx);
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_vertically(1.0, false, cx);
        } else if self.picker {
            cx.emit(Navigate(1));
        } else {
            self.move_to(self.content.len(), cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1.0, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1.0, true, cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visual_line_boundary(false), cx);
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visual_line_boundary(true), cx);
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.visual_line_boundary(false), cx);
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.visual_line_boundary(true), cx);
    }

    fn document_start(&mut self, _: &DocumentStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn document_end(&mut self, _: &DocumentEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_document_start(
        &mut self,
        _: &SelectDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    fn select_document_end(
        &mut self,
        _: &SelectDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.content.len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.emit(Submitted);
            window.blur();
        }
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Dismissed);
        window.blur();
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.undo_stack.pop() else {
            return;
        };
        let current = self.edit_state();
        self.restore_edit_state(state);
        self.redo_stack.push(current);
        cx.emit(Changed);
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.redo_stack.pop() else {
            return;
        };
        let current = self.edit_state();
        self.restore_edit_state(state);
        self.undo_stack.push(current);
        cx.emit(Changed);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.visual_line_boundary(false), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.copy(&Copy, window, cx);
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window);
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
            self.mouse_selection = Some(MouseSelection::Character);
        } else if event.click_count >= 3 {
            let range = line_range_at(&self.content, offset);
            self.set_selection(range.clone(), false, cx);
            self.mouse_selection = Some(MouseSelection::Line(range));
        } else if event.click_count == 2 {
            let range = word_range_at(&self.content, offset);
            self.set_selection(range.clone(), false, cx);
            self.mouse_selection = Some(MouseSelection::Word(range));
        } else {
            self.move_to(offset, cx);
            self.mouse_selection = Some(MouseSelection::Character);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.mouse_selection = None;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.mouse_selection.clone() else {
            return;
        };
        let offset = self.index_for_mouse_position(event.position);
        match selection {
            MouseSelection::Character => self.select_to(offset, cx),
            MouseSelection::Word(anchor) => {
                let target = word_range_at(&self.content, offset);
                if target.end <= anchor.start {
                    self.set_selection(target.start..anchor.end, true, cx);
                } else if target.start >= anchor.end {
                    self.set_selection(anchor.start..target.end, false, cx);
                } else {
                    self.set_selection(anchor, false, cx);
                }
            }
            MouseSelection::Line(anchor) => {
                let target = line_range_at(&self.content, offset);
                if target.end <= anchor.start {
                    self.set_selection(target.start..anchor.end, true, cx);
                } else if target.start >= anchor.end {
                    self.set_selection(anchor.start..target.end, false, cx);
                } else {
                    self.set_selection(anchor, false, cx);
                }
            }
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.preferred_x = None;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.preferred_x = None;
        cx.notify();
    }

    fn set_selection(&mut self, range: Range<usize>, reversed: bool, cx: &mut Context<Self>) {
        self.selected_range = range;
        self.selection_reversed = reversed;
        self.preferred_x = None;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn edit_state(&self) -> EditState {
        EditState {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn restore_edit_state(&mut self, state: EditState) {
        self.content = state.content;
        self.selected_range = state.selected_range;
        self.selection_reversed = state.selection_reversed;
        self.marked_range = None;
        self.preferred_x = None;
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = &self.last_bounds else {
            return 0;
        };
        if let Some(layout) = &self.last_multiline_layout {
            return layout
                .index_for_position(point(
                    position.x - bounds.left(),
                    position.y - bounds.top() + self.scroll_y,
                ))
                .min(self.content.len());
        }
        let Some(line) = &self.last_layout else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let mut boundary = offset;
        let mut seen_word = false;
        for (index, character) in self.content[..offset].char_indices().rev() {
            if character.is_alphanumeric() || character == '_' {
                boundary = index;
                seen_word = true;
            } else if seen_word {
                break;
            } else {
                boundary = index;
            }
        }
        boundary
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        let mut boundary = self.content.len();
        let mut seen_word = false;
        for (relative, character) in self.content[offset..].char_indices() {
            let index = offset + relative;
            if character.is_alphanumeric() || character == '_' {
                seen_word = true;
            } else if seen_word {
                return index;
            }
            boundary = index + character.len_utf8();
        }
        boundary
    }

    fn move_vertically(&mut self, direction: f32, selecting: bool, cx: &mut Context<Self>) {
        let Some(layout) = &self.last_multiline_layout else {
            let offset = if direction < 0.0 {
                0
            } else {
                self.content.len()
            };
            if selecting {
                self.select_to(offset, cx);
            } else {
                self.move_to(offset, cx);
            }
            return;
        };
        let Some(position) = layout.position_for_index(self.cursor_offset()) else {
            return;
        };
        let preferred_x = self.preferred_x.unwrap_or(position.x);
        let target = layout
            .index_for_position(point(
                preferred_x,
                position.y + layout.line_height * direction,
            ))
            .min(self.content.len());
        if selecting {
            self.select_to(target, cx);
        } else {
            self.move_to(target, cx);
        }
        self.preferred_x = Some(preferred_x);
    }

    fn visual_line_boundary(&self, end: bool) -> usize {
        if let Some(layout) = &self.last_multiline_layout
            && let Some(position) = layout.position_for_index(self.cursor_offset())
        {
            return layout
                .index_for_position(point(if end { Pixels::MAX } else { px(0.) }, position.y))
                .min(self.content.len());
        }
        let cursor = self.cursor_offset();
        if end {
            self.content[cursor..]
                .find('\n')
                .map_or(self.content.len(), |relative| cursor + relative)
        } else {
            self.content[..cursor]
                .rfind('\n')
                .map_or(0, |index| index + 1)
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_offset_for_utf16(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset].encode_utf16().count()
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn replace(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        marked_selection_utf16: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let new_text = normalize(new_text, self.multiline);
        let content = format!(
            "{}{}{}",
            &self.content[..range.start],
            new_text,
            &self.content[range.end..]
        );

        let is_marked = marked_selection_utf16.is_some();
        if let Some(selection) = marked_selection_utf16.as_ref() {
            self.marked_range =
                (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
            let start = range.start + utf8_offset_for_utf16(&new_text, selection.start);
            let end = range.start + utf8_offset_for_utf16(&new_text, selection.end);
            self.selected_range = start..end;
        } else {
            let cursor = range.start + new_text.len();
            self.marked_range = None;
            self.selected_range = cursor..cursor;
        }
        self.selection_reversed = false;
        self.preferred_x = None;

        if self.content.as_ref() != content {
            if !is_marked {
                if self.undo_stack.len() == 100 {
                    self.undo_stack.remove(0);
                }
                self.undo_stack.push(EditState {
                    content: self.content.clone(),
                    selected_range: range.clone(),
                    selection_reversed: false,
                });
                self.redo_stack.clear();
            }
            self.content = content.into();
            cx.emit(Changed);
        }
        cx.notify();
    }
}

impl EventEmitter<Changed> for TextInput {}
impl EventEmitter<Submitted> for TextInput {}
impl EventEmitter<Navigate> for TextInput {}
impl EventEmitter<Dismissed> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(range_utf16, new_text, None, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(
            range_utf16,
            new_text,
            Some(new_selected_range_utf16.unwrap_or_else(|| {
                let end = new_text.encode_utf16().count();
                end..end
            })),
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        if let Some(layout) = &self.last_multiline_layout {
            let start = layout.position_for_index(range.start)?;
            let end = layout.position_for_index(range.end)?;
            return Some(Bounds::from_corners(
                point(
                    bounds.left() + start.x,
                    bounds.top() + start.y - self.scroll_y,
                ),
                point(
                    bounds.left() + end.x.max(start.x + px(1.)),
                    bounds.top() + end.y - self.scroll_y + layout.line_height,
                ),
            ));
        }
        let line = self.last_layout.as_ref()?;
        Some(Bounds::from_corners(
            point(
                bounds.left() + line.x_for_index(range.start) - self.scroll_x,
                bounds.top(),
            ),
            point(
                bounds.left() + line.x_for_index(range.end) - self.scroll_x,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        if self.content.is_empty() {
            return Some(0);
        }
        let bounds = self.last_bounds?;
        if let Some(layout) = &self.last_multiline_layout {
            let index = layout.index_for_position(point(
                position.x - bounds.left(),
                position.y - bounds.top() + self.scroll_y,
            ));
            return Some(self.offset_to_utf16(index.min(self.content.len())));
        }
        let line = self.last_layout.as_ref()?;
        let index = line.index_for_x(position.x - bounds.left() + self.scroll_x)?;
        Some(self.offset_to_utf16(index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    multiline_layout: Option<MultilineLayout>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    scroll_x: Pixels,
    scroll_y: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = if self.input.read(cx).multiline {
            relative(1.).into()
        } else {
            window.line_height().into()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let style = window.text_style();
        let (display_text, color) = if content.is_empty() {
            (input.placeholder.clone(), rgb(MUTED).into())
        } else {
            (content, style.color)
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = &input.marked_range {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        if input.multiline {
            let lines = window
                .text_system()
                .shape_text(
                    display_text,
                    font_size,
                    &runs,
                    Some(bounds.size.width),
                    None,
                )
                .unwrap_or_default();
            let layout = MultilineLayout {
                lines: lines.into_vec(),
                line_height: window.line_height(),
            };
            let caret = layout
                .position_for_index(input.cursor_offset())
                .unwrap_or_default();
            let margin = px(2.);
            let mut scroll_y = input.scroll_y;
            if caret.y < scroll_y + margin {
                scroll_y = (caret.y - margin).max(px(0.));
            } else if caret.y + layout.line_height > scroll_y + bounds.size.height - margin {
                scroll_y = caret.y + layout.line_height - bounds.size.height + margin;
            }
            scroll_y = scroll_y.min((layout.size().height - bounds.size.height).max(px(0.)));
            let cursor = selected_range.is_empty().then(|| {
                fill(
                    Bounds::new(
                        point(bounds.left() + caret.x, bounds.top() + caret.y - scroll_y),
                        size(px(1.), layout.line_height),
                    ),
                    rgb(TEXT),
                )
            });
            let selections = if selected_range.is_empty() {
                Vec::new()
            } else {
                multiline_selection_quads(&layout, &selected_range, bounds, scroll_y)
            };
            return PrepaintState {
                line: None,
                multiline_layout: Some(layout),
                cursor,
                selections,
                scroll_x: px(0.),
                scroll_y,
            };
        }
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);
        let viewport_width = bounds.size.width;
        let caret_x = line.x_for_index(input.cursor_offset());
        let margin = px(4.);
        let mut scroll_x = input.scroll_x;
        if caret_x < scroll_x + margin {
            scroll_x = (caret_x - margin).max(px(0.));
        } else if caret_x > scroll_x + viewport_width - margin {
            scroll_x = caret_x - viewport_width + margin;
        }
        scroll_x = scroll_x.min((line.width - viewport_width).max(px(0.)));

        let (selections, cursor) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + line.x_for_index(input.cursor_offset()) - scroll_x,
                            bounds.top(),
                        ),
                        size(px(1.), bounds.bottom() - bounds.top()),
                    ),
                    rgb(TEXT),
                )),
            )
        } else {
            (
                vec![fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start) - scroll_x,
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end) - scroll_x,
                            bounds.bottom(),
                        ),
                    ),
                    rgba(SELECTION),
                )],
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            multiline_layout: None,
            cursor,
            selections,
            scroll_x,
            scroll_y: px(0.),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        if let Some(layout) = prepaint.multiline_layout.take() {
            let mut origin = point(bounds.origin.x, bounds.origin.y - prepaint.scroll_y);
            for line in &layout.lines {
                line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    Some(bounds),
                    window,
                    cx,
                )
                .expect("wrapped text line should paint");
                origin.y += line.size(layout.line_height).height;
            }
            self.input.update(cx, |input, _| {
                input.last_layout = None;
                input.last_multiline_layout = Some(layout);
                input.last_bounds = Some(bounds);
                input.scroll_x = px(0.);
                input.scroll_y = prepaint.scroll_y;
            });
        } else {
            let line = prepaint.line.take().expect("text line was shaped");
            line.paint(
                point(bounds.origin.x - prepaint.scroll_x, bounds.origin.y),
                window.line_height(),
                window,
                cx,
            )
            .expect("text line should paint");
            self.input.update(cx, |input, _| {
                input.last_layout = Some(line);
                input.last_multiline_layout = None;
                input.last_bounds = Some(bounds);
                input.scroll_x = prepaint.scroll_x;
                input.scroll_y = px(0.);
            });
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
    }
}

fn multiline_selection_quads(
    layout: &MultilineLayout,
    range: &Range<usize>,
    bounds: Bounds<Pixels>,
    scroll_y: Pixels,
) -> Vec<PaintQuad> {
    let Some(start) = layout.position_for_index(range.start) else {
        return Vec::new();
    };
    let Some(end) = layout.position_for_index(range.end) else {
        return Vec::new();
    };
    let top = bounds.top() - scroll_y;
    if start.y == end.y {
        return vec![fill(
            Bounds::from_corners(
                point(bounds.left() + start.x, top + start.y),
                point(bounds.left() + end.x, top + start.y + layout.line_height),
            ),
            rgba(SELECTION),
        )];
    }
    let mut quads = vec![fill(
        Bounds::from_corners(
            point(bounds.left() + start.x, top + start.y),
            point(bounds.right(), top + start.y + layout.line_height),
        ),
        rgba(SELECTION),
    )];
    let middle_top = start.y + layout.line_height;
    if end.y > middle_top {
        quads.push(fill(
            Bounds::from_corners(
                point(bounds.left(), top + middle_top),
                point(bounds.right(), top + end.y),
            ),
            rgba(SELECTION),
        ));
    }
    quads.push(fill(
        Bounds::from_corners(
            point(bounds.left(), top + end.y),
            point(bounds.left() + end.x, top + end.y + layout.line_height),
        ),
        rgba(SELECTION),
    ));
    quads
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = if self.focus_handle.is_focused(window) {
            FOCUS
        } else {
            LINE
        };
        div()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::document_start))
            .on_action(cx.listener(Self::document_end))
            .on_action(cx.listener(Self::select_document_start))
            .on_action(cx.listener(Self::select_document_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::delete_word_backward))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .h(if self.multiline { px(132.) } else { px(34.) })
            .px(px(10.))
            .py(px(7.))
            .overflow_hidden()
            .rounded_sm()
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(SURFACE))
            .text_color(rgb(TEXT))
            .text_size(px(14.))
            .line_height(px(20.))
            .child(TextElement { input: cx.entity() })
    }
}

fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let offset = offset.min(text.len());
    if let Some((start, word)) = text
        .unicode_word_indices()
        .find(|(start, word)| *start <= offset && offset <= *start + word.len())
    {
        return start..start + word.len();
    }
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        if start <= offset && offset <= end {
            return start..end;
        }
    }
    text.len()..text.len()
}

fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |relative| offset + relative);
    start..end
}

fn normalize(text: &str, multiline: bool) -> String {
    if multiline {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.replace("\r\n", " ").replace(['\r', '\n'], " ")
    }
}

fn utf8_offset_for_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_offset = 0;
    for character in text.chars() {
        if utf16_offset >= offset {
            break;
        }
        utf8_offset += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf8_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_inputs_flatten_pasted_line_breaks() {
        assert_eq!(
            normalize("first\r\nsecond\nthird", false),
            "first second third"
        );
    }

    #[test]
    fn multiline_inputs_preserve_normalized_line_breaks() {
        assert_eq!(
            normalize("first\r\nsecond\rthird", true),
            "first\nsecond\nthird"
        );
    }

    #[test]
    fn utf16_offsets_keep_unicode_carets_on_utf8_boundaries() {
        assert_eq!(utf8_offset_for_utf16("a😀b", 1), 1);
        assert_eq!(utf8_offset_for_utf16("a😀b", 3), 5);
    }

    #[test]
    fn double_click_selects_the_complete_unicode_word() {
        assert_eq!(word_range_at("hello, naïve world", 9), 7..13);
        assert_eq!(word_range_at("hello, naïve world", 13), 7..13);
    }

    #[test]
    fn triple_click_selects_the_logical_line() {
        assert_eq!(line_range_at("first\nsecond line\nthird", 10), 6..17);
        assert_eq!(line_range_at("first\nsecond line\nthird", 17), 6..17);
    }
}
