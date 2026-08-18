use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Entity, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, Subscription, Timer, TitlebarOptions, Window, WindowBounds,
    WindowOptions, canvas, div, prelude::*, px, relative, rgb, rgba, size,
};
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

use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopCapabilities, DesktopHost, DesktopListenerSnapshot, DesktopSnapshot,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::desktop_transcription_picker::{
    TranscriptionPickerDelegate, TranscriptionPickerProgress, TranscriptionPickerStatus,
};
use crate::desktop_ui::{
    CANVAS, DIVIDER, FAINT, LINE, MUTED, NavigationIcon, OVERLAY_PANEL, OVERLAY_SMOKE,
    PANE_LIST_WIDTH, PANEL_RADIUS, SIDEBAR_WIDTH, SURFACE, SURFACE_HOVER, SURFACE_SELECTED, TEXT,
    TEXT_ON_ACCENT, TEXT_SOFT, accent_color, compact_button, compact_panel, disclosure_button,
    dropdown_backdrop, dropdown_item, dropdown_panel, dropdown_panel_with_width, empty_message,
    error_message, header_button, hotkey_keycaps, navigation_item, pane_body, pane_content,
    pane_header_with_action, section_label, segmented_control, segmented_item, settings_panel,
    settings_row, settings_section_label, sidebar_frame, toggle, window_frame,
};
use crate::events::EventReader;
use crate::history::{History, HistoryEntry, HistoryRetention};
use crate::text_input::{Changed as TextChanged, TextInput};
use crate::windows_i18n::{tr, tr_fill};
use crate::windows_settings::IndicatorPosition;
use crate::windows_ui::{CRITICAL, DIALOG_STROKE, SUCCESS, caption_bar, selection_pill};

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;

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

struct WindowsDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    listener_terminated: Arc<AtomicBool>,
    listener_result: Option<Receiver<ListenerResult>>,
    listener_worker: Option<JoinHandle<()>>,
    session_before_start: Option<u64>,
    awaiting_session_start: bool,
    listen_when_ready: bool,
    status: String,
    error: Option<String>,
    settings_error: Option<String>,
    settings: crate::windows_settings::WindowsSettings,
    microphones: Vec<String>,
    microphone_error: Option<String>,
    launch_at_login: bool,
    login_item_error: Option<String>,
    history: Option<History>,
    history_error: Option<String>,
    last_dictation: Arc<Mutex<Option<String>>>,
    indicator: crate::windows_indicator::WindowsIndicatorSender,
    replacements: Arc<RwLock<crate::text_replacements::ReplacementSet>>,
    modes: Arc<RwLock<Vec<crate::windows_settings::WindowsMode>>>,
    prepared_transcriber: Option<crate::local_transcriber::LocalTranscriber>,
    hints_restart_after: Option<Instant>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    updater: crate::windows_updater::WindowsUpdater,
}

struct WindowsApp {
    host: WindowsDesktopHost,
    pane: WindowsPane,
    history_entries: Vec<HistoryEntry>,
    selected_history: Option<u64>,
    history_copied: Option<u64>,
    history_clear_armed: bool,
    replacement_inputs: Vec<ReplacementInputs>,
    transcription_picker: TranscriptionPickerState,
    mode_inputs: Vec<ModeInputs>,
    recognition_hints_input: Entity<TextInput>,
    _recognition_hints_subscription: Subscription,
    model_catalog_language_filter: Option<String>,
    model_catalog_language_dropdown_open: bool,
    model_catalog_language_dropdown_bounds: Option<Bounds<Pixels>>,
    microphone_picker_open: bool,
    hotkey_picker_open: bool,
    indicator_hwnd: Option<HWND>,
    indicator_scale: f32,
    volume_slider_bounds: Option<Bounds<Pixels>>,
    volume_drag: Option<u8>,
    microphone_dropdown_bounds: Option<Bounds<Pixels>>,
    transcription_dropdown_open: bool,
    transcription_dropdown_bounds: Option<Bounds<Pixels>>,
    dictation_language_dropdown_open: bool,
    dictation_language_dropdown_bounds: Option<Bounds<Pixels>>,
    ui_language_dropdown_open: bool,
    ui_language_dropdown_bounds: Option<Bounds<Pixels>>,
    history_detail_text: Option<(u64, Entity<TextInput>)>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WindowsPane {
    Settings,
    Modes,
    History,
}

struct ReplacementInputs {
    matched_phrase: Entity<TextInput>,
    output: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

struct ModeInputs {
    name: Entity<TextInput>,
    applications: Entity<TextInput>,
    corrections: Vec<ReplacementInputs>,
    _subscriptions: Vec<Subscription>,
}

enum TrayCommand {
    Show,
    ToggleListening,
    Quit,
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

pub fn open(event_path: PathBuf, shutdown: &'static AtomicBool, start_hidden: bool) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let (tray_sender, tray_commands) = mpsc::channel();
    let (indicator_sender, indicator_events) = crate::windows_indicator::channel();
    let (settings, settings_error) = match crate::windows_settings::WindowsSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            crate::windows_settings::WindowsSettings::default(),
            Some(format!("Could not load Windows settings: {error:#}")),
        ),
    };
    let listen_on_launch = settings.listen_on_launch;
    let indicator_position = settings.indicator_position;
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(crate::text_input::key_bindings());
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(MINIMUM_WIDTH), px(MINIMUM_HEIGHT))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("HEX".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    app_id: Some("com.kitlangton.Hex".into()),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let host = WindowsDesktopHost::new(
                            event_path.clone(),
                            settings.clone(),
                            settings_error.clone(),
                            indicator_sender.clone(),
                        );
                        let history_entries = host
                            .history
                            .as_ref()
                            .map_or_else(Vec::new, |history| history.search(""));
                        let selected_history = history_entries.first().map(|entry| entry.id);
                        let replacement_inputs = host
                            .settings
                            .text_replacements
                            .iter()
                            .map(|replacement| WindowsApp::replacement_inputs(replacement, cx))
                            .collect();
                        let mode_inputs = host
                            .settings
                            .modes
                            .iter()
                            .map(|mode| WindowsApp::mode_inputs(mode, cx))
                            .collect();
                        let recognition_hints_input = cx.new(|cx| {
                            crate::text_input::TextInput::multiline_with_height(
                                cx,
                                "Names and terms Whisper should expect, e.g. OpenCode, Effect...",
                                &host.settings.transcription.recognition_hints,
                                px(76.0),
                            )
                        });
                        let recognition_hints_subscription = cx.subscribe(
                            &recognition_hints_input,
                            |this: &mut WindowsApp, input, _: &TextChanged, cx| {
                                let hints = input.read(cx).text().to_string();
                                this.host.set_recognition_hints(hints);
                                cx.notify();
                            },
                        );
                        WindowsApp {
                            host,
                            pane: WindowsPane::Settings,
                            recognition_hints_input,
                            _recognition_hints_subscription: recognition_hints_subscription,
                            history_entries,
                            selected_history,
                            history_copied: None,
                            history_clear_armed: false,
                            replacement_inputs,
                            transcription_picker: TranscriptionPickerState::Closed,
                            mode_inputs,
                            model_catalog_language_filter: None,
                            model_catalog_language_dropdown_open: false,
                            model_catalog_language_dropdown_bounds: None,
                            microphone_picker_open: false,
                            hotkey_picker_open: false,
                            indicator_hwnd: None,
                            indicator_scale: 1.0,
                            volume_slider_bounds: None,
                            volume_drag: None,
                            microphone_dropdown_bounds: None,
                            transcription_dropdown_open: false,
                            transcription_dropdown_bounds: None,
                            dictation_language_dropdown_open: false,
                            dictation_language_dropdown_bounds: None,
                            ui_language_dropdown_open: false,
                            ui_language_dropdown_bounds: None,
                            history_detail_text: None,
                        }
                    })
                },
            )
            .expect("could not open the HEX Windows window");
        let app = window.update(cx, |_, _, cx| cx.entity()).unwrap();
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
        app.update(cx, |app, _| {
            app.indicator_hwnd = indicator_hwnd;
            app.indicator_scale = indicator_scale;
        });
        let tray = match create_tray(tray_sender.clone()) {
            Ok(tray) => Some(tray),
            Err(error) => {
                tracing::warn!(%error, "Windows system tray is unavailable; closing HEX will quit");
                None
            }
        };
        let close_to_tray = tray.is_some();
        let hwnd = window
            .update(cx, |_, window, _| windows_hwnd(window))
            .ok()
            .flatten();
        crate::windows_ui::apply_window_icon(hwnd);
        if listen_on_launch {
            app.update(cx, |app, cx| {
                app.host.start();
                cx.notify();
            });
        }
        let indicator_app = app.clone();
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(16)).await;
                while let Ok(event) = indicator_events.try_recv() {
                    let Some(indicator) = &indicator else {
                        continue;
                    };
                    if indicator
                        .update(cx, |indicator, cx| indicator.handle(event, cx))
                        .is_err()
                    {
                        return;
                    }
                    let position = indicator_app
                        .update(cx, |app, _| app.host.settings.indicator_position)
                        .unwrap_or_default();
                    // The pill is visible only while audio is being captured;
                    // it leaves as soon as the shortcut is released instead of
                    // lingering through transcription.
                    let show = position != IndicatorPosition::Hidden
                        && matches!(
                            event,
                            crate::windows_indicator::WindowsIndicatorEvent::Recording
                                | crate::windows_indicator::WindowsIndicatorEvent::Meter(_)
                        );
                    if show
                        && matches!(
                            event,
                            crate::windows_indicator::WindowsIndicatorEvent::Recording
                        )
                    {
                        position_indicator_window(indicator_hwnd, indicator_scale, position);
                    }
                    set_window_visible(indicator_hwnd, show);
                }
            }
        })
        .detach();
        cx.spawn({
            let app = app.clone();
            async move |cx| {
                loop {
                    Timer::after(Duration::from_millis(250)).await;
                    while let Ok(command) = tray_commands.try_recv() {
                        match command {
                            TrayCommand::Show => {
                                set_window_visible(hwnd, true);
                                let _ = window.update(cx, |_, window, _| {
                                    window.activate_window();
                                });
                            }
                            TrayCommand::ToggleListening => {
                                let _ = app.update(cx, |app, cx| {
                                    if app.host.is_running() {
                                        app.host.stop();
                                    } else {
                                        app.host.start();
                                    }
                                    cx.notify();
                                });
                            }
                            TrayCommand::Quit => {
                                let _ = app.update(cx, |app, cx| {
                                    app.host.stop();
                                    cx.quit();
                                });
                                return;
                            }
                        }
                    }
                    if shutdown.load(Ordering::Relaxed) {
                        let _ = app.update(cx, |app, cx| {
                            app.host.stop();
                            cx.quit();
                        });
                        return;
                    }
                    if app
                        .update(cx, |this, cx| {
                            this.host.refresh();
                            if this.pane == WindowsPane::History {
                                this.reload_history();
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
                        return;
                    }
                }
            }
        })
        .detach();
        let close_app = app.clone();
        window
            .update(cx, |_, window, cx| {
                if close_to_tray {
                    window.on_window_should_close(cx, move |_, _| {
                        set_window_visible(hwnd, false);
                        false
                    });
                } else {
                    // Without a tray, closing must quit the whole app; the
                    // hidden HUD window would otherwise keep the process
                    // alive with no visible windows.
                    window.on_window_should_close(cx, move |_, cx| {
                        close_app.update(cx, |app, cx| {
                            app.host.stop();
                            cx.quit();
                        });
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
            let _keep_tray_alive = &tray;
            async {}
        })
        .detach();
    });
    Ok(())
}

fn create_tray(commands: mpsc::Sender<TrayCommand>) -> Result<TrayIcon> {
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
                button: TrayMouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = commands.send(TrayCommand::Show);
        }
    }));

    Ok(TrayIconBuilder::new()
        .with_tooltip("HEX Dictation")
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

impl WindowsDesktopHost {
    fn new(
        event_path: PathBuf,
        settings: crate::windows_settings::WindowsSettings,
        settings_error: Option<String>,
        indicator: crate::windows_indicator::WindowsIndicatorSender,
    ) -> Self {
        crate::windows_i18n::apply(settings.ui_language.as_deref());
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        let (history, history_error) = match History::open_default(settings.history_retention) {
            Ok(history) => (Some(history), None),
            Err(error) => (
                None,
                Some(format!("Could not open retained history: {error:#}")),
            ),
        };
        let (microphones, microphone_error) = match crate::audio::input_device_names() {
            Ok(microphones) => (microphones, None),
            Err(error) => (
                Vec::new(),
                Some(format!("Could not enumerate microphones: {error:#}")),
            ),
        };
        let (launch_at_login, login_item_error) = match crate::windows_login_item::is_enabled() {
            Ok(enabled) => (enabled, None),
            Err(error) => (
                false,
                Some(format!("Could not read Launch at login: {error:#}")),
            ),
        };
        let replacements = Arc::new(RwLock::new(crate::text_replacements::ReplacementSet::new(
            &settings.text_replacements,
        )));
        let modes = Arc::new(RwLock::new(settings.modes.clone()));
        Self {
            event_path,
            event_reader,
            activity,
            listener_stop: Arc::new(Mutex::new(None)),
            listener_terminated: Arc::new(AtomicBool::new(true)),
            listener_result: None,
            listener_worker: None,
            session_before_start: None,
            awaiting_session_start: false,
            listen_when_ready: false,
            status: "Ready".into(),
            error: None,
            settings_error,
            settings,
            microphones,
            microphone_error,
            launch_at_login,
            login_item_error,
            history,
            history_error,
            last_dictation: Arc::new(Mutex::new(None)),
            indicator,
            replacements,
            modes,
            prepared_transcriber: None,
            hints_restart_after: None,
            transcription_preparation: None,
            transcription_error: None,
            updater: crate::windows_updater::WindowsUpdater::start(),
        }
    }

    fn model_ready(&self) -> bool {
        let model = crate::transcription_models::definition(self.settings.transcription.model);
        crate::transcription_models::is_installed(model, &self.settings.transcription.language)
            && crate::transcription_models::is_verified(model)
    }

    fn start(&mut self) {
        self.listen_when_ready = true;
        if self.listener_result.is_some() {
            return;
        }
        if self.transcription_preparation.is_some() {
            self.status = "Preparing model".into();
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
        let device = self.settings.microphone.clone();
        let selection = self.runtime_selection();
        let hotkey = self.settings.dictation_hotkey.clone();
        let paste_last_hotkey = self.settings.paste_last_hotkey.clone();
        let double_tap_lock = self.settings.double_tap_lock;
        let double_tap_only = self.settings.double_tap_only;
        let while_dictating = self.settings.while_dictating;
        let release_microphone_while_idle = self.settings.release_microphone_while_idle;
        let feedback_volume = self.settings.feedback_volume;
        let history = self.history.clone();
        let last_dictation = self.last_dictation.clone();
        let indicator = self.indicator.clone();
        let replacements = self.replacements.clone();
        let modes = self.modes.clone();
        let prepared_transcriber = self.prepared_transcriber.take();
        let listener_terminated = self.listener_terminated.clone();
        listener_terminated.store(false, Ordering::Release);
        let worker = std::thread::Builder::new()
            .name("windows-listener".into())
            .spawn(move || {
                let result = crate::instance::acquire("windows-listener").and_then(|_instance| {
                    let config = crate::windows_dictation::WindowsDictationConfig {
                        device,
                        hotkey,
                        paste_last_hotkey,
                        double_tap_lock,
                        double_tap_only,
                        while_dictating,
                        release_microphone_while_idle,
                        feedback_volume,
                        history,
                        last_dictation,
                        indicator: Some(indicator),
                        replacements,
                        modes,
                        fallback_to_default_device: true,
                    };
                    if let Some(transcriber) = prepared_transcriber {
                        crate::windows_dictation::run_with_transcriber(
                            &event_path,
                            config,
                            &stop,
                            transcriber,
                        )
                    } else {
                        crate::windows_dictation::run(&event_path, &selection, config, &stop)
                    }
                });
                listener_terminated.store(true, Ordering::Release);
                let _ = result_sender.send(result.map_err(|error| format!("{error:#}")));
            });
        match worker {
            Ok(worker) => {
                self.listener_worker = Some(worker);
                self.listener_result = Some(result_receiver);
                self.session_before_start = self.activity.session_started_at;
                self.awaiting_session_start = true;
                self.status = "Starting".into();
                self.error = None;
            }
            Err(error) => {
                self.listener_terminated.store(true, Ordering::Release);
                self.status = "Unavailable".into();
                self.error = Some(format!("Could not start listener: {error}"));
                self.listener_stop
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
        }
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

    fn set_listen_on_launch(&mut self, enabled: bool) {
        let mut candidate = self.settings.clone();
        candidate.listen_on_launch = enabled;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save Windows settings: {error:#}"));
            }
        }
    }

    fn set_launch_at_login(&mut self, enabled: bool) {
        match crate::windows_login_item::set_enabled(enabled)
            .and_then(|_| crate::windows_login_item::is_enabled())
        {
            Ok(actual) => {
                self.launch_at_login = actual;
                self.login_item_error = None;
            }
            Err(error) => {
                self.login_item_error =
                    Some(format!("Could not change Launch at login: {error:#}"));
            }
        }
    }

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

    fn set_microphone(&mut self, microphone: Option<String>) -> Result<()> {
        if microphone == self.settings.microphone {
            self.microphone_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.microphone = microphone;
        candidate.save().map_err(|error| {
            let error = format!("Could not save microphone selection: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.microphone_error = None;
        self.restart_listener_for_settings();
        Ok(())
    }

    fn set_dictation_hotkey(
        &mut self,
        hotkey: crate::windows_settings::WindowsHotkey,
    ) -> Result<()> {
        hotkey.validate()?;
        if hotkey == self.settings.dictation_hotkey {
            self.settings_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.dictation_hotkey = hotkey;
        candidate.save().map_err(|error| {
            let error = format!("Could not save the dictation shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.restart_listener_for_settings();
        Ok(())
    }

    fn set_history_retention(&mut self, retention: HistoryRetention) -> Result<()> {
        if retention == self.settings.history_retention {
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.history_retention = retention;
        candidate.save().map_err(|error| {
            let error = format!("Could not save history retention: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        if let Some(history) = &self.history {
            history.set_retention(retention).map_err(|error| {
                let error = format!("Could not apply history retention: {error}");
                self.history_error = Some(error.clone());
                eyre!(error)
            })?;
        }
        self.history_error = None;
        Ok(())
    }

    fn set_double_tap_lock(&mut self, enabled: bool) {
        if enabled == self.settings.double_tap_lock {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.double_tap_lock = enabled;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save double-tap lock: {error:#}"));
            }
        }
    }

    fn set_release_microphone_while_idle(&mut self, enabled: bool) {
        if enabled == self.settings.release_microphone_while_idle {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.release_microphone_while_idle = enabled;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error =
                    Some(format!("Could not save the microphone setting: {error:#}"));
            }
        }
    }

    fn set_while_dictating(&mut self, behavior: crate::windows_settings::WhileDictating) {
        if behavior == self.settings.while_dictating {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.while_dictating = behavior;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "Could not save the while-dictating setting: {error:#}"
                ));
            }
        }
    }

    fn set_double_tap_only(&mut self, enabled: bool) {
        if enabled == self.settings.double_tap_only {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.double_tap_only = enabled;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save double-tap only: {error:#}"));
            }
        }
    }

    fn set_paste_last_enabled(&mut self, enabled: bool) {
        let next = enabled.then(crate::windows_settings::WindowsHotkey::paste_last_default);
        if next == self.settings.paste_last_hotkey {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.paste_last_hotkey = next;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save Paste Last: {error:#}"));
            }
        }
    }

    fn set_feedback_volume(&mut self, volume: u8) {
        let volume = volume.min(100);
        if volume == self.settings.feedback_volume {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.feedback_volume = volume;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                crate::feedback::set_enabled(volume > 0);
                crate::feedback::set_volume(f32::from(volume) / 100.0);
                if volume > 0 {
                    std::thread::spawn(move || {
                        if crate::feedback::preload().is_ok() {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                    });
                }
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save feedback volume: {error:#}"));
            }
        }
    }

    fn set_text_replacements(
        &mut self,
        replacements: Vec<crate::text_replacements::TextReplacement>,
    ) -> Result<()> {
        let mut candidate = self.settings.clone();
        candidate.text_replacements = replacements;
        candidate.save().map_err(|error| {
            let error = format!("Could not save text replacements: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        *self
            .replacements
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::text_replacements::ReplacementSet::new(&candidate.text_replacements);
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    fn set_modes(&mut self, modes: Vec<crate::windows_settings::WindowsMode>) -> Result<()> {
        let mut candidate = self.settings.clone();
        candidate.modes = modes;
        candidate.save().map_err(|error| {
            let error = format!("Could not save dictation modes: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        *self
            .modes
            .write()
            .unwrap_or_else(|error| error.into_inner()) = candidate.modes.clone();
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    /// The stored transcription selection with recognition hints stripped
    /// when the model cannot use them, so validation never rejects a load
    /// just because hints linger from a Whisper session.
    fn runtime_selection(&self) -> crate::transcription_models::TranscriptionSelection {
        let mut selection = self.settings.transcription.clone();
        if !crate::transcription_models::definition(selection.model).supports_recognition_hints {
            selection.recognition_hints = String::new();
        }
        selection
    }

    /// Persist edited recognition hints; a running Whisper listener restarts
    /// after the debounce window in [`Self::refresh`] so they take effect.
    fn set_recognition_hints(&mut self, hints: String) {
        if self.settings.transcription.recognition_hints == hints {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.transcription.recognition_hints = hints;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                if crate::transcription_models::definition(self.settings.transcription.model)
                    .supports_recognition_hints
                    && self.is_running()
                {
                    self.hints_restart_after = Some(Instant::now());
                }
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save recognition hints: {error:#}"));
            }
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
            self.status = "Applying settings".into();
        }
    }

    fn refresh(&mut self) {
        if self
            .hints_restart_after
            .is_some_and(|edited| edited.elapsed() >= Duration::from_millis(1200))
        {
            self.hints_restart_after = None;
            self.restart_listener_for_settings();
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
                start_listener = self.listen_when_ready;
            } else {
                match result {
                    Ok(prepared) => {
                        let mut candidate = self.settings.clone();
                        candidate.transcription = prepared.selection;
                        // Hints persist across model switches; models that
                        // cannot use them get a stripped copy at load time.
                        candidate.transcription.recognition_hints =
                            self.settings.transcription.recognition_hints.clone();
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
                                start_listener = self.listen_when_ready;
                            }
                        }
                    }
                    Err(error) => {
                        self.transcription_error = Some(error);
                        start_listener = self.listen_when_ready;
                    }
                }
            }
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
                    start_listener |=
                        self.listen_when_ready && self.transcription_preparation.is_none();
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

        if start_listener {
            self.start();
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

    fn choose_transcription(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
    ) -> Result<()> {
        if self.transcription_preparation.is_some() {
            self.cancel_transcription_preparation();
            let error = "The previous model preparation is still stopping".to_string();
            self.transcription_error = Some(error.clone());
            return Err(eyre!(error));
        }
        let selection = crate::transcription_models::TranscriptionSelection {
            model,
            language,
            recognition_hints: if crate::transcription_models::definition(model)
                .supports_recognition_hints
            {
                self.settings.transcription.recognition_hints.clone()
            } else {
                String::new()
            },
        };
        crate::transcription_models::validate(&selection)?;
        self.restart_listener_for_settings();
        let definition = crate::transcription_models::definition(model);
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let stage = Arc::new(AtomicU8::new(
            crate::transcription_models::ModelPreparationStage::Downloading as u8,
        ));
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        let worker_stage = stage.clone();
        let listener_terminated = self.listener_terminated.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("windows-model-preparation".into())
            .spawn(move || {
                let result = (|| {
                    while !listener_terminated.load(Ordering::Acquire) {
                        if worker_canceled.load(Ordering::Relaxed) {
                            return Err(eyre!("model activation canceled"));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    crate::transcription_models::download_with_stage_progress(
                        definition,
                        &worker_canceled,
                        &worker_progress,
                        &worker_stage,
                    )?;
                    if worker_canceled.load(Ordering::Relaxed) {
                        return Err(eyre!("model activation canceled"));
                    }
                    crate::transcription_models::ModelPreparationStage::Loading
                        .store(&worker_stage);
                    let transcriber = crate::local_transcriber::LocalTranscriber::load(&selection)?;
                    if worker_canceled.load(Ordering::Relaxed) {
                        return Err(eyre!("model activation canceled"));
                    }
                    Ok(PreparedTranscription {
                        selection,
                        transcriber,
                    })
                })()
                .map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
            })?;
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

impl Drop for WindowsDesktopHost {
    fn drop(&mut self) {
        self.updater.stop();
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

impl DesktopHost for WindowsDesktopHost {
    fn capabilities(&self) -> DesktopCapabilities {
        DesktopCapabilities::windows()
    }

    fn snapshot(&self) -> DesktopSnapshot {
        DesktopSnapshot {
            activity: self.activity.clone(),
            dictation_shortcut: self.settings.dictation_hotkey.keycaps(),
            dictation_shortcut_label: self.settings.dictation_hotkey.label(),
            double_tap_lock: self.settings.double_tap_lock,
            double_tap_only: self.settings.double_tap_only,
            paste_last_shortcut: self
                .settings
                .paste_last_hotkey
                .as_ref()
                .map(crate::windows_settings::WindowsHotkey::keycaps),
            listener: Some(DesktopListenerSnapshot {
                running: self.is_running(),
                status: self.status.clone(),
            }),
            operation_error: self
                .error
                .clone()
                .or_else(|| self.settings_error.clone())
                .or_else(|| self.login_item_error.clone()),
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
            update_status: match self.updater.state() {
                crate::windows_updater::UpdateCheck::Checking => DesktopUpdateStatus::Checking,
                crate::windows_updater::UpdateCheck::Current => DesktopUpdateStatus::Current,
                crate::windows_updater::UpdateCheck::Available { .. } => {
                    DesktopUpdateStatus::Available
                }
                crate::windows_updater::UpdateCheck::Failed => DesktopUpdateStatus::Failed,
            },
        }
    }

    fn dispatch(&mut self, action: DesktopAction) -> Result<()> {
        match action {
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
                self.login_item_error = None;
            }
            DesktopAction::StartListening => self.start(),
            DesktopAction::StopListening => self.stop(),
            DesktopAction::RestartIntoUpdate
            | DesktopAction::SetDictationShortcut(_)
            | DesktopAction::SetDoubleTapLock(_)
            | DesktopAction::SetDoubleTapOnly(_) => {
                return Err(eyre!("this desktop action is unavailable on Windows"));
            }
        }
        Ok(())
    }
}

impl WindowsApp {
    fn replacement_inputs(
        replacement: &crate::text_replacements::TextReplacement,
        cx: &mut Context<Self>,
    ) -> ReplacementInputs {
        let matched_phrase =
            cx.new(|cx| TextInput::new(cx, "e.g. open code", &replacement.matched_phrase));
        let output = cx.new(|cx| TextInput::new(cx, "e.g. OpenCode", &replacement.output));
        let matched_changed = cx.subscribe(&matched_phrase, |this, _, _: &TextChanged, cx| {
            this.sync_text_replacements(cx)
        });
        let output_changed = cx.subscribe(&output, |this, _, _: &TextChanged, cx| {
            this.sync_text_replacements(cx)
        });
        ReplacementInputs {
            matched_phrase,
            output,
            _subscriptions: vec![matched_changed, output_changed],
        }
    }

    fn sync_text_replacements(&mut self, cx: &mut Context<Self>) {
        let replacements = self
            .replacement_inputs
            .iter()
            .map(|inputs| crate::text_replacements::TextReplacement {
                matched_phrase: inputs.matched_phrase.read(cx).text().to_string(),
                output: inputs.output.read(cx).text().to_string(),
            })
            .collect();
        let _ = self.host.set_text_replacements(replacements);
        cx.notify();
    }

    fn add_text_replacement(&mut self, cx: &mut Context<Self>) {
        self.replacement_inputs.push(Self::replacement_inputs(
            &crate::text_replacements::TextReplacement::default(),
            cx,
        ));
        self.sync_text_replacements(cx);
    }

    fn remove_text_replacement(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.replacement_inputs.len() {
            self.replacement_inputs.remove(index);
            self.sync_text_replacements(cx);
        }
    }

    fn mode_inputs(
        mode: &crate::windows_settings::WindowsMode,
        cx: &mut Context<Self>,
    ) -> ModeInputs {
        let name = cx.new(|cx| TextInput::new(cx, "e.g. Coding", &mode.name));
        let applications = cx.new(|cx| {
            TextInput::new(
                cx,
                "e.g. code, chrome, slack",
                &mode.applications.join(", "),
            )
        });
        let name_changed = cx.subscribe(&name, |this, _, _: &TextChanged, cx| this.sync_modes(cx));
        let applications_changed = cx.subscribe(&applications, |this, _, _: &TextChanged, cx| {
            this.sync_modes(cx)
        });
        let corrections = mode
            .corrections
            .iter()
            .map(|correction| Self::mode_correction_inputs(correction, cx))
            .collect();
        ModeInputs {
            name,
            applications,
            corrections,
            _subscriptions: vec![name_changed, applications_changed],
        }
    }

    fn mode_correction_inputs(
        correction: &crate::text_replacements::TextReplacement,
        cx: &mut Context<Self>,
    ) -> ReplacementInputs {
        let matched_phrase =
            cx.new(|cx| TextInput::new(cx, "e.g. open code", &correction.matched_phrase));
        let output = cx.new(|cx| TextInput::new(cx, "e.g. OpenCode", &correction.output));
        let matched_changed = cx.subscribe(&matched_phrase, |this, _, _: &TextChanged, cx| {
            this.sync_modes(cx)
        });
        let output_changed =
            cx.subscribe(&output, |this, _, _: &TextChanged, cx| this.sync_modes(cx));
        ReplacementInputs {
            matched_phrase,
            output,
            _subscriptions: vec![matched_changed, output_changed],
        }
    }

    fn sync_modes(&mut self, cx: &mut Context<Self>) {
        let modes = self
            .mode_inputs
            .iter()
            .map(|inputs| crate::windows_settings::WindowsMode {
                name: inputs.name.read(cx).text().to_string(),
                applications: inputs
                    .applications
                    .read(cx)
                    .text()
                    .split(',')
                    .map(|application| application.trim().to_string())
                    .filter(|application| !application.is_empty())
                    .collect(),
                corrections: inputs
                    .corrections
                    .iter()
                    .map(|correction| crate::text_replacements::TextReplacement {
                        matched_phrase: correction.matched_phrase.read(cx).text().to_string(),
                        output: correction.output.read(cx).text().to_string(),
                    })
                    .collect(),
            })
            .collect();
        let _ = self.host.set_modes(modes);
        cx.notify();
    }

    fn add_mode(&mut self, cx: &mut Context<Self>) {
        self.mode_inputs.push(Self::mode_inputs(
            &crate::windows_settings::WindowsMode::default(),
            cx,
        ));
        self.sync_modes(cx);
    }

    fn remove_mode(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.mode_inputs.len() {
            self.mode_inputs.remove(index);
            self.sync_modes(cx);
        }
    }

    fn add_mode_correction(&mut self, mode_index: usize, cx: &mut Context<Self>) {
        let correction =
            Self::mode_correction_inputs(&crate::text_replacements::TextReplacement::default(), cx);
        if let Some(inputs) = self.mode_inputs.get_mut(mode_index) {
            inputs.corrections.push(correction);
            self.sync_modes(cx);
        }
    }

    fn remove_mode_correction(&mut self, mode_index: usize, row: usize, cx: &mut Context<Self>) {
        if let Some(inputs) = self.mode_inputs.get_mut(mode_index)
            && row < inputs.corrections.len()
        {
            inputs.corrections.remove(row);
            self.sync_modes(cx);
        }
    }

    fn reload_history(&mut self) {
        let Some(history) = &self.host.history else {
            self.history_entries.clear();
            self.selected_history = None;
            return;
        };
        self.history_entries = history.search("");
        if self
            .selected_history
            .is_some_and(|id| !self.history_entries.iter().any(|entry| entry.id == id))
        {
            self.selected_history = self.history_entries.first().map(|entry| entry.id);
        } else if self.selected_history.is_none() {
            self.selected_history = self.history_entries.first().map(|entry| entry.id);
        }
        if self
            .history_copied
            .is_some_and(|id| !self.history_entries.iter().any(|entry| entry.id == id))
        {
            self.history_copied = None;
        }
    }

    fn set_history_retention(&mut self, retention: HistoryRetention) {
        if self.host.set_history_retention(retention).is_ok() {
            self.reload_history();
        }
    }

    fn close_popups(&mut self) {
        self.transcription_picker = TranscriptionPickerState::Closed;
        self.model_catalog_language_dropdown_open = false;
        self.microphone_picker_open = false;
        self.hotkey_picker_open = false;
        self.transcription_dropdown_open = false;
        self.dictation_language_dropdown_open = false;
        self.ui_language_dropdown_open = false;
    }

    fn set_ui_language(&mut self, code: Option<&'static str>) {
        if self.host.settings.ui_language.as_deref() == code {
            return;
        }
        let mut candidate = self.host.settings.clone();
        candidate.ui_language = code.map(str::to_string);
        match candidate.save() {
            Ok(()) => {
                self.host.settings = candidate;
                self.host.settings_error = None;
                crate::windows_i18n::apply(code);
            }
            Err(error) => {
                self.host.settings_error =
                    Some(format!("Could not save the interface language: {error:#}"));
            }
        }
    }

    /// The language to keep when switching models: the current one when the
    /// model supports it, otherwise Auto, otherwise the first supported.
    ///
    /// Model support comes from the compiled definition, not the smaller
    /// recommendation list. A model can support a language without being the
    /// model HEX recommends for it.
    fn language_for_model(
        model: crate::transcription_models::TranscriptionModelId,
        preferred: &str,
    ) -> Option<String> {
        let definition = crate::transcription_models::definition(model);
        if definition.supports_language(preferred) {
            return Some(preferred.to_string());
        }
        if definition.supports_language(crate::transcription_models::AUTO_LANGUAGE) {
            return Some(crate::transcription_models::AUTO_LANGUAGE.to_string());
        }
        crate::transcription_models::LANGUAGES
            .iter()
            .find(|(code, _)| definition.supports_language(code))
            .map(|(code, _)| (*code).to_string())
    }

    /// The model browser's initial language filter: the dictation language,
    /// or no filter when dictation detects the language automatically.
    fn catalog_filter_for_language(language: &str) -> Option<String> {
        (language != crate::transcription_models::AUTO_LANGUAGE).then(|| language.to_string())
    }

    fn set_indicator_position(&mut self, position: IndicatorPosition) {
        if position == self.host.settings.indicator_position {
            return;
        }
        let mut candidate = self.host.settings.clone();
        candidate.indicator_position = position;
        match candidate.save() {
            Ok(()) => {
                self.host.settings = candidate;
                self.host.settings_error = None;
                position_indicator_window(self.indicator_hwnd, self.indicator_scale, position);
                if position == IndicatorPosition::Hidden {
                    set_window_visible(self.indicator_hwnd, false);
                }
            }
            Err(error) => {
                self.host.settings_error =
                    Some(format!("Could not save the indicator position: {error:#}"));
            }
        }
    }

    fn update_volume_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.volume_slider_bounds else {
            return;
        };
        if bounds.size.width <= px(0.0) {
            return;
        }
        let fraction = ((position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
        self.volume_drag = Some(((fraction * 20.0).round() * 5.0) as u8);
        cx.notify();
    }

    fn finish_volume_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(volume) = self.volume_drag.take() {
            self.host.set_feedback_volume(volume);
        }
        cx.notify();
    }

    /// Keep one selectable read-only text entity in sync with the selected
    /// history entry so text selection survives the periodic history reloads.
    fn sync_history_detail_text(&mut self, cx: &mut Context<Self>) {
        let entry = self
            .selected_history
            .and_then(|id| self.history_entries.iter().find(|entry| entry.id == id));
        let Some(entry) = entry else {
            self.history_detail_text = None;
            return;
        };
        let stale = match &self.history_detail_text {
            Some((id, input)) => *id != entry.id || input.read(cx).text() != entry.final_text,
            None => true,
        };
        if stale {
            let text = entry.final_text.clone();
            self.history_detail_text = Some((
                entry.id,
                cx.new(|cx| TextInput::read_only_multiline(cx, &text, px(220.0))),
            ));
        }
    }

    fn copy_history_entry(&mut self, id: u64) {
        let Some(entry) = self.history_entries.iter().find(|entry| entry.id == id) else {
            return;
        };
        match arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(entry.final_text.clone()))
        {
            Ok(()) => {
                self.history_copied = Some(id);
                self.host.history_error = None;
            }
            Err(error) => self.host.history_error = Some(error.to_string()),
        }
    }

    fn delete_history_entry(&mut self, id: u64) {
        if let Some(history) = &self.host.history {
            match history.delete(id) {
                Ok(_) => self.host.history_error = None,
                Err(error) => self.host.history_error = Some(error.to_string()),
            }
            self.reload_history();
        }
    }

    fn clear_history(&mut self) {
        if !self.history_clear_armed {
            self.history_clear_armed = true;
            return;
        }
        self.history_clear_armed = false;
        if let Some(history) = &self.host.history {
            match history.clear() {
                Ok(()) => self.host.history_error = None,
                Err(error) => self.host.history_error = Some(error.to_string()),
            }
            self.reload_history();
        }
    }

    fn listener_action(
        &mut self,
        snapshot: &DesktopSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = snapshot
            .listener
            .as_ref()
            .is_some_and(|listener| listener.running);
        if !self.host.model_ready() {
            let language = snapshot.transcription.selection.language.clone();
            return header_button(tr("Install model"))
                .id("install-model")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model_catalog_language_filter =
                        Self::catalog_filter_for_language(&language);
                    this.transcription_picker =
                        TranscriptionPickerState::Choosing(language.clone());
                    cx.notify();
                }))
                .into_any_element();
        }
        header_button(if running {
            tr("Stop listening")
        } else {
            tr("Start listening")
        })
        .id("toggle-listening")
        .on_click(cx.listener(move |this, _, _, cx| {
            let action = if running {
                DesktopAction::StopListening
            } else {
                DesktopAction::StartListening
            };
            let _ = this.host.dispatch(action);
            cx.notify();
        }))
        .into_any_element()
    }

    fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let settings_selected = self.pane == WindowsPane::Settings;
        let modes_selected = self.pane == WindowsPane::Modes;
        let history_selected = self.pane == WindowsPane::History;
        let update = match self.host.updater.state() {
            crate::windows_updater::UpdateCheck::Available { version, url } => Some((version, url)),
            _ => None,
        };
        sidebar_frame()
            .w(px(SIDEBAR_WIDTH))
            .px(px(4.0))
            .pt(px(6.0))
            .pb_4()
            .flex()
            .flex_col()
            .child(
                navigation_item(NavigationIcon::Settings, settings_selected)
                    .id("windows-nav-settings")
                    .child(tr("Settings"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pane = WindowsPane::Settings;
                        cx.notify();
                    })),
            )
            .child(
                navigation_item(NavigationIcon::Modes, modes_selected)
                    .id("windows-nav-modes")
                    .child(tr("Modes"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pane = WindowsPane::Modes;
                        cx.notify();
                    })),
            )
            .child(
                navigation_item(NavigationIcon::History, history_selected)
                    .id("windows-nav-history")
                    .child(tr("History"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pane = WindowsPane::History;
                        this.history_clear_armed = false;
                        this.reload_history();
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when_some(update, |nav, (version, url)| {
                nav.child(
                    div()
                        .id("windows-update-pill")
                        .mx(px(4.0))
                        .mb_2()
                        .h(px(32.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(4.0))
                        .bg(rgb(accent_color()))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_ON_ACCENT))
                        .cursor_pointer()
                        .hover(|pill| pill.opacity(0.9))
                        .child(crate::windows_ui::fluent_icon("\u{E896}", 12.0, 0x000000))
                        .child(tr_fill("Update to {}", &version))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.open_url(&url);
                        })),
                )
            })
            .child(
                div()
                    .px(px(16.0))
                    .text_size(px(10.0))
                    .text_color(rgb(FAINT))
                    .child(format!("HEX {} · Windows", env!("CARGO_PKG_VERSION"))),
            )
            .into_any_element()
    }

    fn render_settings(
        &mut self,
        snapshot: &DesktopSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = snapshot
            .listener
            .as_ref()
            .is_some_and(|listener| listener.running);
        let listener_label = snapshot
            .listener
            .as_ref()
            .map_or("Ready", |listener| listener.status.as_str())
            .to_string();
        let device = snapshot
            .activity
            .device
            .clone()
            .or_else(|| self.host.settings.microphone.clone())
            .unwrap_or_else(|| "Automatic microphone".into());
        let transcription = &snapshot.transcription;
        let transcription_label =
            crate::transcription_models::definition(transcription.selection.model)
                .name
                .to_string();
        let dictation_language_label =
            crate::transcription_models::language_name(&transcription.selection.language)
                .to_string();
        let dictation_language_label = if transcription.selection.language == "auto" {
            tr("Auto").to_string()
        } else {
            dictation_language_label
        };
        let transcription_language = transcription.selection.language.clone();
        let ui_language_label = match self.host.settings.ui_language.as_deref() {
            None => tr("System").to_string(),
            Some(code) => crate::windows_i18n::choice_name(Some(code)).to_string(),
        };
        let microphone_label = self
            .host
            .settings
            .microphone
            .clone()
            .unwrap_or_else(|| tr("Automatic").into());
        let listen_on_launch = self.host.settings.listen_on_launch;
        let launch_at_login = self.host.launch_at_login;
        let double_tap_lock = self.host.settings.double_tap_lock;
        let double_tap_only = self.host.settings.double_tap_only;
        let feedback_volume = self
            .volume_drag
            .unwrap_or(self.host.settings.feedback_volume);
        let volume_fraction = f32::from(feedback_volume) / 100.0;
        let volume_label = if feedback_volume == 0 {
            tr("Off").to_string()
        } else {
            format!("{feedback_volume}%")
        };
        let slider_bounds_entity = cx.entity();
        let feedback_control = div()
            .id("windows-feedback-volume")
            .w(px(220.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .gap_3()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.volume_drag = Some(this.host.settings.feedback_volume);
                    this.update_volume_drag(event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.volume_drag.is_some() {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.update_volume_drag(event.position, cx);
                    } else {
                        this.finish_volume_drag(cx);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_volume_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_volume_drag(cx)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(rgb(0x454545))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .h_full()
                            .w(relative(volume_fraction))
                            .rounded_full()
                            .bg(rgb(accent_color())),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(-5.0))
                            .left(relative(volume_fraction))
                            .ml(px(-7.0))
                            .size(px(14.0))
                            .rounded_full()
                            .border_2()
                            .border_color(rgb(0x1c1c1c))
                            .bg(rgb(accent_color())),
                    )
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                slider_bounds_entity.update(cx, |this, _| {
                                    this.volume_slider_bounds = Some(bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .w(px(36.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT_SOFT))
                    .child(volume_label),
            );
        let release_microphone_while_idle = self.host.settings.release_microphone_while_idle;
        let while_dictating = self.host.settings.while_dictating;
        let while_dictating_control = segmented_control().children(
            [
                (crate::windows_settings::WhileDictating::Mute, "Mute"),
                (
                    crate::windows_settings::WhileDictating::PauseMedia,
                    "Pause media",
                ),
                (
                    crate::windows_settings::WhileDictating::DoNothing,
                    "Do nothing",
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                segmented_item(while_dictating == value)
                    .id(("windows-while-dictating", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.host.set_while_dictating(value);
                        cx.notify();
                    }))
                    .child(tr(label))
            }),
        );
        let indicator_position = self.host.settings.indicator_position;
        let indicator_control = segmented_control().children(
            [
                (IndicatorPosition::Top, "Top"),
                (IndicatorPosition::Bottom, "Bottom"),
                (IndicatorPosition::Hidden, "Off"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                segmented_item(indicator_position == value)
                    .id(("windows-indicator-position", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_indicator_position(value);
                        cx.notify();
                    }))
                    .child(tr(label))
            }),
        );
        let paste_last_enabled = self.host.settings.paste_last_hotkey.is_some();
        let paste_last_control = div()
            .flex()
            .items_center()
            .gap_3()
            .when_some(snapshot.paste_last_shortcut.clone(), |control, keycaps| {
                control.child(hotkey_keycaps(keycaps, 1.0))
            })
            .when(!paste_last_enabled, |control| {
                control.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(FAINT))
                        .child(tr("Disabled")),
                )
            })
            .child(toggle(if paste_last_enabled { 1.0 } else { 0.0 }));
        let action = self.listener_action(snapshot, cx);
        let status_hint = format!(
            "{device} · {}",
            tr_fill("Hold {} to dictate", &snapshot.dictation_shortcut_label)
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Settings", None))
            .child(
                pane_body().child(
                    div()
                        .id("windows-settings-scroll")
                        .size_full()
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
                                                                        .child(
                                                                            tr(&listener_label)
                                                                                .to_string(),
                                                                        ),
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
                                                .child(action),
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
                                                    header_button(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .child(crate::windows_ui::fluent_icon(
                                                                "\u{E896}",
                                                                12.0,
                                                                TEXT,
                                                            ))
                                                            .child(tr("Browse")),
                                                    )
                                                    .id("windows-transcription-install")
                                                    .rounded_none()
                                                    .rounded_l(px(4.0))
                                                    .border_r_0()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.close_popups();
                                                        this.model_catalog_language_filter =
                                                            Self::catalog_filter_for_language(
                                                                &transcription_language,
                                                            );
                                                        this.transcription_picker =
                                                            TranscriptionPickerState::Choosing(
                                                                transcription_language.clone(),
                                                            );
                                                        cx.notify();
                                                    })),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .child(
                                                            disclosure_button(transcription_label)
                                                                .id("windows-transcription-setting")
                                                                .rounded_none()
                                                                .rounded_r(px(4.0))
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
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
                                                        .id("windows-dictation-language")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            let open = this
                                                                .dictation_language_dropdown_open;
                                                            this.close_popups();
                                                            this.dictation_language_dropdown_open =
                                                                !open;
                                                            cx.notify();
                                                        })),
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
                                        .when(
                                            crate::transcription_models::definition(
                                                transcription.selection.model,
                                            )
                                            .supports_recognition_hints,
                                            |panel| {
                                                panel.child(settings_row(
                                                    "Recognition hints",
                                                    "Names and terms to softly prime the speech model",
                                                    div()
                                                        .w(px(320.0))
                                                        .py_2()
                                                        .child(self.recognition_hints_input.clone()),
                                                ))
                                            },
                                        )
                                        .child(settings_row(
                                            "Microphone",
                                            "Uses the selected WASAPI input or the Windows default",
                                            div()
                                                .relative()
                                                .child(
                                                    disclosure_button(microphone_label)
                                                        .id("windows-microphone-setting")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            let open =
                                                                this.microphone_picker_open;
                                                            this.close_popups();
                                                            this.host.refresh_microphones();
                                                            this.microphone_picker_open = !open;
                                                            cx.notify();
                                                        })),
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
                                        .child(
                                            settings_row(
                                                "Dictation shortcut",
                                                "Hold while speaking; release to transcribe and paste",
                                                hotkey_keycaps(
                                                    snapshot.dictation_shortcut.clone(),
                                                    1.0,
                                                ),
                                            )
                                            .id("windows-dictation-shortcut")
                                            .cursor_pointer()
                                            .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.transcription_picker =
                                                    TranscriptionPickerState::Closed;
                                                this.microphone_picker_open = false;
                                                this.hotkey_picker_open = true;
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            settings_row(
                                                "Double-tap to lock",
                                                "Tap the shortcut twice, then speak hands-free; press it again to finish",
                                                toggle(if double_tap_lock { 1.0 } else { 0.0 }),
                                            )
                                            .id("windows-double-tap-lock")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.host
                                                    .set_double_tap_lock(!double_tap_lock);
                                                cx.notify();
                                            })),
                                        )
                                        .when(double_tap_lock, |panel| {
                                            panel.child(
                                                settings_row(
                                                    "Double-tap only",
                                                    "Wait for two complete shortcut taps before recording",
                                                    toggle(if double_tap_only {
                                                        1.0
                                                    } else {
                                                        0.0
                                                    }),
                                                )
                                                .id("windows-double-tap-only")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.host
                                                        .set_double_tap_only(!double_tap_only);
                                                    cx.notify();
                                                })),
                                            )
                                        })
                                        .child(
                                            settings_row(
                                                "Paste last dictation",
                                                "Insert the most recent completed dictation at the current focus",
                                                paste_last_control,
                                            )
                                            .id("windows-paste-last")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.host.set_paste_last_enabled(
                                                    !paste_last_enabled,
                                                );
                                                cx.notify();
                                            })),
                                        )
                                        .child(settings_row(
                                            "While dictating",
                                            "Control other audio while a dictation records",
                                            while_dictating_control,
                                        ))
                                        .child(
                                            settings_row(
                                                "Release microphone while idle",
                                                "Adds first-capture latency and disables audio pre-roll",
                                                toggle(if release_microphone_while_idle {
                                                    1.0
                                                } else {
                                                    0.0
                                                }),
                                            )
                                            .id("windows-release-microphone")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.host.set_release_microphone_while_idle(
                                                    !release_microphone_while_idle,
                                                );
                                                cx.notify();
                                            })),
                                        )
                                        .child(settings_row(
                                            "Recording indicator",
                                            "Show the dictation pill at the top or bottom of the screen",
                                            indicator_control,
                                        ))
                                        .child(
                                            settings_row(
                                                "Feedback volume",
                                                "Recording start, stop, and cancellation tones",
                                                feedback_control,
                                            )
                                            .border_b_0(),
                                        ),
                                )
                                .child(settings_section_label("Application"))
                                .child(
                                    settings_panel()
                                        .child(settings_row(
                                            "Interface language",
                                            "Language of the HEX interface",
                                            div()
                                                .relative()
                                                .child(
                                                    disclosure_button(ui_language_label)
                                                        .id("windows-ui-language")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            let open =
                                                                this.ui_language_dropdown_open;
                                                            this.close_popups();
                                                            this.ui_language_dropdown_open = !open;
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    canvas(
                                                        {
                                                            let entity = cx.entity();
                                                            move |bounds, _, cx| {
                                                                entity.update(cx, |this, _| {
                                                                    this.ui_language_dropdown_bounds =
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
                                        .child(
                                            settings_row(
                                                "Launch at login",
                                                "Start HEX hidden in the Windows system tray after sign-in",
                                                toggle(if launch_at_login { 1.0 } else { 0.0 }),
                                            )
                                            .id("windows-launch-at-login")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.host
                                                    .set_launch_at_login(!launch_at_login);
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            settings_row(
                                                "Listen on launch",
                                                "Start the global dictation listener when HEX opens",
                                                toggle(if listen_on_launch { 1.0 } else { 0.0 }),
                                            )
                                            .id("windows-listen-on-launch")
                                            .border_b_0()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.host
                                                    .set_listen_on_launch(!listen_on_launch);
                                                cx.notify();
                                            })),
                                        ),
                                )
                                .when_some(snapshot.operation_error.clone(), |content, error| {
                                    content
                                        .child(settings_section_label("Problem"))
                                        .child(
                                            settings_panel()
                                                .child(error_message(
                                                    "HEX could not apply this change.",
                                                    error,
                                                ))
                                                .child(
                                                    compact_button(tr("Dismiss"))
                                                        .id("dismiss-windows-error")
                                                        .mx_4()
                                                        .mb_4()
                                                        .border_1()
                                                        .border_color(rgb(LINE))
                                                        .on_click(cx.listener(
                                                            |this, _, _, cx| {
                                                                let _ = this.host.dispatch(
                                                                    DesktopAction::ClearError,
                                                                );
                                                                cx.notify();
                                                            },
                                                        )),
                                                ),
                                        )
                                }),
                            ),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_modes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let add = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                header_button(tr("Add mode"))
                    .id("windows-add-mode")
                    .on_click(cx.listener(|this, _, _, cx| this.add_mode(cx))),
            )
            .child(
                header_button(tr("Add replacement"))
                    .id("windows-add-replacement")
                    .on_click(cx.listener(|this, _, _, cx| this.add_text_replacement(cx))),
            )
            .into_any_element();
        let replacement_count = self.replacement_inputs.len();
        let mode_count = self.mode_inputs.len();
        let mode_panels: Vec<AnyElement> = self
            .mode_inputs
            .iter()
            .enumerate()
            .map(|(mode_index, inputs)| {
                let correction_count = inputs.corrections.len();
                let correction_rows: Vec<AnyElement> = inputs
                    .corrections
                    .iter()
                    .enumerate()
                    .map(|(row, correction)| {
                        div()
                            .id(("windows-mode-correction", mode_index * 1000 + row))
                            .w_full()
                            .p_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when(row + 1 < correction_count, |element| {
                                element.border_b_1().border_color(rgb(DIVIDER))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(correction.matched_phrase.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(13.0))
                                    .text_color(rgb(FAINT))
                                    .child("→"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(correction.output.clone()),
                            )
                            .child(
                                compact_button(tr("Remove"))
                                    .id(("windows-remove-mode-correction", mode_index * 1000 + row))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_mode_correction(mode_index, row, cx);
                                    })),
                            )
                            .into_any_element()
                    })
                    .collect();
                compact_panel()
                    .mt_3()
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .border_b_1()
                            .border_color(rgb(DIVIDER))
                            .child(div().flex_1().min_w(px(0.0)).child(inputs.name.clone()))
                            .child(
                                compact_button(tr("Remove mode"))
                                    .id(("windows-remove-mode", mode_index))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_mode(mode_index, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .border_b_1()
                            .border_color(rgb(DIVIDER))
                            .child(div().text_size(px(11.0)).text_color(rgb(FAINT)).child(tr(
                                "Applies when the focused application contains any of these names",
                            )))
                            .child(inputs.applications.clone()),
                    )
                    .children(correction_rows)
                    .child(
                        div().w_full().p_3().flex().child(
                            compact_button(tr("Add correction"))
                                .id(("windows-add-mode-correction", mode_index))
                                .border_1()
                                .border_color(rgb(LINE))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.add_mode_correction(mode_index, cx);
                                })),
                        ),
                    )
                    .into_any_element()
            })
            .collect();
        let rows: Vec<AnyElement> = self
            .replacement_inputs
            .iter()
            .enumerate()
            .map(|(index, inputs)| {
                div()
                    .id(("windows-replacement", index))
                    .w_full()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(index + 1 < replacement_count, |row| {
                        row.border_b_1().border_color(rgb(DIVIDER))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(inputs.matched_phrase.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(13.0))
                            .text_color(rgb(FAINT))
                            .child("→"),
                    )
                    .child(div().flex_1().min_w(px(0.0)).child(inputs.output.clone()))
                    .child(
                        compact_button(tr("Remove"))
                            .id(("windows-remove-replacement", index))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_text_replacement(index, cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Modes", Some(add)))
            .child(
                pane_body().child(
                    div()
                        .id("windows-modes-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .px_8()
                        .pt_1()
                        .pb_7()
                        .child(
                            div().w_full().flex().justify_center().child(
                                pane_content()
                                    .child(settings_section_label("Default mode"))
                        .child(
                            settings_panel().child(
                                settings_row(
                                    "Text replacements",
                                    "Exact phrase-boundary corrections run before every Windows paste",
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(rgb(TEXT_SOFT))
                                        .child(tr_fill(
                                            "{} rules",
                                            &replacement_count.to_string(),
                                        )),
                                )
                                .border_b_0(),
                            ),
                        )
                        .child(settings_section_label("Application modes"))
                        .when(mode_count == 0, |content| {
                            content.child(compact_panel().child(empty_message(
                                "No modes yet. Add one to correct text in specific applications.",
                            )))
                        })
                        .children(mode_panels)
                        .child(settings_section_label("Replacements"))
                        .child(
                            compact_panel()
                                .when(replacement_count == 0, |panel| {
                                    panel.child(empty_message(
                                        "No replacements yet. Add one to correct recurring phrases.",
                                    ))
                                })
                                .children(rows),
                        )
                                    .when_some(
                                        self.host.settings_error.clone(),
                                        |content, error| {
                                            content.child(error_message(
                                                "Text replacements could not be saved.",
                                                error,
                                            ))
                                        },
                                    ),
                            ),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.sync_history_detail_text(cx);
        let retention = self.host.settings.history_retention;
        let retention_control = header_button(format!("{}: {}", tr("Keep"), tr(retention.label())))
            .id("windows-history-retention")
            .on_click(cx.listener(|this, _, _, cx| {
                let all = HistoryRetention::ALL;
                let index = all
                    .iter()
                    .position(|choice| *choice == this.host.settings.history_retention)
                    .unwrap_or(0);
                this.set_history_retention(all[(index + 1) % all.len()]);
                cx.notify();
            }))
            .into_any_element();
        let clear = header_button(if self.history_clear_armed {
            tr("Really clear all?")
        } else {
            tr("Clear all")
        })
        .id("windows-history-clear")
        .when(self.history_clear_armed, |button| {
            button.text_color(rgb(CRITICAL))
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.clear_history();
            cx.notify();
        }))
        .into_any_element();
        let header_action = div()
            .flex()
            .items_center()
            .gap_3()
            .child(retention_control)
            .child(clear)
            .into_any_element();
        let rows: Vec<AnyElement> = self
            .history_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let id = entry.id;
                let selected = self.selected_history == Some(id);
                let age = event_age(entry.timestamp_ms);
                let preview = entry.final_text.replace('\n', " ");
                let meta = entry
                    .application
                    .clone()
                    .unwrap_or_else(|| tr(entry.kind.label()).to_string());
                div()
                    .id(("windows-history-entry", index))
                    .w_full()
                    .pl(px(1.0))
                    .pr_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.0))
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_SELECTED)))
                    .child(selection_pill(selected))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .w_full()
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT_SOFT))
                                    .line_height(px(18.0))
                                    .truncate()
                                    .child(preview),
                            )
                            .child(div().text_size(px(10.0)).text_color(rgb(FAINT)).child(meta)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(FAINT))
                            .child(age),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_history = Some(id);
                        this.history_clear_armed = false;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let retention_off = retention.is_off();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("History", Some(header_action)))
            .child(
                pane_body().px_8().pt_5().pb_7().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(
                            compact_panel()
                                .id("windows-history-list")
                                .w(px(PANE_LIST_WIDTH))
                                .h_full()
                                .flex_none()
                                .overflow_y_scroll()
                                .when(retention_off, |list| {
                                    list.child(empty_message(
                                        "History is off. New dictations are not retained.",
                                    ))
                                })
                                .when(
                                    !retention_off
                                        && self.history_entries.is_empty()
                                        && self.host.history_error.is_none(),
                                    |list| list.child(empty_message("No dictations retained yet.")),
                                )
                                .when_some(self.host.history_error.clone(), |list, error| {
                                    list.child(error_message("History could not be loaded.", error))
                                })
                                .child(
                                    div()
                                        .w(px(PANE_LIST_WIDTH - 2.0))
                                        .p(px(4.0))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .children(rows),
                                ),
                        )
                        .child(
                            compact_panel()
                                .flex_1()
                                .min_w(px(0.0))
                                .h_full()
                                .flex()
                                .flex_col()
                                .child(self.render_history_detail(cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_history_detail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self
            .selected_history
            .and_then(|id| self.history_entries.iter().find(|entry| entry.id == id))
        else {
            return detail_placeholder("Select a history entry.");
        };
        let id = entry.id;
        let copied = self.history_copied == Some(id);
        let show_raw = entry.raw_text.trim() != entry.final_text.trim();
        let action_button = |label: &'static str, id: &'static str| header_button(label).id(id);
        let mut caption = event_age(entry.timestamp_ms);
        if let Some(application) = &entry.application {
            caption.push_str(" · ");
            caption.push_str(application);
        }
        div()
            .id("windows-history-detail")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .px_6()
            .py_6()
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr(entry.kind.label())),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(FAINT))
                                    .child(caption),
                            ),
                    )
                    .child(
                        div()
                            .pt_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                action_button(
                                    if copied { tr("Copied") } else { tr("Copy") },
                                    "windows-history-copy",
                                )
                                .when(copied, |button| button.text_color(rgb(SUCCESS)))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.copy_history_entry(id);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                action_button(tr("Delete"), "windows-history-delete").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.delete_history_entry(id);
                                        cx.notify();
                                    }),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .mt_5()
                            .pt_5()
                            .border_t_1()
                            .border_color(rgb(DIVIDER))
                            .when_some(
                                self.history_detail_text
                                    .as_ref()
                                    .filter(|(text_id, _)| *text_id == id)
                                    .map(|(_, input)| input.clone()),
                                |detail, input| detail.child(input),
                            ),
                    )
                    .when(show_raw, |detail| {
                        detail.child(
                            div()
                                .mt_5()
                                .pt_5()
                                .border_t_1()
                                .border_color(rgb(DIVIDER))
                                .child(section_label("Raw transcript"))
                                .child(
                                    div()
                                        .pt_3()
                                        .text_size(px(12.0))
                                        .line_height(px(19.0))
                                        .text_color(rgb(MUTED))
                                        .child(entry.raw_text.clone()),
                                ),
                        )
                    }),
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
        let mut choices = vec![(tr("Automatic").to_string(), None)];
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
                dropdown_item(("windows-microphone-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.host.set_microphone(selection.clone()).is_ok() {
                            this.microphone_picker_open = false;
                        }
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("windows-microphone-dropdown-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.microphone_picker_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(360.0))
                    .id("windows-microphone-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items)
                    .when_some(self.host.microphone_error.clone(), |list, error| {
                        list.child(error_message("Microphones could not be enumerated.", error))
                    }),
            )
            .into_any_element()
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
            windows_model_catalog()
                .into_iter()
                .map(|model| {
                    let installed = crate::transcription_models::is_installed(
                        model,
                        crate::transcription_models::AUTO_LANGUAGE,
                    ) && crate::transcription_models::is_verified(model);
                    let state = if installed {
                        tr("Installed").to_string()
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
                dropdown_item(("windows-model-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(language) = Self::language_for_model(model, &preferred)
                            && this.host.choose_transcription(model, language).is_ok()
                        {
                            this.transcription_dropdown_open = false;
                        }
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("windows-transcription-dropdown-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.transcription_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(300.0))
                    .id("windows-transcription-dropdown")
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
            .map(|(code, name)| {
                let name = if *code == "auto" {
                    tr("Auto").to_string()
                } else {
                    (*name).to_string()
                };
                ((*code).to_string(), name)
            })
            .collect();
        let panel_rows = languages.len();
        let items = languages
            .into_iter()
            .enumerate()
            .map(|(index, (code, name))| {
                let selected = selection.language == code;
                dropdown_item(("windows-dictation-language-option", index), name, selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.host.choose_transcription(model, code.clone()).is_ok() {
                            this.dictation_language_dropdown_open = false;
                        }
                        cx.notify();
                    }))
            });
        dropdown_backdrop("windows-dictation-language-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.dictation_language_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel(bounds, viewport_height, panel_rows)
                    .id("windows-dictation-language-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    fn render_ui_language_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.ui_language_dropdown_bounds else {
            return div().into_any_element();
        };
        let current = self.host.settings.ui_language.clone();
        let items = crate::windows_i18n::LANGUAGE_CHOICES
            .iter()
            .enumerate()
            .map(|(index, (code, name))| {
                let selected = current.as_deref() == *code;
                let label = if code.is_none() {
                    tr("System").to_string()
                } else {
                    (*name).to_string()
                };
                let code = *code;
                dropdown_item(("windows-ui-language-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_ui_language(code);
                        this.ui_language_dropdown_open = false;
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("windows-ui-language-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.ui_language_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel(
                    bounds,
                    viewport_height,
                    crate::windows_i18n::LANGUAGE_CHOICES.len(),
                )
                .id("windows-ui-language-dropdown")
                .overflow_y_scroll()
                .on_click(|_, _, cx| cx.stop_propagation())
                .children(items),
            )
            .into_any_element()
    }

    fn render_model_catalog_language_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.model_catalog_language_dropdown_bounds else {
            return div().into_any_element();
        };
        let selected_language = self.model_catalog_language_filter.clone();
        let mut languages: Vec<(&str, &str)> = crate::transcription_models::LANGUAGES
            .iter()
            .filter(|(code, _)| *code != crate::transcription_models::AUTO_LANGUAGE)
            .copied()
            .collect();
        languages.sort_by_key(|(_, name)| *name);
        let choices = std::iter::once((None, tr("All languages").to_string())).chain(
            languages
                .into_iter()
                .map(|(code, name)| (Some(code.to_string()), name.to_string())),
        );
        let items = choices.enumerate().map(|(index, (code, name))| {
            let selected = code == selected_language;
            dropdown_item(
                ("windows-model-language-filter-option", index),
                name,
                selected,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.model_catalog_language_filter = code.clone();
                this.model_catalog_language_dropdown_open = false;
                cx.notify();
            }))
        });
        let panel_rows = crate::transcription_models::LANGUAGES.len();
        dropdown_backdrop("windows-model-language-filter-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.model_catalog_language_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, px(230.0))
                    .id("windows-model-language-filter-dropdown")
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    /// The Windows model browser: the full runtime catalog with install
    /// state, independent of the active dictation language. Installing keeps
    /// the current language when the model supports it, otherwise Auto.
    fn render_model_manager(
        &mut self,
        transcription: &DesktopTranscriptionSnapshot,
        viewport_width: Pixels,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selection = transcription.selection.clone();
        let recommendation_language = self.model_catalog_language_filter.clone();
        let catalog = filtered_windows_model_catalog(recommendation_language.as_deref());
        let filter_label =
            model_catalog_language_filter_label(self.model_catalog_language_filter.as_deref());
        let dialog_width = px(640.0).min(viewport_width - px(40.0));
        let dialog_height =
            px(640.0).min(viewport_height - px(crate::windows_ui::CAPTION_HEIGHT) - px(32.0));
        let mut entries: Vec<(usize, &'static crate::transcription_models::ModelDefinition)> =
            catalog.into_iter().enumerate().collect();
        let active_position = entries.iter().position(|(_, definition)| {
            definition.id == selection.model
                && Self::language_for_model(definition.id, &selection.language)
                    .as_deref()
                    .is_some_and(|language| {
                        crate::transcription_models::is_installed(definition, language)
                            && crate::transcription_models::is_verified(definition)
                    })
        });
        let active_entry = active_position.map(|position| entries.remove(position));
        // One flat list with the active model pinned on top.
        let mut sections: Vec<AnyElement> = Vec::new();
        {
            let build_row = |index: usize,
                             definition: &'static crate::transcription_models::ModelDefinition|
             -> AnyElement {
                let install_language = Self::language_for_model(definition.id, &selection.language);
                let installed = install_language.as_deref().is_some_and(|language| {
                    crate::transcription_models::is_installed(definition, language)
                        && crate::transcription_models::is_verified(definition)
                });
                let downloading = transcription.preparing == Some(definition.id);
                let another_preparing = transcription.preparing.is_some() && !downloading;
                let active = selection.model == definition.id && installed;
                let status = if downloading {
                    let progress = definition.download_bytes().map_or(0.0, |bytes| {
                        (transcription.downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
                    });
                    let stage = transcription
                        .preparation_stage
                        .unwrap_or(crate::transcription_models::ModelPreparationStage::Downloading);
                    let (label, progress) = match stage {
                        crate::transcription_models::ModelPreparationStage::Downloading => (
                            format!("{} {:.0}%", tr("Downloading"), progress * 100.0),
                            Some(TranscriptionPickerProgress::Downloading(progress)),
                        ),
                        crate::transcription_models::ModelPreparationStage::Verifying => {
                            (tr("Verifying model").into(), None)
                        }
                        crate::transcription_models::ModelPreparationStage::Loading => (
                            tr("Loading model").into(),
                            Some(TranscriptionPickerProgress::Loading(0.25)),
                        ),
                    };
                    TranscriptionPickerStatus::Preparing { label, progress }
                } else if active {
                    TranscriptionPickerStatus::Active
                } else {
                    TranscriptionPickerStatus::Available { installed }
                };
                let recommendation = recommendation_language.as_deref().and_then(|language| {
                    crate::transcription_models::choices_for_runtime(language)
                        .iter()
                        .find(|recommended| recommended.model.id == definition.id)
                        .map(|recommended| tr(recommended.recommendation.label()).to_string())
                });
                let detail = format!("{} · {}", definition.coverage, definition.size_label());
                let compatibility_note = install_language.as_deref().and_then(|language| {
                    (language != selection.language).then(|| {
                        tr_fill(
                            "Switches dictation to {}",
                            crate::transcription_models::language_name(language),
                        )
                    })
                });
                let (control, progress_bar): (AnyElement, Option<AnyElement>) = match status {
                    TranscriptionPickerStatus::Active => (
                        // Sized like header_button so the pill lines up
                        // with the Use/Download controls on other rows.
                        div()
                            .h(px(32.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded(px(4.0))
                            .bg(rgb(accent_color()))
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_ON_ACCENT))
                            .child(tr("Active"))
                            .into_any_element(),
                        None,
                    ),
                    TranscriptionPickerStatus::Preparing { label, progress } => (
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(MUTED))
                                    .child(label),
                            )
                            .child(
                                header_button(tr("Cancel"))
                                    .id(("windows-model-cancel", index))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.cancel_transcription_preparation();
                                        cx.notify();
                                    })),
                            )
                            .into_any_element(),
                        progress.map(|progress| {
                            let indicator = match progress {
                                TranscriptionPickerProgress::Downloading(progress) => div()
                                    .h_full()
                                    .w(relative(progress))
                                    .rounded_full()
                                    .bg(rgb(accent_color())),
                                TranscriptionPickerProgress::Loading(progress) => div()
                                    .ml(relative(progress * 0.75))
                                    .h_full()
                                    .w(relative(0.25))
                                    .rounded_full()
                                    .bg(rgb(accent_color())),
                            };
                            div()
                                .mt_2()
                                .h(px(3.0))
                                .w_full()
                                .rounded_full()
                                .bg(rgb(CANVAS))
                                .child(indicator)
                                .into_any_element()
                        }),
                    ),
                    TranscriptionPickerStatus::Available { installed } => {
                        let button = header_button(if another_preparing {
                            tr("Preparing model")
                        } else if installed {
                            tr("Use")
                        } else {
                            tr("Download")
                        })
                        .id(("windows-model-choose", index));
                        let button = if another_preparing {
                            button
                        } else {
                            button.on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if let Some(language) = install_language.clone() {
                                    this.choose_transcription_model(definition.id, language, cx);
                                }
                                cx.notify();
                            }))
                        };
                        let uninstall = installed.then(|| {
                            header_button(crate::windows_ui::fluent_icon("\u{E74D}", 12.0, TEXT))
                                .id(("windows-model-uninstall", index))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if let Err(error) =
                                        crate::transcription_models::uninstall(definition)
                                    {
                                        this.host.transcription_error = Some(format!("{error:#}"));
                                    }
                                    cx.notify();
                                }))
                        });
                        (
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .children(uninstall)
                                .child(button)
                                .into_any_element(),
                            None,
                        )
                    }
                };
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .rounded(px(PANEL_RADIUS))
                    .border_1()
                    .border_color(if active {
                        rgb(accent_color())
                    } else {
                        rgb(LINE)
                    })
                    .bg(rgb(SURFACE))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(13.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(TEXT))
                                                    .child(definition.name),
                                            )
                                            .when_some(recommendation, |name, recommendation| {
                                                name.child(
                                                    div()
                                                        .flex_none()
                                                        .text_size(px(10.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(rgb(accent_color()))
                                                        .child(recommendation),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .pt_1()
                                            .truncate()
                                            .text_size(px(11.0))
                                            .text_color(rgb(MUTED))
                                            .child(detail),
                                    )
                                    .when_some(compatibility_note, |column, note| {
                                        column.child(
                                            div()
                                                .pt_1()
                                                .truncate()
                                                .text_size(px(10.0))
                                                .text_color(rgb(FAINT))
                                                .child(note),
                                        )
                                    }),
                            )
                            .child(div().flex_none().child(control)),
                    )
                    .children(progress_bar)
                    .into_any_element()
            };
            sections.extend(active_entry.map(|(index, definition)| build_row(index, definition)));
            sections.extend(
                entries
                    .iter()
                    .map(|&(index, definition)| build_row(index, definition)),
            );
        }

        div()
            .id("windows-model-manager-backdrop")
            .absolute()
            .top(px(crate::windows_ui::CAPTION_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(OVERLAY_SMOKE))
            .on_click(cx.listener(|this, _, _, cx| {
                this.dismiss_transcription_picker(cx);
            }))
            .child(
                div()
                    .id("windows-model-manager")
                    .w(dialog_width)
                    .h(dialog_height)
                    .flex()
                    .flex_col()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(rgb(DIALOG_STROKE))
                    .bg(rgb(OVERLAY_PANEL))
                    .shadow_2xl()
                    .overflow_hidden()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .border_b_1()
                            .border_color(rgb(DIVIDER))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr("Speech models")),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .relative()
                                            .child(
                                                disclosure_button(filter_label)
                                                    .id("windows-model-language-filter")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.model_catalog_language_dropdown_open =
                                                            !this.model_catalog_language_dropdown_open;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                canvas(
                                                    {
                                                        let entity = cx.entity();
                                                        move |bounds, _, cx| {
                                                            entity.update(cx, |this, _| {
                                                                this.model_catalog_language_dropdown_bounds = Some(bounds);
                                                            });
                                                        }
                                                    },
                                                    |_, _, _, _| {},
                                                )
                                                .w_full()
                                                .h(px(0.0)),
                                            ),
                                    )
                                    .child(
                                        header_button(crate::windows_ui::fluent_icon(
                                            "\u{E8BB}", 12.0, TEXT,
                                        ))
                                        .id("windows-model-manager-close")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.dismiss_transcription_picker(cx);
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("windows-model-manager-list")
                            .flex_1()
                            .min_h(px(0.0))
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .overflow_y_scroll()
                            .children(sections)
                            .when_some(transcription.error.clone(), |list, error| {
                                list.child(
                                    div().w_full().child(error_message(
                                        "Model could not be installed.",
                                        error,
                                    )),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_hotkey_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.host.settings.dictation_hotkey.clone();
        let presets = [
            (
                "Ctrl + Win",
                tr("Recommended Windows push-to-talk shortcut"),
                crate::windows_settings::WindowsHotkey::default(),
            ),
            (
                "Ctrl + Alt + Space",
                tr("Three-key fallback for keyboards without a Windows key"),
                crate::windows_settings::WindowsHotkey::ctrl_alt_space(),
            ),
        ];
        let rows = presets
            .into_iter()
            .enumerate()
            .map(|(index, (title, description, binding))| {
                let selected = binding == current;
                let keycaps = binding.keycaps();
                div()
                    .id(("windows-hotkey-preset", index))
                    .w_full()
                    .min_h(px(66.0))
                    .px_4()
                    .py_3()
                    .mb_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(PANEL_RADIUS))
                    .border_1()
                    .border_color(if selected {
                        rgb(accent_color())
                    } else {
                        rgb(LINE)
                    })
                    .bg(rgb(SURFACE))
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .pt_1()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(description),
                            ),
                    )
                    .child(
                        div()
                            .pl_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(hotkey_keycaps(keycaps, 1.0))
                            .when(selected, |right| {
                                right.child(
                                    div()
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(accent_color()))
                                        .child(tr("Current")),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.host.set_dictation_hotkey(binding.clone()).is_ok() {
                            this.hotkey_picker_open = false;
                        }
                        cx.notify();
                    }))
            });

        div()
            .id("windows-hotkey-picker-backdrop")
            .absolute()
            .top(px(crate::windows_ui::CAPTION_HEIGHT))
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(OVERLAY_SMOKE))
            .on_click(cx.listener(|this, _, _, cx| {
                this.hotkey_picker_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("windows-hotkey-picker")
                    .w(px(560.0))
                    .flex()
                    .flex_col()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(rgb(DIALOG_STROKE))
                    .bg(rgb(OVERLAY_PANEL))
                    .shadow_2xl()
                    .overflow_hidden()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_6()
                            .py_5()
                            .border_b_1()
                            .border_color(rgb(DIVIDER))
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr("Choose dictation shortcut")),
                            )
                            .child(
                                div()
                                    .pt_1()
                                    .text_size(px(12.0))
                                    .text_color(rgb(MUTED))
                                    .child(tr(
                                        "Hold the shortcut to record; release it to transcribe and paste.",
                                    )),
                            ),
                    )
                    .child(div().p_5().pb_2().children(rows))
                    .child(
                        div()
                            .px_6()
                            .pb_5()
                            .text_size(px(10.0))
                            .text_color(rgb(FAINT))
                            .child(tr("Escape still cancels the active recording.")),
                    ),
            )
            .into_any_element()
    }
}

impl TranscriptionPickerDelegate for WindowsApp {
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
        self.model_catalog_language_dropdown_open = false;
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

impl Render for WindowsApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        debug_assert!(self.host.capabilities().listener_control);
        debug_assert!(self.host.capabilities().history);
        debug_assert!(self.host.capabilities().replacements);
        let viewport = window.viewport_size();
        let snapshot = self.host.snapshot();
        let content = match self.pane {
            WindowsPane::Settings => self.render_settings(&snapshot, cx),
            WindowsPane::Modes => self.render_modes(cx),
            WindowsPane::History => self.render_history(cx),
        };
        let model_picker =
            self.transcription_picker
                .language()
                .map(str::to_owned)
                .map(|_language| {
                    self.render_model_manager(
                        &snapshot.transcription,
                        viewport.width,
                        viewport.height,
                        cx,
                    )
                });
        let hotkey_picker = self
            .hotkey_picker_open
            .then(|| self.render_hotkey_picker(cx));
        let model_catalog_language_dropdown = self
            .model_catalog_language_dropdown_open
            .then(|| self.render_model_catalog_language_dropdown(viewport.height, cx));
        let microphone_dropdown = self
            .microphone_picker_open
            .then(|| self.render_microphone_dropdown(viewport.height, cx));
        let transcription_dropdown = self
            .transcription_dropdown_open
            .then(|| self.render_transcription_dropdown(viewport.height, cx));
        let dictation_language_dropdown = self
            .dictation_language_dropdown_open
            .then(|| self.render_dictation_language_dropdown(viewport.height, cx));
        let ui_language_dropdown = self
            .ui_language_dropdown_open
            .then(|| self.render_ui_language_dropdown(viewport.height, cx));
        window_frame()
            .flex_col()
            .child(caption_bar(window))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .flex()
                    .child(self.render_navigation(cx))
                    .child(div().flex_1().h_full().overflow_hidden().child(content)),
            )
            .children(model_picker)
            .children(model_catalog_language_dropdown)
            .children(microphone_dropdown)
            .children(transcription_dropdown)
            .children(dictation_language_dropdown)
            .children(ui_language_dropdown)
            .children(hotkey_picker)
    }
}

fn windows_model_catalog() -> Vec<&'static crate::transcription_models::ModelDefinition> {
    crate::transcription_models::MODELS
        .iter()
        .filter(|model| model.available())
        .collect()
}

fn filtered_windows_model_catalog(
    language_filter: Option<&str>,
) -> Vec<&'static crate::transcription_models::ModelDefinition> {
    windows_model_catalog()
        .into_iter()
        .filter(|model| language_filter.is_none_or(|language| model.supports_language(language)))
        .collect()
}

fn model_catalog_language_filter_label(filter: Option<&str>) -> String {
    match filter {
        None => tr("All languages").to_string(),
        Some(language) => crate::transcription_models::language_name(language).to_string(),
    }
}

fn detail_placeholder(message: &'static str) -> AnyElement {
    let message = tr(message);
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(12.0))
        .text_color(rgb(FAINT))
        .child(message)
        .into_any_element()
}

fn event_age(timestamp_ms: u64) -> String {
    let seconds = crate::history::now_ms().saturating_sub(timestamp_ms) / 1_000;
    let (template, value) = match seconds {
        0..=59 => ("{}s ago", seconds),
        60..=3_599 => ("{}m ago", seconds / 60),
        3_600..=86_399 => ("{}h ago", seconds / 3_600),
        _ => ("{}d ago", seconds / 86_400),
    };
    tr_fill(template, &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::TranscriptionModelId;

    #[test]
    fn windows_catalog_contains_every_available_model_without_language_filtering() {
        let catalog = windows_model_catalog();
        let available = crate::transcription_models::MODELS
            .iter()
            .filter(|model| model.available())
            .count();

        assert_eq!(catalog.len(), available);
        assert!(
            catalog
                .iter()
                .any(|model| model.id == TranscriptionModelId::ParakeetV2)
        );
        assert!(
            catalog
                .iter()
                .any(|model| model.id == TranscriptionModelId::CohereTranscribe)
        );
    }

    #[test]
    fn catalog_filter_keeps_only_models_supporting_the_selected_language() {
        assert_eq!(
            filtered_windows_model_catalog(None).len(),
            windows_model_catalog().len()
        );

        let filtered = filtered_windows_model_catalog(Some("pl"));

        assert!(filtered.len() < windows_model_catalog().len());
        assert!(filtered.iter().all(|model| model.supports_language("pl")));
        assert!(
            filtered
                .iter()
                .all(|model| model.id != TranscriptionModelId::ParakeetUnifiedEnglish)
        );
    }

    #[test]
    fn catalog_opens_filtered_to_the_dictation_language_except_auto() {
        assert_eq!(
            WindowsApp::catalog_filter_for_language("pl").as_deref(),
            Some("pl")
        );
        assert_eq!(
            WindowsApp::catalog_filter_for_language(crate::transcription_models::AUTO_LANGUAGE),
            None
        );
    }

    #[test]
    fn model_switch_uses_support_matrix_instead_of_recommendations() {
        assert_eq!(
            WindowsApp::language_for_model(TranscriptionModelId::Qwen3Asr06B, "en").as_deref(),
            Some("en")
        );
        assert_eq!(
            WindowsApp::language_for_model(TranscriptionModelId::CohereTranscribe, "de").as_deref(),
            Some("de")
        );
        assert_eq!(
            WindowsApp::language_for_model(TranscriptionModelId::ParakeetV2, "de").as_deref(),
            Some("en")
        );
    }
}
