use gpui::{AnyElement, Div, FontWeight, IntoElement, Rgba, div, prelude::*, px, rgb};

#[cfg(target_os = "linux")]
use gpui::{Image, ImageFormat, img};
#[cfg(target_os = "linux")]
use std::sync::Arc;

#[derive(Clone, Copy)]
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) enum NavigationIcon {
    Activity,
    Commands,
    History,
    HudLab,
    Meetings,
    Modes,
    Settings,
    VoiceAction,
}

impl NavigationIcon {
    #[cfg(target_os = "macos")]
    fn sf_symbol(self) -> &'static str {
        match self {
            Self::Settings => "slider.horizontal.3",
            Self::Modes => "square.grid.2x2.fill",
            Self::VoiceAction => "sparkle",
            Self::Commands => "command",
            Self::History => "clock.fill",
            Self::Meetings => "calendar",
            Self::Activity => "chart.bar",
            Self::HudLab => "flask",
        }
    }

    #[cfg(target_os = "windows")]
    fn fluent_glyph(self) -> &'static str {
        match self {
            Self::Settings => "\u{E713}",
            Self::Modes => "\u{E71D}",
            Self::VoiceAction => "\u{E735}",
            Self::Commands => "\u{E756}",
            Self::History => "\u{E81C}",
            Self::Meetings => "\u{E787}",
            Self::Activity => "\u{E9D2}",
            Self::HudLab => "\u{E9F5}",
        }
    }

    #[cfg(target_os = "linux")]
    fn heroicon(self) -> &'static str {
        match self {
            Self::Settings => include_str!("../../assets/icons/settings.svg"),
            Self::Modes => include_str!("../../assets/icons/modes.svg"),
            Self::VoiceAction => include_str!("../../assets/icons/voice-action.svg"),
            Self::Commands => include_str!("../../assets/icons/commands.svg"),
            Self::History => include_str!("../../assets/icons/history.svg"),
            Self::Meetings => include_str!("../../assets/icons/meetings.svg"),
            Self::Activity => include_str!("../../assets/icons/activity.svg"),
            Self::HudLab => include_str!("../../assets/icons/hud-lab.svg"),
        }
    }
}

#[cfg(target_os = "macos")]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    gpui_symbols::Icon::new(icon.sf_symbol())
        .size(px(12.0))
        .color(rgb(if selected { TEXT } else { TEXT_SOFT }))
        .weight(gpui_symbols::SymbolWeight::Semibold)
        .rendering_mode(gpui_symbols::RenderingMode::Monochrome)
        .into_any_element()
}

#[cfg(target_os = "macos")]
fn disclosure_chevron() -> AnyElement {
    gpui_symbols::Icon::new("chevron.down")
        .size(px(10.0))
        .color(rgb(MUTED))
        .weight(gpui_symbols::SymbolWeight::Medium)
        .rendering_mode(gpui_symbols::RenderingMode::Monochrome)
        .into_any_element()
}

#[cfg(target_os = "macos")]
fn plus_icon() -> AnyElement {
    gpui_symbols::Icon::new("plus")
        .size(px(11.0))
        .color(rgb(TEXT_SOFT))
        .weight(gpui_symbols::SymbolWeight::Semibold)
        .rendering_mode(gpui_symbols::RenderingMode::Monochrome)
        .into_any_element()
}

#[cfg(target_os = "linux")]
fn disclosure_chevron() -> AnyElement {
    div()
        .size(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.0))
        .text_color(rgb(MUTED))
        .child("⌄")
        .into_any_element()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn plus_icon() -> AnyElement {
    div()
        .size(px(11.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.0))
        .text_color(rgb(TEXT_SOFT))
        .child("+")
        .into_any_element()
}

#[cfg(target_os = "windows")]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    crate::windows_ui::fluent_icon(
        icon.fluent_glyph(),
        16.0,
        if selected { TEXT } else { TEXT_SOFT },
    )
}

#[cfg(target_os = "linux")]
fn navigation_icon(icon: NavigationIcon, selected: bool) -> AnyElement {
    let color = if selected { "#eeeeee" } else { "#858585" };
    let image = Image::from_bytes(
        ImageFormat::Svg,
        icon.heroicon().replace("currentColor", color).into_bytes(),
    );
    img(Arc::new(image)).size(px(16.0)).into_any_element()
}

#[allow(dead_code)]
pub(crate) const SIDEBAR_WIDTH: f32 = 220.0;

// Shared visual tokens. Windows flattens the Fluent 2 dark palette into these
// same names so every shared control re-themes without forking its body;
// macOS and Linux keep the original values.
#[cfg(not(target_os = "windows"))]
mod tokens {
    pub(crate) const CANVAS: u32 = 0x111111;
    pub(crate) const SIDEBAR: u32 = 0x202020;
    pub(crate) const SURFACE: u32 = 0x171717;
    pub(crate) const SURFACE_HOVER: u32 = 0x1d1d1d;
    pub(crate) const SURFACE_SELECTED: u32 = 0x3a3a3a;
    pub(crate) const LINE: u32 = 0x292929;
    pub(crate) const DIVIDER: u32 = 0x292929;
    pub(crate) const ACCENT: u32 = 0x3b5cf6;
    pub(crate) const TEXT: u32 = 0xeeeeee;
    pub(crate) const TEXT_ON_ACCENT: u32 = 0xeeeeee;
    pub(crate) const TEXT_SOFT: u32 = 0xb8b8b8;
    pub(crate) const MUTED: u32 = 0x858585;
    pub(crate) const FAINT: u32 = 0x626262;
    pub(crate) const NEGATIVE: u32 = 0xc98f89;
    pub(crate) const OVERLAY_SMOKE: u32 = 0x000000bb;
    pub(crate) const OVERLAY_PANEL: u32 = 0x111111;
    pub(crate) const OVERLAY_RADIUS: f32 = 12.0;
    pub(crate) const OVERLAY_STROKE: u32 = 0x292929;
    pub(crate) const OVERLAY_TOP_INSET: f32 = 0.0;
    pub(crate) const PANEL_RADIUS: f32 = 10.0;
}
#[cfg(target_os = "windows")]
mod tokens {
    pub(crate) const CANVAS: u32 = 0x202020;
    pub(crate) const SIDEBAR: u32 = 0x202020;
    pub(crate) const SURFACE: u32 = 0x2b2b2b;
    pub(crate) const SURFACE_HOVER: u32 = 0x2f2f2f;
    pub(crate) const SURFACE_SELECTED: u32 = 0x2d2d2d;
    pub(crate) const LINE: u32 = 0x303030;
    pub(crate) const DIVIDER: u32 = 0x1f1f1f;
    /// Static fallback only; interactive accents resolve through
    /// [`super::accent_color`] to the user's system accent.
    #[allow(dead_code)]
    pub(crate) const ACCENT: u32 = 0x4cc2ff;
    pub(crate) const TEXT: u32 = 0xffffff;
    pub(crate) const TEXT_ON_ACCENT: u32 = 0x000000;
    pub(crate) const TEXT_SOFT: u32 = 0xcfcfcf;
    pub(crate) const MUTED: u32 = 0x9e9e9e;
    pub(crate) const FAINT: u32 = 0x8f8f8f;
    pub(crate) const NEGATIVE: u32 = 0xff99a4;
    pub(crate) const OVERLAY_SMOKE: u32 = 0x0000004d;
    pub(crate) const OVERLAY_PANEL: u32 = 0x2c2c2c;
    pub(crate) const OVERLAY_RADIUS: f32 = 8.0;
    pub(crate) const OVERLAY_STROKE: u32 = 0x3a3a3a;
    // Keep modal backdrops below the custom caption bar so the window can
    // still be dragged, minimized, and closed while a dialog is open.
    pub(crate) const OVERLAY_TOP_INSET: f32 = crate::windows_ui::CAPTION_HEIGHT;
    pub(crate) const PANEL_RADIUS: f32 = 7.0;
}
#[allow(unused_imports)]
pub(crate) use tokens::{
    ACCENT, CANVAS, DIVIDER, FAINT, LINE, MUTED, NEGATIVE, OVERLAY_PANEL, OVERLAY_RADIUS,
    OVERLAY_SMOKE, OVERLAY_STROKE, OVERLAY_TOP_INSET, PANEL_RADIUS, SIDEBAR, SURFACE,
    SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_ON_ACCENT, TEXT_SOFT,
};
const SEGMENTED_ITEM_HOVER: u32 = 0x262626;

/// The interactive accent: the user's Windows accent color on Windows, the
/// compiled [`ACCENT`] everywhere else.
pub(crate) fn accent_color() -> u32 {
    #[cfg(target_os = "windows")]
    {
        crate::windows_ui::accent()
    }
    #[cfg(not(target_os = "windows"))]
    {
        ACCENT
    }
}

pub(crate) const CONTROL_HEIGHT: f32 = 32.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const TEXT_INPUT_HEIGHT: f32 = 34.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const MULTILINE_INPUT_HEIGHT: f32 = 132.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const COMPACT_MULTILINE_INPUT_HEIGHT: f32 = 76.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const COMPACT_PANEL_HEADER_HEIGHT: f32 = 38.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const SECTION_GAP: f32 = 8.0;

pub(crate) fn window_frame() -> Div {
    let frame = div()
        .size_full()
        .relative()
        .overflow_hidden()
        .flex()
        .bg(rgb(CANVAS))
        .text_color(rgb(TEXT));
    #[cfg(target_os = "windows")]
    let frame = frame.font_family(crate::windows_ui::FONT_UI);
    frame
}

pub(crate) fn sidebar_frame() -> Div {
    let frame = div().h_full().flex_none().bg(rgb(SIDEBAR));
    #[cfg(not(target_os = "windows"))]
    let frame = frame.border_r_1().border_color(rgb(LINE));
    frame
}

/// Windows renders the Fluent navigation item: subtle fills, a 16px Segoe
/// Fluent glyph, and the accent selection pill on the left edge.
#[cfg(target_os = "windows")]
pub(crate) fn navigation_item(icon: NavigationIcon, selected: bool) -> Div {
    div()
        .h(px(36.0))
        .my(px(2.0))
        .pl(px(1.0))
        .pr_3()
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(4.0))
        .text_size(px(14.0))
        .text_color(if selected { rgb(TEXT) } else { rgb(TEXT_SOFT) })
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .hover(|item| item.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT)))
        .child(crate::windows_ui::selection_pill(selected))
        .child(
            div()
                .w(px(28.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(navigation_icon(icon, selected)),
        )
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub(crate) fn navigation_item(icon: NavigationIcon, selected: bool) -> Div {
    div()
        .h(px(38.0))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(6.0))
        .text_size(px(13.0))
        .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .hover(|item| item.bg(rgb(0x2a2a2a)).text_color(rgb(TEXT_SOFT)))
        .child(
            div()
                .size(px(22.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .border_1()
                .border_color(rgb(if selected { 0x606060 } else { 0x3a3a3a }))
                .bg(rgb(if selected { 0x494949 } else { 0x2b2b2b }))
                .child(navigation_icon(icon, selected)),
        )
}

/// The one content width every pane is bounded to. Headers and bodies share
/// it; panes must not introduce their own content widths.
pub(crate) const PANE_CONTENT_WIDTH: f32 = 940.0;

/// The one fixed list-column width every list+detail pane uses.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const PANE_LIST_WIDTH: f32 = 320.0;

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn pane_header(title: &'static str) -> AnyElement {
    pane_header_with_action(title, None)
}

/// The one pane header: title left, optional action right, bounded to
/// [`PANE_CONTENT_WIDTH`] like the body below it.
pub(crate) fn pane_header_with_action(
    title: &'static str,
    action: Option<AnyElement>,
) -> AnyElement {
    #[cfg(target_os = "windows")]
    let title = crate::windows_i18n::tr(title);
    div()
        .h(px(70.0))
        .px_8()
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
                .map(|header| {
                    #[cfg(not(target_os = "windows"))]
                    {
                        header.border_b_1().border_color(rgb(LINE))
                    }
                    #[cfg(target_os = "windows")]
                    {
                        header
                    }
                })
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .when_some(action, |header, action| header.child(action)),
        )
        .into_any_element()
}

/// The one pane body: fills the space under the header and centers its
/// children. Put the pane's content column inside [`pane_content`].
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn pane_body() -> Div {
    div()
        .flex_1()
        .min_h(px(0.0))
        .overflow_hidden()
        .flex()
        .justify_center()
}

/// The one pane-header action button: a bordered 30-point chip. Every
/// clickable header action renders this.
#[cfg(target_os = "windows")]
pub(crate) fn header_button(label: impl IntoElement) -> Div {
    div()
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(rgb(crate::windows_ui::CONTROL_STROKE))
        .bg(rgb(crate::windows_ui::CONTROL_FILL))
        .text_size(px(13.0))
        .text_color(rgb(TEXT))
        .hover(|button| button.bg(rgb(crate::windows_ui::CONTROL_FILL_HOVER)))
        .child(label)
}

#[cfg(not(target_os = "windows"))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn header_button(label: impl IntoElement) -> Div {
    div()
        .h(px(30.0))
        .px_3()
        .flex()
        .items_center()
        .rounded_sm()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(SURFACE))
        .text_size(px(12.0))
        .text_color(rgb(TEXT_SOFT))
        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT)))
        .child(label)
}

/// The one pane content column, bounded to [`PANE_CONTENT_WIDTH`].
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn pane_content() -> Div {
    div()
        .w_full()
        .max_w(px(PANE_CONTENT_WIDTH))
        .min_h(px(0.0))
        .flex()
        .flex_col()
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn section_label(label: &'static str) -> AnyElement {
    #[cfg(target_os = "windows")]
    let label = crate::windows_i18n::tr(label);
    #[cfg(target_os = "windows")]
    const LABEL: (f32, u32) = (12.0, MUTED);
    #[cfg(not(target_os = "windows"))]
    const LABEL: (f32, u32) = (11.0, FAINT);
    div()
        .text_size(px(LABEL.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(LABEL.1))
        .child(label)
        .into_any_element()
}

#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) fn listener_status(
    status: impl IntoElement,
    device: impl IntoElement,
    active: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child({
            #[cfg(target_os = "windows")]
            const ACTIVE_DOT: u32 = 0x6ccb5f;
            #[cfg(not(target_os = "windows"))]
            const ACTIVE_DOT: u32 = 0x69d89f;
            div()
                .size(px(8.0))
                .flex_none()
                .rounded_full()
                .bg(if active { rgb(ACTIVE_DOT) } else { rgb(FAINT) })
        })
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT_SOFT))
                        .child(status),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(FAINT))
                        .child(device),
                ),
        )
}

pub(crate) fn hotkey_keycaps(parts: Vec<String>, opacity: f32) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(3.0))
        .opacity(opacity)
        .children(parts.into_iter().map(|part| {
            let (side, label) = split_sided_keycap(&part);
            #[cfg(target_os = "windows")]
            const KEYCAP: (u32, u32, f32) = (0x383838, 0x2d2d2d, 4.0);
            #[cfg(not(target_os = "windows"))]
            const KEYCAP: (u32, u32, f32) = (0x444444, 0x2b2b2b, 5.0);
            div()
                .min_w(px(34.0))
                .h(px(24.0))
                .px_2()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(KEYCAP.2))
                .border_1()
                .border_color(rgb(KEYCAP.0))
                .bg(rgb(KEYCAP.1))
                .text_size(px(12.0))
                .font_weight(FontWeight::NORMAL)
                .text_color(rgb(TEXT))
                .when_some(side, |keycap, side| {
                    keycap.child(
                        div()
                            .absolute()
                            .top(px(2.0))
                            .left(px(4.0))
                            .text_size(px(7.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(FAINT))
                            .child(side),
                    )
                })
                .child(label)
        }))
        .into_any_element()
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

/// Windows renders the Fluent switch: a 40×20 pill that fills with the
/// system accent when on, with a dark thumb on accent per the dark theme.
#[cfg(target_os = "windows")]
pub(crate) fn toggle(position: f32) -> AnyElement {
    let position = position.clamp(0.0, 1.0);
    let on = position > 0.5;
    div()
        .w(px(40.0))
        .h(px(20.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(10.0))
        .map(|track| {
            if on {
                track.bg(rgb(accent_color()))
            } else {
                track.border_1().border_color(rgb(0x9e9e9e))
            }
        })
        .child(
            div()
                .ml(px(if on { 23.0 } else { 4.0 }))
                .size(px(12.0))
                .rounded_full()
                .bg(rgb(if on { 0x000000 } else { 0xcfcfcf })),
        )
        .into_any_element()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn toggle(position: f32) -> AnyElement {
    let color_position = position.clamp(0.0, 1.0);
    div()
        .w(px(24.0))
        .h(px(16.0))
        .p(px(2.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(4.0))
        .bg(mix_color(rgb(0x3a3a3a), rgb(ACCENT), color_position))
        .child(
            div()
                .ml(px(8.0 * position.clamp(-0.04, 1.04)))
                .size(px(12.0))
                .rounded(px(2.0))
                .bg(mix_color(rgb(0xc8c8c8), rgb(0xfafafa), color_position)),
        )
        .into_any_element()
}

#[cfg(target_os = "windows")]
pub(crate) fn settings_section_label(label: &'static str) -> AnyElement {
    let label = crate::windows_i18n::tr(label);
    div()
        .pt(px(24.0))
        .pb_2()
        .px_1()
        .text_size(px(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT))
        .child(label)
        .into_any_element()
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub(crate) fn settings_section_label(label: &'static str) -> AnyElement {
    div()
        .pt_5()
        .pb_2()
        .px_1()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(FAINT))
        .child(label)
        .into_any_element()
}

#[cfg(target_os = "windows")]
pub(crate) fn settings_copy(title: &'static str, description: &'static str) -> AnyElement {
    let title = crate::windows_i18n::tr(title);
    let description = crate::windows_i18n::tr(description);
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(div().text_size(px(14.0)).text_color(rgb(TEXT)).child(title))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(description),
        )
        .into_any_element()
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub(crate) fn settings_copy(title: &'static str, description: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(description),
        )
        .into_any_element()
}

#[cfg(target_os = "windows")]
pub(crate) fn compact_button(label: impl IntoElement) -> Div {
    div()
        .h(px(30.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(4.0))
        .text_size(px(13.0))
        .text_color(rgb(TEXT_SOFT))
        .hover(|button| button.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT)))
        .child(label)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn compact_button(label: impl IntoElement) -> Div {
    div()
        .h(px(30.0))
        .px_3()
        .flex()
        .items_center()
        .rounded_sm()
        .text_size(px(12.0))
        .text_color(rgb(TEXT_SOFT))
        .hover(|button| button.bg(rgb(SURFACE_HOVER)))
        .child(label)
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn compact_plus_button() -> Div {
    compact_button(plus_icon())
        .size(px(28.0))
        .px_0()
        .justify_center()
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn compact_header_plus_button() -> Div {
    compact_plus_button().mr(px(-7.0))
}

pub(crate) fn compact_panel() -> Div {
    div()
        .w_full()
        .rounded(px(PANEL_RADIUS))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(SURFACE))
        .overflow_hidden()
}

#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) fn compact_panel_header(title: impl IntoElement, action: Option<AnyElement>) -> Div {
    div()
        .h(px(COMPACT_PANEL_HEADER_HEIGHT))
        .px_3()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(title),
        )
        .when_some(action, |header, action| header.child(action))
}

#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) fn compact_section_label(label: impl IntoElement) -> Div {
    div()
        .px_1()
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(FAINT))
        .child(label)
}

// The Windows disclosure_button fork is gone: Windows comboboxes come from
// crate::ui::combobox (gpui-component). The not(windows) fork below follows
// once the macOS and Linux shells adopt the same component.
#[cfg(not(target_os = "windows"))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn disclosure_button(label: impl IntoElement) -> Div {
    div()
        .w(px(220.0))
        .h(px(CONTROL_HEIGHT))
        .px_3()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
        .text_size(px(11.0))
        .text_color(rgb(TEXT_SOFT))
        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT)))
        .child(div().min_w(px(0.0)).flex_1().truncate().child(label))
        .child(
            div()
                .size(px(10.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(disclosure_chevron()),
        )
}

#[allow(dead_code)]
pub(crate) fn settings_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
) -> Div {
    div()
        .w_full()
        .min_h(px(72.0))
        .px_4()
        .py_3()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(rgb(DIVIDER))
        .child(settings_copy(title, description))
        .child(control)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn settings_panel() -> Div {
    compact_panel().relative()
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn segmented_control() -> Div {
    #[cfg(target_os = "windows")]
    const RADIUS: f32 = 4.0;
    #[cfg(not(target_os = "windows"))]
    const RADIUS: f32 = 6.0;
    div()
        .h(px(CONTROL_HEIGHT))
        .p(px(2.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn segmented_item(selected: bool) -> Div {
    #[cfg(target_os = "windows")]
    const ITEM: (f32, f32) = (3.0, 12.0);
    #[cfg(not(target_os = "windows"))]
    const ITEM: (f32, f32) = (4.0, 11.0);
    div()
        .h(px(26.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(ITEM.0))
        .text_size(px(ITEM.1))
        .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .when(!selected, |item| {
            item.hover(|item| {
                item.bg(rgb(SEGMENTED_ITEM_HOVER))
                    .text_color(rgb(TEXT_SOFT))
            })
        })
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn mix_color(from: Rgba, to: Rgba, position: f32) -> Rgba {
    let position = position.clamp(0.0, 1.0);
    Rgba {
        r: from.r + (to.r - from.r) * position,
        g: from.g + (to.g - from.g) * position,
        b: from.b + (to.b - from.b) * position,
        a: from.a + (to.a - from.a) * position,
    }
}

#[allow(dead_code)]
pub(crate) fn empty_message(message: &'static str) -> AnyElement {
    #[cfg(target_os = "windows")]
    let message = crate::windows_i18n::tr(message);
    div()
        .p_6()
        .text_size(px(12.0))
        .text_color(rgb(FAINT))
        .child(message)
        .into_any_element()
}

#[allow(dead_code)]
pub(crate) fn error_message(message: &'static str, error: String) -> AnyElement {
    #[cfg(target_os = "windows")]
    let message = crate::windows_i18n::tr(message);
    div()
        .p_6()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(px(12.0))
        .text_color(rgb(NEGATIVE))
        .child(message)
        .child(
            div()
                .text_size(px(11.0))
                .line_height(px(17.0))
                .text_color(rgb(MUTED))
                .child(error),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::split_sided_keycap;

    #[test]
    fn side_badges_only_apply_to_sided_modifiers() {
        assert_eq!(split_sided_keycap("L⌥"), (Some("L"), "⌥".into()));
        assert_eq!(split_sided_keycap("R⌘"), (Some("R"), "⌘".into()));
        assert_eq!(split_sided_keycap("Return"), (None, "Return".into()));
    }
}
