use gpui::{AnyElement, Div, FontWeight, IntoElement, Rgba, div, prelude::*, px, rgb};

#[cfg(target_os = "linux")]
use gpui::{Image, ImageFormat, img};
#[cfg(target_os = "linux")]
use std::sync::Arc;

#[derive(Clone, Copy)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
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

    #[cfg(target_os = "linux")]
    fn heroicon(self) -> &'static str {
        match self {
            Self::Settings => include_str!("../assets/icons/settings.svg"),
            Self::Modes => include_str!("../assets/icons/modes.svg"),
            Self::VoiceAction => include_str!("../assets/icons/voice-action.svg"),
            Self::Commands => include_str!("../assets/icons/commands.svg"),
            Self::History => include_str!("../assets/icons/history.svg"),
            Self::Meetings => include_str!("../assets/icons/meetings.svg"),
            Self::Activity => include_str!("../assets/icons/activity.svg"),
            Self::HudLab => include_str!("../assets/icons/hud-lab.svg"),
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

#[cfg(target_os = "linux")]
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

pub(crate) const CANVAS: u32 = 0x111111;
pub(crate) const SIDEBAR: u32 = 0x202020;
pub(crate) const SURFACE: u32 = 0x171717;
pub(crate) const SURFACE_HOVER: u32 = 0x1d1d1d;
pub(crate) const SURFACE_SELECTED: u32 = 0x3a3a3a;
const SEGMENTED_ITEM_HOVER: u32 = 0x262626;
pub(crate) const LINE: u32 = 0x292929;
pub(crate) const ACCENT: u32 = 0x3b5cf6;
pub(crate) const TEXT: u32 = 0xeeeeee;
pub(crate) const TEXT_SOFT: u32 = 0xb8b8b8;
pub(crate) const MUTED: u32 = 0x858585;
pub(crate) const FAINT: u32 = 0x626262;
pub(crate) const NEGATIVE: u32 = 0xc98f89;

pub(crate) const CONTROL_HEIGHT: f32 = 32.0;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const TEXT_INPUT_HEIGHT: f32 = 34.0;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const MULTILINE_INPUT_HEIGHT: f32 = 132.0;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const COMPACT_MULTILINE_INPUT_HEIGHT: f32 = 76.0;
pub(crate) const PANEL_RADIUS: f32 = 10.0;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const COMPACT_PANEL_HEADER_HEIGHT: f32 = 38.0;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const SECTION_GAP: f32 = 8.0;

pub(crate) fn window_frame() -> Div {
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .flex()
        .bg(rgb(CANVAS))
        .text_color(rgb(TEXT))
}

pub(crate) fn sidebar_frame() -> Div {
    div()
        .h_full()
        .flex_none()
        .bg(rgb(SIDEBAR))
        .border_r_1()
        .border_color(rgb(LINE))
}

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
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) const PANE_LIST_WIDTH: f32 = 320.0;

pub(crate) fn pane_header(title: &'static str) -> AnyElement {
    pane_header_with_action(title, None)
}

/// The one pane header: title left, optional action right, bounded to
/// [`PANE_CONTENT_WIDTH`] like the body below it.
pub(crate) fn pane_header_with_action(
    title: &'static str,
    action: Option<AnyElement>,
) -> AnyElement {
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
                .border_b_1()
                .border_color(rgb(LINE))
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
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(FAINT))
        .child(label)
        .into_any_element()
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn listener_status(
    status: impl IntoElement,
    device: impl IntoElement,
    active: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(8.0))
                .flex_none()
                .rounded_full()
                .bg(if active { rgb(0x69d89f) } else { rgb(FAINT) }),
        )
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
            div()
                .min_w(px(34.0))
                .h(px(24.0))
                .px_2()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .border_1()
                .border_color(rgb(0x444444))
                .bg(rgb(0x2b2b2b))
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

#[allow(dead_code)]
pub(crate) fn settings_copy(title: &'static str, description: &'static str) -> AnyElement {
    div()
        .debug_selector(|| "settings-copy".into())
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

pub(crate) fn compact_plus_button() -> Div {
    compact_button(plus_icon())
        .size(px(28.0))
        .px_0()
        .justify_center()
}

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

#[cfg_attr(target_os = "linux", allow(dead_code))]
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

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn compact_section_label(label: impl IntoElement) -> Div {
    div()
        .px_1()
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(FAINT))
        .child(label)
}

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
        .gap_4()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(settings_copy(title, description)),
        )
        .child(control)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn settings_panel() -> Div {
    compact_panel().relative()
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn segmented_control() -> Div {
    div()
        .h(px(CONTROL_HEIGHT))
        .p(px(2.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn segmented_item(selected: bool) -> Div {
    div()
        .h(px(26.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(4.0))
        .text_size(px(11.0))
        .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
        .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
        .when(!selected, |item| {
            item.hover(|item| {
                item.bg(rgb(SEGMENTED_ITEM_HOVER))
                    .text_color(rgb(TEXT_SOFT))
            })
        })
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

#[allow(dead_code)]
pub(crate) fn empty_message(message: &'static str) -> AnyElement {
    div()
        .p_6()
        .text_size(px(12.0))
        .text_color(rgb(FAINT))
        .child(message)
        .into_any_element()
}

#[allow(dead_code)]
pub(crate) fn error_message(message: &'static str, error: String) -> AnyElement {
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
}
