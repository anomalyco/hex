//! Bridges hex's Fluent design tokens into gpui-component's global Theme so
//! library components render indistinguishably from the hand-rolled Windows
//! controls they replace.

use gpui::{App, Hsla, px};
use gpui_component::{Theme, ThemeMode};

fn color(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

/// One-time gpui-component setup plus the Fluent dark palette. Call at the
/// top of the app's `run` closure, before any window opens.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    let accent = color(crate::windows_ui::accent());
    let theme = Theme::global_mut(cx);
    theme.mode = ThemeMode::Dark;
    theme.font_family = crate::windows_ui::FONT_UI.into();
    theme.font_size = px(16.0);
    theme.radius = px(4.0);
    theme.radius_lg = px(8.0);
    theme.shadow = true;

    let colors = &mut theme.colors;
    colors.background = color(0x202020);
    colors.foreground = color(0xffffff);
    colors.border = color(0x303030);
    colors.input = color(crate::windows_ui::CONTROL_STROKE);
    colors.ring = accent;
    colors.popover = color(0x2c2c2c);
    colors.popover_foreground = color(0xffffff);
    colors.muted = color(0x2b2b2b);
    colors.muted_foreground = color(0x9e9e9e);
    colors.primary = accent;
    colors.primary_foreground = color(0x000000);
    colors.primary_hover = accent.opacity(0.9);
    colors.primary_active = accent.opacity(0.8);
    colors.secondary = color(crate::windows_ui::CONTROL_FILL);
    colors.secondary_foreground = color(0xffffff);
    colors.secondary_hover = color(crate::windows_ui::CONTROL_FILL_HOVER);
    colors.secondary_active = color(crate::windows_ui::CONTROL_FILL_PRESSED);
    colors.accent = color(0x2f2f2f);
    colors.accent_foreground = color(0xffffff);
    colors.list = color(0x2c2c2c);
    colors.list_hover = color(0x323232);
    colors.list_active = accent.opacity(0.2);
    colors.selection = accent.opacity(0.3);
    colors.caret = color(0xffffff);
    colors.danger = color(crate::windows_ui::CRITICAL);
    colors.danger_foreground = color(0x000000);
    colors.success = color(crate::windows_ui::SUCCESS);
    colors.success_foreground = color(0x000000);
    colors.scrollbar = color(0x00000000);
    colors.scrollbar_thumb = color(0x4d4d4d);
    colors.scrollbar_thumb_hover = color(0x5c5c5c);
    colors.switch = color(0x454545);
    colors.switch_thumb = color(0xffffff);
    colors.slider_bar = accent;
    colors.slider_thumb = color(0xffffff);
}
