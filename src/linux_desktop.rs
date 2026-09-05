//! The service-owned, focus-free Wayland recording HUD. Settings never owns this runtime.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::eyre::Result;
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::linux_session::LinuxSession;

const TICK: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum IndicatorState {
    #[default]
    Hidden,
    Recording,
    Processing,
}

impl IndicatorState {
    fn for_capture(recording: bool, pending: usize) -> Self {
        if recording {
            Self::Recording
        } else if pending > 0 {
            Self::Processing
        } else {
            Self::Hidden
        }
    }
}

#[derive(Default)]
struct IndicatorSlot {
    generation: u64,
    state: IndicatorState,
}

impl IndicatorSlot {
    fn acquire(&mut self) -> u64 {
        self.generation += 1;
        self.state = IndicatorState::Hidden;
        self.generation
    }
    fn set(&mut self, generation: u64, state: IndicatorState) {
        if self.generation == generation {
            self.state = state;
        }
    }
}

#[derive(Default)]
struct State {
    indicator: IndicatorSlot,
    initialized: bool,
    limitation: Option<String>,
}

struct Runtime {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let owner_state = state.clone();
        let owner_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| run(&owner_state, &owner_stop));
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("Linux desktop integration unavailable: {error:#}")),
                Err(_) => Some("Linux desktop integration stopped unexpectedly".into()),
            };
            if let Some(error) = error {
                tracing::warn!(%error);
                let mut state = owner_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.initialized = true;
                state.limitation = Some(error);
            }
        });
        Runtime {
            state,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    })
}

pub(crate) fn hud_limitation() -> Option<String> {
    if !LinuxSession::detect().is_wayland() {
        return None;
    }
    RUNTIME.get().and_then(|runtime| {
        let state = runtime
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.initialized {
            state.limitation.clone()
        } else {
            Some("Recording HUD is starting; it may be unavailable on this compositor.".into())
        }
    })
}

pub(crate) fn shutdown() {
    if let Some(runtime) = RUNTIME.get() {
        runtime.stop.store(true, Ordering::Release);
        let mut worker = runtime
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

pub(crate) struct LinuxIndicator {
    generation: Option<u64>,
}

impl LinuxIndicator {
    pub(crate) fn new() -> Self {
        let generation = LinuxSession::detect().is_wayland().then(|| {
            runtime()
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .indicator
                .acquire()
        });
        Self { generation }
    }
    pub(crate) fn update(&self, recording: bool, pending: usize) {
        if let Some(generation) = self.generation {
            runtime()
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .indicator
                .set(generation, IndicatorState::for_capture(recording, pending));
        }
    }
}

impl Drop for LinuxIndicator {
    fn drop(&mut self) {
        self.update(false, 0);
    }
}

fn run(state: &Mutex<State>, stop: &AtomicBool) -> Result<()> {
    gtk::init()?;
    let indicator = if gtk_layer_shell::is_supported() {
        Some(create_indicator()?)
    } else {
        None
    };
    {
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.initialized = true;
        if indicator.is_none() {
            let limitation = "Recording HUD unavailable: this compositor does not support Wayland layer-shell. Dictation still works.";
            tracing::warn!("{limitation}");
            state.limitation = Some(limitation.into());
        }
    }
    let context = gtk::glib::MainContext::default();
    let mut displayed = IndicatorState::Hidden;
    while !stop.load(Ordering::Acquire) {
        let desired = state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .indicator
            .state;
        if desired != displayed {
            if let Some((window, label)) = &indicator {
                match desired {
                    IndicatorState::Hidden => window.hide(),
                    IndicatorState::Recording | IndicatorState::Processing => {
                        label.set_text(if desired == IndicatorState::Recording {
                            "Recording"
                        } else {
                            "Transcribing"
                        });
                        window.show_all();
                    }
                }
            }
            displayed = desired;
        }
        for _ in 0..32 {
            if !context.iteration(false) {
                break;
            }
        }
        std::thread::sleep(TICK);
    }
    if let Some((window, _)) = indicator {
        window.close();
    }
    Ok(())
}

fn create_indicator() -> Result<(gtk::Window, gtk::Label)> {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_decorated(false);
    window.set_accept_focus(false);
    window.set_focus_on_map(false);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_app_paintable(true);
    if let Some(screen) = GtkWindowExt::screen(&window)
        && let Some(visual) = screen.rgba_visual()
    {
        window.set_visual(Some(&visual));
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);
    window.set_anchor(Edge::Top, true);
    window.set_layer_shell_margin(Edge::Top, 16);
    window.style_context().add_class("hex-hud");
    let css = gtk::CssProvider::new();
    css.load_from_data(b"window.hex-hud { background: transparent; } .hex-hud label { background: #20221f; color: #d9ff68; border: 1px solid #44483b; border-radius: 18px; padding: 9px 18px; font: 13px sans-serif; }")?;
    if let Some(screen) = GtkWindowExt::screen(&window) {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let label = gtk::Label::new(None);
    window.add(&label);
    window.connect_realize(|window| {
        if let Some(surface) = window.window() {
            surface.set_pass_through(true);
            surface.input_shape_combine_region(&gtk::cairo::Region::create(), 0, 0);
        }
    });
    Ok((window, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hud_remains_visible_until_all_output_finishes() {
        assert_eq!(
            IndicatorState::for_capture(true, 2),
            IndicatorState::Recording
        );
        assert_eq!(
            IndicatorState::for_capture(false, 2),
            IndicatorState::Processing
        );
        assert_eq!(
            IndicatorState::for_capture(false, 1),
            IndicatorState::Processing
        );
        assert_eq!(
            IndicatorState::for_capture(false, 0),
            IndicatorState::Hidden
        );
    }
    #[test]
    fn old_listener_cannot_hide_or_update_a_restarted_hud() {
        let mut slot = IndicatorSlot::default();
        let old = slot.acquire();
        slot.set(old, IndicatorState::Processing);
        let current = slot.acquire();
        assert_eq!(slot.state, IndicatorState::Hidden);
        slot.set(current, IndicatorState::Recording);
        slot.set(old, IndicatorState::Hidden);
        assert_eq!(slot.state, IndicatorState::Recording);
        slot.set(current, IndicatorState::Hidden);
        assert_eq!(slot.state, IndicatorState::Hidden);
    }
}
