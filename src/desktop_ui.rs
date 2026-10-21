//! Shared desktop components. Every pane builds from these and the tokens in
//! [`crate::design_tokens`]; see `docs/design-system.md` for which component
//! does which job.

use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, IntoElement, Pixels, Rgba, ShapedLine,
    SharedString, Stateful, Window, div, prelude::*, px, rgb,
};

#[cfg(any(target_os = "linux", target_os = "windows"))]
use gpui::{Image, ImageFormat, img};
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::sync::Arc;

use crate::design_tokens::*;

#[derive(Clone, Copy)]
pub(crate) enum NavigationIcon {
    Activity,
    Commands,
    Fun,
    History,
    HudLab,
    Meetings,
    Modes,
    Settings,
    VoiceAction,
}

impl NavigationIcon {
    /// Line-weight symbols only, so no sidebar item reads heavier than another.
    #[cfg(target_os = "macos")]
    fn sf_symbol(self) -> &'static str {
        match self {
            Self::Settings => "slider.horizontal.3",
            Self::Modes => "square.grid.2x2",
            Self::VoiceAction => "sparkle",
            Self::Commands => "command",
            Self::History => "clock",
            Self::Meetings => "calendar",
            Self::Fun => "wand.and.stars",
            Self::Activity => "chart.bar",
            Self::HudLab => "flask",
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn heroicon(self) -> &'static str {
        match self {
            Self::Settings => include_str!("../assets/icons/settings.svg"),
            Self::Modes => include_str!("../assets/icons/modes.svg"),
            Self::VoiceAction => include_str!("../assets/icons/voice-action.svg"),
            Self::Commands => include_str!("../assets/icons/commands.svg"),
            Self::History => include_str!("../assets/icons/history.svg"),
            Self::Meetings => include_str!("../assets/icons/meetings.svg"),
            Self::Fun => include_str!("../assets/icons/hud-lab.svg"),
            Self::Activity => include_str!("../assets/icons/activity.svg"),
            Self::HudLab => include_str!("../assets/icons/hud-lab.svg"),
        }
    }
}

#[cfg(target_os = "macos")]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    gpui_symbols::Icon::new(icon.sf_symbol())
        .size(px(ICON_SIZE))
        .color(rgb(if selected {
            text_primary()
        } else {
            text_tertiary()
        }))
        .weight(gpui_symbols::SymbolWeight::Medium)
        .rendering_mode(gpui_symbols::RenderingMode::Monochrome)
        .into_any_element()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    let color = format!(
        "#{:06x}",
        if selected {
            text_primary()
        } else {
            text_tertiary()
        }
    );
    let image = Image::from_bytes(
        ImageFormat::Svg,
        icon.heroicon().replace("currentColor", &color).into_bytes(),
    );
    img(Arc::new(image))
        .size(px(ICON_SIZE + 2.0))
        .into_any_element()
}

/// The one inline icon set. On macOS every icon is a line-weight SF Symbol at
/// medium weight; Linux, without SF Symbols, falls back to a matching glyph,
/// and Windows draws the matching Segoe Fluent Icons glyph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Icon {
    ArrowRight,
    ChevronDown,
    Close,
    DragHandle,
    Ellipsis,
    Globe,
    Plus,
    Microphone,
    Keyboard,
    Accessibility,
    Waveform,
    Check,
}

impl Icon {
    /// The ellipsis's three dots read lighter than the other glyphs at the
    /// same box, so it is drawn one size up to match them optically.
    fn optical_size(self) -> f32 {
        match self {
            Self::Ellipsis
            | Self::Microphone
            | Self::Keyboard
            | Self::Accessibility
            | Self::Waveform => ICON_SIZE,
            _ => ICON_SIZE_SMALL,
        }
    }

    #[cfg(target_os = "macos")]
    fn sf_symbol(self) -> &'static str {
        match self {
            Self::ArrowRight => "arrow.right",
            Self::ChevronDown => "chevron.down",
            Self::Close => "xmark",
            Self::DragHandle => "line.3.horizontal",
            Self::Ellipsis => "ellipsis",
            Self::Globe => "globe",
            Self::Plus => "plus",
            Self::Microphone => "mic",
            Self::Keyboard => "keyboard",
            Self::Accessibility => "accessibility",
            Self::Waveform => "waveform",
            Self::Check => "checkmark",
        }
    }

    #[cfg(target_os = "linux")]
    fn glyph(self) -> &'static str {
        match self {
            Self::ArrowRight => "→",
            Self::ChevronDown => "⌄",
            Self::Close => "×",
            Self::DragHandle => "≡",
            Self::Ellipsis => "…",
            Self::Globe => "◎",
            Self::Plus => "+",
            Self::Microphone => "●",
            Self::Keyboard => "⌨",
            Self::Accessibility => "◉",
            Self::Waveform => "≋",
            Self::Check => "✓",
        }
    }

    #[cfg(target_os = "windows")]
    fn fluent_glyph(self) -> &'static str {
        match self {
            Self::ArrowRight => "\u{E72A}",
            Self::ChevronDown => "\u{E70D}",
            Self::Close => "\u{E711}",
            Self::DragHandle => "\u{E700}",
            Self::Ellipsis => "\u{E712}",
            Self::Globe => "\u{E774}",
            Self::Plus => "\u{E710}",
            Self::Microphone => "\u{E720}",
            Self::Keyboard => "\u{E765}",
            Self::Accessibility => "\u{E776}",
            Self::Waveform => "\u{E8D6}",
            Self::Check => "\u{E73E}",
        }
    }
}

/// An inline icon at [`ICON_SIZE_SMALL`] (optically matched) in `color`.
pub(crate) fn icon(glyph: Icon, color: u32) -> AnyElement {
    icon_at(glyph, glyph.optical_size(), color)
}

/// An icon drawn at `size`, for illustrations such as onboarding art.
#[cfg(target_os = "macos")]
pub(crate) fn icon_at(icon: Icon, size: f32, color: u32) -> AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui_symbols::Icon::new(icon.sf_symbol())
                .size(px(size))
                .color(rgb(color))
                .weight(gpui_symbols::SymbolWeight::Medium)
                .rendering_mode(gpui_symbols::RenderingMode::Monochrome),
        )
        .into_any_element()
}

/// An icon drawn at `size`, for illustrations such as onboarding art.
#[cfg(target_os = "linux")]
pub(crate) fn icon_at(icon: Icon, size: f32, color: u32) -> AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_step(TypeStep {
            size,
            line_height: size,
            weight: None,
        })
        .text_color(rgb(color))
        .child(icon.glyph())
        .into_any_element()
}

/// An icon drawn at `size`, for illustrations such as onboarding art.
#[cfg(target_os = "windows")]
pub(crate) fn icon_at(icon: Icon, size: f32, color: u32) -> AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(crate::windows_ui::fluent_icon(
            icon.fluent_glyph(),
            size,
            color,
        ))
        .into_any_element()
}

/// The window root: canvas background and the body type step every unsized
/// descendant inherits.
pub(crate) fn window_frame() -> Div {
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .flex()
        .bg(rgb(canvas()))
        .text_body()
        .text_color(rgb(text_primary()))
}

/// The navigation column: [`SIDEBAR_WIDTH`] wide, clear of the title bar.
pub(crate) fn sidebar_frame() -> Div {
    div()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .px_3()
        .pt(px(TITLEBAR_INSET))
        .pb_4()
        .flex()
        .flex_col()
        .flex_none()
        .bg(rgb(sidebar()))
        .border_r_1()
        .border_color(rgb(line()))
}

pub(crate) fn navigation_item(icon: NavigationIcon, selected: bool) -> Div {
    div()
        .h(px(NAV_ITEM_HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .rounded(RADIUS_CONTROL)
        .text_body()
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if selected {
            text_primary()
        } else {
            text_tertiary()
        }))
        .when(selected, |item| item.bg(rgb(surface_selected())))
        .when(!selected, |item| {
            item.hover(|item| {
                item.bg(rgb(sidebar_hover()))
                    .text_color(rgb(text_secondary()))
            })
        })
        .child(
            div()
                .size(px(ICON_SIZE + 4.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(navigation_icon(icon, selected)),
        )
}

pub(crate) fn pane_header(title: &'static str) -> AnyElement {
    pane_header_with_action(title, None)
}

/// The one pane header: title left, optional action right, inset by
/// [`PANE_GUTTER`] and bounded to [`PANE_CONTENT_WIDTH`] like the body below.
pub(crate) fn pane_header_with_action(
    title: &'static str,
    action: Option<AnyElement>,
) -> AnyElement {
    div()
        .h(px(PANE_HEADER_HEIGHT))
        .px(px(PANE_GUTTER))
        .flex_none()
        .flex()
        .justify_center()
        .child(
            div()
                .h_full()
                .w_full()
                .max_w(px(PANE_CONTENT_WIDTH))
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .border_b_1()
                .border_color(rgb(line()))
                .child(div().flex_none().text_title().child(title))
                .when_some(action, |header, action| {
                    header.child(div().flex().items_center().gap_2().child(action))
                }),
        )
        .into_any_element()
}

/// The one fixed pane body under the header, inset by [`PANE_GUTTER`] so
/// its cards align with the header rule. Put the column in [`pane_content`].
pub(crate) fn pane_body() -> Div {
    div()
        .flex_1()
        .min_h(px(0.0))
        .overflow_hidden()
        .flex()
        .justify_center()
        .px(px(PANE_GUTTER))
        .pt_5()
        .pb_6()
}

/// The scrolling pane body: the scrollbar sits at the window edge while the
/// content keeps the [`PANE_GUTTER`] inset and [`PANE_CONTENT_WIDTH`].
pub(crate) fn pane_scroll_body(id: impl Into<ElementId>, content: Div) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .px(px(PANE_GUTTER))
        .pb_6()
        .child(
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(content.w_full().max_w(px(PANE_CONTENT_WIDTH)).min_w_0()),
        )
}

/// The one pane content column, bounded to [`PANE_CONTENT_WIDTH`].
pub(crate) fn pane_content() -> Div {
    div()
        .w_full()
        .max_w(px(PANE_CONTENT_WIDTH))
        .min_h(px(0.0))
        .flex()
        .flex_col()
}

/// The list column of a list+detail pane: a scrolling card `width` wide
/// ([`PANE_LIST_WIDTH`], or [`PANE_LIST_WIDTH_COMPACT`] beside an editor) with
/// its empty notice and load error ahead of its rows. Callers decide when
/// `empty` applies.
pub(crate) fn pane_list(
    id: impl Into<ElementId>,
    width: f32,
    empty: Option<&'static str>,
    error_title: &'static str,
    error: Option<String>,
    rows: Vec<AnyElement>,
) -> Stateful<Div> {
    panel()
        .id(id)
        .w(px(width))
        .h_full()
        .flex_none()
        .overflow_y_scroll()
        .when_some(empty, |list, message| {
            list.flex().flex_col().child(detail_placeholder(message))
        })
        .when_some(error, |list, error| {
            list.child(error_message(error_title, error))
        })
        .when(!rows.is_empty(), |list| {
            // A scrolling card gives its content no definite width, which
            // truncated rows need, so the column states its own.
            list.child(
                div()
                    .w(px(width - 2.0))
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows),
            )
        })
}

/// The detail card beside a [`pane_list`], filling the remaining width.
pub(crate) fn pane_detail(content: impl IntoElement) -> Div {
    panel()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .flex_col()
        .child(content)
}

/// A [`section_label`] heading a group of rows in a [`pane_list`], inset to
/// the row text. The first group sits closer to the top of the card.
pub(crate) fn list_section_label(label: impl IntoElement, first: bool) -> Div {
    section_label(label)
        .px_3()
        .when(first, |label| label.pt_1())
        .when(!first, |label| label.pt_3())
        .pb_1()
}

/// A rounded, selectable row inside a list column.
pub(crate) fn list_row(selected: bool) -> Div {
    div()
        .w_full()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .rounded(RADIUS_CONTROL)
        .when(selected, |row| row.bg(rgb(surface_selected())))
        .when(!selected, |row| {
            row.hover(|row| row.bg(rgb(surface_hover())))
        })
}

/// The title line of a [`list_row`].
pub(crate) fn list_row_title(text: impl Into<SharedString>, selected: bool) -> Div {
    truncated_line(BODY, text)
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(if selected {
            text_primary()
        } else {
            text_secondary()
        }))
}

/// The metadata line of a [`list_row`].
pub(crate) fn list_row_meta(text: impl Into<SharedString>, selected: bool) -> Div {
    truncated_line(CAPTION, text).text_color(rgb(if selected {
        text_secondary()
    } else {
        text_tertiary()
    }))
}

/// One line that ends in an ellipsis when it overflows. Add `flex_1` in rows.
///
/// The line is shaped while painting, against the width its box finally
/// receives. GPUI's own truncation measures text during layout, where
/// ancestors sizing themselves to their content can hand it a provisional
/// width that then sticks.
pub(crate) fn truncated_line(step: TypeStep, text: impl Into<SharedString>) -> Div {
    let text = text.into();
    div()
        .w_full()
        .min_w_0()
        .h(px(step.line_height))
        .flex_shrink()
        .text_step(step)
        .child(
            gpui::canvas(
                move |bounds, window, cx| fit_line(text, bounds.size.width, window, cx),
                |bounds, line, window, cx| {
                    let _ = line.paint(bounds.origin, bounds.size.height, window, cx);
                },
            )
            .size_full(),
        )
}

/// Shapes `text` in the inherited text style, cut to `width` with "…".
fn fit_line(text: SharedString, width: Pixels, window: &mut Window, cx: &mut App) -> ShapedLine {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let mut runs = vec![style.to_run(text.len())];
    let text = cx
        .text_system()
        .line_wrapper(style.font(), font_size)
        .truncate_line(text, width, "…", &mut runs);
    window
        .text_system()
        .shape_line(text, font_size, &runs, None)
}

/// A card: the elevated surface every grouped control and list sits on.
pub(crate) fn panel() -> Div {
    div()
        .w_full()
        .relative()
        .rounded(RADIUS_PANEL)
        .border_1()
        .border_color(rgb(line()))
        .bg(rgb(surface()))
        .overflow_hidden()
}

/// A card's title bar, with an optional trailing control.
pub(crate) fn panel_header(title: impl IntoElement, action: Option<AnyElement>) -> Div {
    div()
        .h(px(PANEL_HEADER_HEIGHT))
        .px_4()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(rgb(line()))
        .child(
            div()
                .text_body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .when_some(action, |header, action| header.child(action))
}

/// A small label above a group: caption, semibold, sentence case.
pub(crate) fn section_label(label: impl IntoElement) -> Div {
    div()
        .text_caption()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(text_tertiary()))
        .child(label)
}

/// A [`section_label`] spaced to head a settings card, inset to the card's
/// row text so both share one left edge.
pub(crate) fn settings_section_label(label: &'static str) -> AnyElement {
    section_label(label).pt_6().pb_2().px_4().into_any_element()
}

/// The fixed top of a detail card: a [`detail_title`], an optional metadata
/// line, and an optional action row, ruled off edge to edge from the
/// [`detail_body`] below.
pub(crate) fn detail_header(
    title: impl IntoElement,
    meta: Vec<AnyElement>,
    actions: Vec<AnyElement>,
) -> Div {
    div()
        .flex_none()
        .px_6()
        .pt_5()
        .pb_4()
        .flex()
        .flex_col()
        .gap_2()
        .border_b_1()
        .border_color(rgb(line()))
        .child(title)
        .when(!meta.is_empty(), |header| {
            header.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_footnote()
                    .text_color(rgb(text_tertiary()))
                    .children(meta),
            )
        })
        .when(!actions.is_empty(), |header| {
            header.child(
                div()
                    .debug_selector(|| "detail-actions".into())
                    .pt_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .children(actions),
            )
        })
}

/// The title of a [`detail_header`].
pub(crate) fn detail_title(text: impl Into<SharedString>) -> Div {
    div().text_headline().child(text.into())
}

/// The scrolling content under a [`detail_header`]. It stays a block: a flex
/// column here measures `detail_row` values at their full length.
pub(crate) fn detail_body(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .px_6()
        .pt_5()
        .pb_6()
}

pub(crate) fn settings_copy(title: &'static str, description: impl IntoElement) -> AnyElement {
    div()
        .debug_selector(|| "settings-copy".into())
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_body()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .text_footnote()
                .text_color(rgb(text_tertiary()))
                .child(description),
        )
        .into_any_element()
}

pub(crate) fn settings_row(
    title: &'static str,
    description: impl IntoElement,
    control: impl IntoElement,
) -> Div {
    div()
        .w_full()
        .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
        .px_4()
        .py_2()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_4()
        .border_b_1()
        .border_color(rgb(line()))
        .child(
            div()
                .flex_1()
                .min_w(px(SETTINGS_COPY_MIN_WIDTH))
                .child(settings_copy(title, description)),
        )
        // Controls keep their width; in a narrow window they move below
        // the copy instead of clipping at the panel edge.
        .child(div().flex_none().ml_auto().child(control))
}

/// A label and value inside a detail card.
pub(crate) fn detail_row(label: &'static str, value: impl Into<String>) -> AnyElement {
    div()
        .py_3()
        .flex()
        .items_start()
        .gap_4()
        .border_b_1()
        .border_color(rgb(line()))
        .child(
            div()
                .w(px(92.0))
                .flex_none()
                .pt(px(baseline_offset(BODY, FOOTNOTE)))
                .text_footnote()
                .text_color(rgb(text_tertiary()))
                .child(label),
        )
        .child(
            div()
                .debug_selector(|| "detail-row-value".into())
                .flex_1()
                .min_w_0()
                .text_body()
                .text_color(rgb(text_secondary()))
                .child(value.into()),
        )
        .into_any_element()
}

pub(crate) fn listener_status(
    status: impl IntoElement,
    device: impl IntoElement,
    active: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(status_dot(if active { success() } else { text_disabled() }))
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_footnote()
                        .text_color(rgb(text_secondary()))
                        .child(status),
                )
                .child(
                    div()
                        .text_caption()
                        .text_color(rgb(text_tertiary()))
                        .child(device),
                ),
        )
}

/// A small filled circle that marks live or recorded state.
pub(crate) fn status_dot(color: u32) -> Div {
    div()
        .size(px(6.0))
        .flex_none()
        .rounded_full()
        .bg(rgb(color))
}

pub(crate) fn hotkey_keycaps(parts: Vec<String>, opacity: f32) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(3.0))
        .opacity(opacity)
        .children(parts.into_iter().map(|part| {
            let (side, label) = split_sided_keycap(&part);
            div()
                .min_w(px(KEYCAP_WIDTH))
                .h(px(CONTROL_HEIGHT_SMALL))
                .px_2()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_SMALL)
                .border_1()
                .border_color(rgb(line_strong()))
                .bg(rgb(surface_hover()))
                .text_footnote()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(text_primary()))
                .when_some(side, |keycap, side| {
                    keycap.child(
                        div()
                            .absolute()
                            .top(px(2.0))
                            .left(px(4.0))
                            .text_step(KEYCAP_SIDE_BADGE)
                            .text_color(rgb(text_tertiary()))
                            .child(side),
                    )
                })
                .child(label)
        }))
        .into_any_element()
}

/// The shortcut drawn large in onboarding art. `pressed` (0…1) sinks each
/// cap onto its base and lights its edge with the accent.
pub(crate) fn hero_keycaps(parts: Vec<String>, pressed: f32) -> Div {
    let pressed = pressed.clamp(0.0, 1.0);
    div()
        .flex()
        .items_end()
        .gap_2()
        .h(px(HERO_KEYCAP_SIZE + HERO_KEYCAP_DEPTH))
        .children(parts.into_iter().map(move |part| {
            let (_, label) = split_sided_keycap(&part);
            div()
                .min_w(px(HERO_KEYCAP_SIZE))
                .h(px(HERO_KEYCAP_SIZE + HERO_KEYCAP_DEPTH * (1.0 - pressed)))
                .px_4()
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_PANEL)
                .border_1()
                .border_b(px(1.0 + HERO_KEYCAP_DEPTH * (1.0 - pressed)))
                .border_color(mix_color(rgb(line_strong()), rgb(accent()), pressed))
                .bg(mix_color(
                    rgb(surface_hover()),
                    rgb(accent_surface()),
                    pressed,
                ))
                .text_title()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(text_primary()))
                .child(label)
        }))
}

/// A determinate or indeterminate progress track around its `indicator`.
pub(crate) fn progress_track(indicator: Div) -> Div {
    div()
        .flex_1()
        .h(px(PROGRESS_HEIGHT))
        .rounded(RADIUS_KNOB)
        .bg(rgb(canvas()))
        .child(indicator.h_full().rounded(RADIUS_KNOB).bg(rgb(accent())))
}

/// Where a multi-step flow is: one dot per step, the current one stretched.
pub(crate) fn step_dots(index: usize, count: usize) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .children((0..count).map(|step| {
            div()
                .h(px(STEP_DOT_SIZE))
                .w(px(if step == index {
                    STEP_DOT_ACTIVE_WIDTH
                } else {
                    STEP_DOT_SIZE
                }))
                .rounded_full()
                .bg(rgb(if step == index {
                    text_primary()
                } else {
                    line_strong()
                }))
        }))
}

fn split_sided_keycap(part: &str) -> (Option<&'static str>, String) {
    match part {
        "L⌃" => (Some("L"), "⌃".into()),
        "R⌃" => (Some("R"), "⌃".into()),
        "L⌥" => (Some("L"), "⌥".into()),
        "R⌥" => (Some("R"), "⌥".into()),
        "L⇧" => (Some("L"), "⇧".into()),
        "R⇧" => (Some("R"), "⇧".into()),
        "L⌘" => (Some("L"), "⌘".into()),
        "R⌘" => (Some("R"), "⌘".into()),
        _ => (None, part.to_string()),
    }
}

pub(crate) fn toggle(position: f32) -> AnyElement {
    let color_position = position.clamp(0.0, 1.0);
    let knob = TOGGLE_HEIGHT - 4.0;
    div()
        .w(px(TOGGLE_WIDTH))
        .h(px(TOGGLE_HEIGHT))
        .p(px(2.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(RADIUS_SMALL)
        .bg(mix_color(rgb(line_strong()), rgb(accent()), color_position))
        .child(
            div()
                .ml(px(
                    (TOGGLE_WIDTH - TOGGLE_HEIGHT) * position.clamp(-0.04, 1.04)
                ))
                .size(px(knob))
                .rounded(RADIUS_KNOB)
                .bg(mix_color(
                    rgb(text_secondary()),
                    rgb(on_accent()),
                    color_position,
                )),
        )
        .into_any_element()
}

/// The four button roles. Text buttons are always bordered or filled; only
/// icon buttons and menu items are borderless.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ButtonKind {
    /// The one main action in a view: accent fill.
    Primary,
    /// Every other action: recessed canvas fill with a line border.
    Secondary,
    /// Borderless; for [`icon_button`]s inside cards and rows only.
    Ghost,
    /// Destructive or armed confirmation: danger tint.
    Danger,
}

/// A [`CONTROL_HEIGHT`] button.
pub(crate) fn button(kind: ButtonKind, label: impl IntoElement) -> Div {
    button_frame(kind)
        .h(px(CONTROL_HEIGHT))
        .px_3()
        .text_footnote()
        .child(label)
}

/// A [`CONTROL_HEIGHT_SMALL`] button for actions inside list rows and cards.
pub(crate) fn small_button(kind: ButtonKind, label: impl IntoElement) -> Div {
    button_frame(kind)
        .h(px(CONTROL_HEIGHT_SMALL))
        .px_2()
        .text_caption()
        .child(label)
}

/// A square [`CONTROL_HEIGHT`] button holding one [`Icon`]. Use
/// [`ButtonKind::Ghost`] inside cards and rows, [`ButtonKind::Secondary`]
/// where the icon stands alone, such as a pane header.
pub(crate) fn icon_button(kind: ButtonKind, glyph: Icon) -> Div {
    let color = match kind {
        ButtonKind::Ghost => text_tertiary(),
        ButtonKind::Secondary => text_secondary(),
        ButtonKind::Primary => on_accent(),
        ButtonKind::Danger => danger(),
    };
    button_frame(kind)
        .size(px(CONTROL_HEIGHT))
        .justify_center()
        .child(icon(glyph, color))
}

fn button_frame(kind: ButtonKind) -> Div {
    let frame = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .rounded(RADIUS_CONTROL)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    match kind {
        ButtonKind::Primary => frame
            .bg(rgb(accent()))
            .text_color(rgb(on_accent()))
            .hover(|button| button.bg(mix_color(rgb(accent()), rgb(on_accent()), 0.12))),
        ButtonKind::Secondary => frame
            .border_1()
            .border_color(rgb(line()))
            .bg(rgb(canvas()))
            .text_color(rgb(text_secondary()))
            .hover(|button| {
                button
                    .bg(rgb(surface_hover()))
                    .text_color(rgb(text_primary()))
            }),
        ButtonKind::Ghost => frame.text_color(rgb(text_secondary())).hover(|button| {
            button
                .bg(rgb(surface_hover()))
                .text_color(rgb(text_primary()))
        }),
        ButtonKind::Danger => frame
            .border_1()
            .border_color(rgb(danger_border()))
            .bg(rgb(danger_surface()))
            .text_color(rgb(danger()))
            .hover(|button| button.border_color(rgb(danger()))),
    }
}

/// Text standing where a button will return: an in-progress action.
pub(crate) fn busy_label(label: impl IntoElement) -> Div {
    div()
        .h(px(CONTROL_HEIGHT))
        .px_3()
        .flex()
        .flex_none()
        .items_center()
        .text_footnote()
        .text_color(rgb(text_tertiary()))
        .child(label)
}

/// A dropdown trigger: a secondary control with its value and a chevron.
pub(crate) fn disclosure_button(label: impl Into<SharedString>) -> Div {
    disclosure_frame()
        .w(px(DISCLOSURE_WIDTH))
        .child(truncated_line(FOOTNOTE, label).flex_1())
        .child(icon(Icon::ChevronDown, text_tertiary()))
}

/// The frame of a dropdown trigger for custom content; end it with an
/// [`Icon::ChevronDown`].
pub(crate) fn disclosure_frame() -> Div {
    div()
        .h(px(CONTROL_HEIGHT))
        .px_3()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .rounded(RADIUS_CONTROL)
        .border_1()
        .border_color(rgb(line()))
        .bg(rgb(canvas()))
        .text_footnote()
        .text_color(rgb(text_secondary()))
        .cursor_pointer()
        .hover(|button| {
            button
                .bg(rgb(surface_hover()))
                .text_color(rgb(text_primary()))
        })
}

/// The tone of a [`tag`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    Neutral,
    Accent,
    Success,
    Danger,
}

/// A non-interactive status label: tinted fill, no border, never clickable.
pub(crate) fn tag(tone: Tone, label: impl IntoElement) -> Div {
    let (text, fill) = match tone {
        Tone::Neutral => (text_secondary(), surface_selected()),
        Tone::Accent => (accent_text(), accent_surface()),
        Tone::Success => (success(), success_surface()),
        Tone::Danger => (danger(), danger_surface()),
    };
    div()
        .h(px(TAG_HEIGHT))
        .px(px(6.0))
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .rounded(RADIUS_SMALL)
        .bg(rgb(fill))
        .text_caption()
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(text))
        .child(label)
}

/// A floating menu or popover surface. Callers position it.
pub(crate) fn menu() -> Div {
    div()
        .p_1()
        .flex()
        .flex_col()
        .rounded(RADIUS_PANEL)
        .border_1()
        .border_color(rgb(line_strong()))
        .bg(rgb(surface()))
        .shadow_lg()
        .occlude()
}

/// A [`menu`] whose entrance progress (`0.0..=1.0`) fades its opacity.
pub(crate) fn menu_with_progress(progress: f32) -> Div {
    menu().opacity(progress.clamp(0.0, 1.0))
}

/// A [`menu`] that drops [`MENU_OFFSET`] below the [`CONTROL_HEIGHT`] control
/// that opens it, with entrance progress (`0.0..=1.0`) fading and sliding
/// [`MENU_ENTER_OFFSET`] down from its trigger.
pub(crate) fn dropdown_menu_with_progress(progress: f32) -> Div {
    let progress = progress.clamp(0.0, 1.0);
    let offset = ((1.0 - progress) * MENU_ENTER_OFFSET * 64.0).round() / 64.0;
    menu_with_progress(progress)
        .absolute()
        .top(px(CONTROL_HEIGHT + MENU_OFFSET - offset))
}

/// One choice inside a [`menu`].
pub(crate) fn menu_item(selected: bool) -> Div {
    menu_item_frame()
        .text_color(rgb(if selected {
            text_primary()
        } else {
            text_secondary()
        }))
        .when(selected, |item| item.bg(rgb(surface_selected())))
        .hover(|item| item.bg(rgb(surface_hover())))
}

/// A destructive choice: last in its [`menu`], after a [`menu_separator`].
pub(crate) fn danger_menu_item() -> Div {
    menu_item_frame()
        .text_color(rgb(danger()))
        .hover(|item| item.bg(rgb(danger_surface())))
}

/// A choice that is temporarily unavailable, such as work in progress.
pub(crate) fn disabled_menu_item() -> Div {
    menu_item_frame().text_color(rgb(text_tertiary()))
}

/// A rule between groups of [`menu`] choices.
pub(crate) fn menu_separator() -> Div {
    div().mx_2().my_1().h(px(1.0)).flex_none().bg(rgb(line()))
}

fn menu_item_frame() -> Div {
    div()
        .w_full()
        .h(px(MENU_ITEM_HEIGHT))
        .px_2()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(RADIUS_CONTROL)
        .text_footnote()
}

pub(crate) const SOUND_VOLUME_STEPS: [(&str, f32); 5] = [
    ("Off", 0.0),
    ("25%", 0.25),
    ("50%", 0.5),
    ("75%", 0.75),
    ("100%", 1.0),
];

pub(crate) fn segmented_control() -> Div {
    div()
        .h(px(CONTROL_HEIGHT))
        .p(px(2.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(RADIUS_CONTROL)
        .border_1()
        .border_color(rgb(line()))
        .bg(rgb(canvas()))
}

const SEGMENT_HEIGHT: f32 = CONTROL_HEIGHT - 6.0;

pub(crate) fn segmented_item(selected: bool) -> Div {
    div()
        .h(px(SEGMENT_HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .rounded(RADIUS_SMALL)
        .text_caption()
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if selected {
            text_primary()
        } else {
            text_tertiary()
        }))
        .when(selected, |item| item.bg(rgb(surface_selected())))
        .when(!selected, |item| {
            item.hover(|item| {
                item.bg(rgb(surface_hover()))
                    .text_color(rgb(text_secondary()))
            })
        })
}

/// A [`segmented_control`] whose selection indicator slides between items.
/// `position` is a fractional item index and `widths` lists every item's
/// width in order; append the items with [`sliding_segmented_item`].
pub(crate) fn sliding_segmented_control(position: f32, widths: &[f32]) -> Div {
    let (left, width) = segmented_geometry(position, widths);
    sliding_segmented_rect(left, width)
}

/// A [`segmented_control`] whose selection indicator springs directly in pixel
/// space (`left` and `width`) so unequal item widths never cause velocity jumps
/// or intermediate width pumping.
pub(crate) fn sliding_segmented_for(
    window: &mut Window,
    key: &'static str,
    selected_index: usize,
    widths: &[f32],
) -> Div {
    use crate::motion::WindowMotion;
    let last = widths.len().saturating_sub(1);
    let idx = selected_index.min(last);
    let target_left = 2.0 + widths[..idx].iter().sum::<f32>();
    let target_width = widths.get(idx).copied().unwrap_or(0.0);
    let left = window.spring((key, 0u64), target_left, SPRING_SNAP);
    let width = window.spring((key, 1u64), target_width, SPRING_SNAP);
    sliding_segmented_rect(left, width)
}

fn sliding_segmented_rect(left: f32, width: f32) -> Div {
    let (left, width) = crate::motion::snap_edge_pair(left, left + width);
    segmented_control().relative().child(
        div()
            .absolute()
            .left(px(left))
            .top(px(2.0))
            .w(px(width))
            .h(px(SEGMENT_HEIGHT))
            .rounded(RADIUS_SMALL)
            .border_1()
            .border_color(rgb(line_strong()))
            .bg(rgb(surface_selected())),
    )
}

/// A fixed-width, centered item that stays transparent so the sliding
/// indicator beneath it shows through.
pub(crate) fn sliding_segmented_item(width: f32, selected: bool) -> Div {
    segmented_item(selected)
        .w(px(width))
        .px(px(0.0))
        .justify_center()
        .bg(gpui::transparent_black())
}

fn segmented_geometry(position: f32, widths: &[f32]) -> (f32, f32) {
    let last = widths.len().saturating_sub(1);
    let position = position.clamp(0.0, last as f32);
    let lower = (position.floor() as usize).min(last);
    let upper = (lower + 1).min(last);
    let progress = position - lower as f32;
    let left = |index: usize| 2.0 + widths[..index].iter().sum::<f32>();
    (
        left(lower) + (left(upper) - left(lower)) * progress,
        widths[lower] + (widths[upper] - widths[lower]) * progress,
    )
}

pub(crate) fn mix_color(from: Rgba, to: Rgba, position: f32) -> Rgba {
    let position = position.clamp(0.0, 1.0);
    Rgba {
        r: from.r + (to.r - from.r) * position,
        g: from.g + (to.g - from.g) * position,
        b: from.b + (to.b - from.b) * position,
        a: from.a + (to.a - from.a) * position,
    }
}

/// The one empty state: one centered footnote line. Detail panes centre it
/// vertically with [`detail_placeholder`].
pub(crate) fn empty_state(message: &'static str) -> AnyElement {
    div()
        .w_full()
        .p_6()
        .flex()
        .justify_center()
        .text_center()
        .text_footnote()
        .text_color(rgb(text_tertiary()))
        .child(message)
        .into_any_element()
}

/// An [`empty_state`] centred in a detail card.
pub(crate) fn detail_placeholder(message: &'static str) -> AnyElement {
    div()
        .flex_1()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(empty_state(message))
        .into_any_element()
}

pub(crate) fn error_message(message: &'static str, error: String) -> AnyElement {
    div()
        .p_4()
        .flex()
        .flex_col()
        .gap_1()
        .text_footnote()
        .text_color(rgb(danger()))
        .child(message)
        .child(
            div()
                .text_caption()
                .text_color(rgb(text_tertiary()))
                .child(error),
        )
        .into_any_element()
}

/// An [`error_message`] on a danger-tinted card.
pub(crate) fn error_panel(message: &'static str, error: String) -> Div {
    div()
        .w_full()
        .rounded(RADIUS_CONTROL)
        .border_1()
        .border_color(rgb(danger_border()))
        .bg(rgb(danger_surface()))
        .child(error_message(message, error))
}

/// A one-line inline error under a control or row.
pub(crate) fn inline_error(error: impl IntoElement) -> Div {
    div()
        .px_4()
        .py_2()
        .text_footnote()
        .text_color(rgb(danger()))
        .child(error)
}

#[cfg(test)]
mod tests {
    use super::{segmented_geometry, split_sided_keycap};

    #[test]
    fn side_badges_only_apply_to_sided_modifiers() {
        assert_eq!(split_sided_keycap("L⌥"), (Some("L"), "⌥".into()));
        assert_eq!(split_sided_keycap("R⌘"), (Some("R"), "⌘".into()));
        assert_eq!(split_sided_keycap("Return"), (None, "Return".into()));
    }

    #[test]
    fn segmented_indicator_interpolates_between_uneven_items() {
        let widths = [50.0, 90.0, 80.0];
        assert_eq!(segmented_geometry(0.0, &widths), (2.0, 50.0));
        assert_eq!(segmented_geometry(1.0, &widths), (52.0, 90.0));
        assert_eq!(segmented_geometry(2.0, &widths), (142.0, 80.0));
        assert_eq!(segmented_geometry(0.5, &widths), (27.0, 70.0));
        assert_eq!(segmented_geometry(-1.0, &widths), (2.0, 50.0));
        assert_eq!(segmented_geometry(5.0, &widths), (142.0, 80.0));
        assert_eq!(segmented_geometry(2.5, &[34.0; 5]), (87.0, 34.0));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod layout_tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, Window, size};

    struct SettingsRow;

    impl Render for SettingsRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w_full().child(
                settings_row(
                    "Microphone mode",
                    "Default: keeps the microphone open for the fastest start. A short pre-roll helps catch the beginning of speech. Audio is not saved by default.",
                    div()
                        .debug_selector(|| "settings-control".into())
                        .w(px(220.0))
                        .h(px(CONTROL_HEIGHT))
                        .flex_none(),
                )
                .debug_selector(|| "settings-row".into()),
            )
        }
    }

    #[gpui::test]
    fn settings_row_wraps_copy_without_displacing_fixed_control(cx: &mut TestAppContext) {
        // Real GPUI/Taffy layout with deterministic test-platform text metrics.
        let (_, cx) = cx.add_window_view(|_, _| SettingsRow);
        let mut heights = Vec::new();
        for width in [940.0, 756.0, 576.0] {
            cx.simulate_resize(size(px(width), px(400.0)));
            cx.run_until_parked();
            let row = cx.debug_bounds("settings-row").unwrap();
            let copy = cx.debug_bounds("settings-copy").unwrap();
            let control = cx.debug_bounds("settings-control").unwrap();
            assert_eq!(row.size.width, px(width));
            assert_eq!(control.size.width, px(220.0));
            assert_eq!(control.size.height, px(CONTROL_HEIGHT));
            assert!(
                control.right() <= row.right(),
                "{width}: {control:?} outside {row:?}"
            );
            assert!(copy.left() >= row.left());
            assert!(
                copy.right() + px(16.0) <= control.left(),
                "{width}: gap between {copy:?} and {control:?} is below 16px"
            );
            assert!(copy.top() >= row.top() && copy.bottom() <= row.bottom());
            heights.push((row.size.height, copy.size.height));
        }
        assert!(
            heights[2].0 > heights[0].0,
            "narrow row must grow: {heights:?}"
        );
        assert!(
            heights[2].1 > heights[0].1,
            "narrow copy must wrap: {heights:?}"
        );
    }

    const LONG: &str = "The parser needs a retry when the socket drops during the reconnect";

    struct Blank;

    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn fit_line_cuts_only_overflowing_text_with_an_ellipsis(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Blank);
        cx.update(|window, cx| {
            let cut = fit_line(LONG.into(), px(120.0), window, cx);
            assert!(cut.text.ends_with('…'), "not cut: {:?}", cut.text);
            assert!(
                cut.width <= px(120.0),
                "{:?} is wider than its box",
                cut.width
            );
            let whole = fit_line("Work notes".into(), px(400.0), window, cx);
            assert_eq!(whole.text.as_ref(), "Work notes");
        });
    }

    /// Renders `build` and reports the width of the element it marks `line`.
    struct Probe(fn() -> AnyElement);

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child((self.0)())
        }
    }

    fn measured_width(
        cx: &mut TestAppContext,
        selector: &'static str,
        build: fn() -> AnyElement,
    ) -> f32 {
        let (_, cx) = cx.add_window_view(move |_, _| Probe(build));
        cx.simulate_resize(size(px(1000.0), px(400.0)));
        cx.run_until_parked();
        f32::from(cx.debug_bounds(selector).unwrap().size.width)
    }

    fn line_width(cx: &mut TestAppContext, build: fn() -> AnyElement) -> f32 {
        measured_width(cx, "line", build)
    }

    fn line(step: TypeStep, text: &'static str) -> Div {
        truncated_line(step, text).debug_selector(|| "line".into())
    }

    #[gpui::test]
    fn truncated_line_takes_the_row_beside_a_timestamp(cx: &mut TestAppContext) {
        // History and Meetings list rows.
        let width = line_width(cx, || {
            pane_list(
                "probe-list",
                PANE_LIST_WIDTH,
                None,
                "",
                None,
                vec![
                    list_row(false)
                        .flex_row()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(line(BODY, LONG))
                                .child(list_row_meta("Dictation · Zed", false)),
                        )
                        .child(div().flex_none().child("1d ago"))
                        .into_any_element(),
                ],
            )
            .into_any_element()
        });
        assert!(width > 180.0 && width < PANE_LIST_WIDTH, "{width}");
    }

    #[gpui::test]
    fn detail_rows_fill_a_detail_body(cx: &mut TestAppContext) {
        let width = measured_width(cx, "detail-row-value", || {
            div()
                .w(px(600.0))
                .h(px(300.0))
                .flex()
                .flex_col()
                .child(detail_body("probe").child(detail_row("Latency", LONG)))
                .into_any_element()
        });
        assert!(width > 400.0, "{width}");
    }

    #[gpui::test]
    fn truncated_line_takes_the_row_beside_a_tag(cx: &mut TestAppContext) {
        // The meeting detail metadata row.
        let width = line_width(cx, || {
            div()
                .w(px(700.0))
                .px_6()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(tag(Tone::Success, "Ready"))
                        .child(line(FOOTNOTE, "Today · 8:18 PM · 38 min · Zoom").flex_1()),
                )
                .into_any_element()
        });
        assert!(width > 500.0, "{width}");
    }

    #[gpui::test]
    fn truncated_line_survives_intrinsic_sizing_of_block_ancestors(cx: &mut TestAppContext) {
        // A flex item's automatic minimum width sizes its block subtree to
        // content first: the transformation rows in Modes.
        let width = line_width(cx, || {
            div()
                .w(px(600.0))
                .flex()
                .child(
                    div().flex_1().child(
                        panel().child(
                            div()
                                .flex()
                                .items_center()
                                .child(line(BODY, "Smart punctuation").flex_1())
                                .child(icon_button(ButtonKind::Ghost, Icon::Close)),
                        ),
                    ),
                )
                .into_any_element()
        });
        assert!(width > 500.0, "{width}");
    }
}
