use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Keystroke, Timer, Window, WindowBounds,
    WindowKind, WindowOptions, div, prelude::*, px, rgb, size,
};
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
    disclosure_button, hotkey_keycaps, navigation_item, pane_body, pane_content, pane_header,
    settings_panel, settings_row, settings_section_label, sidebar_frame, toggle, window_frame,
};
use crate::events::EventReader;
use crate::linux_desktop::TrayCommand;
use crate::linux_session::LinuxSession;
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
    stage: Arc<AtomicU8>,
    worker: JoinHandle<PreparationResult>,
}

#[derive(Clone)]
enum SettingsChange {
    Capture,
    Hotkey(crate::linux_settings::LinuxHotkey),
    DoubleTap(bool),
    PasteWithShift(bool),
}

struct SettingsEdit {
    change: SettingsChange,
    resume: bool,
    canceled: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<crate::linux_settings::LinuxSettings>>>,
}

struct LinuxDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    listener_worker: Option<JoinHandle<ListenerResult>>,
    session_before_start: Option<u64>,
    awaiting_session_start: bool,
    listen_when_ready: bool,
    status: String,
    error: Option<String>,
    dismissed_failure_at: Option<u64>,
    settings_error: Option<String>,
    settings: crate::linux_settings::LinuxSettings,
    settings_edit: Option<SettingsEdit>,
    prepared_transcriber: Option<crate::linux_transcriber::LinuxTranscriber>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    update: UpdateState,
}

struct LinuxApp {
    host: LinuxDesktopHost,
    quitting: bool,
    restart_requested: bool,
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

pub fn open(event_path: PathBuf, start_hidden: bool, shutdown: &'static AtomicBool) -> Result<()> {
    let listener_stop = Arc::new(Mutex::new(None));
    let quit_stop = listener_stop.clone();
    let (tray_sender, tray_commands) = mpsc::channel();
    let session = LinuxSession::detect();
    // GPUI cannot hide/reopen a native Wayland toplevel. Keep it manageable without a tray.
    let close_to_tray = !session.is_wayland()
        && match crate::linux_desktop::start_tray(tray_sender) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "system tray is unavailable; closing HEX will quit");
                false
            }
        };
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
                    app_id: Some("hex".into()),
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
                        quitting: false,
                        restart_requested: false,
                        transcription_picker: TranscriptionPickerState::Closed,
                    })
                },
            )
            .expect("could not open the HEX window");
        let app = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let quit_app = app.downgrade();
        let x11_window = Rc::new(Cell::new(None));
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
        let tray_x11_window = x11_window.clone();
        cx.spawn(async move |cx| {
            let mut had_tray = close_to_tray;
            loop {
                Timer::after(Duration::from_millis(250)).await;
                let has_tray = close_to_tray && crate::linux_desktop::tray_available();
                if had_tray && !has_tray && !session.is_wayland() {
                    let _ = set_x11_window_mapped(&tray_x11_window, true);
                    let _ = tray_window.update(cx, |_, window, _| window.activate_window());
                }
                had_tray = has_tray;
                while let Ok(command) = tray_commands.try_recv() {
                    match command {
                        TrayCommand::Show => {
                            if !session.is_wayland() {
                                let _ = set_x11_window_mapped(&tray_x11_window, true);
                            }
                            let _ = tray_window.update(cx, |_, window, _| {
                                window.activate_window();
                            });
                        }
                        TrayCommand::ToggleListening => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                if !app.quitting && app.host.is_running() {
                                    let _ = app.host.dispatch(DesktopAction::StopListening);
                                } else if !app.quitting {
                                    let _ = app.host.dispatch(DesktopAction::StartListening);
                                }
                                cx.notify();
                            });
                        }
                        TrayCommand::Quit => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                app.request_quit();
                                cx.notify();
                            });
                        }
                    }
                }
                if app
                    .update(cx, |this, cx| {
                        if shutdown.load(Ordering::Relaxed) {
                            this.request_quit();
                        }
                        this.host.refresh();
                        if this.quitting && !this.host.has_pending_work() {
                            if this.restart_requested
                                && this
                                    .host
                                    .dispatch(DesktopAction::RestartIntoUpdate)
                                    .is_err()
                            {
                                this.restart_requested = false;
                                this.quitting = false;
                            } else {
                                cx.quit();
                            }
                        }
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
                window.set_window_title("HEX");
                let close_app = cx.entity().downgrade();
                let close_x11_window = x11_window.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    if close_to_tray
                        && crate::linux_desktop::tray_available()
                        && set_x11_window_mapped(&close_x11_window, false)
                    {
                        return false;
                    }
                    // A missing X11 handle must not swallow close and strand a microphone.
                    if close_app
                        .update(cx, |app, cx| {
                            app.request_quit();
                            cx.notify();
                        })
                        .is_err()
                    {
                        cx.quit();
                    }
                    false
                });
                window.activate_window();
            })
            .ok();
        cx.activate(true);
        if start_hidden && close_to_tray {
            let _ = set_x11_window_mapped(&x11_window, false);
        }
        cx.on_app_quit(move |cx| {
            let _ = quit_app.update(cx, |app, _| app.request_quit());
            if let Some(stop) = quit_stop
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                stop.store(true, Ordering::Relaxed);
            }
            crate::linux_desktop::shutdown();
            async {}
        })
        .detach();
    });
    crate::linux_desktop::shutdown();
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
    let pid_atom = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
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
        let pid = connection
            .get_property(
                false,
                window,
                pid_atom,
                x11rb::protocol::xproto::AtomEnum::CARDINAL,
                0,
                1,
            )?
            .reply()?;
        if title == b"HEX"
            && pid.value32().and_then(|mut values| values.next()) == Some(std::process::id())
        {
            hex_window = Some(window);
            break;
        }
    }
    hex_window.ok_or_else(|| color_eyre::eyre::eyre!("HEX window not found"))
}

fn set_x11_window_mapped(cache: &Cell<Option<u32>>, mapped: bool) -> bool {
    // Keep a successful lookup so an unmapped window can be reopened, but never cache
    // failure: the WM may not have published its client list during login yet.
    let Some(window) = cache.get().or_else(|| find_hex_window().ok()) else {
        return false;
    };
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
    if let Err(error) = &result {
        tracing::warn!(%error, mapped, "could not change HEX window visibility");
    }
    cache.set(result.is_ok().then_some(window));
    result.is_ok()
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
            listener_worker: None,
            session_before_start: None,
            awaiting_session_start: false,
            listen_when_ready: false,
            status: "Ready".into(),
            error: None,
            dismissed_failure_at: None,
            settings_error: error,
            settings,
            settings_edit: None,
            prepared_transcriber: None,
            transcription_preparation: None,
            transcription_error: None,
            update,
        }
    }

    fn start(&mut self) {
        if self.settings_edit.is_some() {
            return;
        }
        self.listen_when_ready = true;
        if self.is_running() || self.transcription_preparation.is_some() {
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
            result.map_err(|error| format!("{error:#}"))
        });
        self.listener_worker = Some(worker);
        self.session_before_start = self.activity.session_started_at;
        self.awaiting_session_start = true;
        self.status = "Starting".into();
    }

    fn stop(&mut self) {
        self.listen_when_ready = false;
        if let Some(edit) = &mut self.settings_edit {
            edit.resume = false;
            edit.canceled.store(true, Ordering::Release);
        }
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

        let mut start_listener = false;
        if self
            .transcription_preparation
            .as_ref()
            .is_some_and(|preparation| preparation.worker.is_finished())
            && let Some(preparation) = self.transcription_preparation.take()
        {
            let was_canceled = preparation.canceled.load(Ordering::Relaxed);
            let result = preparation
                .worker
                .join()
                .unwrap_or_else(|_| Err("model preparation worker stopped unexpectedly".into()));
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

        if self
            .listener_worker
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(worker) = self.listener_worker.take()
        {
            let result = worker
                .join()
                .unwrap_or_else(|_| Err("listener worker stopped unexpectedly".into()));
            self.awaiting_session_start = false;
            self.listener_stop
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            match result {
                Ok(()) => self.status = "Ready".into(),
                Err(error) => {
                    self.status = "Unavailable".into();
                    self.error = Some(error);
                }
            }
        }

        self.refresh_settings_edit();

        self.activity.refresh(&mut self.event_reader);
        if self.awaiting_session_start
            && self.activity.session_started_at != self.session_before_start
        {
            self.awaiting_session_start = false;
        }
        if self.is_running()
            && self.listen_when_ready
            && !self.awaiting_session_start
            && let Some(status) = self.activity.state_label()
        {
            self.status = status.into();
        }
    }

    fn is_running(&self) -> bool {
        self.listener_worker.is_some()
    }

    fn set_dictation_hotkey(&mut self, shortcut: DesktopShortcut) -> Result<()> {
        if shortcut.function {
            self.error = Some("The Fn modifier cannot be registered on Linux".into());
            self.cancel_settings_edit();
            return Err(color_eyre::eyre::eyre!(
                "the Fn modifier cannot be registered on Linux"
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
        if let Some(edit) = &mut self.settings_edit {
            if matches!(edit.change, SettingsChange::Capture) && edit.worker.is_none() {
                edit.change = SettingsChange::Hotkey(binding);
            }
        } else {
            self.begin_settings_edit(SettingsChange::Hotkey(binding));
        }
        Ok(())
    }

    fn begin_settings_edit(&mut self, change: SettingsChange) {
        if self.settings_edit.is_some() {
            return;
        }
        if self.transcription_preparation.is_some() {
            self.error = Some("Wait for model preparation before editing shortcut settings".into());
            return;
        }
        let resume = self.is_running() && self.listen_when_ready;
        self.stop();
        self.error = None;
        self.settings_edit = Some(SettingsEdit {
            change,
            resume,
            canceled: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
    }

    fn cancel_settings_edit(&mut self) {
        if let Some(edit) = &self.settings_edit {
            edit.canceled.store(true, Ordering::Release);
        }
    }

    fn capturing_hotkey(&self) -> bool {
        self.settings_edit.as_ref().is_some_and(|edit| {
            !edit.canceled.load(Ordering::Acquire) && matches!(edit.change, SettingsChange::Capture)
        })
    }

    fn has_pending_work(&self) -> bool {
        self.is_running()
            || self.settings_edit.is_some()
            || self.transcription_preparation.is_some()
    }

    fn refresh_settings_edit(&mut self) {
        if self.is_running() {
            return;
        }
        let Some(edit) = &mut self.settings_edit else {
            return;
        };
        if edit
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        let canceled = edit.canceled.load(Ordering::Acquire);
        if let Some(worker) = edit.worker.take() {
            match worker.join() {
                // Once persistence completed, keep the in-memory projection consistent even
                // if Cancel raced with the final atomic save.
                Ok(Ok(settings)) => {
                    self.settings = settings;
                    self.settings_error = None;
                }
                Ok(Err(error)) if !canceled => {
                    self.error = Some(format!("Could not update dictation settings: {error:#}"));
                }
                Err(_) if !canceled => {
                    self.error = Some("Settings worker stopped unexpectedly".into())
                }
                _ => {}
            }
        } else if !canceled {
            let wayland = LinuxSession::detect().is_wayland();
            if !wayland && matches!(edit.change, SettingsChange::Capture) {
                // X11 uses focused GPUI keystrokes, never raw input-device access.
                self.status = "Press a shortcut".into();
                return;
            }
            let change = edit.change.clone();
            let mut candidate = self.settings.clone();
            let canceled = edit.canceled.clone();
            edit.worker = Some(std::thread::spawn(move || {
                let binding = match change {
                    SettingsChange::Capture => {
                        Some(crate::linux_input::capture_wayland_binding(&canceled)?)
                    }
                    SettingsChange::Hotkey(binding) => Some(binding),
                    SettingsChange::DoubleTap(enabled) => {
                        candidate.double_tap_lock = enabled;
                        None
                    }
                    SettingsChange::PasteWithShift(enabled) => {
                        candidate.paste_with_shift = enabled;
                        None
                    }
                };
                if let Some(binding) = binding {
                    binding.validate()?;
                    crate::linux_input::LinuxHotkeyMonitor::start(binding.clone(), false)?;
                    candidate.dictation_hotkey = binding;
                }
                if canceled.load(Ordering::Acquire) {
                    return Err(color_eyre::eyre::eyre!("Shortcut edit canceled"));
                }
                candidate.save()?;
                Ok(candidate)
            }));
            self.status = if self.capturing_hotkey() {
                "Press a shortcut"
            } else {
                "Saving settings"
            }
            .into();
            return;
        }
        let resume = self.settings_edit.take().is_some_and(|edit| edit.resume);
        self.status = "Ready".into();
        if resume {
            self.start();
        }
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
        if self.is_running() || self.settings_edit.is_some() {
            let error = "Stop listening and finish shortcut editing before changing the transcription model".to_string();
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
        let worker = std::thread::spawn(move || {
            (|| {
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
            .map_err(|error| format!("{error:#}"))
        });
        self.transcription_preparation = Some(TranscriptionPreparation {
            canceled,
            model,
            progress,
            stage,
            worker,
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
    fn request_quit(&mut self) {
        self.quitting = true;
        self.host.stop();
        self.host.cancel_transcription_preparation();
    }

    fn capture_hotkey(&mut self, keystroke: &Keystroke) -> bool {
        if !self.host.capturing_hotkey() {
            return false;
        }
        if keystroke.key == "escape" {
            self.host.cancel_settings_edit();
            return true;
        }
        if LinuxSession::detect().is_wayland() || self.host.is_running() {
            return false;
        }
        let shortcut = DesktopShortcut {
            alt: keystroke.modifiers.alt,
            control: keystroke.modifiers.control,
            function: keystroke.modifiers.function,
            key: keystroke.key.clone(),
            platform: keystroke.modifiers.platform,
            shift: keystroke.modifiers.shift,
        };
        let _ = self
            .host
            .dispatch(DesktopAction::SetDictationShortcut(shortcut));
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
        self.stop();
        self.cancel_transcription_preparation();
        self.listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if self
            .listener_worker
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(worker) = self.listener_worker.take()
        {
            let _ = worker.join();
        }
        if self
            .transcription_preparation
            .as_ref()
            .is_some_and(|preparation| preparation.worker.is_finished())
            && let Some(preparation) = self.transcription_preparation.take()
        {
            let _ = preparation.worker.join();
        }
        if let Some(edit) = &mut self.settings_edit
            && edit.worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = edit.worker.take()
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
            operation_error: self
                .error
                .clone()
                .or_else(|| self.settings_error.clone())
                .or_else(|| {
                    self.activity
                        .last_failure
                        .as_ref()
                        .filter(|(at, _)| Some(*at) != self.dismissed_failure_at)
                        .map(|(_, message)| message.clone())
                }),
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
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
                self.dismissed_failure_at = self.activity.last_failure.as_ref().map(|(at, _)| *at);
            }
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
                self.begin_settings_edit(SettingsChange::DoubleTap(enabled));
            }
            DesktopAction::SetDoubleTapOnly(_) => {
                return Err(color_eyre::eyre::eyre!(
                    "double-tap-only is unavailable on X11"
                ));
            }
            DesktopAction::StartListening => {
                self.error = None;
                self.dismissed_failure_at = self.activity.last_failure.as_ref().map(|(at, _)| *at);
                self.start();
            }
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
        let editing = self.host.settings_edit.is_some();
        let paste_with_shift = self.host.settings.paste_with_shift;
        let shortcut = if self.host.capturing_hotkey() {
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
                .child(if running {
                    "Stopping listener..."
                } else {
                    "Press a shortcut..."
                })
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
                pane_body().child(div()
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
                                .child(settings_section_label("LISTENER"))
                                .child(settings_panel().child(settings_row(
                                    "Dictation",
                                    "Global hotkey dictation",
                                    div().flex().items_center().gap_3()
                                        .child(div().text_size(px(12.0)).text_color(rgb(MUTED))
                                            .child(snapshot.listener.as_ref().map_or_else(|| "Ready".to_string(), |listener| listener.status.clone())))
                                        .child(compact_button(if running { "Stop" } else { "Start" })
                                        .id("linux-listener-toggle")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !this.quitting {
                                                let _ = this.host.dispatch(if running {
                                                    DesktopAction::StopListening
                                                } else {
                                                    DesktopAction::StartListening
                                                });
                                                cx.notify();
                                            }
                                        }))),
                                )))
                                .when_some(snapshot.operation_error.clone(), |content, error| {
                                    content.child(
                                        div().mt_3().p_3().rounded(px(6.0)).border_1().border_color(rgb(LINE))
                                            .child(div().text_size(px(12.0)).child(error))
                                            .child(div().mt_2().flex().gap_2()
                                                .when(!running, |buttons| buttons.child(compact_button("Retry").id("linux-error-retry")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if !this.quitting {
                                                            let _ = this.host.dispatch(DesktopAction::StartListening);
                                                            cx.notify();
                                                        }
                                                    }))))
                                                .child(compact_button("Dismiss").id("linux-error-dismiss")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let _ = this.host.dispatch(DesktopAction::ClearError);
                                                        cx.notify();
                                                    })))),
                                    )
                                })
                                .when_some(crate::linux_desktop::hud_limitation(), |content, limitation| {
                                    content.child(div().mt_3().text_size(px(12.0)).text_color(rgb(MUTED)).child(limitation))
                                })
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
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if !this.quitting {
                                                        if this.host.capturing_hotkey() {
                                                            this.host.cancel_settings_edit();
                                                        } else {
                                                            this.host.begin_settings_edit(SettingsChange::Capture);
                                                        }
                                                        cx.notify();
                                                    }
                                                }),
                                            ),
                                        )
                                        .when(editing, |panel| panel.child(settings_row(
                                            "Updating shortcut settings",
                                            "Listening resumes after the change if it was previously running.",
                                            compact_button("Cancel").id("linux-cancel-shortcut")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.host.cancel_settings_edit();
                                                    cx.notify();
                                                })),
                                        ))),
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
                                        .id("double-tap-setting")
                                        .when(editing, |row| row.opacity(0.5))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if !this.quitting && !editing {
                                                    let enabled =
                                                        !this.host.snapshot().double_tap_lock;
                                                    let _ = this.host.dispatch(
                                                        DesktopAction::SetDoubleTapLock(enabled),
                                                    );
                                                    cx.notify();
                                                }
                                            }),
                                        ),
                                    ).child(
                                        settings_row(
                                            "Terminal paste shortcut",
                                            "Use Ctrl-Shift-V instead of Ctrl-V",
                                            toggle(if paste_with_shift { 1.0 } else { 0.0 }),
                                        )
                                        .border_b_0()
                                        .id("terminal-paste-setting")
                                        .when(editing, |row| row.opacity(0.5))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !this.quitting && !editing {
                                                this.host.begin_settings_edit(SettingsChange::PasteWithShift(!paste_with_shift));
                                                cx.notify();
                                            }
                                        })),
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
                                        .child(settings_row(
                                            "Quit HEX",
                                            "Stop listening and close the application",
                                            compact_button(if self.quitting { "Stopping..." } else { "Quit" })
                                                .id("linux-quit")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.request_quit();
                                                    cx.notify();
                                                })),
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
                                                        this.restart_requested = true;
                                                        this.request_quit();
                                                        cx.notify();
                                                    })),
                                            ))
                                        }),
                                ),
                        ),
                    )),
            )
            .into_any_element()
    }

    fn transcription_picker_view(
        &self,
        language: String,
        transcription: &DesktopTranscriptionSnapshot,
    ) -> TranscriptionPickerView {
        let models = crate::transcription_models::choices_for_runtime(&language)
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
        if self.quitting {
            return;
        }
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
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .overflow_hidden()
                    .child(content),
            )
            .children(model_picker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::{
        ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
    };

    fn host_for_edit(running: bool) -> LinuxDesktopHost {
        let settings = crate::linux_settings::LinuxSettings {
            transcription: TranscriptionSelection {
                // Prevent the restart assertions from opening a real microphone.
                model: TranscriptionModelId::AppleSpeech,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            Arc::new(Mutex::new(None)),
            settings,
            None,
            UpdateState::Unmanaged,
        );
        if running {
            host.listener_worker = Some(std::thread::spawn(|| Ok(())));
            *host.listener_stop.lock().unwrap() = Some(Arc::new(AtomicBool::new(false)));
            host.listen_when_ready = true;
        }
        host
    }

    fn set_finished_edit(
        host: &mut LinuxDesktopHost,
        result: Result<crate::linux_settings::LinuxSettings>,
    ) {
        let worker = std::thread::spawn(move || result);
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        host.settings_edit.as_mut().unwrap().worker = Some(worker);
    }

    #[test]
    fn listener_completion_waits_for_the_worker_and_preserves_errors() {
        for outcome in [Some(Ok(())), Some(Err("listener failed".into())), None] {
            let expected = outcome
                .clone()
                .unwrap_or_else(|| Err("listener worker stopped unexpectedly".into()))
                .err();
            let mut host = host_for_edit(true);
            let (release, held) = mpsc::channel();
            host.listener_worker = Some(std::thread::spawn(move || {
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                outcome.expect("fixture listener panic")
            }));
            host.refresh();
            assert!(host.is_running());
            assert!(host.listener_stop.lock().unwrap().is_some());
            release.send(()).unwrap();
            while !host.listener_worker.as_ref().unwrap().is_finished() {
                std::thread::yield_now();
            }
            host.refresh();
            assert!(!host.is_running());
            assert!(host.listener_stop.lock().unwrap().is_none());
            assert_eq!(host.error, expected);
            assert_eq!(
                host.status,
                if expected.is_some() {
                    "Unavailable"
                } else {
                    "Ready"
                }
            );
        }
    }

    #[test]
    fn preparation_completion_preserves_selection_and_honors_cancellation() {
        for (canceled, panics) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut host = host_for_edit(false);
            let original = host.settings.clone();
            let (release, held) = mpsc::channel();
            host.transcription_preparation = Some(TranscriptionPreparation {
                canceled: Arc::new(AtomicBool::new(false)),
                model: TranscriptionModelId::default(),
                progress: Arc::new(AtomicU64::new(0)),
                stage: Arc::new(AtomicU8::new(ModelPreparationStage::Loading as u8)),
                worker: std::thread::spawn(move || {
                    held.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(!panics, "fixture preparation panic");
                    Err("model unavailable".into())
                }),
            });
            if canceled {
                host.cancel_transcription_preparation();
            }
            host.refresh();
            assert!(host.has_pending_work());
            release.send(()).unwrap();
            while !host
                .transcription_preparation
                .as_ref()
                .unwrap()
                .worker
                .is_finished()
            {
                std::thread::yield_now();
            }
            host.refresh();
            assert!(!host.has_pending_work());
            assert_eq!(host.settings, original);
            assert_eq!(
                host.transcription_error.as_deref(),
                if canceled {
                    None
                } else if panics {
                    Some("model preparation worker stopped unexpectedly")
                } else {
                    Some("model unavailable")
                }
            );
        }
    }

    #[test]
    fn preference_edits_wait_for_stop_and_only_restart_a_previously_running_listener() {
        for (running, paste) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut host = host_for_edit(running);
            let mut candidate = host.settings.clone();
            if paste {
                candidate.paste_with_shift = !candidate.paste_with_shift;
                host.begin_settings_edit(SettingsChange::PasteWithShift(
                    candidate.paste_with_shift,
                ));
            } else {
                candidate.double_tap_lock = !candidate.double_tap_lock;
                host.dispatch(DesktopAction::SetDoubleTapLock(candidate.double_tap_lock))
                    .unwrap();
            }
            assert_eq!(host.settings_edit.as_ref().unwrap().resume, running);
            if running {
                host.refresh_settings_edit();
                assert!(
                    host.listener_stop
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .load(Ordering::Relaxed)
                );
                assert!(host.settings_edit.as_ref().unwrap().worker.is_none());
                assert_ne!(host.settings, candidate);
            }
            host.listener_worker = None;
            host.listener_stop.lock().unwrap().take();
            set_finished_edit(&mut host, Ok(candidate.clone()));
            host.refresh_settings_edit();
            assert_eq!(host.settings, candidate);
            assert!(host.settings_edit.is_none());
            assert_eq!(host.listen_when_ready, running);
        }
    }

    #[test]
    fn canceled_or_failed_capture_restores_prior_listening_without_changing_settings() {
        for running in [false, true] {
            for cancel in [false, true] {
                let mut host = host_for_edit(running);
                let original = host.settings.clone();
                host.begin_settings_edit(SettingsChange::Capture);
                assert!(host.capturing_hotkey());
                host.listener_worker = None;
                host.listener_stop.lock().unwrap().take();
                if cancel {
                    host.cancel_settings_edit();
                } else {
                    set_finished_edit(
                        &mut host,
                        Err(color_eyre::eyre::eyre!("input permission denied")),
                    );
                }
                host.refresh_settings_edit();
                assert_eq!(host.settings, original);
                assert_eq!(host.listen_when_ready, running);
                assert_eq!(host.error.is_some(), !cancel);
                assert!(host.settings_edit.is_none());
                assert!(!host.capturing_hotkey());
            }
        }
    }

    #[test]
    fn cancel_signals_worker_and_does_not_wait_or_restart_until_it_finishes() {
        let mut host = host_for_edit(true);
        host.begin_settings_edit(SettingsChange::Capture);
        host.listener_worker = None;
        host.listener_stop.lock().unwrap().take();
        let (release, held) = mpsc::channel();
        host.settings_edit.as_mut().unwrap().worker = Some(std::thread::spawn(move || {
            let _ = held.recv();
            Err(color_eyre::eyre::eyre!("capture canceled"))
        }));
        host.cancel_settings_edit();
        host.refresh_settings_edit();
        let edit = host.settings_edit.as_mut().unwrap();
        assert!(edit.canceled.load(Ordering::Acquire));
        assert!(edit.worker.is_some());
        assert!(!host.listen_when_ready);
        release.send(()).unwrap();
        let _ = host
            .settings_edit
            .as_mut()
            .unwrap()
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap()
            .unwrap_err();
        host.refresh_settings_edit();
        assert!(host.listen_when_ready);
        assert!(host.settings_edit.is_none());
    }

    #[test]
    fn quit_during_edit_cancels_capture_and_suppresses_restart() {
        let mut app = LinuxApp {
            host: host_for_edit(true),
            quitting: false,
            restart_requested: false,
            transcription_picker: TranscriptionPickerState::Closed,
        };
        app.host.begin_settings_edit(SettingsChange::Capture);
        app.request_quit();
        assert!(app.quitting);
        assert!(app.host.has_pending_work());
        let edit = app.host.settings_edit.as_ref().unwrap();
        assert!(!edit.resume);
        assert!(edit.canceled.load(Ordering::Acquire));
        app.host.listener_worker = None;
        app.host.listener_stop.lock().unwrap().take();
        app.host.refresh_settings_edit();
        assert!(!app.host.listen_when_ready);
        assert!(!app.host.has_pending_work());
    }

    #[test]
    fn dismiss_clears_operational_and_settings_errors() {
        let mut host = host_for_edit(false);
        host.error = Some("listener failed".into());
        host.settings_error = Some("settings failed".into());
        host.dispatch(DesktopAction::ClearError).unwrap();
        assert!(host.snapshot().operation_error.is_none());
    }

    #[test]
    fn output_failures_are_visible_and_dismissal_only_acknowledges_that_failure() {
        let mut host = host_for_edit(false);
        host.activity.last_failure = Some((10, "paste helper failed".into()));
        assert_eq!(
            host.snapshot().operation_error.as_deref(),
            Some("paste helper failed")
        );
        host.dispatch(DesktopAction::ClearError).unwrap();
        assert!(host.snapshot().operation_error.is_none());
        host.activity.last_failure = Some((11, "paste helper failed".into()));
        assert_eq!(
            host.snapshot().operation_error.as_deref(),
            Some("paste helper failed")
        );
        host.dispatch(DesktopAction::StartListening).unwrap();
        assert!(host.snapshot().operation_error.is_none());
    }

    #[test]
    fn start_during_preparation_defers_listening_and_stop_clears_the_request() {
        let settings = crate::linux_settings::LinuxSettings {
            // An unavailable model prevents a regressed guard from opening audio.
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::AppleSpeech,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            Arc::new(Mutex::new(None)),
            settings,
            None,
            UpdateState::Unmanaged,
        );
        host.transcription_preparation = Some(TranscriptionPreparation {
            canceled: Arc::new(AtomicBool::new(false)),
            model: TranscriptionModelId::default(),
            progress: Arc::new(AtomicU64::new(0)),
            stage: Arc::new(AtomicU8::new(ModelPreparationStage::Downloading as u8)),
            worker: std::thread::spawn(|| Err("fixture preparation".into())),
        });

        host.dispatch(DesktopAction::StartListening).unwrap();

        assert!(host.listen_when_ready);
        assert_eq!(host.status, "Ready");
        assert!(!host.is_running());
        assert!(host.listener_worker.is_none());
        assert!(host.listener_stop.lock().unwrap().is_none());
        assert!(host.transcription_preparation.is_some());

        host.dispatch(DesktopAction::StopListening).unwrap();

        assert!(!host.listen_when_ready);
        assert!(host.transcription_preparation.is_some());
    }
}
