use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Keystroke, Timer, TitlebarOptions, Window,
    WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, rgb, size,
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
use crate::desktop_transcription_picker::{
    TranscriptionPickerDelegate, TranscriptionPickerModel, TranscriptionPickerProgress,
    TranscriptionPickerStatus, TranscriptionPickerView, render_transcription_picker,
    transcription_selection_is_active,
};
use crate::desktop_ui::{
    LINE, MUTED, NavigationIcon, SIDEBAR_WIDTH, SURFACE, TEXT_SOFT, compact_button,
    disclosure_button, hotkey_keycaps, navigation_item, pane_header, settings_panel, settings_row,
    settings_section_label, sidebar_frame, toggle, window_frame,
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
    transcriber: crate::linux_transcriber::LinuxTranscriber,
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
    prepared_transcriber: Option<crate::linux_transcriber::LinuxTranscriber>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    update: UpdateState,
}

struct LinuxApp {
    host: LinuxDesktopHost,
    capturing_hotkey: bool,
    transcription_picker: TranscriptionPickerState,
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
    Ok(Icon::from_rgba(data, SIZE, SIZE)?)
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
                let transcriber = crate::linux_transcriber::LinuxTranscriber::load(&selection)?;
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
        let transcription_label = format!(
            "{} · {}",
            crate::transcription_models::language_name(&transcription.selection.language),
            crate::transcription_models::definition(transcription.selection.model).name
        );
        let transcription_language = transcription.selection.language.clone();

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
                            div()
                                .w_full()
                                .max_w(px(788.0))
                                .relative()
                                .child(settings_section_label("DICTATION"))
                                .child(
                                    settings_panel()
                                        .child(settings_row(
                                            "Local transcription",
                                            "Language and on-device speech model",
                                            disclosure_button(transcription_label)
                                                .id("transcription-model-setting")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.transcription_picker =
                                                        TranscriptionPickerState::Choosing(
                                                            transcription_language.clone(),
                                                        );
                                                    cx.notify();
                                                })),
                                        ))
                                        .child(settings_row(
                                            "Microphone",
                                            "Automatically chooses the preferred available input",
                                            disclosure_button("Automatic"),
                                        ))
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
                                .child(settings_section_label("BEHAVIOR"))
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
                                .child(settings_section_label("APPLICATION"))
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
                                ),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn transcription_picker_view(
        &self,
        language: String,
        transcription: &DesktopTranscriptionSnapshot,
    ) -> TranscriptionPickerView {
        let models = crate::transcription_models::choices_for_runtime(&language, false)
            .into_iter()
            .map(|choice| {
                let model = choice.model;
                let installed = crate::transcription_models::is_installed(model, &language);
                let downloading = transcription.preparing == Some(model.id);
                let active = transcription_selection_is_active(
                    &transcription.selection,
                    model,
                    &language,
                    installed,
                );
                let progress = if downloading {
                    model.download_bytes().map_or(0.0, |bytes| {
                        (transcription.downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
                    })
                } else {
                    0.0
                };
                let status = if downloading {
                    let stage = transcription
                        .preparation_stage
                        .unwrap_or(crate::transcription_models::ModelPreparationStage::Downloading);
                    let (label, progress) = match stage {
                        crate::transcription_models::ModelPreparationStage::Downloading => (
                            format!("Downloading {:.0}%", progress * 100.0),
                            Some(TranscriptionPickerProgress::Downloading(progress)),
                        ),
                        crate::transcription_models::ModelPreparationStage::Verifying => {
                            ("Verifying model".into(), None)
                        }
                        crate::transcription_models::ModelPreparationStage::Loading => (
                            "Loading model".into(),
                            Some(TranscriptionPickerProgress::Loading(0.25)),
                        ),
                    };
                    TranscriptionPickerStatus::Preparing { label, progress }
                } else if active {
                    TranscriptionPickerStatus::Active
                } else {
                    TranscriptionPickerStatus::Available { installed }
                };
                TranscriptionPickerModel { choice, status }
            })
            .collect();
        TranscriptionPickerView {
            error: transcription.error.clone(),
            language,
            models,
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.host.snapshot();
        let content = self.render_shared_settings(&snapshot, cx);
        let model_picker =
            self.transcription_picker
                .language()
                .map(str::to_owned)
                .map(|language| {
                    render_transcription_picker(
                        self.transcription_picker_view(language, &snapshot.transcription),
                        cx,
                    )
                });
        window_frame()
            .child(self.render_shared_navigation())
            .child(div().flex_1().h_full().overflow_hidden().child(content))
            .children(model_picker)
    }
}
