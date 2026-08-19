use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, Entity, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, SharedString, Subscription, Timer,
    TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, prelude::*, px, relative,
    rgb, size,
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
use crate::desktop_history_pane::{
    HistoryPaneAction, HistoryPaneDelegate, HistoryPaneState,
    render_history_pane as render_shared_history_pane,
};
use crate::desktop_host::{
    DesktopAction, DesktopCapabilities, DesktopHost, DesktopListenerSnapshot,
    DesktopMicrophoneSnapshot, DesktopShortcut, DesktopSnapshot, DesktopTranscriptionSnapshot,
    DesktopUpdateStatus,
};
use crate::desktop_mode_basics::{
    ModeApplicationEditorView, ModeBasicsAction, ModeBasicsDelegate, ModeBasicsView,
    render_mode_basics as render_shared_mode_basics,
};
use crate::desktop_mode_list::{
    ModeActivation, ModeListAction, ModeListDelegate, ModeListEntry, ModeListView, ModeTarget,
    render_mode_list as render_shared_mode_list,
};
use crate::desktop_mode_processing::{
    ModeProcessingAction, ModeProcessingDelegate, ModeProcessingUnavailableView,
    ModeProcessingView, render_mode_processing as render_shared_mode_processing,
};
use crate::desktop_replacement_editor::{
    ReplacementEditorAction, ReplacementEditorDelegate, ReplacementEditorInput,
    ReplacementEditorView, render_replacement_editor as render_shared_replacement_editor,
};
use crate::desktop_shell::{DesktopPane, render_navigation_items};
use crate::desktop_transcription_picker::TranscriptionPickerDelegate;
use crate::desktop_ui::{
    FAINT, LINE, MUTED, PANE_LIST_WIDTH, SECTION_GAP, SIDEBAR_WIDTH, TEXT, TEXT_ON_ACCENT,
    TEXT_SOFT, accent_color, compact_button, compact_section_label, disclosure_button,
    dropdown_backdrop, dropdown_item, dropdown_panel, dropdown_panel_with_width, error_message,
    header_button, hotkey_keycaps, pane_body, pane_content, pane_header_with_action,
    segmented_control, segmented_item, settings_copy, settings_panel, settings_row,
    settings_section_label, sidebar_frame, toggle, window_frame,
};
use crate::desktop_voice_action_pane::{
    OPENCODE_SETUP_URL, VoiceActionError, VoiceActionPaneAction, VoiceActionPaneDelegate,
    VoiceActionReadyView, VoiceActionSettingRow, VoiceActionView,
    render_voice_action_pane as render_shared_voice_action_pane,
};
use crate::events::EventReader;
use crate::history::{History, HistoryRetention};
use crate::text_input::{Changed as TextChanged, TextInput};
use crate::windows_i18n::{tr, tr_fill};
use crate::windows_settings::IndicatorPosition;
use crate::windows_ui::{CRITICAL, SUCCESS, caption_bar};

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
    last_dictation: Arc<Mutex<Option<String>>>,
    indicator: crate::windows_indicator::WindowsIndicatorSender,
    replacements: Arc<RwLock<crate::text_replacements::ReplacementSet>>,
    mode_runtime: Arc<RwLock<crate::windows_dictation::WindowsModeRuntime>>,
    voice_action_model: Arc<RwLock<Option<crate::opencode::Model>>>,
    opencode_catalog: Option<crate::opencode::ModelCatalog>,
    opencode_catalog_rx: Option<std::sync::mpsc::Receiver<Result<crate::opencode::ModelCatalog>>>,
    opencode_catalog_error: Option<String>,
    prepared_transcriber: Option<crate::local_transcriber::LocalTranscriber>,
    hints_restart_after: Option<Instant>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    updater: crate::windows_updater::WindowsUpdater,
}

struct WindowsApp {
    host: WindowsDesktopHost,
    pane: DesktopPane,
    hud_lab: crate::desktop_hud_lab::HudLabState,
    history_pane: HistoryPaneState,
    replacement_inputs: Vec<ReplacementEditorInput>,
    transcription_picker: TranscriptionPickerState,
    mode_inputs: Vec<ModeInputs>,
    global_processing_prompt: Entity<TextInput>,
    _global_processing_prompt_subscription: Subscription,
    selected_mode: ModeTarget,
    opencode_model_dropdown: Option<OpenCodeModelTarget>,
    opencode_model_dropdown_bounds: Option<Bounds<Pixels>>,
    recognition_hints_input: Entity<TextInput>,
    _recognition_hints_subscription: Subscription,
    history_search_input: Entity<TextInput>,
    _history_search_subscription: Subscription,
    restart_scheduled: bool,
    onboarding: bool,
    onboarding_selection: crate::transcription_models::TranscriptionSelection,
    onboarding_language_dropdown_open: bool,
    onboarding_language_dropdown_bounds: Option<Bounds<Pixels>>,
    model_catalog_language_filter: Option<String>,
    model_catalog_language_dropdown_open: bool,
    model_catalog_language_dropdown_bounds: Option<Bounds<Pixels>>,
    microphone_picker_open: bool,
    /// Which shortcut row is recording the user's next chord, if any.
    hotkey_capture: Option<(HotkeyTarget, crate::windows_input::ChordCapture)>,
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
}

/// Which shortcut a settings row is re-recording.
#[derive(Clone, Copy, Eq, PartialEq)]
enum HotkeyTarget {
    Dictation,
    PasteLast,
    VoiceAction,
}

impl HotkeyTarget {
    /// Paste Last and Voice Action chords need a regular key (the hook
    /// cannot distinguish a modifier-only tap from ordinary typing), so a
    /// modifier-only capture keeps recording for them.
    fn requires_key(self) -> bool {
        matches!(self, Self::PasteLast | Self::VoiceAction)
    }
}

struct ModeInputs {
    name: Entity<TextInput>,
    applications: Entity<TextInput>,
    websites: Entity<TextInput>,
    processing_prompt: Entity<TextInput>,
    corrections: Vec<ReplacementEditorInput>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum OpenCodeModelTarget {
    VoiceAction,
    Mode(ModeTarget),
}

impl OpenCodeModelTarget {
    fn id_fragment(self) -> String {
        match self {
            Self::VoiceAction => "voice-action".into(),
            Self::Mode(target) => format!("mode-{}", target.id_fragment()),
        }
    }
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
    // First-run detection must precede anything that saves settings.
    let onboarding = !crate::windows_settings::exists()
        || std::env::var_os("HEX_ONBOARDING").is_some_and(|value| value == "1");
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
                        let (host, history_error) = WindowsDesktopHost::new(
                            event_path.clone(),
                            settings.clone(),
                            settings_error.clone(),
                            indicator_sender.clone(),
                        );
                        let history_pane =
                            HistoryPaneState::new(host.history.clone(), history_error);
                        let replacement_inputs = host
                            .settings
                            .text_replacements
                            .iter()
                            .map(|replacement| {
                                ReplacementEditorInput::new(
                                    replacement,
                                    WindowsApp::sync_text_replacements,
                                    cx,
                                )
                            })
                            .collect();
                        let mode_inputs = host
                            .settings
                            .modes
                            .iter()
                            .map(|mode| WindowsApp::mode_inputs(mode, cx))
                            .collect();
                        let global_processing_prompt = cx.new(|cx| {
                            TextInput::multiline_with_height(
                                cx,
                                "Tell OpenCode exactly how to transform the dictated text.",
                                &host.settings.dictation_post_processing.prompt,
                                px(92.0),
                            )
                        });
                        let global_processing_prompt_subscription = cx.subscribe(
                            &global_processing_prompt,
                            |this: &mut WindowsApp, input, _: &TextChanged, cx| {
                                this.sync_global_processing_prompt(
                                    input.read(cx).text().to_string(),
                                    cx,
                                );
                            },
                        );
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
                        let history_search_input =
                            cx.new(|cx| crate::text_input::TextInput::new(cx, "Search", ""));
                        let history_search_subscription = cx.subscribe(
                            &history_search_input,
                            |this: &mut WindowsApp, input, _: &TextChanged, cx| {
                                this.history_pane
                                    .set_query(input.read(cx).text().to_string());
                                cx.notify();
                            },
                        );
                        let onboarding_selection = host.settings.transcription.clone();
                        WindowsApp {
                            host,
                            pane: DesktopPane::Settings,
                            hud_lab: crate::desktop_hud_lab::HudLabState::new(),
                            history_pane,
                            recognition_hints_input,
                            _recognition_hints_subscription: recognition_hints_subscription,
                            history_search_input,
                            _history_search_subscription: history_search_subscription,
                            restart_scheduled: false,
                            onboarding,
                            onboarding_selection,
                            onboarding_language_dropdown_open: false,
                            onboarding_language_dropdown_bounds: None,
                            replacement_inputs,
                            transcription_picker: TranscriptionPickerState::Closed,
                            mode_inputs,
                            global_processing_prompt,
                            _global_processing_prompt_subscription:
                                global_processing_prompt_subscription,
                            selected_mode: ModeTarget::Global,
                            opencode_model_dropdown: None,
                            opencode_model_dropdown_bounds: None,
                            model_catalog_language_filter: None,
                            model_catalog_language_dropdown_open: false,
                            model_catalog_language_dropdown_bounds: None,
                            microphone_picker_open: false,
                            hotkey_capture: None,
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
        // The microphone stays cold behind the first-run dialog; finishing
        // onboarding starts the listener instead.
        if listen_on_launch && !onboarding {
            app.update(cx, |app, cx| {
                app.host.start();
                cx.notify();
            });
        }
        let indicator_app = app.clone();
        cx.spawn(async move |cx| {
            // The pump is the HUD's display link: it forwards dictation
            // events, steps the springs once per tick, and keeps the OS
            // window on screen exactly while the model says the HUD is
            // visible — through transcription and the exit animation,
            // like the macOS indicator.
            let mut shown = false;
            loop {
                Timer::after(Duration::from_millis(16)).await;
                while let Ok(event) = indicator_events.try_recv() {
                    let Some(indicator) = &indicator else {
                        continue;
                    };
                    if indicator
                        .update(cx, |indicator, _| indicator.handle(event))
                        .is_err()
                    {
                        return;
                    }
                }
                let Some(indicator) = &indicator else {
                    continue;
                };
                let Ok(visible) = indicator.update(cx, |indicator, cx| indicator.tick(cx)) else {
                    return;
                };
                let position = indicator_app
                    .update(cx, |app, _| app.host.settings.indicator_position)
                    .unwrap_or_default();
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
                            if this.pane == DesktopPane::History {
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

impl WindowsDesktopHost {
    fn new(
        event_path: PathBuf,
        settings: crate::windows_settings::WindowsSettings,
        settings_error: Option<String>,
        indicator: crate::windows_indicator::WindowsIndicatorSender,
    ) -> (Self, Option<String>) {
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
        let mode_runtime = Arc::new(RwLock::new(
            crate::windows_dictation::WindowsModeRuntime::from_settings(&settings),
        ));
        let voice_action_model = Arc::new(RwLock::new(settings.voice_action_model.clone()));
        let mut host = Self {
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
            last_dictation: Arc::new(Mutex::new(None)),
            indicator,
            replacements,
            mode_runtime,
            voice_action_model,
            opencode_catalog: None,
            opencode_catalog_rx: None,
            opencode_catalog_error: None,
            prepared_transcriber: None,
            hints_restart_after: None,
            transcription_preparation: None,
            transcription_error: None,
            updater: crate::windows_updater::WindowsUpdater::start(),
        };
        // A configured voice-action hotkey with no chosen model adopts the
        // OpenCode default without a visit to the Voice Action tab.
        if host.settings.voice_action_hotkey.is_some()
            && host.settings.voice_action_model.is_none()
            && crate::windows_voice_action::opencode_installed()
        {
            host.request_opencode_catalog();
        }
        (host, history_error)
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
        let voice_action_hotkey = self.settings.voice_action_hotkey.clone();
        let double_tap_lock = self.settings.double_tap_lock;
        let double_tap_only = self.settings.double_tap_only;
        let while_dictating = self.settings.while_dictating;
        let release_microphone_while_idle = self.settings.release_microphone_while_idle;
        let feedback_volume = self.settings.feedback_volume;
        let history = self.history.clone();
        let last_dictation = self.last_dictation.clone();
        let indicator = self.indicator.clone();
        let replacements = self.replacements.clone();
        let mode_runtime = self.mode_runtime.clone();
        let voice_action_model = self.voice_action_model.clone();
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
                        voice_action_hotkey,
                        double_tap_lock,
                        double_tap_only,
                        while_dictating,
                        release_microphone_while_idle,
                        feedback_volume,
                        history,
                        last_dictation,
                        indicator: Some(indicator),
                        replacements,
                        mode_runtime,
                        voice_action_model,
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
        hotkey.validate().map_err(|error| {
            let error = format!("That chord cannot be a dictation shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
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
        Ok(())
    }

    fn set_double_tap_lock(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.settings.double_tap_lock {
            self.settings_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.double_tap_lock = enabled;
        candidate.save().map_err(|error| {
            let error = format!("Could not save double-tap lock: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.restart_listener_for_settings();
        Ok(())
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

    fn set_voice_action_model(&mut self, model: Option<crate::opencode::Model>) {
        if model == self.settings.voice_action_model {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.voice_action_model = model;
        match candidate.save() {
            Ok(()) => {
                *self
                    .voice_action_model
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) =
                    candidate.voice_action_model.clone();
                self.settings = candidate;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error =
                    Some(format!("Could not save the voice action model: {error:#}"));
            }
        }
    }

    /// Fetch the OpenCode model catalog on a worker thread; the refresh
    /// loop polls the receiver.
    fn request_opencode_catalog(&mut self) {
        if self.opencode_catalog_rx.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.opencode_catalog_rx = Some(receiver);
        let _ = std::thread::Builder::new()
            .name("windows-opencode-catalog".into())
            .spawn(move || {
                let _ = sender.send(crate::opencode::load_model_catalog());
            });
    }

    fn poll_opencode_catalog(&mut self) -> bool {
        let Some(receiver) = &self.opencode_catalog_rx else {
            return false;
        };
        match receiver.try_recv() {
            Ok(Ok(catalog)) => {
                // First load with nothing configured adopts the default.
                if self.settings.voice_action_model.is_none()
                    && let Some(default) = catalog
                        .default_key
                        .as_ref()
                        .and_then(|key| catalog.models.iter().find(|choice| &choice.key == key))
                {
                    self.set_voice_action_model(Some(default.model()));
                }
                self.opencode_catalog = Some(catalog);
                self.opencode_catalog_error = None;
                self.opencode_catalog_rx = None;
                true
            }
            Ok(Err(error)) => {
                self.opencode_catalog_error = Some(format!("{error:#}"));
                self.opencode_catalog_rx = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.opencode_catalog_error =
                    Some("the OpenCode catalog worker stopped unexpectedly".into());
                self.opencode_catalog_rx = None;
                true
            }
        }
    }

    fn set_voice_action_enabled(&mut self, enabled: bool) {
        if enabled == self.settings.voice_action_hotkey.is_some() {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.voice_action_hotkey =
            enabled.then(crate::windows_settings::WindowsHotkey::voice_action_default);
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "Could not save the voice action setting: {error:#}"
                ));
            }
        }
    }

    /// Persists a Voice Action binding; choosing one while the feature is
    /// disabled enables it with that binding.
    fn set_voice_action_hotkey(
        &mut self,
        hotkey: crate::windows_settings::WindowsHotkey,
    ) -> Result<()> {
        hotkey.validate().map_err(|error| {
            let error = format!("That chord cannot be a voice action shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        if Some(&hotkey) == self.settings.voice_action_hotkey.as_ref() {
            self.settings_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.voice_action_hotkey = Some(hotkey);
        candidate.save().map_err(|error| {
            let error = format!("Could not save the voice action shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.restart_listener_for_settings();
        Ok(())
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

    fn set_double_tap_only(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.settings.double_tap_only {
            self.settings_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.double_tap_only = enabled;
        candidate.save().map_err(|error| {
            let error = format!("Could not save double-tap only: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.restart_listener_for_settings();
        Ok(())
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

    /// Persists a captured Paste Last binding; capturing one while the
    /// feature is disabled enables it with that binding.
    fn set_paste_last_hotkey(
        &mut self,
        hotkey: crate::windows_settings::WindowsHotkey,
    ) -> Result<()> {
        hotkey.validate().map_err(|error| {
            let error = format!("That chord cannot be a Paste Last shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        if Some(&hotkey) == self.settings.paste_last_hotkey.as_ref() {
            self.settings_error = None;
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.paste_last_hotkey = Some(hotkey);
        candidate.save().map_err(|error| {
            let error = format!("Could not save the Paste Last shortcut: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        self.restart_listener_for_settings();
        Ok(())
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
            .mode_runtime
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::windows_dictation::WindowsModeRuntime::from_settings(&candidate);
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    fn set_mode_post_processing(
        &mut self,
        target: ModeTarget,
        settings: crate::dictation_processing::PostProcessingSettings,
    ) -> Result<()> {
        let mut candidate = self.settings.clone();
        match target {
            ModeTarget::Global => candidate.dictation_post_processing = settings,
            ModeTarget::Mode(index) => {
                let mode = candidate
                    .modes
                    .get_mut(index)
                    .ok_or_else(|| eyre!("dictation mode {index} no longer exists"))?;
                mode.post_processing = settings;
            }
        }
        candidate.save().map_err(|error| {
            let error = format!("Could not save mode processing: {error:#}");
            self.settings_error = Some(error.clone());
            eyre!(error)
        })?;
        *self
            .mode_runtime
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::windows_dictation::WindowsModeRuntime::from_settings(&candidate);
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
        self.poll_opencode_catalog();
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
            microphone: DesktopMicrophoneSnapshot {
                devices: self.microphones.clone(),
                error: self.microphone_error.clone(),
                selected: self.settings.microphone.clone(),
            },
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
                crate::windows_updater::UpdateCheck::ReadyToRestart { .. } => {
                    DesktopUpdateStatus::ReadyToRestart
                }
                crate::windows_updater::UpdateCheck::Failed => DesktopUpdateStatus::Failed,
            },
        }
    }

    fn dispatch(&mut self, action: DesktopAction) -> Result<()> {
        match action {
            DesktopAction::CheckForUpdates => self.updater.request_check(),
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
                self.login_item_error = None;
            }
            DesktopAction::RefreshMicrophones => self.refresh_microphones(),
            DesktopAction::RestartIntoUpdate => {
                let crate::windows_updater::UpdateCheck::ReadyToRestart { executable, .. } =
                    self.updater.state()
                else {
                    return Err(eyre!("no installed Windows update is ready"));
                };
                crate::windows_updater::relaunch_at(&executable).map_err(|error| {
                    let message = format!("Could not restart HEX: {error:#}");
                    self.settings_error = Some(message);
                    error
                })?;
            }
            DesktopAction::SetDictationShortcut(shortcut) => {
                self.set_dictation_hotkey(windows_hotkey(shortcut)?)?;
            }
            DesktopAction::SetDoubleTapLock(enabled) => {
                self.set_double_tap_lock(enabled)?;
            }
            DesktopAction::SetDoubleTapOnly(enabled) => {
                self.set_double_tap_only(enabled)?;
            }
            DesktopAction::SetMicrophone(microphone) => {
                self.set_microphone(microphone)?;
            }
            DesktopAction::StartListening => self.start(),
            DesktopAction::StopListening => self.stop(),
        }
        Ok(())
    }
}

fn windows_hotkey(shortcut: DesktopShortcut) -> Result<crate::windows_settings::WindowsHotkey> {
    if shortcut.function {
        return Err(eyre!("the Fn modifier is unavailable on Windows"));
    }
    let key = if shortcut.key == " " {
        Some("space".into())
    } else if shortcut.key.trim().is_empty() {
        None
    } else {
        Some(shortcut.key.to_ascii_lowercase())
    };
    let hotkey = crate::windows_settings::WindowsHotkey {
        control: shortcut.control,
        windows: shortcut.platform,
        alt: shortcut.alt,
        shift: shortcut.shift,
        key,
    };
    hotkey.validate()?;
    Ok(hotkey)
}

impl WindowsApp {
    fn sync_text_replacements(&mut self, cx: &mut Context<Self>) {
        let replacements = self
            .replacement_inputs
            .iter()
            .map(|inputs| inputs.value(cx))
            .collect();
        let _ = self.host.set_text_replacements(replacements);
        cx.notify();
    }

    fn mode_inputs(
        mode: &crate::windows_settings::WindowsMode,
        cx: &mut Context<Self>,
    ) -> ModeInputs {
        let name = cx.new(|cx| TextInput::new(cx, "e.g. Coding", &mode.name));
        let applications = cx
            .new(|cx| TextInput::new(cx, "e.g. code, chrome, slack", mode.applications.join(", ")));
        let websites =
            cx.new(|cx| TextInput::new(cx, "e.g. x.com, github.com", mode.websites.join(", ")));
        let processing_prompt = cx.new(|cx| {
            TextInput::multiline_with_height(
                cx,
                "Tell OpenCode exactly how to transform the dictated text.",
                &mode.post_processing.prompt,
                px(92.0),
            )
        });
        let name_changed = cx.subscribe(&name, |this, _, _: &TextChanged, cx| this.sync_modes(cx));
        let applications_changed = cx.subscribe(&applications, |this, _, _: &TextChanged, cx| {
            this.sync_modes(cx)
        });
        let websites_changed = cx.subscribe(&websites, |this, _, _: &TextChanged, cx| {
            this.sync_modes(cx)
        });
        let processing_prompt_changed = cx
            .subscribe(&processing_prompt, |this, _, _: &TextChanged, cx| {
                this.sync_modes(cx)
            });
        let corrections = mode
            .corrections
            .iter()
            .map(|correction| ReplacementEditorInput::new(correction, Self::sync_modes, cx))
            .collect();
        ModeInputs {
            name,
            applications,
            websites,
            processing_prompt,
            corrections,
            _subscriptions: vec![
                name_changed,
                applications_changed,
                websites_changed,
                processing_prompt_changed,
            ],
        }
    }

    fn sync_global_processing_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let mut processing = self.host.settings.dictation_post_processing.clone();
        processing.prompt = prompt;
        let _ = self
            .host
            .set_mode_post_processing(ModeTarget::Global, processing);
        cx.notify();
    }

    fn mode_values(
        &self,
        excluded: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<crate::windows_settings::WindowsMode> {
        self.mode_inputs
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != excluded)
            .map(|(index, inputs)| crate::windows_settings::WindowsMode {
                name: inputs.name.read(cx).text().to_string(),
                applications: inputs
                    .applications
                    .read(cx)
                    .text()
                    .split(',')
                    .map(|application| application.trim().to_string())
                    .filter(|application| !application.is_empty())
                    .collect(),
                websites: inputs
                    .websites
                    .read(cx)
                    .text()
                    .split(',')
                    .map(|website| website.trim().to_string())
                    .filter(|website| !website.is_empty())
                    .collect(),
                corrections: inputs
                    .corrections
                    .iter()
                    .map(|correction| correction.value(cx))
                    .collect(),
                post_processing: {
                    let mut processing = self
                        .host
                        .settings
                        .modes
                        .get(index)
                        .map_or_else(Default::default, |mode| mode.post_processing.clone());
                    processing.prompt = inputs.processing_prompt.read(cx).text().to_string();
                    processing
                },
            })
            .collect()
    }

    fn sync_modes(&mut self, cx: &mut Context<Self>) {
        let modes = self.mode_values(None, cx);
        let _ = self.host.set_modes(modes);
        cx.notify();
    }

    fn add_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = crate::windows_settings::WindowsMode {
            name: format!("Mode {}", self.mode_inputs.len() + 1),
            ..Default::default()
        };
        self.mode_inputs.push(Self::mode_inputs(&mode, cx));
        self.selected_mode = ModeTarget::Mode(self.mode_inputs.len().saturating_sub(1));
        self.sync_modes(cx);
        window.blur();
    }

    fn remove_mode(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.mode_inputs.len() {
            // Build the persisted collection against the old indexes before
            // removing the input row. This keeps every later mode's processing
            // profile attached to that mode when indexes shift left.
            let modes = self.mode_values(Some(index), cx);
            if self.host.set_modes(modes).is_err() {
                cx.notify();
                return;
            }
            self.mode_inputs.remove(index);
            self.selected_mode = match self.selected_mode {
                ModeTarget::Mode(selected) if selected == index => ModeTarget::Global,
                ModeTarget::Mode(selected) if selected > index => ModeTarget::Mode(selected - 1),
                selected => selected,
            };
            cx.notify();
        }
    }

    fn reload_history(&mut self) {
        self.history_pane.reload();
    }

    fn set_history_retention(&mut self, retention: HistoryRetention) {
        match self.host.set_history_retention(retention) {
            Ok(()) => self.history_pane.set_retention(retention),
            Err(error) => self.history_pane.set_error(Some(error.to_string())),
        }
    }

    fn close_popups(&mut self) {
        self.transcription_picker = TranscriptionPickerState::Closed;
        self.model_catalog_language_dropdown_open = false;
        self.opencode_model_dropdown = None;
        self.microphone_picker_open = false;
        self.end_hotkey_capture();
        self.transcription_dropdown_open = false;
        self.dictation_language_dropdown_open = false;
        self.ui_language_dropdown_open = false;
    }

    /// Ends any active chord capture and re-arms the hotkey hook.
    fn end_hotkey_capture(&mut self) {
        if self.hotkey_capture.take().is_some() {
            crate::windows_input::set_capture_inhibited(false);
        }
    }

    /// Starts recording the user's next chord for `target` (clicking the
    /// same binding again cancels). A 30ms poll loop reads the keyboard
    /// directly, so modifier-only chords work; the loop parks itself when
    /// the capture ends. While a capture runs the hotkey hook is
    /// inhibited (no dictation can trigger and no key is suppressed), and
    /// the capture cancels itself the moment this app loses the
    /// foreground, so keystrokes typed elsewhere can never rebind.
    fn toggle_hotkey_capture(&mut self, target: HotkeyTarget, cx: &mut Context<Self>) {
        if self
            .hotkey_capture
            .as_ref()
            .is_some_and(|(active, _)| *active == target)
        {
            self.end_hotkey_capture();
            cx.notify();
            return;
        }
        self.close_popups();
        self.hotkey_capture = Some((target, crate::windows_input::ChordCapture::new()));
        crate::windows_input::set_capture_inhibited(true);
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(30)).await;
                let done = this
                    .update(cx, |this, cx| {
                        let Some((active, capture)) = this.hotkey_capture.as_mut() else {
                            return true;
                        };
                        if *active != target {
                            return true;
                        }
                        if !crate::windows_input::app_is_foreground() {
                            this.end_hotkey_capture();
                            cx.notify();
                            return true;
                        }
                        match capture.poll() {
                            crate::windows_input::ChordPoll::Pending => false,
                            crate::windows_input::ChordPoll::Cancelled => {
                                this.end_hotkey_capture();
                                cx.notify();
                                true
                            }
                            crate::windows_input::ChordPoll::Captured(hotkey) => {
                                if target.requires_key() && hotkey.key.is_none() {
                                    *capture = crate::windows_input::ChordCapture::new();
                                    return false;
                                }
                                this.end_hotkey_capture();
                                let _ = match target {
                                    HotkeyTarget::Dictation => this.host.dispatch(
                                        DesktopAction::SetDictationShortcut(DesktopShortcut {
                                            alt: hotkey.alt,
                                            control: hotkey.control,
                                            function: false,
                                            key: hotkey.key.unwrap_or_default(),
                                            platform: hotkey.windows,
                                            shift: hotkey.shift,
                                        }),
                                    ),
                                    HotkeyTarget::PasteLast => {
                                        this.host.set_paste_last_hotkey(hotkey)
                                    }
                                    HotkeyTarget::VoiceAction => {
                                        this.host.set_voice_action_hotkey(hotkey)
                                    }
                                };
                                cx.notify();
                                true
                            }
                        }
                    })
                    .unwrap_or(true);
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    /// The accent-colored "recording" hint a shortcut row shows in place
    /// of its keycaps while a capture is active.
    fn capture_hint() -> Div {
        div()
            .text_size(px(11.0))
            .text_color(rgb(accent_color()))
            .child(tr("Press the new shortcut, Esc cancels"))
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
        crate::transcription_models::language_for_model(model, preferred)
    }

    /// The model browser's initial language filter: the dictation language,
    /// or no filter when dictation detects the language automatically.
    fn catalog_filter_for_language(language: &str) -> Option<String> {
        crate::desktop_model_catalog::catalog_filter_for_language(language)
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

    fn copy_history_entry(&mut self, id: u64) {
        self.history_pane.copy(id);
    }

    fn delete_history_entry(&mut self, id: u64) {
        self.history_pane.delete(id);
    }

    fn clear_history(&mut self) {
        self.history_pane.clear();
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

    fn select_pane(&mut self, pane: DesktopPane, cx: &mut Context<Self>) {
        debug_assert!(DesktopPane::available(self.host.capabilities()).contains(&pane));
        self.close_popups();
        self.pane = pane;
        self.history_pane.disarm_clear();
        match pane {
            DesktopPane::VoiceAction if crate::windows_voice_action::opencode_installed() => {
                self.host.request_opencode_catalog();
            }
            DesktopPane::History => self.reload_history(),
            DesktopPane::Settings
            | DesktopPane::Modes
            | DesktopPane::Commands
            | DesktopPane::VoiceAction
            | DesktopPane::HudLab
            | DesktopPane::Meetings
            | DesktopPane::Activity => {}
        }
        cx.notify();
    }

    fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let items = render_navigation_items(
            self.pane,
            self.host.capabilities(),
            |label| tr(label).to_string(),
            Self::select_pane,
            cx,
        );
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
            .children(items)
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
            .or_else(|| snapshot.microphone.selected.clone())
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
        let microphone_label = snapshot
            .microphone
            .selected
            .clone()
            .unwrap_or_else(|| tr("Automatic").into());
        let listen_on_launch = self.host.settings.listen_on_launch;
        let launch_at_login = self.host.launch_at_login;
        let double_tap_lock = snapshot.double_tap_lock;
        let double_tap_only = snapshot.double_tap_only;
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
        let capturing_dictation = matches!(self.hotkey_capture, Some((HotkeyTarget::Dictation, _)));
        let capturing_paste_last =
            matches!(self.hotkey_capture, Some((HotkeyTarget::PasteLast, _)));
        let paste_last_control = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                // Clicking the binding records a new chord; the toggle
                // below stays its own click target.
                div()
                    .id("windows-paste-last-binding")
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_3()
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_hotkey_capture(HotkeyTarget::PasteLast, cx);
                    }))
                    .child(if capturing_paste_last {
                        Self::capture_hint().into_any_element()
                    } else {
                        div()
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
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .id("windows-paste-last-toggle")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.host.set_paste_last_enabled(!paste_last_enabled);
                        cx.notify();
                    }))
                    .child(toggle(if paste_last_enabled { 1.0 } else { 0.0 })),
            );
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
                                                                )
                                                                .when_some(
                                                                    snapshot
                                                                        .activity
                                                                        .last_failure
                                                                        .clone(),
                                                                    |column, failure| {
                                                                        column.child(
                                                                            div()
                                                                                .text_size(px(
                                                                                    12.0,
                                                                                ))
                                                                                .text_color(rgb(
                                                                                    CRITICAL,
                                                                                ))
                                                                                .truncate()
                                                                                .child(tr_fill(
                                                                                    "Last dictation failed: {}",
                                                                                    &failure,
                                                                                )),
                                                                        )
                                                                    },
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
                                                            let _ = this.host.dispatch(
                                                                DesktopAction::RefreshMicrophones,
                                                            );
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
                                                div()
                                                    .id("windows-dictation-shortcut")
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.toggle_hotkey_capture(
                                                            HotkeyTarget::Dictation,
                                                            cx,
                                                        );
                                                    }))
                                                    .child(if capturing_dictation {
                                                        Self::capture_hint().into_any_element()
                                                    } else {
                                                        hotkey_keycaps(
                                                            snapshot.dictation_shortcut.clone(),
                                                            1.0,
                                                        )
                                                        .into_any_element()
                                                    }),
                                            ),
                                        )
                                        .child(
                                            settings_row(
                                                "Double-tap to lock",
                                                "Tap the shortcut twice, then speak hands-free; press it again to finish",
                                                toggle(if double_tap_lock { 1.0 } else { 0.0 }),
                                            )
                                            .id("windows-double-tap-lock")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let _ = this.host.dispatch(
                                                    DesktopAction::SetDoubleTapLock(
                                                        !double_tap_lock,
                                                    ),
                                                );
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
                                                    let _ = this.host.dispatch(
                                                        DesktopAction::SetDoubleTapOnly(
                                                            !double_tap_only,
                                                        ),
                                                    );
                                                    cx.notify();
                                                })),
                                            )
                                        })
                                        .child(settings_row(
                                            "Paste last dictation",
                                            "Insert the most recent completed dictation at the current focus",
                                            paste_last_control,
                                        ))
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
                                        .child(settings_row(
                                            "Software updates",
                                            "Checks for new HEX releases in the background",
                                            match self.host.updater.state() {
                                                crate::windows_updater::UpdateCheck::Checking => {
                                                    header_button(tr("Checking"))
                                                        .id("windows-update-check")
                                                        .into_any_element()
                                                }
                                                crate::windows_updater::UpdateCheck::Available {
                                                    version,
                                                    url,
                                                } => header_button(tr_fill(
                                                    "Update to {}",
                                                    &version,
                                                ))
                                                .id("windows-update-check")
                                                .text_color(rgb(accent_color()))
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.open_url(&url);
                                                }))
                                                .into_any_element(),
                                                crate::windows_updater::UpdateCheck::ReadyToRestart {
                                                    version,
                                                    ..
                                                } => header_button(tr_fill(
                                                    "Restart into {}",
                                                    &version,
                                                ))
                                                .id("windows-update-check")
                                                .text_color(rgb(accent_color()))
                                                .when(self.restart_scheduled, |button| {
                                                    button.opacity(0.5)
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    // One watcher only; a second click while the
                                                    // quit drains would spawn a second instance.
                                                    if this.restart_scheduled {
                                                        return;
                                                    }
                                                    match this.host.dispatch(
                                                        DesktopAction::RestartIntoUpdate,
                                                    ) {
                                                        Ok(()) => {
                                                            this.restart_scheduled = true;
                                                            let _ = this.host.dispatch(
                                                                DesktopAction::StopListening,
                                                            );
                                                            cx.quit();
                                                        }
                                                        Err(_) => {
                                                            cx.notify();
                                                        }
                                                    }
                                                }))
                                                .into_any_element(),
                                                crate::windows_updater::UpdateCheck::Current
                                                | crate::windows_updater::UpdateCheck::Failed => {
                                                    header_button(tr("Check now"))
                                                        .id("windows-update-check")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            let _ = this.host.dispatch(
                                                                DesktopAction::CheckForUpdates,
                                                            );
                                                            cx.notify();
                                                        }))
                                                        .into_any_element()
                                                }
                                            },
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

    fn render_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let ModeTarget::Mode(index) = self.selected_mode
            && index >= self.mode_inputs.len()
        {
            self.selected_mode = ModeTarget::Global;
        }
        let add = header_button(tr("Add mode"))
            .id("windows-add-mode")
            .on_click(cx.listener(|this, _, window, cx| this.add_mode(window, cx)))
            .into_any_element();
        let mut entries = vec![ModeListEntry {
            target: ModeTarget::Global,
            title: tr("Global").into(),
            empty_subtitle: "Fallback for everything",
            activations: Vec::new(),
        }];
        entries.extend(self.mode_inputs.iter().enumerate().map(|(index, inputs)| {
            let name = inputs.name.read(cx).text().trim().to_string();
            let activations = self
                .host
                .settings
                .modes
                .get(index)
                .map(|mode| {
                    mode.applications
                        .iter()
                        .cloned()
                        .map(|name| ModeActivation::application(name, None))
                        .chain(mode.websites.iter().cloned().map(ModeActivation::website))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ModeListEntry {
                target: ModeTarget::Mode(index),
                title: if name.is_empty() {
                    tr("Untitled mode").into()
                } else {
                    name
                },
                empty_subtitle: "No activation rules",
                activations,
            }
        }));
        let mode_list = render_shared_mode_list(
            ModeListView {
                entries,
                selected: self.selected_mode,
                secondary_action: false,
            },
            cx,
        );
        let detail = self.render_windows_mode_detail(window, cx);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Modes", Some(add)))
            .child(
                pane_body().p_5().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(
                            div()
                                .id("modes-list")
                                .w(px(PANE_LIST_WIDTH))
                                .h_full()
                                .flex_none()
                                .child(mode_list),
                        )
                        .child(detail),
                ),
            )
            .into_any_element()
    }

    fn render_windows_mode_detail(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut detail = div()
            .id("mode-detail")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .child(match self.selected_mode {
                ModeTarget::Global => {
                    let processing =
                        self.render_windows_mode_processing(ModeTarget::Global, window, cx);
                    let basics = render_shared_mode_basics(
                        ModeBasicsView::Global {
                            title: "Global",
                            description: "Used unless a more specific mode matches.",
                        },
                        cx,
                    );
                    let replacements = render_shared_replacement_editor(
                        ReplacementEditorView {
                            target: ModeTarget::Global,
                            title: "Replacements",
                            empty_message:
                                "No replacements yet. Add one to correct recurring phrases.",
                            rows: &self.replacement_inputs,
                        },
                        cx,
                    );
                    div()
                        .w_full()
                        .max_w(px(700.0))
                        .flex()
                        .flex_col()
                        .gap(px(SECTION_GAP))
                        .pb_5()
                        .child(basics)
                        .child(div().h(px(4.0)))
                        .child(compact_section_label(tr("TEXT PROCESSING")))
                        .child(replacements)
                        .child(processing)
                        .into_any_element()
                }
                ModeTarget::Mode(mode_index) => {
                    let processing = self.render_windows_mode_processing(
                        ModeTarget::Mode(mode_index),
                        window,
                        cx,
                    );
                    let inputs = &self.mode_inputs[mode_index];
                    let name = inputs.name.clone();
                    let applications = inputs.applications.clone();
                    let websites = inputs.websites.clone();
                    let basics = render_shared_mode_basics(
                        ModeBasicsView::Custom {
                            target: ModeTarget::Mode(mode_index),
                            name,
                            applications: Box::new(ModeApplicationEditorView::Freeform {
                                title: "Applications",
                                description:
                                    "Applies when the focused application contains any of these names",
                                input: applications,
                            }),
                            websites,
                            websites_title: "Web pages",
                            websites_description:
                                "Or when the browser is on one of these sites; sites win over applications",
                            remove_mode: true,
                        },
                        cx,
                    );
                    let corrections = render_shared_replacement_editor(
                        ReplacementEditorView {
                            target: ModeTarget::Mode(mode_index),
                            title: "Corrections",
                            empty_message: "No corrections in this mode.",
                            rows: &inputs.corrections,
                        },
                        cx,
                    );
                    div()
                        .w_full()
                        .max_w(px(700.0))
                        .flex()
                        .flex_col()
                        .gap(px(SECTION_GAP))
                        .pb_5()
                        .child(basics)
                        .child(div().h(px(4.0)))
                        .child(compact_section_label(tr("TEXT PROCESSING")))
                        .child(corrections)
                        .child(processing)
                        .into_any_element()
                }
            });
        if let Some(error) = self.host.settings_error.clone() {
            detail = detail.child(error_message(
                "Text replacements could not be saved.",
                error,
            ));
        }
        detail.into_any_element()
    }

    fn mode_post_processing(
        &self,
        target: ModeTarget,
    ) -> Option<&crate::dictation_processing::PostProcessingSettings> {
        match target {
            ModeTarget::Global => Some(&self.host.settings.dictation_post_processing),
            ModeTarget::Mode(index) => self
                .host
                .settings
                .modes
                .get(index)
                .map(|mode| &mode.post_processing),
        }
    }

    fn opencode_model_key(&self, target: OpenCodeModelTarget) -> Option<String> {
        match target {
            OpenCodeModelTarget::VoiceAction => self
                .host
                .settings
                .voice_action_model
                .as_ref()
                .map(|model| format!("{}/{}", model.provider, model.id)),
            OpenCodeModelTarget::Mode(target) => self.mode_post_processing(target)?.model.clone(),
        }
    }

    fn opencode_model_label(&self, target: OpenCodeModelTarget) -> String {
        let current_key = self.opencode_model_key(target);
        if let (Some(key), Some(catalog)) = (&current_key, &self.host.opencode_catalog)
            && let Some(choice) = catalog.models.iter().find(|choice| &choice.key == key)
        {
            return choice.name.clone();
        }
        if let Some(key) = current_key {
            return key
                .split_once('/')
                .map_or(key.clone(), |(_, model)| model.to_string());
        }
        if self.host.opencode_catalog_rx.is_some() {
            return tr("Loading models").to_string();
        }
        match target {
            OpenCodeModelTarget::VoiceAction => tr("Choose a model").to_string(),
            OpenCodeModelTarget::Mode(_) => self
                .host
                .opencode_catalog
                .as_ref()
                .and_then(|catalog| catalog.default_name.clone())
                .map_or_else(
                    || "OpenCode default".into(),
                    |name| format!("{name} — Default"),
                ),
        }
    }

    fn render_opencode_model_control(
        &mut self,
        target: OpenCodeModelTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target_id = target.id_fragment();
        let label = self.opencode_model_label(target);
        div()
            .relative()
            .child(
                disclosure_button(label)
                    .id(SharedString::from(format!(
                        "windows-opencode-model-{target_id}"
                    )))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let was_open = this.opencode_model_dropdown == Some(target);
                        this.close_popups();
                        if !was_open {
                            this.opencode_model_dropdown = Some(target);
                            if this.host.opencode_catalog.is_none() {
                                this.host.request_opencode_catalog();
                            }
                        }
                        cx.notify();
                    })),
            )
            .child(
                canvas(
                    {
                        let entity = cx.entity();
                        move |bounds, _, cx| {
                            entity.update(cx, |this, _| {
                                this.opencode_model_dropdown_bounds = Some(bounds);
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .w_full()
                .h(px(0.0)),
            )
            .into_any_element()
    }

    fn set_opencode_model(
        &mut self,
        target: OpenCodeModelTarget,
        model: Option<crate::opencode::Model>,
    ) {
        match target {
            OpenCodeModelTarget::VoiceAction => self.host.set_voice_action_model(model),
            OpenCodeModelTarget::Mode(target) => {
                let Some(mut processing) = self.mode_post_processing(target).cloned() else {
                    return;
                };
                processing.model = model
                    .as_ref()
                    .map(|model| format!("{}/{}", model.provider, model.id));
                processing.variant = model.and_then(|model| model.variant);
                let _ = self.host.set_mode_post_processing(target, processing);
            }
        }
    }

    fn render_windows_mode_processing(
        &mut self,
        target: ModeTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(settings) = self.mode_post_processing(target).cloned() else {
            return div().into_any_element();
        };
        let installed = crate::opencode::opencode_installed();
        let compact = window.viewport_size().width < px(980.0);
        let settings_view = if settings.enabled {
            let model_control =
                self.render_opencode_model_control(OpenCodeModelTarget::Mode(target), cx);
            let prompt = match target {
                ModeTarget::Global => self.global_processing_prompt.clone(),
                ModeTarget::Mode(index) => self.mode_inputs[index].processing_prompt.clone(),
            };
            Some(
                div()
                    .pt_4()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .when(compact, |row| row.flex_col().items_start().gap_3())
                            .when(!compact, |row| row.items_center().justify_between().gap_4())
                            .child(div().flex_1().min_w(px(0.0)).child(settings_copy(
                                "Model",
                                "Rewrites each completed dictation through OpenCode",
                            )))
                            .child(
                                div()
                                    .when(!compact, |control| control.w(px(320.0)).flex_none())
                                    .when(compact, |control| control.w_full())
                                    .child(model_control),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(settings_copy(
                                "Instructions",
                                "Tell OpenCode exactly how to transform the dictated text",
                            ))
                            .child(prompt),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let unavailable = if !installed {
            Some(ModeProcessingUnavailableView {
                title: "OpenCode is required",
                description: "Install and configure OpenCode to rewrite dictation.",
                error: self.host.opencode_catalog_error.clone(),
                retry_label: Some("Check again"),
                setup_label: Some("Open setup"),
            })
        } else {
            self.host
                .opencode_catalog_error
                .clone()
                .map(|error| ModeProcessingUnavailableView {
                    title: "Models could not be loaded",
                    description: "Retry the OpenCode model catalog.",
                    error: Some(error),
                    retry_label: Some("Retry"),
                    setup_label: None,
                })
        };
        render_shared_mode_processing(
            ModeProcessingView {
                target,
                enabled: settings.enabled,
                toggle_position: if settings.enabled { 1.0 } else { 0.0 },
                can_toggle: settings.enabled || installed,
                settings: settings_view,
                unavailable,
            },
            cx,
        )
    }

    /// The Voice Action pane, mirroring macOS: an explainer, the capture
    /// shortcut, and the OpenCode processing status.
    fn render_voice_action(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.host.settings.voice_action_hotkey.is_some();
        let opencode_installed = crate::windows_voice_action::opencode_installed();
        let capturing_voice_action =
            matches!(self.hotkey_capture, Some((HotkeyTarget::VoiceAction, _)));
        let shortcut_control = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                // Clicking the binding records a new chord; the row around
                // it stays inert.
                div()
                    .id("windows-voice-action-binding")
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_3()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_hotkey_capture(HotkeyTarget::VoiceAction, cx);
                    }))
                    .child(if capturing_voice_action {
                        Self::capture_hint().into_any_element()
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when_some(
                                self.host
                                    .settings
                                    .voice_action_hotkey
                                    .as_ref()
                                    .map(crate::windows_settings::WindowsHotkey::keycaps),
                                |control, keycaps| control.child(hotkey_keycaps(keycaps, 1.0)),
                            )
                            .when(!enabled, |control| {
                                control.child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(rgb(FAINT))
                                        .child(tr("Disabled")),
                                )
                            })
                            .into_any_element()
                    }),
            )
            .child(
                // The toggle is its own click target: it must flip the
                // feature without opening the rebind picker underneath.
                div()
                    .id("windows-voice-action-toggle")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.host.set_voice_action_enabled(!enabled);
                        cx.notify();
                    }))
                    .child(toggle(if enabled { 1.0 } else { 0.0 })),
            );
        let model_control =
            self.render_opencode_model_control(OpenCodeModelTarget::VoiceAction, cx);
        let opencode_status = div()
            .text_size(px(12.0))
            .text_color(if opencode_installed {
                rgb(crate::windows_ui::SUCCESS)
            } else {
                rgb(crate::windows_ui::CRITICAL)
            })
            .child(if opencode_installed {
                tr("Installed")
            } else {
                tr("OpenCode not found")
            });

        let mut processing = Vec::with_capacity(2);
        if opencode_installed {
            processing.push(VoiceActionSettingRow::translated(
                "Model",
                "Fulfils each voice action; served by OpenCode",
                model_control,
            ));
        }
        processing.push(VoiceActionSettingRow::translated(
            "OpenCode",
            "Voice actions run through your local OpenCode install",
            opencode_status,
        ));
        let view = VoiceActionView::Ready(Box::new(VoiceActionReadyView {
            shortcut: VoiceActionSettingRow::translated(
                "Shortcut",
                "Hold to speak; selected text is included automatically",
                shortcut_control,
            ),
            processing,
            error: self
                .host
                .settings_error
                .clone()
                .map(|error| VoiceActionError {
                    title: "The shortcut could not be saved.",
                    detail: error,
                }),
        }));
        render_shared_voice_action_pane(view, window, cx)
    }

    fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = self.history_pane.view(
            self.host.settings.history_retention,
            self.history_search_input.clone(),
            cx,
        );
        render_shared_history_pane(view, cx)
    }
    fn render_microphone_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(bounds) = self.microphone_dropdown_bounds else {
            return div().into_any_element();
        };
        let DesktopMicrophoneSnapshot {
            devices,
            error,
            selected: current,
        } = self.host.snapshot().microphone;
        let mut choices = vec![(tr("Automatic").to_string(), None)];
        choices.extend(
            devices
                .iter()
                .cloned()
                .map(|microphone| (microphone.clone(), Some(microphone))),
        );
        let panel_rows = choices.len() + usize::from(error.is_some()) * 2;
        let items = choices
            .into_iter()
            .enumerate()
            .map(|(index, (label, selection))| {
                let selected = selection == current;
                dropdown_item(("windows-microphone-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this
                            .host
                            .dispatch(DesktopAction::SetMicrophone(selection.clone()))
                            .is_ok()
                        {
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
                    .when_some(error, |list, error| {
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

    fn render_opencode_model_dropdown(
        &mut self,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(target) = self.opencode_model_dropdown else {
            return div().into_any_element();
        };
        let Some(bounds) = self.opencode_model_dropdown_bounds else {
            return div().into_any_element();
        };
        let target_id = target.id_fragment();
        let backdrop = dropdown_backdrop("windows-opencode-model-backdrop").on_click(cx.listener(
            |this, _, _, cx| {
                this.opencode_model_dropdown = None;
                cx.notify();
            },
        ));
        let width = px(320.0);
        let Some(catalog) = self.host.opencode_catalog.clone() else {
            let message = self.host.opencode_catalog_error.clone().map_or_else(
                || tr("Loading models").to_string(),
                |error| tr_fill("Models could not be loaded: {}", &error),
            );
            return backdrop
                .child(
                    dropdown_panel_with_width(bounds, viewport_height, 1, width)
                        .id(SharedString::from(format!(
                            "windows-opencode-model-dropdown-{target_id}"
                        )))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px_3()
                                .py_2()
                                .text_size(px(12.0))
                                .text_color(rgb(MUTED))
                                .child(message),
                        ),
                )
                .into_any_element();
        };
        let current_key = self.opencode_model_key(target);
        let includes_default = matches!(target, OpenCodeModelTarget::Mode(_));
        let panel_rows = (catalog.models.len() + usize::from(includes_default)).max(1);
        let mut items = Vec::with_capacity(panel_rows);
        if includes_default {
            let label = catalog.default_name.clone().map_or_else(
                || "OpenCode default".into(),
                |name| format!("{name} — OpenCode default"),
            );
            items.push(
                dropdown_item(
                    SharedString::from(format!("windows-opencode-model-default-{target_id}")),
                    label,
                    current_key.is_none(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.set_opencode_model(target, None);
                    this.opencode_model_dropdown = None;
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        items.extend(catalog.models.iter().enumerate().map(|(index, choice)| {
            let selected = current_key.as_deref() == Some(choice.key.as_str());
            let label = if catalog.default_key.as_ref() == Some(&choice.key) {
                format!("{} — {}", choice.name, tr("Default"))
            } else {
                choice.name.clone()
            };
            let model = choice.model();
            dropdown_item(
                SharedString::from(format!("windows-opencode-model-option-{target_id}-{index}")),
                label,
                selected,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_opencode_model(target, Some(model.clone()));
                this.opencode_model_dropdown = None;
                cx.notify();
            }))
            .into_any_element()
        }));
        backdrop
            .child(
                dropdown_panel_with_width(bounds, viewport_height, panel_rows, width)
                    .id(SharedString::from(format!(
                        "windows-opencode-model-dropdown-{target_id}"
                    )))
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    /// The model browser dialog; the implementation is shared with the
    /// Linux shell in `desktop::model_catalog`.
    fn render_model_manager(
        &mut self,
        transcription: &DesktopTranscriptionSnapshot,
        viewport_width: Pixels,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::desktop_model_catalog::render_model_catalog(
            self,
            transcription,
            viewport_width,
            viewport_height,
            cx,
        )
    }
}

impl crate::desktop_onboarding::OnboardingDelegate for WindowsApp {
    fn onboarding_selection(&self) -> crate::transcription_models::TranscriptionSelection {
        self.onboarding_selection.clone()
    }

    fn set_onboarding_selection(
        &mut self,
        selection: crate::transcription_models::TranscriptionSelection,
    ) {
        self.onboarding_selection = selection;
    }

    fn onboarding_language_dropdown_open(&self) -> bool {
        self.onboarding_language_dropdown_open
    }

    fn set_onboarding_language_dropdown_open(&mut self, open: bool) {
        self.onboarding_language_dropdown_open = open;
    }

    fn onboarding_language_dropdown_bounds(&self) -> Option<Bounds<Pixels>> {
        self.onboarding_language_dropdown_bounds
    }

    fn set_onboarding_language_dropdown_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.onboarding_language_dropdown_bounds = Some(bounds);
    }

    fn begin_onboarding_download(&mut self, _cx: &mut Context<Self>) {
        let selection = self.onboarding_selection.clone();
        let _ = self
            .host
            .choose_transcription(selection.model, selection.language);
    }

    fn cancel_onboarding_download(&mut self) {
        self.host.cancel_transcription_preparation();
    }

    fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        self.onboarding = false;
        // The dialog's pending selection becomes the persisted one when it
        // is genuinely usable; a stale prepared transcriber for another
        // selection must not serve it.
        let pending = self.onboarding_selection.clone();
        let definition = crate::transcription_models::definition(pending.model);
        if pending != self.host.settings.transcription
            && crate::transcription_models::validate(&pending).is_ok()
            && crate::transcription_models::is_installed(definition, &pending.language)
            && crate::transcription_models::is_verified(definition)
        {
            self.host.settings.transcription = pending;
            self.host.prepared_transcriber = None;
        }
        if let Err(error) = self.host.settings.save() {
            self.host.settings_error = Some(format!("Could not save Windows settings: {error:#}"));
        }
        // The launch-time listener start was deferred while the dialog
        // held the screen.
        if self.host.settings.listen_on_launch {
            self.host.start();
        }
        cx.notify();
    }

    fn onboarding_top_inset() -> Pixels {
        px(crate::windows_ui::CAPTION_HEIGHT)
    }
}

impl crate::desktop_model_catalog::ModelCatalogDelegate for WindowsApp {
    fn catalog_language_filter(&self) -> Option<String> {
        self.model_catalog_language_filter.clone()
    }

    fn set_catalog_language_filter(&mut self, filter: Option<String>) {
        self.model_catalog_language_filter = filter;
    }

    fn catalog_filter_dropdown_open(&self) -> bool {
        self.model_catalog_language_dropdown_open
    }

    fn set_catalog_filter_dropdown_open(&mut self, open: bool) {
        self.model_catalog_language_dropdown_open = open;
    }

    fn catalog_filter_dropdown_bounds(&self) -> Option<Bounds<Pixels>> {
        self.model_catalog_language_dropdown_bounds
    }

    fn set_catalog_filter_dropdown_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.model_catalog_language_dropdown_bounds = Some(bounds);
    }

    fn report_transcription_error(&mut self, error: String) {
        self.host.transcription_error = Some(error);
    }

    fn dialog_top_inset() -> Pixels {
        px(crate::windows_ui::CAPTION_HEIGHT)
    }

    fn uninstall_control() -> AnyElement {
        crate::windows_ui::fluent_icon("\u{E74D}", 12.0, TEXT)
    }

    fn close_control() -> AnyElement {
        crate::windows_ui::fluent_icon("\u{E8BB}", 12.0, TEXT)
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

impl crate::desktop_hud_lab::HudLabDelegate for WindowsApp {
    fn hud_lab(&self) -> &crate::desktop_hud_lab::HudLabState {
        &self.hud_lab
    }

    fn hud_lab_mut(&mut self) -> &mut crate::desktop_hud_lab::HudLabState {
        &mut self.hud_lab
    }

    fn configure_platform_hud(&mut self, tuning: crate::desktop_hud_lab::HudTuning) {
        self.host
            .indicator
            .send(crate::windows_indicator::WindowsIndicatorEvent::Configure(
                tuning,
            ));
    }
}

impl ModeBasicsDelegate for WindowsApp {
    fn handle_mode_basics_action(
        &mut self,
        action: ModeBasicsAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ModeBasicsAction::RemoveMode(ModeTarget::Mode(index)) = action {
            self.remove_mode(index, cx);
        }
    }
}

impl ModeListDelegate for WindowsApp {
    fn handle_mode_list_action(
        &mut self,
        action: ModeListAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match action {
            ModeListAction::Select(target) | ModeListAction::OpenContextMenu { target, .. } => {
                target
            }
        };
        if let ModeTarget::Mode(index) = target
            && index >= self.mode_inputs.len()
        {
            return;
        }
        self.selected_mode = target;
        self.close_popups();
        window.blur();
        cx.notify();
    }
}

impl ReplacementEditorDelegate for WindowsApp {
    fn handle_replacement_editor_action(
        &mut self,
        action: ReplacementEditorAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ReplacementEditorAction::Add(ModeTarget::Global) => {
                let input = ReplacementEditorInput::new(
                    &crate::text_replacements::TextReplacement::default(),
                    Self::sync_text_replacements,
                    cx,
                );
                let focus = input.matched_phrase_focus(cx);
                self.replacement_inputs.push(input);
                self.sync_text_replacements(cx);
                focus.focus(window);
            }
            ReplacementEditorAction::Add(ModeTarget::Mode(mode_index)) => {
                if mode_index >= self.mode_inputs.len() {
                    return;
                }
                let input = ReplacementEditorInput::new(
                    &crate::text_replacements::TextReplacement::default(),
                    Self::sync_modes,
                    cx,
                );
                let focus = input.matched_phrase_focus(cx);
                self.mode_inputs[mode_index].corrections.push(input);
                self.sync_modes(cx);
                focus.focus(window);
            }
            ReplacementEditorAction::Remove {
                target: ModeTarget::Global,
                index,
            } => {
                if index < self.replacement_inputs.len() {
                    self.replacement_inputs.remove(index);
                    self.sync_text_replacements(cx);
                }
            }
            ReplacementEditorAction::Remove {
                target: ModeTarget::Mode(mode_index),
                index,
            } => {
                if let Some(mode) = self.mode_inputs.get_mut(mode_index)
                    && index < mode.corrections.len()
                {
                    mode.corrections.remove(index);
                    self.sync_modes(cx);
                }
            }
        }
    }
}

impl HistoryPaneDelegate for WindowsApp {
    fn handle_history_action(&mut self, action: HistoryPaneAction, cx: &mut Context<Self>) {
        match action {
            HistoryPaneAction::SetRetention(retention) => self.set_history_retention(retention),
            HistoryPaneAction::Select(id) => self.history_pane.select(id),
            HistoryPaneAction::Copy(id) => self.copy_history_entry(id),
            HistoryPaneAction::Delete(id) => self.delete_history_entry(id),
            HistoryPaneAction::Clear => self.clear_history(),
        }
        cx.notify();
    }
}

impl VoiceActionPaneDelegate for WindowsApp {
    fn handle_voice_action_pane_action(
        &mut self,
        action: VoiceActionPaneAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            VoiceActionPaneAction::RetryOpenCode => self.host.request_opencode_catalog(),
            VoiceActionPaneAction::OpenOpenCodeSetup => {
                if let Err(error) = crate::commands_engine::execute(
                    crate::commands_engine::Action::OpenUrl(OPENCODE_SETUP_URL.into()),
                ) {
                    tracing::error!(%error, "could not open the OpenCode beta documentation");
                }
            }
        }
        cx.notify();
    }
}

impl ModeProcessingDelegate for WindowsApp {
    fn handle_mode_processing_action(
        &mut self,
        action: ModeProcessingAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ModeProcessingAction::SetEnabled { target, enabled } => {
                let Some(mut processing) = self.mode_post_processing(target).cloned() else {
                    return;
                };
                processing.enabled = enabled;
                if self
                    .host
                    .set_mode_post_processing(target, processing)
                    .is_ok()
                    && enabled
                    && self.host.opencode_catalog.is_none()
                {
                    self.host.request_opencode_catalog();
                }
            }
            ModeProcessingAction::RetryOpenCode => self.host.request_opencode_catalog(),
            ModeProcessingAction::OpenOpenCodeSetup => {
                if let Err(error) = crate::commands_engine::execute(
                    crate::commands_engine::Action::OpenUrl(OPENCODE_SETUP_URL.into()),
                ) {
                    tracing::error!(%error, "could not open the OpenCode beta documentation");
                }
            }
        }
        cx.notify();
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
            DesktopPane::Settings => self.render_settings(&snapshot, cx),
            DesktopPane::Activity => crate::desktop_activity_pane::render_activity_pane(&snapshot),
            DesktopPane::Modes => self.render_modes(window, cx),
            DesktopPane::VoiceAction => self.render_voice_action(window, cx),
            DesktopPane::History => self.render_history(cx),
            DesktopPane::HudLab => crate::desktop_hud_lab::render_hud_lab_pane(self, window, cx),
            DesktopPane::Commands | DesktopPane::Meetings => {
                unreachable!("capability-filtered Windows pane")
            }
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
        let model_catalog_language_dropdown =
            self.model_catalog_language_dropdown_open.then(|| {
                crate::desktop_model_catalog::render_model_catalog_filter_dropdown(
                    self,
                    viewport.height,
                    cx,
                )
            });
        let opencode_model_dropdown = self
            .opencode_model_dropdown
            .map(|_| self.render_opencode_model_dropdown(viewport.height, cx));
        let onboarding = self.onboarding.then(|| {
            crate::desktop_onboarding::render_onboarding(
                self,
                &snapshot.transcription,
                snapshot.dictation_shortcut.clone(),
                viewport.width,
                viewport.height,
                cx,
            )
        });
        let onboarding_language_dropdown =
            (self.onboarding && self.onboarding_language_dropdown_open).then(|| {
                crate::desktop_onboarding::render_onboarding_language_dropdown(
                    self,
                    viewport.height,
                    cx,
                )
            });
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
            .children(opencode_model_dropdown)
            .children(microphone_dropdown)
            .children(transcription_dropdown)
            .children(dictation_language_dropdown)
            .children(ui_language_dropdown)
            .children(onboarding)
            .children(onboarding_language_dropdown)
    }
}

fn windows_model_catalog() -> Vec<&'static crate::transcription_models::ModelDefinition> {
    crate::transcription_models::available_models()
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

    #[test]
    fn portable_shortcuts_preserve_windows_modifiers_and_normalize_space() {
        let hotkey = windows_hotkey(DesktopShortcut {
            alt: true,
            control: true,
            function: false,
            key: " ".into(),
            platform: true,
            shift: false,
        })
        .unwrap();

        assert!(hotkey.alt);
        assert!(hotkey.control);
        assert!(hotkey.windows);
        assert!(!hotkey.shift);
        assert_eq!(hotkey.key.as_deref(), Some("space"));
    }

    #[test]
    fn portable_shortcuts_reject_windows_fn_and_unsafe_bare_letters() {
        assert!(
            windows_hotkey(DesktopShortcut {
                alt: false,
                control: false,
                function: true,
                key: "f12".into(),
                platform: false,
                shift: false,
            })
            .is_err()
        );
        assert!(
            windows_hotkey(DesktopShortcut {
                alt: false,
                control: false,
                function: false,
                key: "k".into(),
                platform: false,
                shift: false,
            })
            .is_err()
        );
    }
}
