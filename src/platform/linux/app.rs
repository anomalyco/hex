use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Keystroke, Pixels, Timer, TitlebarOptions,
    Window, WindowBounds, WindowKind, WindowOptions, canvas, div, prelude::*, px, rgb, size,
};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopCapabilities, DesktopHost, DesktopListenerSnapshot, DesktopShortcut,
    DesktopSnapshot, DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::desktop_transcription_picker::TranscriptionPickerDelegate;
use crate::desktop_ui::{
    FAINT, LINE, MUTED, NavigationIcon, SIDEBAR_WIDTH, SUCCESS, SURFACE, TEXT, TEXT_SOFT,
    compact_button, disclosure_button, dropdown_backdrop, dropdown_item, dropdown_panel_with_width,
    error_message, header_button, hotkey_keycaps, navigation_item, pane_content, pane_header,
    settings_panel, settings_row, settings_section_label, sidebar_frame, toggle, window_frame,
};
use crate::events::EventReader;
use crate::linux_updater::InstalledUpdate;

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

type ListenerResult = std::result::Result<(), String>;
type PreparationResult = std::result::Result<PreparedTranscription, String>;

struct PreparedTranscription {
    selection: crate::transcription_models::TranscriptionSelection,
    transcriber: crate::local_transcriber::LocalTranscriber,
}

struct TranscriptionPreparation {
    canceled: Arc<AtomicBool>,
    model: crate::transcription_models::TranscriptionModelId,
    progress: Arc<AtomicU64>,
    result: Receiver<PreparationResult>,
    stage: Arc<AtomicU8>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
enum TrayCommand {
    Show,
    ToggleListening,
    Quit,
}

struct TrayRuntime {
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TrayRuntime {
    fn shutdown(&self) {
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            gtk::glib::MainContext::default().invoke(gtk::main_quit);
            let _ = worker.join();
        }
    }
}

struct LinuxDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    listener_result: Option<Receiver<ListenerResult>>,
    listener_worker: Option<JoinHandle<()>>,
    session_before_start: Option<u64>,
    awaiting_session_start: bool,
    listen_when_ready: bool,
    status: String,
    error: Option<String>,
    settings_error: Option<String>,
    settings: crate::linux_settings::LinuxSettings,
    microphones: Vec<String>,
    microphone_error: Option<String>,
    prepared_transcriber: Option<crate::local_transcriber::LocalTranscriber>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    update: UpdateState,
}

struct LinuxApp {
    host: LinuxDesktopHost,
    capturing_hotkey: bool,
    transcription_picker: TranscriptionPickerState,
    catalog_language_filter: Option<String>,
    catalog_filter_dropdown_open: bool,
    catalog_filter_dropdown_bounds: Option<Bounds<Pixels>>,
    transcription_dropdown_open: bool,
    transcription_dropdown_bounds: Option<Bounds<Pixels>>,
    dictation_language_dropdown_open: bool,
    dictation_language_dropdown_bounds: Option<Bounds<Pixels>>,
    microphone_dropdown_open: bool,
    microphone_dropdown_bounds: Option<Bounds<Pixels>>,
}

enum TranscriptionPickerState {
    Closed,
    Choosing(String),
    Preparing(String),
}

impl TranscriptionPickerState {
    fn language(&self) -> Option<&str> {
        match self {
            Self::Closed => None,
            Self::Choosing(language) | Self::Preparing(language) => Some(language),
        }
    }
}

enum UpdateState {
    Unmanaged,
    Checking(Receiver<Result<Option<InstalledUpdate>, String>>),
    Failed(Instant),
    Waiting(Instant),
    Ready(InstalledUpdate),
}

pub fn open(event_path: PathBuf, start_hidden: bool) -> Result<()> {
    let listener_stop = Arc::new(Mutex::new(None));
    let quit_stop = listener_stop.clone();
    let (tray_sender, tray_commands) = mpsc::channel();
    let tray = match spawn_tray(tray_sender) {
        Ok(tray) => Some(tray),
        Err(error) => {
            tracing::warn!(%error, "system tray is unavailable; closing HEX will quit");
            None
        }
    };
    let close_to_tray = tray.is_some();
    let (settings, settings_error) = match crate::linux_settings::LinuxSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            crate::linux_settings::LinuxSettings::default(),
            Some(format!("Could not load Linux settings: {error:#}")),
        ),
    };
    let update = if crate::linux_updater::managed_install() {
        UpdateState::Checking(start_update_check())
    } else {
        UpdateState::Unmanaged
    };
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(MINIMUM_WIDTH), px(MINIMUM_HEIGHT))),
                    kind: WindowKind::Floating,
                    titlebar: Some(TitlebarOptions {
                        title: Some("HEX".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| LinuxApp {
                        host: LinuxDesktopHost::new(
                            event_path.clone(),
                            listener_stop.clone(),
                            settings.clone(),
                            settings_error.clone(),
                            update,
                        ),
                        capturing_hotkey: false,
                        transcription_picker: TranscriptionPickerState::Closed,
                        catalog_language_filter: None,
                        catalog_filter_dropdown_open: false,
                        catalog_filter_dropdown_bounds: None,
                        transcription_dropdown_open: false,
                        transcription_dropdown_bounds: None,
                        dictation_language_dropdown_open: false,
                        dictation_language_dropdown_bounds: None,
                        microphone_dropdown_open: false,
                        microphone_dropdown_bounds: None,
                    })
                },
            )
            .expect("could not open the HEX X11 window");
        let app = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let x11_window = find_hex_window().ok();
        app.update(cx, |app, cx| {
            let _ = app.host.dispatch(DesktopAction::StartListening);
            cx.notify();
        });
        let hotkey_app = app.clone();
        cx.observe_keystrokes(move |event, _, cx| {
            hotkey_app.update(cx, |app, cx| {
                if app.capture_hotkey(&event.keystroke) {
                    cx.notify();
                }
            });
        })
        .detach();
        let tray_window = window;
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                while let Ok(command) = tray_commands.try_recv() {
                    match command {
                        TrayCommand::Show => {
                            if let Some(window) = x11_window {
                                set_x11_window_mapped(window, true);
                            }
                            let _ = tray_window.update(cx, |_, window, _| {
                                window.activate_window();
                            });
                        }
                        TrayCommand::ToggleListening => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                if app
                                    .host
                                    .snapshot()
                                    .listener
                                    .is_some_and(|listener| listener.running)
                                {
                                    let _ = app.host.dispatch(DesktopAction::StopListening);
                                } else {
                                    let _ = app.host.dispatch(DesktopAction::StartListening);
                                }
                                cx.notify();
                            });
                        }
                        TrayCommand::Quit => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                let _ = app.host.dispatch(DesktopAction::StopListening);
                                cx.quit();
                            });
                            return;
                        }
                    }
                }
                if app
                    .update(cx, |this, cx| {
                        this.host.refresh();
                        if matches!(
                            this.transcription_picker,
                            TranscriptionPickerState::Preparing(_)
                        ) && this.host.transcription_preparation.is_none()
                        {
                            let picker = std::mem::replace(
                                &mut this.transcription_picker,
                                TranscriptionPickerState::Closed,
                            );
                            if this.host.transcription_error.is_some()
                                && let TranscriptionPickerState::Preparing(language) = picker
                            {
                                this.transcription_picker =
                                    TranscriptionPickerState::Choosing(language);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        window
            .update(cx, |_, window, cx| {
                if close_to_tray {
                    window.on_window_should_close(cx, move |_, _| {
                        if let Some(window) = x11_window {
                            set_x11_window_mapped(window, false);
                        }
                        false
                    });
                }
                window.activate_window();
            })
            .ok();
        cx.activate(true);
        if start_hidden && let Some(window) = x11_window {
            set_x11_window_mapped(window, false);
        }
        cx.on_app_quit(move |_| {
            if let Some(stop) = quit_stop
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                stop.store(true, Ordering::Relaxed);
            }
            if let Some(tray) = &tray {
                tray.shutdown();
            }
            async {}
        })
        .detach();
    });
    Ok(())
}

fn find_hex_window() -> color_eyre::Result<u32> {
    let (connection, screen) = x11rb::rust_connection::RustConnection::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let client_list = connection
        .intern_atom(false, b"_NET_CLIENT_LIST")?
        .reply()?
        .atom;
    let clients = connection
        .get_property(
            false,
            root,
            client_list,
            x11rb::protocol::xproto::AtomEnum::WINDOW,
            0,
            u32::MAX,
        )?
        .reply()?;
    let windows = clients.value32().into_iter().flatten();
    let mut hex_window = None;
    for window in windows {
        let title = connection
            .get_property(
                false,
                window,
                x11rb::protocol::xproto::AtomEnum::WM_NAME,
                x11rb::protocol::xproto::AtomEnum::STRING,
                0,
                64,
            )?
            .reply()?
            .value;
        if title == b"HEX" {
            hex_window = Some(window);
            break;
        }
    }
    hex_window.ok_or_else(|| color_eyre::eyre::eyre!("HEX window not found"))
}

fn set_x11_window_mapped(window: u32, mapped: bool) {
    let result = (|| -> color_eyre::Result<()> {
        let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)?;
        if mapped {
            connection.map_window(window)?.check()?;
        } else {
            connection.unmap_window(window)?.check()?;
        }
        connection.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(%error, mapped, "could not change HEX window visibility");
    }
}

fn spawn_tray(commands: mpsc::Sender<TrayCommand>) -> Result<TrayRuntime> {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            gtk::init()?;
            let menu = Menu::new();
            let show = MenuItem::with_id("show", "Show HEX", true, None);
            let toggle = MenuItem::with_id("toggle", "Start / Stop Listening", true, None);
            let quit = MenuItem::with_id("quit", "Quit HEX", true, None);
            menu.append(&show)?;
            menu.append(&toggle)?;
            menu.append(&quit)?;

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

            let _tray = TrayIconBuilder::new()
                .with_tooltip("HEX Dictation")
                .with_icon(tray_icon()?)
                .with_menu(Box::new(menu))
                .build()?;
            let _ = ready_sender.send(Ok(()));
            gtk::main();
            Ok(())
        })();
        if let Err(error) = result {
            let _ = ready_sender.try_send(Err(error));
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| color_eyre::eyre::eyre!("timed out starting the system tray"))??;
    Ok(TrayRuntime {
        worker: Mutex::new(Some(worker)),
    })
}

fn tray_icon() -> Result<Icon> {
    // Pre-rendered from app/AppIcon.icon/Assets/Image.png so the tray
    // matches the branded app icon on every shell.
    const SIZE: u32 = 32;
    const DATA: &[u8] = include_bytes!("../../../resources/linux/tray-32.rgba");
    Ok(Icon::from_rgba(DATA.to_vec(), SIZE, SIZE)?)
}

impl LinuxDesktopHost {
    fn new(
        event_path: PathBuf,
        listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        settings: crate::linux_settings::LinuxSettings,
        error: Option<String>,
        update: UpdateState,
    ) -> Self {
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        let (microphones, microphone_error) = match crate::audio::input_device_names() {
            Ok(microphones) => (microphones, None),
            Err(error) => (
                Vec::new(),
                Some(format!("Could not enumerate microphones: {error:#}")),
            ),
        };
        Self {
            event_reader,
            event_path,
            activity,
            listener_stop,
            listener_result: None,
            listener_worker: None,
            session_before_start: None,
            awaiting_session_start: false,
            listen_when_ready: false,
            status: "Ready".into(),
            error: None,
            settings_error: error,
            settings,
            microphones,
            microphone_error,
            prepared_transcriber: None,
            transcription_preparation: None,
            transcription_error: None,
            update,
        }
    }

    fn start(&mut self) {
        self.listen_when_ready = true;
        if self.listener_result.is_some() {
            return;
        }
        let model = crate::transcription_models::definition(self.settings.transcription.model);
        if !crate::transcription_models::is_installed(model, &self.settings.transcription.language)
        {
            self.status = "Model required".into();
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(stop.clone());
        let (result_sender, result_receiver) = mpsc::channel();
        let event_path = self.event_path.clone();
        let prepared_transcriber = self.prepared_transcriber.take();
        let worker = std::thread::spawn(move || {
            let result = crate::instance::acquire("listener").and_then(|_instance| {
                if let Some(transcriber) = prepared_transcriber {
                    crate::linux_dictation::run_with_transcriber(
                        &event_path,
                        None,
                        &stop,
                        transcriber,
                    )
                } else {
                    crate::linux_dictation::run(&event_path, None, &stop)
                }
            });
            let result = result.map_err(|error| format!("{error:#}"));
            let _ = result_sender.send(result);
        });
        self.listener_worker = Some(worker);
        self.listener_result = Some(result_receiver);
        self.session_before_start = self.activity.session_started_at;
        self.awaiting_session_start = true;
        self.status = "Starting".into();
        self.error = None;
    }

    fn stop(&mut self) {
        self.listen_when_ready = false;
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            stop.store(true, Ordering::Relaxed);
            self.status = "Stopping".into();
        }
    }

    fn refresh(&mut self) {
        if matches!(&self.update, UpdateState::Waiting(at) | UpdateState::Failed(at) if Instant::now() >= *at)
        {
            self.update = UpdateState::Checking(start_update_check());
        }
        let update_result = match &self.update {
            UpdateState::Checking(receiver) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("update worker stopped unexpectedly".into()))
                }
                Err(TryRecvError::Empty) => None,
            },
            _ => None,
        };
        if let Some(result) = update_result {
            self.update = match result {
                Ok(Some(executable)) => UpdateState::Ready(executable),
                Ok(None) => UpdateState::Waiting(Instant::now() + UPDATE_INTERVAL),
                Err(error) => {
                    tracing::warn!(%error, "Linux update check failed");
                    UpdateState::Failed(Instant::now() + UPDATE_INTERVAL)
                }
            };
        }

        let transcription_result =
            self.transcription_preparation
                .as_ref()
                .and_then(|preparation| match preparation.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Disconnected) => {
                        Some(Err("model preparation worker stopped unexpectedly".into()))
                    }
                    Err(TryRecvError::Empty) => None,
                });
        let mut start_listener = false;
        if let Some(result) = transcription_result {
            let mut preparation = self
                .transcription_preparation
                .take()
                .expect("completed transcription preparation exists");
            let was_canceled = preparation.canceled.load(Ordering::Relaxed);
            if let Some(worker) = preparation.worker.take() {
                let _ = worker.join();
            }
            if was_canceled {
                self.transcription_error = None;
            } else {
                match result {
                    Ok(prepared) => {
                        let mut candidate = self.settings.clone();
                        candidate.transcription = prepared.selection;
                        match candidate.save() {
                            Ok(()) => {
                                self.settings = candidate;
                                self.prepared_transcriber = Some(prepared.transcriber);
                                self.transcription_error = None;
                                self.settings_error = None;
                                self.status = "Ready".into();
                                start_listener = self.listen_when_ready;
                            }
                            Err(error) => {
                                self.transcription_error = Some(format!(
                                    "Could not save transcription selection: {error:#}"
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        self.transcription_error = Some(error);
                    }
                }
            }
        }
        if start_listener {
            self.start();
        }

        if let Some(receiver) = &self.listener_result {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.status = "Ready".into();
                    self.awaiting_session_start = false;
                    self.listener_result = None;
                    self.join_listener();
                    self.listener_stop
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                }
                Ok(Err(error)) => {
                    self.status = "Unavailable".into();
                    self.awaiting_session_start = false;
                    self.error = Some(error);
                    self.listener_result = None;
                    self.join_listener();
                    self.listener_stop
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.status = "Unavailable".into();
                    self.awaiting_session_start = false;
                    self.error = Some("listener worker stopped unexpectedly".into());
                    self.listener_result = None;
                    self.join_listener();
                }
            }
        }

        self.activity.refresh(&mut self.event_reader);
        if self.awaiting_session_start
            && self.activity.session_started_at != self.session_before_start
        {
            self.awaiting_session_start = false;
        }
        if self.listener_result.is_some()
            && !self.awaiting_session_start
            && let Some(status) = self.activity.state_label()
        {
            self.status = status.into();
        }
    }

    fn is_running(&self) -> bool {
        self.listener_result.is_some()
    }

    fn join_listener(&mut self) {
        if let Some(worker) = self.listener_worker.take() {
            let _ = worker.join();
        }
    }

    fn set_dictation_hotkey(&mut self, shortcut: DesktopShortcut) -> Result<()> {
        if shortcut.function {
            self.error = Some("The Fn modifier cannot be registered on X11".into());
            return Err(color_eyre::eyre::eyre!(
                "the Fn modifier cannot be registered on X11"
            ));
        }
        let binding = crate::linux_settings::LinuxHotkey {
            control: shortcut.control,
            alt: shortcut.alt,
            shift: shortcut.shift,
            super_key: shortcut.platform,
            key: if shortcut.key == " " {
                "space".into()
            } else {
                shortcut.key.to_ascii_lowercase()
            },
        };
        if let Err(error) = binding.validate().and_then(|()| {
            crate::linux_input::X11HotkeyMonitor::start(binding.clone(), false).map(drop)
        }) {
            self.error = Some(format!("Could not register {}: {error:#}", binding.label()));
            return Err(error);
        }
        let mut candidate = self.settings.clone();
        candidate.dictation_hotkey = binding;
        if let Err(error) = candidate.save() {
            self.error = Some(format!("Could not save shortcut: {error:#}"));
            return Err(error);
        }
        self.settings = candidate;
        self.error = None;
        self.settings_error = None;
        Ok(())
    }

    fn restart_into_update(&self) -> Result<()> {
        let UpdateState::Ready(update) = &self.update else {
            return Err(color_eyre::eyre::eyre!("no installed update is ready"));
        };
        crate::linux_updater::relaunch(update)
    }

    /// Re-enumerate capture devices; the dropdown trigger calls this so
    /// hotplugged microphones and late audio-server startup are visible.
    fn refresh_microphones(&mut self) {
        match crate::audio::input_device_names() {
            Ok(microphones) => {
                self.microphones = microphones;
                self.microphone_error = None;
            }
            Err(error) => {
                self.microphone_error = Some(format!("Could not enumerate microphones: {error:#}"));
            }
        }
    }

    /// Persist the microphone choice; the listener opens its input once at
    /// start, so changes require a stopped listener like the model and
    /// language.
    fn set_microphone(&mut self, microphone: Option<String>) -> Result<()> {
        if microphone == self.settings.microphone {
            return Ok(());
        }
        if self.is_running() {
            let error = "Stop listening before changing the microphone".to_string();
            self.error = Some(error.clone());
            return Err(eyre!(error));
        }
        let mut candidate = self.settings.clone();
        candidate.microphone = microphone;
        if let Err(error) = candidate.save() {
            self.error = Some(format!("Could not save the microphone choice: {error:#}"));
            return Err(error);
        }
        self.settings = candidate;
        self.error = None;
        self.settings_error = None;
        Ok(())
    }

    fn choose_transcription(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
    ) -> Result<()> {
        if self.is_running() {
            let error = "Stop listening before changing the transcription model".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        if self.transcription_preparation.is_some() {
            self.cancel_transcription_preparation();
            let error = "The previous model preparation is still stopping".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        let selection = crate::transcription_models::TranscriptionSelection {
            model,
            language,
            recognition_hints: String::new(),
        };
        crate::transcription_models::validate(&selection)?;
        let definition = crate::transcription_models::definition(model);
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let stage = Arc::new(AtomicU8::new(
            crate::transcription_models::ModelPreparationStage::Downloading as u8,
        ));
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        let worker_stage = stage.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = (|| {
                crate::transcription_models::download_with_stage_progress(
                    definition,
                    &worker_canceled,
                    &worker_progress,
                    &worker_stage,
                )?;
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                crate::transcription_models::ModelPreparationStage::Loading.store(&worker_stage);
                let transcriber = crate::local_transcriber::LocalTranscriber::load(&selection)?;
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                Ok(PreparedTranscription {
                    selection,
                    transcriber,
                })
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
        });
        self.transcription_preparation = Some(TranscriptionPreparation {
            canceled,
            model,
            progress,
            result: receiver,
            stage,
            worker: Some(worker),
        });
        self.transcription_error = None;
        Ok(())
    }

    fn cancel_transcription_preparation(&mut self) {
        if let Some(preparation) = &self.transcription_preparation {
            preparation.canceled.store(true, Ordering::Relaxed);
        }
        self.transcription_error = None;
    }
}

impl LinuxApp {
    fn close_popups(&mut self) {
        self.catalog_filter_dropdown_open = false;
        self.transcription_dropdown_open = false;
        self.dictation_language_dropdown_open = false;
        self.microphone_dropdown_open = false;
    }

    fn render_transcription_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.transcription_dropdown_bounds else {
            return div().into_any_element();
        };
        let selection = self.host.settings.transcription.clone();
        let models: Vec<(String, crate::transcription_models::TranscriptionModelId)> =
            crate::transcription_models::available_models()
                .into_iter()
                .map(|model| {
                    let installed = crate::transcription_models::is_installed(
                        model,
                        crate::transcription_models::AUTO_LANGUAGE,
                    ) && crate::transcription_models::is_verified(model);
                    let state = if installed {
                        "Installed".to_string()
                    } else {
                        model.size_label()
                    };
                    (format!("{} · {state}", model.name), model.id)
                })
                .collect();
        let panel_rows = models.len();
        let items = models
            .into_iter()
            .enumerate()
            .map(|(index, (label, model))| {
                let selected = selection.model == model;
                let preferred = selection.language.clone();
                dropdown_item(("linux-model-option", index), label, selected).on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(language) =
                            crate::transcription_models::language_for_model(model, &preferred)
                            && this.host.choose_transcription(model, language).is_ok()
                        {
                            this.transcription_dropdown_open = false;
                        }
                        cx.notify();
                    },
                ))
            });
        dropdown_backdrop("linux-transcription-dropdown-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.transcription_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(300.0))
                    .id("linux-transcription-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    fn render_dictation_language_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.dictation_language_dropdown_bounds else {
            return div().into_any_element();
        };
        let selection = self.host.settings.transcription.clone();
        let model = selection.model;
        let definition = crate::transcription_models::definition(model);
        let languages: Vec<(String, String)> = crate::transcription_models::LANGUAGES
            .iter()
            .filter(|(code, _)| definition.supports_language(code))
            .map(|(code, name)| ((*code).to_string(), (*name).to_string()))
            .collect();
        let panel_rows = languages.len();
        let items = languages
            .into_iter()
            .enumerate()
            .map(|(index, (code, name))| {
                let selected = selection.language == code;
                dropdown_item(("linux-dictation-language-option", index), name, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.host.choose_transcription(model, code.clone()).is_ok() {
                            this.dictation_language_dropdown_open = false;
                        }
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("linux-dictation-language-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.dictation_language_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(260.0))
                    .id("linux-dictation-language-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    fn render_microphone_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.microphone_dropdown_bounds else {
            return div().into_any_element();
        };
        let current = self.host.settings.microphone.clone();
        let mut choices = vec![("Automatic".to_string(), None)];
        choices.extend(
            self.host
                .microphones
                .iter()
                .cloned()
                .map(|microphone| (microphone.clone(), Some(microphone))),
        );
        let panel_rows = choices.len() + usize::from(self.host.microphone_error.is_some()) * 2;
        let items = choices
            .into_iter()
            .enumerate()
            .map(|(index, (label, selection))| {
                let selected = selection == current;
                dropdown_item(("linux-microphone-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.host.set_microphone(selection.clone()).is_ok() {
                            this.microphone_dropdown_open = false;
                        }
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("linux-microphone-dropdown-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.microphone_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(360.0))
                    .id("linux-microphone-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items)
                    .when_some(self.host.microphone_error.clone(), |list, error| {
                        list.child(error_message("Microphones could not be enumerated.", error))
                    }),
            )
            .into_any_element()
    }

    fn capture_hotkey(&mut self, keystroke: &Keystroke) -> bool {
        if !self.capturing_hotkey {
            return false;
        }
        if keystroke.key == "escape" {
            self.capturing_hotkey = false;
            return true;
        }
        let shortcut = DesktopShortcut {
            alt: keystroke.modifiers.alt,
            control: keystroke.modifiers.control,
            function: keystroke.modifiers.function,
            key: keystroke.key.clone(),
            platform: keystroke.modifiers.platform,
            shift: keystroke.modifiers.shift,
        };
        if self
            .host
            .dispatch(DesktopAction::SetDictationShortcut(shortcut))
            .is_ok()
        {
            self.capturing_hotkey = false;
        }
        true
    }
}

fn start_update_check() -> Receiver<Result<Option<InstalledUpdate>, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = crate::linux_updater::install_latest().map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

impl Drop for LinuxDesktopHost {
    fn drop(&mut self) {
        self.cancel_transcription_preparation();
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            stop.store(true, Ordering::Relaxed);
        }
        self.join_listener();
        if let Some(mut preparation) = self.transcription_preparation.take()
            && let Some(worker) = preparation.worker.take()
        {
            let _ = worker.join();
        }
    }
}

impl DesktopHost for LinuxDesktopHost {
    fn capabilities(&self) -> DesktopCapabilities {
        DesktopCapabilities::linux_x11()
    }

    fn snapshot(&self) -> DesktopSnapshot {
        let update_status = match &self.update {
            UpdateState::Unmanaged => DesktopUpdateStatus::Unavailable,
            UpdateState::Checking(_) => DesktopUpdateStatus::Checking,
            UpdateState::Failed(_) => DesktopUpdateStatus::Failed,
            UpdateState::Waiting(_) => DesktopUpdateStatus::Current,
            UpdateState::Ready(_) => DesktopUpdateStatus::ReadyToRestart,
        };
        DesktopSnapshot {
            activity: self.activity.clone(),
            dictation_shortcut: self.settings.dictation_hotkey.keycaps(),
            dictation_shortcut_label: self.settings.dictation_hotkey.label(),
            double_tap_lock: self.settings.double_tap_lock,
            double_tap_only: false,
            paste_last_shortcut: None,
            listener: Some(DesktopListenerSnapshot {
                running: self.is_running(),
                status: self.status.clone(),
            }),
            operation_error: self.error.clone().or_else(|| self.settings_error.clone()),
            observations_path: self.event_path.display().to_string(),
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: self
                    .transcription_preparation
                    .as_ref()
                    .map_or(0, |preparation| {
                        preparation.progress.load(Ordering::Relaxed)
                    }),
                error: self.transcription_error.clone(),
                preparation_stage: self.transcription_preparation.as_ref().map(|preparation| {
                    crate::transcription_models::ModelPreparationStage::load(&preparation.stage)
                }),
                selection: self.settings.transcription.clone(),
                preparing: self
                    .transcription_preparation
                    .as_ref()
                    .map(|preparation| preparation.model),
            },
            update_status,
        }
    }

    fn dispatch(&mut self, action: DesktopAction) -> Result<()> {
        match action {
            DesktopAction::ClearError => self.error = None,
            DesktopAction::RestartIntoUpdate => {
                if let Err(error) = self.restart_into_update() {
                    self.error = Some(format!("Could not restart HEX: {error:#}"));
                    return Err(error);
                }
            }
            DesktopAction::SetDictationShortcut(shortcut) => {
                self.set_dictation_hotkey(shortcut)?;
            }
            DesktopAction::SetDoubleTapLock(enabled) => {
                let mut candidate = self.settings.clone();
                candidate.double_tap_lock = enabled;
                if let Err(error) = candidate.save() {
                    self.error = Some(format!("Could not save double-tap setting: {error:#}"));
                    return Err(error);
                }
                self.settings = candidate;
                self.error = None;
                self.settings_error = None;
            }
            DesktopAction::SetDoubleTapOnly(_) => {
                return Err(eyre!("double-tap-only is unavailable on X11"));
            }
            DesktopAction::StartListening => self.start(),
            DesktopAction::StopListening => self.stop(),
        }
        Ok(())
    }
}

impl LinuxApp {
    fn render_shared_navigation(&self) -> AnyElement {
        debug_assert!(self.host.capabilities().listener_control);
        sidebar_frame()
            .w(px(SIDEBAR_WIDTH))
            .px(px(14.0))
            .pt(px(52.0))
            .pb_4()
            .flex()
            .flex_col()
            .child(
                navigation_item(NavigationIcon::Settings, true)
                    .id("linux-nav-settings")
                    .child("Settings"),
            )
            .child(div().flex_1())
            .into_any_element()
    }

    fn render_shared_settings(
        &mut self,
        snapshot: &DesktopSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = snapshot
            .listener
            .as_ref()
            .is_some_and(|listener| listener.running);
        let shortcut = if self.capturing_hotkey {
            div()
                .w(px(180.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .border_1()
                .border_color(rgb(TEXT_SOFT))
                .bg(rgb(SURFACE))
                .text_size(px(11.0))
                .child("Press a shortcut...")
                .into_any_element()
        } else {
            hotkey_keycaps(snapshot.dictation_shortcut.clone(), 1.0)
        };
        let update_ready = snapshot.update_status == DesktopUpdateStatus::ReadyToRestart;
        let transcription = &snapshot.transcription;
        let model_label = match transcription.preparing {
            Some(preparing) => format!(
                "Preparing {}",
                crate::transcription_models::definition(preparing).name
            ),
            None => crate::transcription_models::definition(transcription.selection.model)
                .name
                .to_string(),
        };
        let dictation_language_label = if transcription.selection.language == "auto" {
            "Auto".to_string()
        } else {
            crate::transcription_models::language_name(&transcription.selection.language)
                .to_string()
        };
        let microphone_label = self
            .host
            .settings
            .microphone
            .clone()
            .unwrap_or_else(|| "Automatic".into());
        // The catalog dialog shows this error itself while open; the panel
        // row covers dropdown-initiated failures only.
        let transcription_error = transcription
            .error
            .clone()
            .filter(|_| matches!(self.transcription_picker, TranscriptionPickerState::Closed));
        let listener_label = snapshot
            .listener
            .as_ref()
            .map_or("Ready", |listener| listener.status.as_str())
            .to_string();
        let device = snapshot
            .activity
            .device
            .clone()
            .unwrap_or_else(|| "Automatic microphone".into());
        let status_hint = format!(
            "{device} · Hold {} to dictate",
            snapshot.dictation_shortcut_label
        );
        let listener_action = header_button(if running {
            "Stop listening"
        } else {
            "Start listening"
        })
        .id("linux-listener-toggle")
        .on_click(cx.listener(move |this, _, _, cx| {
            let action = if running {
                DesktopAction::StopListening
            } else {
                DesktopAction::StartListening
            };
            let _ = this.host.dispatch(action);
            cx.notify();
        }));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header("Settings"))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_8()
                    .pt_1()
                    .pb_7()
                    .child(
                        div().w_full().flex().justify_center().child(
                            pane_content()
                                .relative()
                                .child(
                                    div().pt(px(20.0)).child(
                                        settings_panel().child(
                                            div()
                                                .w_full()
                                                .min_h(px(72.0))
                                                .px_4()
                                                .py_3()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap_4()
                                                .child(
                                                    div()
                                                        .min_w(px(0.0))
                                                        .flex()
                                                        .items_center()
                                                        .gap_3()
                                                        .child(
                                                            div()
                                                                .size(px(10.0))
                                                                .flex_none()
                                                                .rounded_full()
                                                                .bg(if running {
                                                                    rgb(SUCCESS)
                                                                } else {
                                                                    rgb(FAINT)
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .min_w(px(0.0))
                                                                .flex()
                                                                .flex_col()
                                                                .gap(px(2.0))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(14.0))
                                                                        .text_color(rgb(TEXT))
                                                                        .child(listener_label),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(12.0))
                                                                        .text_color(rgb(MUTED))
                                                                        .truncate()
                                                                        .child(status_hint),
                                                                ),
                                                        ),
                                                )
                                                .child(listener_action),
                                        ),
                                    ),
                                )
                                .child(settings_section_label("Dictation"))
                                .child(
                                    settings_panel()
                                        .child(settings_row(
                                            "Model",
                                            "Choose any on-device speech model",
                                            div()
                                                .flex()
                                                .items_center()
                                                .child(
                                                    header_button("Browse")
                                                        .id("linux-model-browse")
                                                        .mr_3()
                                                        .when(running, |button| {
                                                            button.opacity(0.5)
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if running {
                                                                    return;
                                                                }
                                                                this.close_popups();
                                                                let language = this
                                                                    .host
                                                                    .settings
                                                                    .transcription
                                                                    .language
                                                                    .clone();
                                                                this.catalog_language_filter =
                                                                    crate::desktop_model_catalog::catalog_filter_for_language(
                                                                        &language,
                                                                    );
                                                                this.transcription_picker =
                                                                    TranscriptionPickerState::Choosing(
                                                                        language,
                                                                    );
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .child(
                                                            disclosure_button(model_label)
                                                                .id("linux-model-setting")
                                                .when(running, |button| {
                                                    button.opacity(0.5)
                                                })
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        if running {
                                                                            return;
                                                                        }
                                                                        let open = this
                                                                            .transcription_dropdown_open;
                                                                        this.close_popups();
                                                                        this.transcription_dropdown_open =
                                                                            !open;
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        )
                                                        .child(
                                                            canvas(
                                                                {
                                                                    let entity = cx.entity();
                                                                    move |bounds, _, cx| {
                                                                        entity.update(cx, |this, _| {
                                                                            this.transcription_dropdown_bounds =
                                                                                Some(bounds);
                                                                        });
                                                                    }
                                                                },
                                                                |_, _, _, _| {},
                                                            )
                                                            .w_full()
                                                            .h(px(0.0)),
                                                        ),
                                                ),
                                        ))
                                        .child(settings_row(
                                            "Language",
                                            "The language you dictate in; Auto detects it",
                                            div()
                                                .relative()
                                                .child(
                                                    disclosure_button(dictation_language_label)
                                                        .id("linux-dictation-language")
                                                .when(running, |button| {
                                                    button.opacity(0.5)
                                                })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if running {
                                                                    return;
                                                                }
                                                                let open = this
                                                                    .dictation_language_dropdown_open;
                                                                this.close_popups();
                                                                this.dictation_language_dropdown_open =
                                                                    !open;
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    canvas(
                                                        {
                                                            let entity = cx.entity();
                                                            move |bounds, _, cx| {
                                                                entity.update(cx, |this, _| {
                                                                    this.dictation_language_dropdown_bounds =
                                                                        Some(bounds);
                                                                });
                                                            }
                                                        },
                                                        |_, _, _, _| {},
                                                    )
                                                    .w_full()
                                                    .h(px(0.0)),
                                                ),
                                        ))
                                        .child(settings_row(
                                            "Microphone",
                                            "Uses the selected input or the system default",
                                            div()
                                                .relative()
                                                .child(
                                                    disclosure_button(microphone_label)
                                                        .id("linux-microphone-setting")
                                                .when(running, |button| {
                                                    button.opacity(0.5)
                                                })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if running {
                                                                    return;
                                                                }
                                                                let open =
                                                                    this.microphone_dropdown_open;
                                                                this.close_popups();
                                                                if !open {
                                                                    this.host.refresh_microphones();
                                                                }
                                                                this.microphone_dropdown_open =
                                                                    !open;
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    canvas(
                                                        {
                                                            let entity = cx.entity();
                                                            move |bounds, _, cx| {
                                                                entity.update(cx, |this, _| {
                                                                    this.microphone_dropdown_bounds =
                                                                        Some(bounds);
                                                                });
                                                            }
                                                        },
                                                        |_, _, _, _| {},
                                                    )
                                                    .w_full()
                                                    .h(px(0.0)),
                                                ),
                                        ))
                                        .when_some(
                                            transcription_error,
                                            |panel, error| {
                                                panel.child(error_message(
                                                    "The transcription change did not apply.",
                                                    error,
                                                ))
                                            },
                                        )
                                        .child(
                                            settings_row(
                                                "Dictation shortcut",
                                                "Hold to dictate, release to transcribe",
                                                shortcut,
                                            )
                                            .border_b_0()
                                            .id("dictation-hotkey-setting")
                                            .when(running, |row| row.opacity(0.5))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if !running {
                                                        this.capturing_hotkey =
                                                            !this.capturing_hotkey;
                                                        let _ = this
                                                            .host
                                                            .dispatch(DesktopAction::ClearError);
                                                        cx.notify();
                                                    }
                                                }),
                                            ),
                                        ),
                                )
                                .child(settings_section_label("Behavior"))
                                .child(
                                    settings_panel().child(
                                        settings_row(
                                            "Double-tap to lock",
                                            "Double-tap the shortcut for hands-free dictation",
                                            toggle(if snapshot.double_tap_lock {
                                                1.0
                                            } else {
                                                0.0
                                            }),
                                        )
                                        .border_b_0()
                                        .id("double-tap-setting")
                                        .when(running, |row| row.opacity(0.5))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if !running {
                                                    let enabled =
                                                        !this.host.snapshot().double_tap_lock;
                                                    let _ = this.host.dispatch(
                                                        DesktopAction::SetDoubleTapLock(enabled),
                                                    );
                                                    cx.notify();
                                                }
                                            }),
                                        ),
                                    ),
                                )
                                .child(settings_section_label("Application"))
                                .child(
                                    settings_panel()
                                        .child(settings_row(
                                            "HEX",
                                            "Private local dictation for Linux",
                                            div().text_size(px(12.0)).text_color(rgb(MUTED)).child(
                                                format!("Version {}", env!("CARGO_PKG_VERSION")),
                                            ),
                                        ))
                                        .when(update_ready, |panel| {
                                            panel.child(settings_row(
                                                "Update ready",
                                                "Restart into the verified Linux release.",
                                                compact_button("Restart")
                                                    .id("restart-update")
                                                    .border_1()
                                                    .border_color(rgb(LINE))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if this
                                                            .host
                                                            .dispatch(
                                                                DesktopAction::RestartIntoUpdate,
                                                            )
                                                            .is_ok()
                                                        {
                                                            let _ = this.host.dispatch(
                                                                DesktopAction::StopListening,
                                                            );
                                                            cx.quit();
                                                        } else {
                                                            cx.notify();
                                                        }
                                                    })),
                                            ))
                                        }),
                                )
                                .when_some(snapshot.operation_error.clone(), |content, error| {
                                    content.child(settings_section_label("Problem")).child(
                                        settings_panel()
                                            .child(error_message(
                                                "HEX could not apply this change.",
                                                error,
                                            ))
                                            .child(
                                                compact_button("Dismiss")
                                                    .id("dismiss-linux-error")
                                                    .mx_4()
                                                    .mb_4()
                                                    .border_1()
                                                    .border_color(rgb(LINE))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let _ = this
                                                            .host
                                                            .dispatch(DesktopAction::ClearError);
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl crate::desktop_model_catalog::ModelCatalogDelegate for LinuxApp {
    fn catalog_language_filter(&self) -> Option<String> {
        self.catalog_language_filter.clone()
    }

    fn set_catalog_language_filter(&mut self, filter: Option<String>) {
        self.catalog_language_filter = filter;
    }

    fn catalog_filter_dropdown_open(&self) -> bool {
        self.catalog_filter_dropdown_open
    }

    fn set_catalog_filter_dropdown_open(&mut self, open: bool) {
        self.catalog_filter_dropdown_open = open;
    }

    fn catalog_filter_dropdown_bounds(&self) -> Option<Bounds<Pixels>> {
        self.catalog_filter_dropdown_bounds
    }

    fn set_catalog_filter_dropdown_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.catalog_filter_dropdown_bounds = Some(bounds);
    }

    fn report_transcription_error(&mut self, error: String) {
        self.host.transcription_error = Some(error);
    }
}

impl TranscriptionPickerDelegate for LinuxApp {
    fn cancel_transcription_preparation(&mut self) {
        self.host.cancel_transcription_preparation();
        if let TranscriptionPickerState::Preparing(language) = std::mem::replace(
            &mut self.transcription_picker,
            TranscriptionPickerState::Closed,
        ) {
            self.transcription_picker = TranscriptionPickerState::Choosing(language);
        }
    }

    fn choose_transcription_model(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
        _cx: &mut Context<Self>,
    ) {
        if self
            .host
            .choose_transcription(model, language.clone())
            .is_ok()
        {
            self.transcription_picker = TranscriptionPickerState::Preparing(language);
        }
    }

    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>) {
        self.transcription_picker = TranscriptionPickerState::Closed;
        cx.notify();
    }

    fn select_transcription_language(&mut self, language: String, cx: &mut Context<Self>) {
        if self.transcription_picker.language() != Some(&language) {
            self.host.cancel_transcription_preparation();
            self.transcription_picker = TranscriptionPickerState::Choosing(language);
            cx.notify();
        }
    }
}

impl Render for LinuxApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let snapshot = self.host.snapshot();
        let content = self.render_shared_settings(&snapshot, cx);
        let model_picker = if self.transcription_picker.language().is_some() {
            Some(crate::desktop_model_catalog::render_model_catalog(
                self,
                &snapshot.transcription,
                viewport.width,
                viewport.height,
                cx,
            ))
        } else {
            None
        };
        let catalog_filter_dropdown = self.catalog_filter_dropdown_open.then(|| {
            crate::desktop_model_catalog::render_model_catalog_filter_dropdown(
                self,
                viewport.height,
                cx,
            )
        });
        let transcription_dropdown = self
            .transcription_dropdown_open
            .then(|| self.render_transcription_dropdown(viewport.height, cx));
        let dictation_language_dropdown = self
            .dictation_language_dropdown_open
            .then(|| self.render_dictation_language_dropdown(viewport.height, cx));
        let microphone_dropdown = self
            .microphone_dropdown_open
            .then(|| self.render_microphone_dropdown(viewport.height, cx));
        window_frame()
            .child(self.render_shared_navigation())
            .child(div().flex_1().h_full().overflow_hidden().child(content))
            .children(transcription_dropdown)
            .children(dictation_language_dropdown)
            .children(microphone_dropdown)
            .children(model_picker)
            .children(catalog_filter_dropdown)
    }
}
