//! Shared desktop components. Every pane builds from these and the tokens in
//! [`crate::design_tokens`]; see `docs/design-system.md` for which component
//! does which job.

use gpui::{
    AnyElement, App, Bounds, Deferred, Div, Element, ElementId, FontWeight, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Position, Rgba, ShapedLine, SharedString,
    Stateful, Style, Window, canvas, deferred, div, point, prelude::*, px, rgb,
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
    sf_symbol(
        icon.sf_symbol(),
        ICON_SIZE,
        if selected {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        },
    )
}

/// An SF Symbol rasterized by AppKit at medium weight.
///
/// AppKit draws into a premultiplied bitmap while GPUI's sprite pipeline
/// expects straight alpha, so the coverage is divided back out here; without
/// it antialiased strokes are multiplied by their alpha twice and thin glyphs
/// all but vanish over translucent materials.
#[cfg(target_os = "macos")]
fn sf_symbol(name: &'static str, size: f32, color: u32) -> AnyElement {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    type Rendered = Option<(Arc<gpui::RenderImage>, u32, u32)>;
    type Cache = Mutex<HashMap<(&'static str, u32, u32), Rendered>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();

    let key = (name, size.to_bits(), color);
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let rendered = cache
        .entry(key)
        .or_insert_with(|| {
            let (width, height, mut data) = gpui_symbols::SfSymbol::new(name)
                .size(size)
                .color(color)
                .weight(gpui_symbols::SymbolWeight::Medium)
                .rendering_mode(gpui_symbols::RenderingMode::Monochrome)
                .render_rgba()?;
            for pixel in data.as_chunks_mut::<4>().0 {
                let alpha = pixel[3] as u32;
                if alpha > 0 && alpha < 255 {
                    for channel in &mut pixel[..3] {
                        *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                    }
                }
                pixel.swap(0, 2);
            }
            let frame = image::Frame::new(image::RgbaImage::from_raw(width, height, data)?);
            Some((Arc::new(gpui::RenderImage::new(vec![frame])), width, height))
        })
        .clone();
    drop(cache);

    let Some((image, width, height)) = rendered else {
        return gpui::Empty.into_any_element();
    };
    let aspect = width as f32 / height as f32;
    let (display_width, display_height) = if aspect >= 1.0 {
        (size, size / aspect)
    } else {
        (size * aspect, size)
    };
    gpui::img(gpui::ImageSource::Render(image))
        .w(px(display_width))
        .h(px(display_height))
        .into_any_element()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    let color = format!(
        "#{:06x}",
        if selected {
            TEXT_PRIMARY
        } else {
            TEXT_TERTIARY
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
        .child(sf_symbol(icon.sf_symbol(), size, color))
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
        .bg(rgb(CANVAS))
        .text_body()
        .text_color(rgb(TEXT_PRIMARY))
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
        .bg(rgb(SIDEBAR))
        .border_r_1()
        .border_color(rgb(LINE))
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
            TEXT_PRIMARY
        } else {
            TEXT_TERTIARY
        }))
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .when(!selected, |item| {
            item.hover(|item| item.bg(rgb(SIDEBAR_HOVER)).text_color(rgb(TEXT_SECONDARY)))
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
                .border_color(rgb(LINE))
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
        .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
        .when(!selected, |row| row.hover(|row| row.bg(rgb(SURFACE_HOVER))))
}

/// The title line of a [`list_row`].
pub(crate) fn list_row_title(text: impl Into<SharedString>, selected: bool) -> Div {
    truncated_line(BODY, text)
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(if selected {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        }))
}

/// The metadata line of a [`list_row`].
pub(crate) fn list_row_meta(text: impl Into<SharedString>, selected: bool) -> Div {
    truncated_line(CAPTION, text).text_color(rgb(if selected {
        TEXT_SECONDARY
    } else {
        TEXT_TERTIARY
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
            canvas(
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
        .border_color(rgb(LINE))
        .bg(rgb(SURFACE))
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
        .border_color(rgb(LINE))
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
        .text_color(rgb(TEXT_TERTIARY))
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
        .border_color(rgb(LINE))
        .child(title)
        .when(!meta.is_empty(), |header| {
            header.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_footnote()
                    .text_color(rgb(TEXT_TERTIARY))
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
                .text_color(rgb(TEXT_TERTIARY))
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
        .border_color(rgb(LINE))
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
        .border_color(rgb(LINE))
        .child(
            div()
                .w(px(92.0))
                .flex_none()
                .pt(px(baseline_offset(BODY, FOOTNOTE)))
                .text_footnote()
                .text_color(rgb(TEXT_TERTIARY))
                .child(label),
        )
        .child(
            div()
                .debug_selector(|| "detail-row-value".into())
                .flex_1()
                .min_w_0()
                .text_body()
                .text_color(rgb(TEXT_SECONDARY))
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
        .child(status_dot(if active { SUCCESS } else { TEXT_DISABLED }))
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_footnote()
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(status),
                )
                .child(
                    div()
                        .text_caption()
                        .text_color(rgb(TEXT_TERTIARY))
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
                .border_color(rgb(LINE_STRONG))
                .bg(rgb(SURFACE_HOVER))
                .text_footnote()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_PRIMARY))
                .when_some(side, |keycap, side| {
                    keycap.child(
                        div()
                            .absolute()
                            .top(px(2.0))
                            .left(px(4.0))
                            .text_step(KEYCAP_SIDE_BADGE)
                            .text_color(rgb(TEXT_TERTIARY))
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
                .border_color(mix_color(rgb(LINE_STRONG), rgb(ACCENT), pressed))
                .bg(mix_color(rgb(SURFACE_HOVER), rgb(ACCENT_SURFACE), pressed))
                .text_title()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_PRIMARY))
                .child(label)
        }))
}

/// A determinate or indeterminate progress track around its `indicator`.
pub(crate) fn progress_track(indicator: Div) -> Div {
    div()
        .flex_1()
        .h(px(PROGRESS_HEIGHT))
        .rounded(RADIUS_KNOB)
        .bg(rgb(CANVAS))
        .child(indicator.h_full().rounded(RADIUS_KNOB).bg(rgb(ACCENT)))
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
                    TEXT_PRIMARY
                } else {
                    LINE_STRONG
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
        .bg(mix_color(rgb(LINE_STRONG), rgb(ACCENT), color_position))
        .child(
            div()
                .ml(px(
                    (TOGGLE_WIDTH - TOGGLE_HEIGHT) * position.clamp(-0.04, 1.04)
                ))
                .size(px(knob))
                .rounded(RADIUS_KNOB)
                .bg(mix_color(
                    rgb(TEXT_SECONDARY),
                    rgb(TEXT_PRIMARY),
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
        ButtonKind::Ghost => TEXT_TERTIARY,
        ButtonKind::Secondary => TEXT_SECONDARY,
        ButtonKind::Primary => TEXT_PRIMARY,
        ButtonKind::Danger => DANGER,
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
            .bg(rgb(ACCENT))
            .text_color(rgb(TEXT_PRIMARY))
            .hover(|button| button.bg(mix_color(rgb(ACCENT), rgb(TEXT_PRIMARY), 0.12))),
        ButtonKind::Secondary => frame
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT_SECONDARY))
            .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_PRIMARY))),
        ButtonKind::Ghost => frame
            .text_color(rgb(TEXT_SECONDARY))
            .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_PRIMARY))),
        ButtonKind::Danger => frame
            .border_1()
            .border_color(rgb(DANGER_BORDER))
            .bg(rgb(DANGER_SURFACE))
            .text_color(rgb(DANGER))
            .hover(|button| button.border_color(rgb(DANGER))),
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
        .text_color(rgb(TEXT_TERTIARY))
        .child(label)
}

/// A dropdown trigger: a secondary control with its value and a chevron.
pub(crate) fn disclosure_button(label: impl Into<SharedString>) -> Div {
    disclosure_frame()
        .w(px(DISCLOSURE_WIDTH))
        .child(truncated_line(FOOTNOTE, label).flex_1())
        .child(icon(Icon::ChevronDown, TEXT_TERTIARY))
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
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
        .text_footnote()
        .text_color(rgb(TEXT_SECONDARY))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_PRIMARY)))
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
        Tone::Neutral => (TEXT_SECONDARY, SURFACE_SELECTED),
        Tone::Accent => (ACCENT_TEXT, ACCENT_SURFACE),
        Tone::Success => (SUCCESS, SUCCESS_SURFACE),
        Tone::Danger => (DANGER, DANGER_SURFACE),
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
        .border_color(rgb(LINE_STRONG))
        .bg(rgb(SURFACE))
        .shadow_lg()
        .occlude()
}

/// A [`menu`] whose entrance progress (`0.0..=1.0`) fades its opacity.
pub(crate) fn menu_with_progress(progress: f32) -> Div {
    menu().opacity(progress.clamp(0.0, 1.0))
}

/// Which edge of its trigger a [`dropdown`] lines up with when it fits.
#[derive(Clone, Copy)]
pub(crate) enum DropdownAlign {
    Left,
    Right,
}

/// Floats a [`menu`] [`MENU_OFFSET`] below the [`CONTROL_HEIGHT`] control
/// that opens it, painted over the rest of the window. Without room it opens
/// above the control or lines up with the control's other edge, and otherwise
/// stays inside the window. Entrance progress (`0.0..=1.0`) fades it in and
/// slides it [`MENU_ENTER_OFFSET`] away from the control. The control's parent
/// must be `relative()`.
pub(crate) fn dropdown(align: DropdownAlign, progress: f32, menu: impl IntoElement) -> Deferred {
    let progress = progress.clamp(0.0, 1.0);
    deferred(DropdownPlacement {
        align,
        slide: px(((1.0 - progress) * MENU_ENTER_OFFSET * 64.0).round() / 64.0),
        menu: div()
            .absolute()
            .opacity(progress)
            .child(menu)
            .into_any_element(),
    })
}

/// Lays out over the [`CONTROL_HEIGHT`] strip of the trigger's parent and
/// moves its menu to the first side of that strip where it fits.
struct DropdownPlacement {
    align: DropdownAlign,
    slide: Pixels,
    menu: AnyElement,
}

impl IntoElement for DropdownPlacement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DropdownPlacement {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let menu = self.menu.request_layout(window, cx);
        let mut style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.0).into();
        style.inset.left = px(0.0).into();
        style.inset.right = px(0.0).into();
        style.size.height = px(CONTROL_HEIGHT).into();
        (window.request_layout(style, [menu], cx), menu)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        trigger: Bounds<Pixels>,
        menu: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let menu = window.layout_bounds(*menu);
        let size = menu.size;
        let inset = window.client_inset().unwrap_or_default();
        let viewport = window.viewport_size();
        let left = trigger.left();
        let right = trigger.right() - size.width;
        let (preferred_x, fallback_x) = match self.align {
            DropdownAlign::Left => (left, right),
            DropdownAlign::Right => (right, left),
        };
        let x = place_on_axis(
            preferred_x,
            fallback_x,
            size.width,
            inset,
            viewport.width - inset,
        );
        let below = trigger.bottom() + px(MENU_OFFSET);
        let above = trigger.top() - px(MENU_OFFSET) - size.height;
        let y = place_on_axis(below, above, size.height, inset, viewport.height - inset);
        let slide = if y < trigger.top() {
            self.slide
        } else {
            -self.slide
        };
        let offset = point(x, y + slide) - menu.origin;
        window.with_element_offset(point(offset.x.round(), offset.y.round()), |window| {
            self.menu.prepaint(window, cx)
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.menu.paint(window, cx);
    }
}

/// Where a `size`-long span starts on one axis: at `preferred` when it fits
/// within `min..max`, else at `fallback` when that fits, else shifted inside.
fn place_on_axis(
    preferred: Pixels,
    fallback: Pixels,
    size: Pixels,
    min: Pixels,
    max: Pixels,
) -> Pixels {
    let fits = |start: Pixels| start >= min && start + size <= max;
    if fits(preferred) {
        preferred
    } else if fits(fallback) {
        fallback
    } else if preferred + size > max && max - size > min {
        max - size
    } else {
        min
    }
}

/// One choice inside a [`menu`].
pub(crate) fn menu_item(selected: bool) -> Div {
    menu_item_frame()
        .text_color(rgb(if selected {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        }))
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .hover(|item| item.bg(rgb(SURFACE_HOVER)))
}

/// A destructive choice: last in its [`menu`], after a [`menu_separator`].
pub(crate) fn danger_menu_item() -> Div {
    menu_item_frame()
        .text_color(rgb(DANGER))
        .hover(|item| item.bg(rgb(DANGER_SURFACE)))
}

/// A choice that is temporarily unavailable, such as work in progress.
pub(crate) fn disabled_menu_item() -> Div {
    menu_item_frame().text_color(rgb(TEXT_TERTIARY))
}

/// A rule between groups of [`menu`] choices.
pub(crate) fn menu_separator() -> Div {
    div().mx_2().my_1().h(px(1.0)).flex_none().bg(rgb(LINE))
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
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
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
            TEXT_PRIMARY
        } else {
            TEXT_TERTIARY
        }))
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .when(!selected, |item| {
            item.hover(|item| item.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SECONDARY)))
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
            .border_color(rgb(LINE_STRONG))
            .bg(rgb(SURFACE_SELECTED)),
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
        .text_color(rgb(TEXT_TERTIARY))
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
        .text_color(rgb(DANGER))
        .child(message)
        .child(
            div()
                .text_caption()
                .text_color(rgb(TEXT_TERTIARY))
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
        .border_color(rgb(DANGER_BORDER))
        .bg(rgb(DANGER_SURFACE))
        .child(error_message(message, error))
}

/// A one-line inline error under a control or row.
pub(crate) fn inline_error(error: impl IntoElement) -> Div {
    div()
        .px_4()
        .py_2()
        .text_footnote()
        .text_color(rgb(DANGER))
        .child(error)
}

#[cfg(test)]
mod tests {
    use super::{
        ButtonKind, DropdownAlign, Icon, dropdown, icon_button, menu, place_on_axis,
        segmented_geometry, split_sided_keycap,
    };
    use crate::design_tokens::{CONTROL_HEIGHT, MENU_OFFSET, MENU_WIDTH};
    use gpui::{Context, IntoElement, Pixels, Point, Render, Window, div, point, prelude::*, px};

    #[test]
    fn dropdown_axis_prefers_its_side_then_the_other_then_stays_inside() {
        let place = |preferred: f32, fallback: f32, size: f32| {
            place_on_axis(px(preferred), px(fallback), px(size), px(0.0), px(300.0))
        };
        assert_eq!(place(150.0, 20.0, 100.0), px(150.0));
        assert_eq!(place(250.0, 20.0, 100.0), px(20.0));
        assert_eq!(place(-40.0, 120.0, 100.0), px(120.0));
        assert_eq!(place(250.0, -60.0, 100.0), px(200.0));
        assert_eq!(place(-40.0, 250.0, 100.0), px(0.0));
        assert_eq!(place(10.0, -500.0, 400.0), px(0.0));
    }

    #[gpui::test]
    fn dropdown_opens_on_the_sides_of_its_trigger_with_room(cx: &mut gpui::TestAppContext) {
        struct Corner {
            align: DropdownAlign,
            origin: Point<Pixels>,
            progress: f32,
        }

        impl Render for Corner {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(
                    div()
                        .absolute()
                        .left(self.origin.x)
                        .top(self.origin.y)
                        .child(
                            icon_button(ButtonKind::Secondary, Icon::Ellipsis)
                                .id("trigger")
                                .debug_selector(|| "trigger".into()),
                        )
                        .child(dropdown(
                            self.align,
                            self.progress,
                            menu()
                                .debug_selector(|| "menu".into())
                                .w(px(MENU_WIDTH))
                                .h(px(120.0)),
                        )),
                )
            }
        }

        let (view, cx) = cx.add_window_view(|_, _| Corner {
            align: DropdownAlign::Left,
            origin: point(px(0.0), px(0.0)),
            progress: 1.0,
        });
        let viewport = cx.update(|window, _| window.viewport_size());
        let near = px(40.0);
        let far = point(
            viewport.width - px(CONTROL_HEIGHT) - near,
            viewport.height - px(CONTROL_HEIGHT) - near,
        );
        let mut place = |align, origin, progress| {
            view.update(cx, |view, cx| {
                *view = Corner {
                    align,
                    origin,
                    progress,
                };
                cx.notify();
            });
            cx.run_until_parked();
            (
                cx.debug_bounds("trigger").unwrap(),
                cx.debug_bounds("menu").unwrap(),
            )
        };

        let (trigger, menu) = place(DropdownAlign::Left, point(px(0.0), px(0.0)), 1.0);
        assert_eq!(menu.left(), trigger.left());
        assert_eq!(menu.top(), trigger.bottom() + px(MENU_OFFSET));

        let (trigger, menu) = place(DropdownAlign::Left, point(far.x, px(0.0)), 1.0);
        assert_eq!(menu.right(), trigger.right(), "no room on the right");
        assert_eq!(menu.top(), trigger.bottom() + px(MENU_OFFSET));

        let (trigger, menu) = place(DropdownAlign::Right, point(near, far.y), 1.0);
        assert_eq!(menu.left(), trigger.left(), "no room on the left");
        assert_eq!(
            menu.bottom(),
            trigger.top() - px(MENU_OFFSET),
            "no room below"
        );

        for progress in [0.0, 0.5, 1.0] {
            let (trigger, menu) = place(DropdownAlign::Left, far, progress);
            assert_eq!(menu.right(), trigger.right(), "progress {progress}");
            assert!(menu.bottom() <= trigger.top(), "progress {progress}");
        }
        let (trigger, menu) = place(DropdownAlign::Left, far, 1.0);
        assert_eq!(menu.bottom(), trigger.top() - px(MENU_OFFSET));
    }

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
