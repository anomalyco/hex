//! The Windows dictation HUD: the shared indicator
//! model and the CPU port of the macOS shader (`desktop_indicator_shader`),
//! drawn as `rune::Pixels` in a Rune overlay: a topmost tool window that
//! never activates and lets every click through.
//!
//! A HUD thread owns the model. It drains the listener's events, steps the
//! springs every 16 ms while the capsule is on screen (sleeping otherwise),
//! shades each frame, and hands it to the overlay, which only draws. The
//! listener never waits on it.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{Cx, El, OverlayHandle, OverlaySpec, Pixels, ScreenEdge, el};

use crate::app_settings::IndicatorPosition;
use crate::desktop_indicator_model::{
    DictationIndicatorEvent, IndicatorModel, WINDOW_HEIGHT, WINDOW_WIDTH,
};

pub(crate) use crate::desktop_indicator_model::DictationIndicatorEvent as WindowsIndicatorEvent;

const FRAME: Duration = Duration::from_millis(16);
/// The shader's backing scale, matching the macOS HUD's Retina rendering.
const BACKING_SCALE: f32 = 2.0;
const TOP_OFFSET: f32 = crate::desktop_indicator_model::TOP_OFFSET as f32;
const BOTTOM_OFFSET: f32 = 24.0;

/// How the listener reports captures to the HUD; sending never waits.
#[derive(Clone)]
pub(crate) struct WindowsIndicatorSender {
    events: Sender<WindowsIndicatorEvent>,
    position: Arc<Mutex<IndicatorPosition>>,
}

impl WindowsIndicatorSender {
    pub(crate) fn send(&self, event: WindowsIndicatorEvent) {
        let _ = self.events.send(event);
    }

    pub(crate) fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }

    pub(crate) fn set_position(&self, position: IndicatorPosition) {
        *self
            .position
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = position;
    }
}

pub(crate) struct Hud {
    pixels: Pixels,
}

/// The overlay and the frame it shows; [`HudHandle::start`] feeds both from
/// the HUD thread.
pub(crate) struct HudHandle {
    overlay: OverlayHandle<Hud>,
    pixels: Pixels,
}

impl HudHandle {
    pub(crate) fn start(self, initial: IndicatorPosition) -> WindowsIndicatorSender {
        let (events, receiver) = mpsc::channel();
        let position = Arc::new(Mutex::new(initial));
        let hud_position = position.clone();
        let _ = std::thread::Builder::new()
            .name("hex-hud".into())
            .spawn(move || {
                run(self, receiver, || {
                    *hud_position
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                })
            });
        WindowsIndicatorSender { events, position }
    }
}

/// Adds the overlay to the window's app.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> HudHandle {
    let pixels = Pixels::new();
    let overlay = app.overlay(
        OverlaySpec {
            name: "Hex HUD",
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            edge: ScreenEdge::Top,
            margin: TOP_OFFSET,
            align: rune::Align::Center,
            inset: 0.0,
            interactive: false,
            view,
        },
        Hud {
            pixels: pixels.clone(),
        },
    );
    HudHandle { overlay, pixels }
}

/// The HUD thread: returns when the listener's event channel closes.
fn run(
    hud: HudHandle,
    events: Receiver<WindowsIndicatorEvent>,
    position: impl Fn() -> IndicatorPosition,
) {
    let HudHandle { overlay, pixels } = hud;
    let mut model = IndicatorModel::new();
    model.set_backing_scale(BACKING_SCALE);
    let (width, height) = (
        (WINDOW_WIDTH * BACKING_SCALE) as usize,
        (WINDOW_HEIGHT * BACKING_SCALE) as usize,
    );
    let mut shown: Option<IndicatorPosition> = None;
    loop {
        let event = if shown.is_some() {
            events.recv_timeout(FRAME)
        } else {
            events.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match event {
            Ok(event) => {
                model.handle(event);
                while let Ok(event) = events.try_recv() {
                    model.handle(event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let frame = model.frame(Instant::now());
        let position = position();
        let show = frame.visible && position != IndicatorPosition::Hidden;
        if !show {
            if shown.take().is_some() {
                overlay.hide();
            }
            continue;
        }
        let rgba = crate::desktop_indicator_shader::render(&frame.uniforms, width, height);
        pixels.set(width as u32, height as u32, rgba);
        // An update wakes the overlay to draw the new frame.
        overlay.update(|_, _| {});
        if shown != Some(position) {
            let (edge, margin) = match position {
                IndicatorPosition::Bottom => (ScreenEdge::Bottom, BOTTOM_OFFSET),
                _ => (ScreenEdge::Top, TOP_OFFSET),
            };
            overlay.place(edge, margin);
            overlay.show();
            shown = Some(position);
        }
    }
    overlay.hide();
}

fn view(hud: &Hud, _: &mut Cx<Hud>) -> El {
    el().key("hud").w_full().h_full().pixels(&hud.pixels)
}
