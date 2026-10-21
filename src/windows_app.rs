//! The Windows lifecycle around the shared app window: the tray, the HUD
//! window, and the listener host, which outlive the window while Hex hides
//! in the tray. The window itself is `app_window`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use gpui::{App, Application, Timer, Window, prelude::*};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{
    Icon, MouseButton as TrayMouseButton, MouseButtonState, TrayIcon, TrayIconBuilder,
    TrayIconEvent,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetSystemMetrics, GetWindowLongPtrW, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    SystemParametersInfoW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use crate::app_settings::{AppSettings, IndicatorPosition};
use crate::desktop_activity::DesktopActivity;
use crate::desktop_i18n::{tr, tr_fill};
use crate::events::{EventReader, TranscriptPhase, VoiceEvent};
use crate::history::History;

type ListenerResult = std::result::Result<(), String>;

/// The listener and the runtime state it shares with the dictation loop.
/// Settings reach it through [`WindowsDesktopHost::apply_settings`].
pub(crate) struct WindowsDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    /// When the current session's newest dictation completed.
    last_completed_at: Option<u64>,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    listener_result: Option<Receiver<ListenerResult>>,
    listener_worker: Option<JoinHandle<()>>,
    session_before_start: Option<u64>,
    awaiting_session_start: bool,
    listen_when_ready: bool,
    status: String,
    error: Option<String>,
    settings: AppSettings,
    history: Option<History>,
    last_dictation: Arc<Mutex<Option<String>>>,
    indicator: crate::windows_indicator::WindowsIndicatorSender,
    mode_runtime: Arc<RwLock<crate::dictation_modes::ModeRuntime>>,
    transformations: Arc<crate::personal_commands::TransformationClient>,
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_checked_at: Instant,
    voice_action_model: Arc<RwLock<Option<crate::dictation_processor::Model>>>,
    opencode_catalog_rx: Option<Receiver<Result<crate::dictation_processor::ModelCatalog>>>,
    hints_restart_after: Option<Instant>,
    updater: crate::windows_updater::WindowsUpdater,
}

/// What the window shows of the listener.
pub(crate) struct ListenerState {
    pub(crate) running: bool,
    pub(crate) status: String,
    pub(crate) last_failure: Option<String>,
    pub(crate) error: Option<String>,
}

enum TrayCommand {
    Show,
    ToggleListening,
    Quit,
}

pub fn open(event_path: PathBuf, shutdown: &'static AtomicBool, start_hidden: bool) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let (tray_sender, tray_commands) = mpsc::channel();
    let (indicator_sender, indicator_events) = crate::windows_indicator::channel();
    let (settings, settings_error) = match AppSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            AppSettings::default(),
            Some(tr_fill(
                "Could not load settings: {}",
                &format!("{error:#}"),
            )),
        ),
    };
    let listen_on_launch = settings.enable_on_launch;
    let indicator_position = settings.indicator_position;
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(crate::text_input::key_bindings());
        let host = Rc::new(RefCell::new(WindowsDesktopHost::new(
            event_path.clone(),
            settings.clone(),
            settings_error.clone(),
            indicator_sender.clone(),
        )));
        // The microphone stays cold until first-run setup finishes.
        let setup_done = crate::onboarding::completion_recorded();
        if listen_on_launch && setup_done {
            host.borrow_mut().start();
        }
        let window = crate::app_window::open(
            event_path.clone(),
            host.clone(),
            listen_on_launch && !setup_done,
            cx,
        )
        .expect("could not open the Hex window");
        let (indicator, indicator_hwnd, indicator_scale) = match cx
            .open_window(crate::windows_indicator::window_options(cx), |_, cx| {
                cx.new(|_| crate::windows_indicator::WindowsIndicator::new())
            }) {
            Ok(indicator_window) => {
                let indicator = indicator_window.update(cx, |_, _, cx| cx.entity()).ok();
                let placement = indicator_window
                    .update(cx, |_, window, _| {
                        (windows_hwnd(window), window.scale_factor())
                    })
                    .ok();
                let hwnd = placement.and_then(|(hwnd, _)| hwnd);
                let scale_factor = placement.map_or(1.0, |(_, scale_factor)| scale_factor);
                configure_indicator_window(hwnd, scale_factor, indicator_position);
                (indicator, hwnd, scale_factor)
            }
            Err(error) => {
                tracing::warn!(%error, "Windows dictation HUD is unavailable");
                (None, None, 1.0)
            }
        };
        let tray = match create_tray(tray_sender.clone()) {
            Ok(tray) => Some(tray),
            Err(error) => {
                tracing::warn!(%error, "Windows system tray is unavailable; closing Hex will quit");
                None
            }
        };
        let close_to_tray = tray.is_some();
        let hwnd = window
            .update(cx, |_, window, _| windows_hwnd(window))
            .ok()
            .flatten();
        crate::windows_ui::apply_window_icon(hwnd);
        let indicator_host = host.clone();
        cx.spawn(async move |cx| {
            // The pump is the HUD's display link: it forwards dictation
            // events, steps the springs once per tick, and keeps the OS
            // window on screen exactly while the model says the HUD is
            // visible, through transcription and the exit animation.
            let mut shown = false;
            loop {
                Timer::after(Duration::from_millis(16)).await;
                let Some(indicator) = &indicator else {
                    while indicator_events.try_recv().is_ok() {}
                    continue;
                };
                while let Ok(event) = indicator_events.try_recv() {
                    if indicator
                        .update(cx, |indicator, _| indicator.handle(event))
                        .is_err()
                    {
                        return;
                    }
                }
                let Ok(visible) = indicator.update(cx, |indicator, cx| indicator.tick(cx)) else {
                    return;
                };
                let position = indicator_host.borrow().indicator_position();
                let show = visible && position != IndicatorPosition::Hidden;
                if show != shown {
                    if show {
                        position_indicator_window(indicator_hwnd, indicator_scale, position);
                    }
                    set_window_visible(indicator_hwnd, show);
                    shown = show;
                }
            }
        })
        .detach();
        let lifecycle_host = host.clone();
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                while let Ok(command) = tray_commands.try_recv() {
                    match command {
                        TrayCommand::Show => {
                            set_window_visible(hwnd, true);
                            let _ = window.update(cx, |_, window, _| window.activate_window());
                        }
                        TrayCommand::ToggleListening => {
                            let mut host = lifecycle_host.borrow_mut();
                            if host.is_running() {
                                host.stop();
                            } else {
                                host.start();
                            }
                        }
                        TrayCommand::Quit => {
                            lifecycle_host.borrow_mut().stop();
                            let _ = cx.update(|cx| cx.quit());
                            return;
                        }
                    }
                }
                if shutdown.load(Ordering::Relaxed) {
                    lifecycle_host.borrow_mut().stop();
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
                lifecycle_host.borrow_mut().refresh();
            }
        })
        .detach();
        let close_host = host.clone();
        window
            .update(cx, |_, window, cx| {
                if close_to_tray {
                    window.on_window_should_close(cx, move |_, _| {
                        set_window_visible(hwnd, false);
                        false
                    });
                } else {
                    // Without a tray, closing must quit the whole app; the
                    // hidden HUD window would otherwise keep it alive.
                    window.on_window_should_close(cx, move |_, cx| {
                        close_host.borrow_mut().stop();
                        cx.quit();
                        true
                    });
                }
                window.activate_window();
            })
            .ok();
        if start_hidden && close_to_tray {
            set_window_visible(hwnd, false);
        }
        cx.on_app_quit(move |_| {
            let _keep_alive = (&tray, &host);
            async {}
        })
        .detach();
    });
    Ok(())
}

fn create_tray(commands: mpsc::Sender<TrayCommand>) -> Result<TrayIcon> {
    let menu = Menu::new();
    let show = MenuItem::with_id("show", tr("Show Hex"), true, None);
    let toggle = MenuItem::with_id("toggle", tr("Enable / Disable Hex"), true, None);
    let quit = MenuItem::with_id("quit", tr("Quit Hex"), true, None);
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
                button: TrayMouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = commands.send(TrayCommand::Show);
        }
    }));

    Ok(TrayIconBuilder::new()
        .with_tooltip(tr("Hex Dictation"))
        .with_icon(tray_icon()?)
        .with_menu(Box::new(menu))
        // Left click shows the window (handled above); only right click
        // opens the menu. The crate default menus on both buttons.
        .with_menu_on_left_click(false)
        .build()?)
}

fn tray_icon() -> Result<Icon> {
    Ok(Icon::from_rgba(
        crate::windows_ui::tray_icon_rgba(),
        crate::windows_ui::TRAY_ICON_SIZE,
        crate::windows_ui::TRAY_ICON_SIZE,
    )?)
}

fn windows_hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
        _ => None,
    }
}

fn set_window_visible(hwnd: Option<HWND>, visible: bool) {
    if let Some(hwnd) = hwnd {
        unsafe {
            ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

fn configure_indicator_window(hwnd: Option<HWND>, scale_factor: f32, position: IndicatorPosition) {
    let Some(hwnd) = hwnd else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (style | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as isize,
        );
        ShowWindow(hwnd, SW_HIDE);
    }
    crate::windows_ui::disable_window_tint(hwnd);
    crate::windows_ui::strip_popup_chrome(hwnd);
    position_indicator_window(Some(hwnd), scale_factor, position);
}

/// GPUI defers popup placement until the first activation, but the HUD is
/// shown without ever being activated; place the pill-sized bounds explicitly
/// in physical pixels or the window keeps its huge creation-default rectangle.
fn position_indicator_window(hwnd: Option<HWND>, scale_factor: f32, position: IndicatorPosition) {
    let Some(hwnd) = hwnd else {
        return;
    };
    let width = (crate::windows_indicator::WINDOW_WIDTH * scale_factor).round() as i32;
    let height = (crate::windows_indicator::WINDOW_HEIGHT * scale_factor).round() as i32;
    unsafe {
        let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
        let y = match position {
            IndicatorPosition::Bottom => {
                let mut work_area = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let bottom =
                    if SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut work_area).cast(), 0)
                        != 0
                    {
                        work_area.bottom
                    } else {
                        GetSystemMetrics(SM_CYSCREEN)
                    };
                bottom
                    - height
                    - (crate::windows_indicator::BOTTOM_OFFSET * scale_factor).round() as i32
            }
            IndicatorPosition::Top | IndicatorPosition::Hidden => {
                (crate::windows_indicator::TOP_OFFSET * scale_factor).round() as i32
            }
        };
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
    }
}

use crate::windows_dictation::voice_action_model;

/// The settings a running listener captured when it started; a change
/// restarts it.
fn listener_settings_changed(previous: &AppSettings, current: &AppSettings) -> bool {
    previous.microphone != current.microphone
        || previous.dictation_hotkey != current.dictation_hotkey
        || previous.paste_last_hotkey != current.paste_last_hotkey
        || previous.voice_action.enabled != current.voice_action.enabled
        || (current.voice_action.enabled && previous.edit_hotkey != current.edit_hotkey)
        || previous.double_tap_lock != current.double_tap_lock
        || previous.double_tap_only != current.double_tap_only
        || previous.pass_through_dictation_shortcut != current.pass_through_dictation_shortcut
        || previous.shortcut_mode != current.shortcut_mode
        || previous.recording_audio_behavior != current.recording_audio_behavior
        || previous.release_microphone_while_idle != current.release_microphone_while_idle
        || previous.transcription.model != current.transcription.model
        || previous.transcription.language != current.transcription.language
}

impl WindowsDesktopHost {
    fn new(
        event_path: PathBuf,
        settings: AppSettings,
        settings_error: Option<String>,
        indicator: crate::windows_indicator::WindowsIndicatorSender,
    ) -> Self {
        crate::desktop_i18n::apply(settings.ui_language.as_deref());
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        let last_completed_at = last_completed_at(event_reader.events().iter());
        let history = match History::open_default(settings.history_retention) {
            Ok(history) => Some(history),
            Err(error) => {
                tracing::warn!(%error, "could not open retained history");
                None
            }
        };
        let mode_runtime = Arc::new(RwLock::new(
            crate::dictation_modes::ModeRuntime::from_settings(&settings),
        ));
        let voice_action_model = Arc::new(RwLock::new(voice_action_model(&settings)));
        let mut host = Self {
            event_path,
            event_reader,
            activity,
            last_completed_at,
            listener_stop: Arc::new(Mutex::new(None)),
            listener_result: None,
            listener_worker: None,
            session_before_start: None,
            awaiting_session_start: false,
            listen_when_ready: false,
            status: "Disabled".into(),
            error: settings_error,
            settings,
            history,
            last_dictation: Arc::new(Mutex::new(None)),
            indicator,
            mode_runtime,
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
            personal_commands: None,
            personal_commands_checked_at: Instant::now(),
            voice_action_model,
            opencode_catalog_rx: None,
            hints_restart_after: None,
            updater: crate::windows_updater::WindowsUpdater::start(),
        };
        host.refresh_personal_commands_host();
        host.adopt_default_voice_action_model();
        host
    }

    fn model_ready(&self) -> bool {
        let model = crate::transcription_models::definition(self.settings.transcription.model);
        crate::transcription_models::is_installed(model, &self.settings.transcription.language)
            && crate::transcription_models::is_verified(model)
    }

    pub(crate) fn start(&mut self) {
        self.listen_when_ready = true;
        if self.listener_result.is_some() {
            return;
        }
        if !self.model_ready() {
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
        let selection = self.runtime_selection();
        let config = crate::windows_dictation::WindowsDictationConfig {
            history: self.history.clone(),
            last_dictation: self.last_dictation.clone(),
            indicator: Some(self.indicator.clone()),
            mode_runtime: self.mode_runtime.clone(),
            transformations: self.transformations.clone(),
            voice_action_model: self.voice_action_model.clone(),
            ..crate::windows_dictation::WindowsDictationConfig::from_settings(&self.settings)
        };
        let worker = std::thread::Builder::new()
            .name("windows-listener".into())
            .spawn(move || {
                let result = crate::instance::acquire("windows-listener").and_then(|_instance| {
                    crate::windows_dictation::run(&event_path, &selection, config, &stop)
                });
                let _ = result_sender.send(result.map_err(|error| format!("{error:#}")));
            });
        match worker {
            Ok(worker) => {
                self.listener_worker = Some(worker);
                self.listener_result = Some(result_receiver);
                self.session_before_start = self.activity.session_started_at;
                self.awaiting_session_start = true;
                self.status = "Enabling…".into();
                self.error = None;
            }
            Err(error) => {
                self.status = "Couldn't enable".into();
                self.error = Some(tr_fill("Could not start listener: {}", &error.to_string()));
                self.listener_stop
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        self.listen_when_ready = false;
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            stop.store(true, Ordering::Relaxed);
            self.status = "Disabling…".into();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.listener_result.is_some()
    }

    pub(crate) fn history(&self) -> Option<History> {
        self.history.clone()
    }

    pub(crate) fn indicator_position(&self) -> IndicatorPosition {
        self.settings.indicator_position
    }

    pub(crate) fn update_state(&self) -> crate::windows_updater::UpdateCheck {
        self.updater.state()
    }

    pub(crate) fn check_for_updates(&mut self) {
        self.updater.request_check();
    }

    /// Starts the installed update; the caller quits Hex afterwards.
    pub(crate) fn restart_into_update(&mut self) -> Result<()> {
        let crate::windows_updater::UpdateCheck::ReadyToRestart { executable, .. } =
            self.updater.state()
        else {
            return Err(eyre!("no installed Windows update is ready"));
        };
        crate::windows_updater::relaunch_at(&executable).inspect_err(|error| {
            self.error = Some(tr_fill("Could not restart Hex: {}", &format!("{error:#}")));
        })?;
        self.stop();
        Ok(())
    }

    pub(crate) fn listener_state(&self) -> ListenerState {
        ListenerState {
            running: self.is_running(),
            status: self.status.clone(),
            last_failure: visible_failure(&self.activity, self.last_completed_at),
            error: self.error.clone(),
        }
    }

    /// Applies the settings the shared window saved. Shared state the
    /// listener reads changes in place; anything the listener captured at
    /// start restarts a running listener.
    pub(crate) fn apply_settings(&mut self, settings: &AppSettings) {
        let previous = std::mem::replace(&mut self.settings, settings.clone());
        let current = settings;
        let modes_changed = previous.dictation_processing != current.dictation_processing;
        if modes_changed {
            *self
                .mode_runtime
                .write()
                .unwrap_or_else(|error| error.into_inner()) =
                crate::dictation_modes::ModeRuntime::from_settings(current);
        }
        if previous.voice_action != current.voice_action {
            *self
                .voice_action_model
                .write()
                .unwrap_or_else(|error| error.into_inner()) = voice_action_model(current);
            self.adopt_default_voice_action_model();
        }
        if previous.ui_language != current.ui_language {
            crate::desktop_i18n::apply(current.ui_language.as_deref());
        }
        if previous.transcription.recognition_hints != current.transcription.recognition_hints
            && crate::transcription_models::definition(current.transcription.model)
                .supports_recognition_hints
            && self.is_running()
        {
            self.hints_restart_after = Some(Instant::now());
        }
        if modes_changed {
            self.refresh_personal_commands_host();
        }
        let model_changed = previous.transcription.model != current.transcription.model
            || previous.transcription.language != current.transcription.language;
        if listener_settings_changed(&previous, current) && self.is_running() {
            self.restart_listener_for_settings();
        } else if model_changed && self.listen_when_ready && !self.is_running() {
            // A listener waiting for a model starts once one is chosen.
            self.start();
        }
    }

    /// With an enabled voice action and no chosen model, voice actions use
    /// OpenCode's default model; settings keep "default" so the choice
    /// follows OpenCode.
    fn adopt_default_voice_action_model(&mut self) {
        if self.settings.voice_action.enabled
            && self.settings.voice_action.model.is_none()
            && self.opencode_catalog_rx.is_none()
            && crate::dictation_processor::opencode_installed()
        {
            let (sender, receiver) = mpsc::channel();
            self.opencode_catalog_rx = Some(receiver);
            let _ = std::thread::Builder::new()
                .name("windows-opencode-catalog".into())
                .spawn(move || {
                    let _ = sender.send(crate::dictation_processor::load_model_catalog());
                });
        }
    }

    fn poll_opencode_catalog(&mut self) {
        let Some(receiver) = &self.opencode_catalog_rx else {
            return;
        };
        let catalog = match receiver.try_recv() {
            Ok(catalog) => catalog,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err(eyre!("the OpenCode catalog worker stopped unexpectedly"))
            }
        };
        self.opencode_catalog_rx = None;
        match catalog {
            Ok(catalog) => {
                if self.settings.voice_action.model.is_none()
                    && let Some(default) = catalog
                        .default_key
                        .as_ref()
                        .and_then(|key| catalog.models.iter().find(|choice| &choice.key == key))
                {
                    *self
                        .voice_action_model
                        .write()
                        .unwrap_or_else(|error| error.into_inner()) = Some(default.model());
                }
            }
            Err(error) => tracing::warn!(%error, "could not load the OpenCode model catalog"),
        }
    }

    /// The selection with recognition hints stripped when the model cannot
    /// use them, so validation never rejects a load because hints linger
    /// from a Whisper session.
    fn runtime_selection(&self) -> crate::transcription_models::TranscriptionSelection {
        let mut selection = self.settings.transcription.clone();
        if !crate::transcription_models::definition(selection.model).supports_recognition_hints {
            selection.recognition_hints = String::new();
        }
        selection
    }

    /// Hosts the personal transformations workspace while any mode uses a
    /// transformation or the workspace exists.
    fn refresh_personal_commands_host(&mut self) {
        let enabled =
            crate::app_settings::transformations_used(&self.settings.dictation_processing)
                || crate::personal_commands::workspace_configured();
        match (enabled, self.personal_commands.is_some()) {
            (true, false) => {
                self.personal_commands = crate::personal_commands::PersonalCommands::start(
                    crate::commands::CommandConfig::new(),
                    self.transformations.clone(),
                );
            }
            (false, true) => self.personal_commands = None,
            _ => {}
        }
    }

    fn restart_listener_for_settings(&mut self) {
        if !self.is_running() {
            return;
        }
        self.listen_when_ready = true;
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            stop.store(true, Ordering::Relaxed);
            self.status = "Applying settings…".into();
        }
    }

    fn refresh(&mut self) {
        self.poll_opencode_catalog();
        // Settings may set up the workspace at any time.
        if self.personal_commands_checked_at.elapsed() >= Duration::from_secs(2) {
            self.personal_commands_checked_at = Instant::now();
            self.refresh_personal_commands_host();
        }
        if self
            .hints_restart_after
            .is_some_and(|edited| edited.elapsed() >= Duration::from_millis(1200))
        {
            self.hints_restart_after = None;
            self.restart_listener_for_settings();
        }
        let mut start_listener = false;
        if let Some(receiver) = &self.listener_result {
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("listener worker stopped unexpectedly".into()))
                }
            };
            if let Some(result) = finished {
                self.awaiting_session_start = false;
                self.listener_result = None;
                self.join_listener();
                self.listener_stop
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                match result {
                    Ok(()) => {
                        self.status = "Disabled".into();
                        start_listener = self.listen_when_ready;
                    }
                    Err(error) => {
                        self.status = "Couldn't enable".into();
                        self.error = Some(error);
                    }
                }
            }
        }
        if start_listener {
            self.start();
        }

        self.activity.refresh(&mut self.event_reader);
        self.last_completed_at = last_completed_at(self.event_reader.events().iter());
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

    fn join_listener(&mut self) {
        if let Some(worker) = self.listener_worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WindowsDesktopHost {
    fn drop(&mut self) {
        self.updater.stop();
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            stop.store(true, Ordering::Relaxed);
        }
        self.join_listener();
    }
}

/// When the current session's newest dictation completed. `events` is
/// oldest first, as `EventReader` keeps them.
fn last_completed_at<'a>(events: impl DoubleEndedIterator<Item = &'a VoiceEvent>) -> Option<u64> {
    for event in events.rev() {
        match event {
            VoiceEvent::SessionStarted { .. } => return None,
            VoiceEvent::Transcript {
                timestamp_ms,
                phase: TranscriptPhase::Completed,
                text,
                ..
            } if !text.trim().is_empty() => return Some(*timestamp_ms),
            _ => {}
        }
    }
    None
}

/// The newest failure, hidden once a later dictation completed, so a silent
/// failure still reaches the Settings window without sticking around.
fn visible_failure(activity: &DesktopActivity, last_completed_at: Option<u64>) -> Option<String> {
    activity
        .last_failure
        .as_ref()
        .filter(|(failed_at, _)| last_completed_at.is_none_or(|completed| completed < *failed_at))
        .map(|(_, message)| message.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(timestamp_ms: u64, text: &str) -> VoiceEvent {
        VoiceEvent::Transcript {
            timestamp_ms,
            phase: TranscriptPhase::Completed,
            latency_ms: 10,
            text: text.into(),
        }
    }

    #[test]
    fn the_newest_completion_is_scoped_to_the_current_session() {
        let events = [
            transcript(1, "Old session"),
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
        ];
        assert_eq!(last_completed_at(events.iter()), None);
        let events = [
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
            transcript(3, "First"),
            transcript(4, "Second"),
            transcript(5, "  "),
        ];
        assert_eq!(last_completed_at(events.iter()), Some(4));
    }

    #[test]
    fn newest_failure_shows_until_a_later_transcript_completes() {
        let activity = DesktopActivity {
            last_failure: Some((5, "OpenCode generation failed".into())),
            ..DesktopActivity::default()
        };
        assert_eq!(
            visible_failure(&activity, Some(4)).as_deref(),
            Some("OpenCode generation failed")
        );
        assert_eq!(visible_failure(&activity, Some(6)), None);
    }

    #[test]
    fn listener_restarts_only_for_settings_it_captured() {
        let previous = AppSettings::default();
        let mut current = previous.clone();
        current.indicator_position = IndicatorPosition::Bottom;
        current.sound_effect_volume = 0.25;
        assert!(!listener_settings_changed(&previous, &current));
        current.microphone = Some("Headset".into());
        assert!(listener_settings_changed(&previous, &current));
        current = previous.clone();
        current.pass_through_dictation_shortcut = true;
        assert!(listener_settings_changed(&previous, &current));
    }

    #[test]
    fn voice_action_model_keys_split_into_provider_and_id() {
        let mut settings = AppSettings::default();
        assert!(voice_action_model(&settings).is_none());
        settings.voice_action.model = Some("openai/gpt-5.6-sol".into());
        settings.voice_action.variant = Some("fast".into());
        let model = voice_action_model(&settings).unwrap();
        assert_eq!(model.provider, "openai");
        assert_eq!(model.id, "gpt-5.6-sol");
        assert_eq!(model.variant.as_deref(), Some("fast"));
    }
}
