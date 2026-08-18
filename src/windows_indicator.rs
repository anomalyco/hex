use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, Bounds, Context, Render, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, pulsating_between, px, rgb,
    rgba, size,
};

// The window is exactly the pill: any larger canvas shows Windows'
// composition tint as a ghost rectangle around the HUD.
pub(crate) const WINDOW_WIDTH: f32 = 118.0;
pub(crate) const WINDOW_HEIGHT: f32 = 32.0;
pub(crate) const TOP_OFFSET: f32 = 12.0;
pub(crate) const BOTTOM_OFFSET: f32 = 24.0;
const BAR_COUNT: usize = 14;

#[derive(Clone, Copy, Debug)]
pub enum WindowsIndicatorEvent {
    Recording,
    Meter(f32),
    Processing,
    Hidden,
}

#[derive(Clone)]
pub struct WindowsIndicatorSender(Sender<WindowsIndicatorEvent>);

impl WindowsIndicatorSender {
    pub fn send(&self, event: WindowsIndicatorEvent) {
        let _ = self.0.send(event);
    }

    pub fn meter(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
        let rms = (sum / samples.len() as f32).sqrt();
        self.send(WindowsIndicatorEvent::Meter((rms * 8.0).clamp(0.0, 1.0)));
    }
}

pub fn channel() -> (WindowsIndicatorSender, Receiver<WindowsIndicatorEvent>) {
    let (sender, receiver) = mpsc::channel();
    (WindowsIndicatorSender(sender), receiver)
}

pub struct WindowsIndicator {
    recording: bool,
    level: f32,
}

impl WindowsIndicator {
    pub fn new() -> Self {
        Self {
            recording: true,
            level: 0.0,
        }
    }

    pub fn handle(&mut self, event: WindowsIndicatorEvent, cx: &mut Context<Self>) {
        match event {
            WindowsIndicatorEvent::Recording => {
                self.recording = true;
                self.level = 0.0;
            }
            WindowsIndicatorEvent::Meter(level) => {
                self.recording = true;
                self.level = self.level * 0.65 + level * 0.35;
            }
            WindowsIndicatorEvent::Processing => {
                self.recording = false;
                self.level = 0.0;
            }
            WindowsIndicatorEvent::Hidden => {
                self.level = 0.0;
            }
        }
        cx.notify();
    }
}

impl Render for WindowsIndicator {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let accent = crate::windows_ui::accent();
        let (badge_fill, badge_glyph) = if self.recording {
            (accent, 0x000000)
        } else {
            (0x2d2d2d, 0xcfcfcf)
        };
        let content = if self.recording {
            let level = self.level;
            div()
                .h(px(20.0))
                .flex()
                .items_center()
                .gap(px(2.0))
                .children((0..BAR_COUNT).map(move |index| {
                    let center = (BAR_COUNT - 1) as f32 / 2.0;
                    let distance = (index as f32 - center).abs();
                    let shape = (1.0 - distance * 0.09).max(0.3);
                    let height = 3.0 + 15.0 * level.max(0.12) * shape;
                    div()
                        .w(px(2.5))
                        .h(px(height))
                        .rounded_full()
                        .bg(rgb(accent))
                }))
                .into_any_element()
        } else {
            div()
                .h(px(20.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .children((0..3_usize).map(|index| {
                    div()
                        .size(px(5.0))
                        .rounded_full()
                        .bg(rgb(accent))
                        .with_animation(
                            ("hud-processing-dot", index as u64),
                            Animation::new(Duration::from_millis(700 + index as u64 * 160))
                                .repeat()
                                .with_easing(pulsating_between(0.25, 1.0)),
                            |dot, delta| dot.opacity(delta),
                        )
                }))
                .into_any_element()
        };
        div()
            .size_full()
            .pl(px(5.0))
            .pr(px(10.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded_full()
            .bg(rgba(0x1c1c1cfa))
            .child(
                div()
                    .size(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(badge_fill))
                    .child(crate::windows_ui::fluent_icon(
                        "\u{E720}",
                        11.0,
                        badge_glyph,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .justify_center()
                    .child(content),
            )
    }
}

pub fn window_options(cx: &App) -> WindowOptions {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let (display_id, bounds) = cx.primary_display().map_or_else(
        || (None, Bounds::centered(None, window_size, cx)),
        |display| {
            let display_bounds = display.bounds();
            let origin = point(
                display_bounds.origin.x + (display_bounds.size.width - px(WINDOW_WIDTH)) / 2.0,
                display_bounds.origin.y + px(TOP_OFFSET),
            );
            (
                Some(display.id()),
                Bounds {
                    origin,
                    size: window_size,
                },
            )
        },
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    }
}
