use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::linux_session::LinuxSession;

const TICK: Duration = Duration::from_millis(16);

#[derive(Clone, Copy)]
pub(crate) enum TrayCommand {
    Show,
    ToggleListening,
    Quit,
}

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

struct TrayRequest {
    commands: Sender<TrayCommand>,
    canceled: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
}

#[derive(Default)]
struct State {
    indicator: IndicatorSlot,
    tray: Option<TrayRequest>,
    tray_available: bool,
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
                state.tray_available = false;
                state.limitation = Some(error.clone());
                if let Some(request) = state.tray.take() {
                    let _ = request.ready.try_send(Err(error));
                }
            }
        });
        Runtime {
            state,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    })
}

pub(crate) fn start_tray(commands: Sender<TrayCommand>) -> Result<()> {
    let runtime = runtime();
    let canceled = Arc::new(AtomicBool::new(false));
    let (ready, receiver) = mpsc::sync_channel(1);
    {
        let mut state = runtime
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = &state.limitation {
            return Err(eyre!(error.clone()));
        }
        state.tray = Some(TrayRequest {
            commands,
            canceled: canceled.clone(),
            ready,
        });
    }
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result.map_err(|error| eyre!(error)),
        Err(_) => {
            // A late initialization must not install a tray after the caller chose trayless mode.
            canceled.store(true, Ordering::Release);
            Err(eyre!("timed out starting the system tray"))
        }
    }
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

pub(crate) fn tray_available() -> bool {
    RUNTIME.get().is_some_and(|runtime| {
        runtime
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .tray_available
    })
}

/// Only the process owner shuts GTK down, never an individual listener or HUD.
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
    // All GTK objects and event dispatch stay on this one process-lifetime thread.
    // MainContext::invoke is unsuitable: it may execute immediately on its caller.
    gtk::init()?;
    let wayland = LinuxSession::detect().is_wayland();
    let indicator = if wayland && gtk_layer_shell::is_supported() {
        Some(create_indicator()?)
    } else {
        None
    };
    {
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.initialized = true;
        if wayland && indicator.is_none() {
            let limitation = "Recording HUD unavailable: this compositor does not support Wayland layer-shell. Dictation still works.";
            tracing::warn!("{limitation}");
            state.limitation = Some(limitation.into());
        }
    }
    let context = gtk::glib::MainContext::default();
    let mut tray: Option<(TrayIcon, Arc<AtomicBool>)> = None;
    let mut check_tray_at = Instant::now();
    let mut displayed = IndicatorState::Hidden;
    while !stop.load(Ordering::Acquire) {
        let (request, desired) = {
            let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
            (state.tray.take(), state.indicator.state)
        };
        if let Some(request) = request
            && !request.canceled.load(Ordering::Acquire)
        {
            match create_tray(request.commands) {
                Ok(icon) => {
                    if request.ready.try_send(Ok(())).is_ok() {
                        tray = Some((icon, request.canceled));
                        state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .tray_available = true;
                        check_tray_at = Instant::now() + Duration::from_secs(2);
                    }
                }
                Err(error) => {
                    let _ = request.ready.try_send(Err(format!("{error:#}")));
                }
            }
        }
        if tray
            .as_ref()
            .is_some_and(|(_, canceled)| canceled.load(Ordering::Acquire))
        {
            tray = None;
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .tray_available = false;
        }
        if tray.is_some() && Instant::now() >= check_tray_at {
            let available = tray_host_available();
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .tray_available = available;
            check_tray_at = Instant::now() + Duration::from_secs(2);
        }
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
        // Bounded, nonblocking dispatch makes shutdown independent of pending GTK sources.
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
    drop(tray);
    state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .tray_available = false;
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

fn create_tray(commands: Sender<TrayCommand>) -> Result<TrayIcon> {
    if !tray_host_available() {
        return Err(eyre!("no system tray host is available"));
    }
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("show", "Show HEX", true, None))?;
    menu.append(&MenuItem::with_id(
        "toggle",
        "Start / Stop Listening",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id("quit", "Quit HEX", true, None))?;
    let menu_commands = commands.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = match event.id.as_ref() {
            "show" => Some(TrayCommand::Show),
            "toggle" => Some(TrayCommand::ToggleListening),
            "quit" => Some(TrayCommand::Quit),
            _ => None,
        };
        if let Some(command) = command {
            let _ = menu_commands.send(command);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = commands.send(TrayCommand::Show);
        }
    }));
    const SIZE: u32 = 22;
    let mut data = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for y in 3..19 {
        for x in 4..18 {
            if matches!(x, 4..=7 | 14..=17) || (9..=12).contains(&y) {
                let index = ((y * SIZE + x) * 4) as usize;
                data[index..index + 4].copy_from_slice(&[0xd9, 0xff, 0x68, 0xff]);
            }
        }
    }
    Ok(TrayIconBuilder::new()
        .with_tooltip("HEX Dictation")
        .with_icon(Icon::from_rgba(data, SIZE, SIZE)?)
        .with_menu(Box::new(menu))
        .build()?)
}

fn tray_host_available() -> bool {
    // Successful AppIndicator construction alone does not mean a panel can display it.
    // Without a host, close-to-tray would leave the listener impossible to reopen.
    use gtk::glib::variant::ToVariant;
    if let Ok(bus) = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
    {
        for name in [
            "org.kde.StatusNotifierWatcher",
            "org.freedesktop.StatusNotifierWatcher",
        ] {
            if let Ok(reply) = bus.call_sync(
                Some(name),
                "/StatusNotifierWatcher",
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(name, "IsStatusNotifierHostRegistered").to_variant()),
                None,
                gtk::gio::DBusCallFlags::NO_AUTO_START,
                250,
                gtk::gio::Cancellable::NONE,
            ) && reply
                .child_value(0)
                .as_variant()
                .and_then(|value| value.get::<bool>())
                == Some(true)
            {
                return true;
            }
        }
    }
    use x11rb::protocol::xproto::ConnectionExt;
    let legacy_host = (|| -> Result<bool> {
        let (connection, screen) = x11rb::rust_connection::RustConnection::connect(None)?;
        let selection = connection
            .intern_atom(false, format!("_NET_SYSTEM_TRAY_S{screen}").as_bytes())?
            .reply()?
            .atom;
        Ok(connection.get_selection_owner(selection)?.reply()?.owner != 0)
    })();
    legacy_host.unwrap_or(false)
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
