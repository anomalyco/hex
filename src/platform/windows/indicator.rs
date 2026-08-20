//! The Windows dictation HUD: the shared indicator state machine drives the
//! CPU port of the Metal shader, rendered at the mac's exact
//! geometry (112x64 logical points at 2x backing scale) into a
//! transparent click-through popup. Event vocabulary, springs, and
//! timing are identical to the macOS HUD.
//!
//! The app's 16ms pump owns the frame cadence: it forwards events into
//! [`WindowsIndicator::handle`], steps the model with
//! [`WindowsIndicator::tick`], and shows or hides the OS window from the
//! returned visibility — mirroring how the macOS shell drives its Metal
//! renderer from a display link and orders the window in and out.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use gpui::{
    App, Bounds, Context, Render, RenderImage, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, img, point, prelude::*, px, size,
};

use crate::desktop::indicator_model::{DictationIndicatorEvent, IndicatorFrame, IndicatorModel};
use crate::desktop::indicator_shader;

pub(crate) const WINDOW_WIDTH: f32 = crate::desktop::indicator_model::WINDOW_WIDTH;
pub(crate) const WINDOW_HEIGHT: f32 = crate::desktop::indicator_model::WINDOW_HEIGHT;
pub(crate) const TOP_OFFSET: f32 = crate::desktop::indicator_model::TOP_OFFSET as f32;
pub(crate) const BOTTOM_OFFSET: f32 = 24.0;
const BACKING_SCALE: f32 = 2.0;

pub use crate::desktop::indicator_model::DictationIndicatorEvent as WindowsIndicatorEvent;

#[derive(Clone)]
pub struct WindowsIndicatorSender(Sender<WindowsIndicatorEvent>);

impl WindowsIndicatorSender {
    pub fn send(&self, event: WindowsIndicatorEvent) {
        let _ = self.0.send(event);
    }

    pub fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }
}

pub fn channel() -> (WindowsIndicatorSender, Receiver<WindowsIndicatorEvent>) {
    let (sender, receiver) = mpsc::channel();
    (WindowsIndicatorSender(sender), receiver)
}

pub struct WindowsIndicator {
    model: IndicatorModel,
    frame: Option<IndicatorFrame>,
    was_visible: bool,
    /// The previous frame's texture, evicted from the sprite atlas on the
    /// next paint — without this every frame leaks an atlas tile.
    last_render_image: Option<Arc<RenderImage>>,
}

impl WindowsIndicator {
    pub fn new() -> Self {
        let mut model = IndicatorModel::new();
        model.set_backing_scale(BACKING_SCALE);
        Self {
            model,
            frame: None,
            was_visible: false,
            last_render_image: None,
        }
    }

    pub fn handle(&mut self, event: WindowsIndicatorEvent) {
        self.model.handle(event);
    }

    /// Advances the springs one frame and reports whether the HUD window
    /// should be on screen. Repaints only around visible frames so the
    /// idle HUD costs a few spring updates per tick and nothing more.
    pub fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let frame = self.model.frame(Instant::now());
        let visible = frame.visible;
        self.frame = Some(frame);
        if visible || self.was_visible {
            cx.notify();
        }
        self.was_visible = visible;
        visible
    }
}

impl Render for WindowsIndicator {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(previous) = self.last_render_image.take() {
            let _ = window.drop_image(previous);
        }
        let Some(frame) = self.frame.as_ref().filter(|frame| frame.visible) else {
            return div().size_full().into_any_element();
        };
        let width = (WINDOW_WIDTH * BACKING_SCALE) as usize;
        let height = (WINDOW_HEIGHT * BACKING_SCALE) as usize;
        let mut bytes = indicator_shader::render(&frame.uniforms, width, height);
        // gpui's raw frames carry BGRA in an RGBA container.
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(width as u32, height as u32, bytes)
            .expect("the shader buffer matches its dimensions");
        let render_image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        self.last_render_image = Some(render_image.clone());
        div()
            .size_full()
            .child(img(render_image).w(px(WINDOW_WIDTH)).h(px(WINDOW_HEIGHT)))
            .into_any_element()
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
