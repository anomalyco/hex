use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Keystroke, Pixels, SharedString, Subscription,
    Timer, TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions, canvas, div,
    prelude::*, px, rgb, size,
};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

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
use crate::desktop_i18n::{tr, tr_fill};
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
    ModeProcessingView, ModeVariantPickerView,
    render_mode_processing as render_shared_mode_processing, render_mode_variant_picker,
};
use crate::desktop_mode_transformations::{
    ModeTransformationsAction, ModeTransformationsDelegate, ModeTransformationsView,
    ModeTransformationsWorkspaceAction, TransformationCatalogEntry, TransformationWorkspaceView,
    render_mode_transformations as render_shared_mode_transformations, reorder_transformation,
};
use crate::desktop_replacement_editor::{
    ReplacementEditorAction, ReplacementEditorDelegate, ReplacementEditorInput,
    ReplacementEditorView, render_replacement_editor as render_shared_replacement_editor,
};
use crate::desktop_shell::{DesktopPane, render_navigation_items};
use crate::desktop_transcription_picker::TranscriptionPickerDelegate;
use crate::desktop_ui::{
    FAINT, LINE, MUTED, PANE_LIST_WIDTH, SECTION_GAP, SIDEBAR_WIDTH, SUCCESS, SURFACE, TEXT,
    TEXT_SOFT, compact_button, compact_section_label, disclosure_button, dropdown_backdrop,
    dropdown_item, dropdown_panel_with_width, error_message, header_button, hotkey_keycaps,
    pane_body, pane_content, pane_header, pane_header_with_action, settings_copy, settings_panel,
    settings_row, settings_section_label, sidebar_frame, toggle, window_frame,
};
use crate::desktop_voice_action_pane::{
    OPENCODE_SETUP_URL, VoiceActionError, VoiceActionPaneAction, VoiceActionPaneDelegate,
    VoiceActionReadyView, VoiceActionSettingRow, VoiceActionUnavailableView, VoiceActionView,
    render_voice_action_pane as render_shared_voice_action_pane,
};
use crate::events::EventReader;
use crate::history::{History, HistoryRetention};
use crate::linux_updater::InstalledUpdate;
use crate::text_input::{Changed as TextChanged, TextInput};

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const OPENCODE_INSTALL_RETRY_INTERVAL: Duration = Duration::from_secs(60);

type ListenerResult = std::result::Result<(), String>;
type PreparationResult = std::result::Result<PreparedTranscription, String>;
type PersonalCommandsSetupResult = std::result::Result<PathBuf, String>;
type OpenCodeCatalogProbe = (bool, Option<Result<crate::opencode::ModelCatalog>>);

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
    history: Option<History>,
    history_error: Option<String>,
    replacements: crate::linux_dictation::SharedReplacements,
    modes: crate::linux_dictation::SharedModes,
    transformations: crate::linux_dictation::SharedTransformations,
    voice_action: crate::linux_dictation::SharedVoiceAction,
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_status: crate::personal_commands::StatusSnapshot,
    personal_commands_setup: Option<Receiver<PersonalCommandsSetupResult>>,
    personal_commands_setup_error: Option<String>,
    opencode_catalog: Option<crate::opencode::ModelCatalog>,
    opencode_catalog_rx: Option<Receiver<OpenCodeCatalogProbe>>,
    opencode_catalog_error: Option<String>,
    opencode_installed: bool,
    opencode_probe_complete: bool,
    opencode_retry_at: Option<Instant>,
    update: UpdateState,
}

struct LinuxApp {
    host: LinuxDesktopHost,
    pane: DesktopPane,
    hud_lab: crate::desktop_hud_lab::HudLabState,
    history_pane: HistoryPaneState,
    history_search_input: gpui::Entity<TextInput>,
    _history_search_subscription: Subscription,
    replacement_inputs: Vec<ReplacementEditorInput>,
    mode_inputs: Vec<LinuxModeInputs>,
    global_processing_prompt: gpui::Entity<TextInput>,
    _global_processing_prompt_subscription: Subscription,
    global_processing_deadline: gpui::Entity<TextInput>,
    _global_processing_deadline_subscription: Subscription,
    selected_mode: ModeTarget,
    opencode_model_dropdown: Option<OpenCodeModelTarget>,
    opencode_model_dropdown_bounds: Option<Bounds<Pixels>>,
    opencode_variant_picker_open: Option<ModeTarget>,
    transformation_picker_open: bool,
    capturing_hotkey: Option<HotkeyTarget>,
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
    ui_language_dropdown_open: bool,
    ui_language_dropdown_bounds: Option<Bounds<Pixels>>,
    onboarding: bool,
    onboarding_selection: crate::transcription_models::TranscriptionSelection,
    onboarding_language_dropdown_open: bool,
    onboarding_language_dropdown_bounds: Option<Bounds<Pixels>>,
}

struct LinuxModeInputs {
    name: gpui::Entity<TextInput>,
    applications: gpui::Entity<TextInput>,
    processing_prompt: gpui::Entity<TextInput>,
    processing_deadline: gpui::Entity<TextInput>,
    corrections: Vec<ReplacementEditorInput>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HotkeyTarget {
    Dictation,
    VoiceAction,
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
    // First-run detection must precede anything that saves settings.
    let onboarding = !crate::linux_settings::exists()
        || std::env::var_os("HEX_ONBOARDING").is_some_and(|value| value == "1");
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
        cx.bind_keys(crate::text_input::key_bindings());
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
                    cx.new(|cx| {
                        let host = LinuxDesktopHost::new(
                            event_path.clone(),
                            listener_stop.clone(),
                            settings.clone(),
                            settings_error.clone(),
                            update,
                        );
                        let history_pane =
                            HistoryPaneState::new(host.history.clone(), host.history_error.clone());
                        let history_search_input = cx.new(|cx| TextInput::new(cx, "Search", ""));
                        let history_search_subscription = cx.subscribe(
                            &history_search_input,
                            |this: &mut LinuxApp, input, _: &TextChanged, cx| {
                                this.history_pane
                                    .set_query(input.read(cx).text().to_string());
                                cx.notify();
                            },
                        );
                        let replacement_inputs = settings
                            .text_replacements
                            .iter()
                            .map(|replacement| {
                                ReplacementEditorInput::new(
                                    replacement,
                                    LinuxApp::sync_text_replacements,
                                    cx,
                                )
                            })
                            .collect();
                        let mode_inputs = settings
                            .modes
                            .iter()
                            .map(|mode| LinuxApp::mode_inputs(mode, cx))
                            .collect();
                        let global_processing_prompt = cx.new(|cx| {
                            TextInput::multiline_with_height(
                                cx,
                                "Tell OpenCode exactly how to transform the dictated text.",
                                &settings.dictation_post_processing.prompt,
                                px(92.0),
                            )
                        });
                        let global_processing_prompt_subscription = cx.subscribe(
                            &global_processing_prompt,
                            |this: &mut LinuxApp, input, _: &TextChanged, cx| {
                                this.sync_global_processing_prompt(
                                    input.read(cx).text().to_string(),
                                    cx,
                                );
                            },
                        );
                        let global_processing_deadline = cx.new(|cx| {
                            TextInput::new(
                                cx,
                                "Seconds",
                                settings
                                    .dictation_post_processing
                                    .deadline_seconds
                                    .to_string(),
                            )
                        });
                        let global_processing_deadline_subscription = cx.subscribe(
                            &global_processing_deadline,
                            |this: &mut LinuxApp, input, _: &TextChanged, cx| {
                                this.sync_global_processing_deadline(
                                    input.read(cx).text().to_string(),
                                    cx,
                                );
                            },
                        );
                        LinuxApp {
                            host,
                            pane: DesktopPane::Settings,
                            hud_lab: crate::desktop_hud_lab::HudLabState::new(),
                            history_pane,
                            history_search_input,
                            _history_search_subscription: history_search_subscription,
                            replacement_inputs,
                            mode_inputs,
                            global_processing_prompt,
                            _global_processing_prompt_subscription:
                                global_processing_prompt_subscription,
                            global_processing_deadline,
                            _global_processing_deadline_subscription:
                                global_processing_deadline_subscription,
                            selected_mode: ModeTarget::Global,
                            opencode_model_dropdown: None,
                            opencode_model_dropdown_bounds: None,
                            opencode_variant_picker_open: None,
                            transformation_picker_open: false,
                            onboarding,
                            // A fresh run offers the locale's recommendation,
                            // like the Windows first-run default.
                            onboarding_selection: if onboarding {
                                crate::transcription_models::recommended_selection(
                                    &crate::desktop_i18n::system_language(),
                                )
                            } else {
                                settings.transcription.clone()
                            },
                            onboarding_language_dropdown_open: false,
                            onboarding_language_dropdown_bounds: None,
                            capturing_hotkey: None,
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
                            ui_language_dropdown_open: false,
                            ui_language_dropdown_bounds: None,
                        }
                    })
                },
            )
            .expect("could not open the HEX X11 window");
        let app = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let x11_window = find_hex_window().ok();
        // The microphone stays cold behind the first-run dialog; finishing
        // onboarding starts the listener instead.
        if !onboarding {
            app.update(cx, |app, cx| {
                let _ = app.host.dispatch(DesktopAction::StartListening);
                cx.notify();
            });
        }
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
                        if this.pane == DesktopPane::History {
                            this.history_pane.reload();
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
        crate::desktop_i18n::apply(settings.ui_language.as_deref());
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
        let (history, history_error) = match History::open_default(settings.history_retention) {
            Ok(history) => (Some(history), None),
            Err(error) => (
                None,
                Some(format!("Could not open retained history: {error:#}")),
            ),
        };
        let replacements = Arc::new(RwLock::new(crate::text_replacements::ReplacementSet::new(
            &settings.text_replacements,
        )));
        let modes = Arc::new(RwLock::new(
            crate::linux_dictation::LinuxModeRuntime::from_settings(&settings),
        ));
        let transformations = Arc::new(crate::personal_commands::TransformationClient::default());
        let voice_action = Arc::new(RwLock::new(settings.voice_action.clone()));
        let personal_commands_configured = crate::personal_commands::workspace_configured();
        let personal_commands = (settings.transformations_enabled()
            || personal_commands_configured)
            .then(|| {
                crate::personal_commands::PersonalCommands::start(
                    crate::commands_engine::CommandConfig::new(),
                    transformations.clone(),
                )
            })
            .flatten();
        let mut personal_commands_status = if personal_commands_configured {
            crate::personal_commands::load_status().unwrap_or_else(|error| {
                crate::personal_commands::StatusSnapshot {
                    host_state: crate::personal_commands::HostState::Unavailable,
                    last_reload_error: Some(format!("{error:#}")),
                    ..crate::personal_commands::StatusSnapshot::default()
                }
            })
        } else {
            crate::personal_commands::StatusSnapshot::default()
        };
        crate::personal_commands::include_builtin_transformations(&mut personal_commands_status);
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
            history,
            history_error,
            replacements,
            modes,
            transformations,
            voice_action,
            personal_commands,
            personal_commands_status,
            personal_commands_setup: None,
            personal_commands_setup_error: None,
            opencode_catalog: None,
            opencode_catalog_rx: None,
            opencode_catalog_error: None,
            opencode_installed: false,
            opencode_probe_complete: false,
            opencode_retry_at: None,
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
        let history = self.history.clone();
        let replacements = self.replacements.clone();
        let modes = self.modes.clone();
        let transformations = self.transformations.clone();
        let voice_action = self.voice_action.clone();
        let worker = std::thread::spawn(move || {
            let result = crate::instance::acquire("listener").and_then(|_instance| {
                let output = crate::linux_dictation::LinuxOutputRuntime::new(
                    history,
                    replacements,
                    modes,
                    transformations,
                    voice_action,
                );
                if let Some(transcriber) = prepared_transcriber {
                    crate::linux_dictation::run_with_transcriber(
                        &event_path,
                        None,
                        &stop,
                        transcriber,
                        output,
                    )
                } else {
                    crate::linux_dictation::run_with_history(&event_path, None, &stop, output)
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
        if self
            .opencode_retry_at
            .is_some_and(|retry_at| Instant::now() >= retry_at)
        {
            self.opencode_retry_at = None;
            self.request_opencode_catalog();
        }
        self.poll_opencode_catalog();
        if crate::personal_commands::workspace_configured()
            && let Ok(mut status) = crate::personal_commands::load_status()
        {
            crate::personal_commands::include_builtin_transformations(&mut status);
            if status != self.personal_commands_status {
                self.personal_commands_status = status;
            }
        } else if self.personal_commands_setup.is_none()
            && self.personal_commands_setup_error.is_none()
            && self.personal_commands_status.host_state
                != crate::personal_commands::HostState::NotConfigured
        {
            self.personal_commands_status = crate::personal_commands::StatusSnapshot::default();
            crate::personal_commands::include_builtin_transformations(
                &mut self.personal_commands_status,
            );
        }
        self.poll_personal_commands_setup();
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

        let mut restart_listener = false;
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
                    restart_listener = self.listen_when_ready;
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
        if restart_listener {
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

    fn set_dictation_hotkey(&mut self, shortcut: DesktopShortcut) -> Result<()> {
        if self.is_running() {
            let error = "Stop listening before changing the dictation shortcut".to_string();
            self.error = Some(error.clone());
            return Err(eyre!(error));
        }
        let binding = Self::hotkey_from_shortcut(shortcut)?;
        let mut candidate = self.settings.clone();
        candidate.dictation_hotkey = binding.clone();
        if let Err(error) = crate::linux_input::X11HotkeyMonitor::start(
            candidate.dictation_hotkey.clone(),
            candidate.voice_action_hotkey.clone(),
            candidate.double_tap_lock,
        )
        .map(drop)
        {
            self.error = Some(format!("Could not register {}: {error:#}", binding.label()));
            return Err(error);
        }
        if let Err(error) = candidate.save() {
            self.error = Some(format!("Could not save shortcut: {error:#}"));
            return Err(error);
        }
        self.settings = candidate;
        self.error = None;
        self.settings_error = None;
        Ok(())
    }

    fn hotkey_from_shortcut(
        shortcut: DesktopShortcut,
    ) -> Result<crate::linux_settings::LinuxHotkey> {
        if shortcut.function {
            return Err(eyre!("the Fn modifier cannot be registered on X11"));
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
        binding.validate()?;
        Ok(binding)
    }

    fn set_voice_action_hotkey(&mut self, shortcut: DesktopShortcut) -> Result<()> {
        if self.is_running() {
            let error = "Stop listening before changing the Voice Action shortcut".to_string();
            self.settings_error = Some(error.clone());
            return Err(eyre!(error));
        }
        let binding = Self::hotkey_from_shortcut(shortcut).map_err(|error| {
            self.settings_error = Some(format!("That Voice Action shortcut is invalid: {error:#}"));
            error
        })?;
        let mut candidate = self.settings.clone();
        candidate.voice_action_hotkey = Some(binding.clone());
        crate::linux_input::X11HotkeyMonitor::start(
            candidate.dictation_hotkey.clone(),
            candidate.voice_action_hotkey.clone(),
            candidate.double_tap_lock,
        )
        .map(drop)
        .map_err(|error| {
            self.settings_error =
                Some(format!("Could not register {}: {error:#}", binding.label()));
            error
        })?;
        if let Err(error) = candidate.save() {
            self.settings_error = Some(format!("Could not save Voice Action shortcut: {error:#}"));
            return Err(error);
        }
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    fn set_voice_action_enabled(&mut self, enabled: bool) {
        if enabled == self.settings.voice_action_hotkey.is_some() {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.voice_action_hotkey =
            enabled.then(crate::linux_settings::LinuxHotkey::voice_action_default);
        if enabled
            && candidate.voice_action.model.is_none()
            && let Some(default) = self
                .opencode_catalog
                .as_ref()
                .and_then(|catalog| catalog.default_key.as_ref())
                .and_then(|key| {
                    self.opencode_catalog
                        .as_ref()
                        .and_then(|catalog| catalog.models.iter().find(|choice| &choice.key == key))
                })
        {
            candidate.voice_action.model = Some(default.model());
        }
        match candidate.save() {
            Ok(()) => {
                *self
                    .voice_action
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = candidate.voice_action.clone();
                self.settings = candidate;
                self.settings_error = None;
                self.restart_listener_for_settings();
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "Could not save the Voice Action setting: {error:#}"
                ));
            }
        }
    }

    fn set_voice_action_model(&mut self, model: Option<crate::opencode::Model>) {
        if model == self.settings.voice_action.model {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.voice_action.model = model;
        match candidate.save() {
            Ok(()) => {
                *self
                    .voice_action
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = candidate.voice_action.clone();
                self.settings = candidate;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error =
                    Some(format!("Could not save the Voice Action model: {error:#}"));
            }
        }
    }

    fn restart_listener_for_settings(&mut self) {
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            self.listen_when_ready = true;
            stop.store(true, Ordering::Relaxed);
            self.status = "Restarting".into();
        }
    }

    fn restart_into_update(&self) -> Result<()> {
        let UpdateState::Ready(update) = &self.update else {
            return Err(color_eyre::eyre::eyre!("no installed update is ready"));
        };
        crate::linux_updater::relaunch(update)
    }

    /// Persist and apply the interface language; `None` follows the
    /// system locale.
    fn set_ui_language(&mut self, code: Option<&'static str>) {
        if self.settings.ui_language.as_deref() == code {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.ui_language = code.map(str::to_string);
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
                crate::desktop_i18n::apply(code);
            }
            Err(error) => {
                self.settings_error =
                    Some(format!("Could not save the interface language: {error:#}"));
            }
        }
    }

    /// Persist the opt-in voice-command flag; the listener reads it when
    /// it starts.
    fn set_commands_enabled(&mut self, enabled: bool) {
        if enabled == self.settings.commands_enabled {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.commands_enabled = enabled;
        match candidate.save() {
            Ok(()) => {
                self.settings = candidate;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error =
                    Some(format!("Could not save the commands setting: {error:#}"));
            }
        }
    }

    fn set_history_retention(&mut self, retention: HistoryRetention) -> Result<()> {
        if retention == self.settings.history_retention {
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.history_retention = retention;
        candidate.save().map_err(|error| {
            let message = format!("Could not save history retention: {error:#}");
            self.settings_error = Some(message.clone());
            eyre!(message)
        })?;
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    fn set_text_replacements(
        &mut self,
        replacements: Vec<crate::text_replacements::TextReplacement>,
    ) -> Result<()> {
        if replacements == self.settings.text_replacements {
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.text_replacements = replacements;
        candidate.save().map_err(|error| {
            let message = format!("Could not save text replacements: {error:#}");
            self.settings_error = Some(message.clone());
            eyre!(message)
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

    fn set_modes(&mut self, modes: Vec<crate::linux_settings::LinuxMode>) -> Result<()> {
        if modes == self.settings.modes {
            return Ok(());
        }
        let mut candidate = self.settings.clone();
        candidate.modes = modes;
        candidate.save().map_err(|error| {
            let message = format!("Could not save modes: {error:#}");
            self.settings_error = Some(message.clone());
            eyre!(message)
        })?;
        *self
            .modes
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::linux_dictation::LinuxModeRuntime::from_settings(&candidate);
        self.settings = candidate;
        self.settings_error = None;
        self.refresh_personal_commands_host();
        Ok(())
    }

    fn refresh_personal_commands_host(&mut self) {
        let enabled = self.settings.transformations_enabled()
            || crate::personal_commands::workspace_configured();
        match (enabled, self.personal_commands.is_some()) {
            (true, false) => {
                self.personal_commands = crate::personal_commands::PersonalCommands::start(
                    crate::commands_engine::CommandConfig::new(),
                    self.transformations.clone(),
                );
            }
            (false, true) => self.personal_commands = None,
            _ => {}
        }
    }

    fn initialize_personal_commands_workspace(&mut self) {
        if self.personal_commands_setup.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Err(error) = std::thread::Builder::new()
            .name("linux-personal-commands-setup".into())
            .spawn(move || {
                let result = crate::personal_commands::initialize_workspace()
                    .map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
            })
        {
            self.personal_commands_setup_error = Some(format!(
                "Could not start the custom transformation setup worker: {error}"
            ));
            return;
        }
        self.personal_commands_setup = Some(receiver);
        self.personal_commands_setup_error = None;
        self.personal_commands_status.host_state = crate::personal_commands::HostState::Starting;
        self.personal_commands_status.last_reload_error = None;
        crate::personal_commands::include_builtin_transformations(
            &mut self.personal_commands_status,
        );
    }

    fn retry_personal_commands_host(&mut self) {
        self.personal_commands_setup_error = None;
        self.personal_commands = None;
        self.personal_commands_status.host_state = crate::personal_commands::HostState::Starting;
        self.personal_commands_status.last_reload_error = None;
        crate::personal_commands::include_builtin_transformations(
            &mut self.personal_commands_status,
        );
        self.refresh_personal_commands_host();
    }

    fn poll_personal_commands_setup(&mut self) {
        let result =
            self.personal_commands_setup
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Disconnected) => Some(Err(
                        "Custom transformation setup stopped unexpectedly".into(),
                    )),
                    Err(TryRecvError::Empty) => None,
                });
        let Some(result) = result else {
            return;
        };
        self.personal_commands_setup = None;
        match result {
            Ok(_) => self.retry_personal_commands_host(),
            Err(error) => {
                self.personal_commands_setup_error = Some(error.clone());
                self.personal_commands_status.host_state =
                    crate::personal_commands::HostState::Unavailable;
                self.personal_commands_status.last_reload_error = Some(error);
                crate::personal_commands::include_builtin_transformations(
                    &mut self.personal_commands_status,
                );
            }
        }
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
            let message = format!("Could not save mode processing: {error:#}");
            self.settings_error = Some(message.clone());
            eyre!(message)
        })?;
        *self
            .modes
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::linux_dictation::LinuxModeRuntime::from_settings(&candidate);
        self.settings = candidate;
        self.settings_error = None;
        Ok(())
    }

    fn set_mode_transformations(
        &mut self,
        target: ModeTarget,
        transformations: Vec<String>,
    ) -> Result<()> {
        let mut candidate = self.settings.clone();
        match target {
            ModeTarget::Global => candidate.dictation_transformations = transformations,
            ModeTarget::Mode(index) => {
                let mode = candidate
                    .modes
                    .get_mut(index)
                    .ok_or_else(|| eyre!("dictation mode {index} no longer exists"))?;
                mode.transformations = transformations;
            }
        }
        candidate.save().map_err(|error| {
            let message = format!("Could not save mode transformations: {error:#}");
            self.settings_error = Some(message.clone());
            eyre!(message)
        })?;
        *self
            .modes
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::linux_dictation::LinuxModeRuntime::from_settings(&candidate);
        self.settings = candidate;
        self.settings_error = None;
        self.refresh_personal_commands_host();
        Ok(())
    }

    fn request_opencode_catalog(&mut self) {
        if self.opencode_catalog_rx.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.opencode_catalog_rx = Some(receiver);
        self.opencode_retry_at = None;
        let _ = std::thread::Builder::new()
            .name("linux-opencode-catalog".into())
            .spawn(move || {
                let installed = crate::opencode::opencode_installed();
                let catalog = installed.then(crate::opencode::load_model_catalog);
                let _ = sender.send((installed, catalog));
            });
    }

    fn poll_opencode_catalog(&mut self) -> bool {
        let Some(receiver) = &self.opencode_catalog_rx else {
            return false;
        };
        match receiver.try_recv() {
            Ok((installed, Some(Ok(catalog)))) => {
                let default_voice_action_model = (self.settings.voice_action_hotkey.is_some()
                    && self.settings.voice_action.model.is_none())
                .then(|| {
                    catalog
                        .default_key
                        .as_ref()
                        .and_then(|key| catalog.models.iter().find(|choice| &choice.key == key))
                        .map(|choice| choice.model())
                })
                .flatten();
                if let Some(model) = default_voice_action_model {
                    self.set_voice_action_model(Some(model));
                }
                self.opencode_installed = installed;
                self.opencode_probe_complete = true;
                self.opencode_catalog = Some(catalog);
                self.opencode_catalog_error = None;
                self.opencode_retry_at = None;
                self.opencode_catalog_rx = None;
                true
            }
            Ok((installed, Some(Err(error)))) => {
                self.opencode_installed = installed;
                self.opencode_probe_complete = true;
                self.opencode_catalog_error = Some(format!("{error:#}"));
                self.opencode_retry_at = None;
                self.opencode_catalog_rx = None;
                true
            }
            Ok((installed, None)) => {
                self.opencode_installed = installed;
                self.opencode_probe_complete = true;
                self.opencode_catalog = None;
                self.opencode_catalog_error = None;
                self.opencode_retry_at = Some(Instant::now() + OPENCODE_INSTALL_RETRY_INTERVAL);
                self.opencode_catalog_rx = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.opencode_catalog_error =
                    Some("the OpenCode catalog worker stopped unexpectedly".into());
                self.opencode_probe_complete = true;
                self.opencode_retry_at = None;
                self.opencode_catalog_rx = None;
                true
            }
        }
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
        self.opencode_model_dropdown = None;
        self.opencode_variant_picker_open = None;
        self.transcription_dropdown_open = false;
        self.dictation_language_dropdown_open = false;
        self.microphone_dropdown_open = false;
        self.ui_language_dropdown_open = false;
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
        let items = crate::desktop_i18n::LANGUAGE_CHOICES
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
                dropdown_item(("linux-ui-language-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.host.set_ui_language(code);
                        this.ui_language_dropdown_open = false;
                        cx.notify();
                    }),
                )
            });
        dropdown_backdrop("linux-ui-language-backdrop")
            .on_click(cx.listener(|this, _, _, cx| {
                this.ui_language_dropdown_open = false;
                cx.notify();
            }))
            .child(
                dropdown_panel_with_width(
                    bounds,
                    viewport_height,
                    crate::desktop_i18n::LANGUAGE_CHOICES.len(),
                    px(240.0),
                )
                .id("linux-ui-language-dropdown")
                .overflow_y_scroll()
                .on_click(|_, _, cx| cx.stop_propagation())
                .children(items),
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
            crate::transcription_models::available_models()
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
                dropdown_item(("linux-microphone-option", index), label, selected).on_click(
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this
                            .host
                            .dispatch(DesktopAction::SetMicrophone(selection.clone()))
                            .is_ok()
                        {
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
                    .when_some(error, |list, error| {
                        list.child(error_message("Microphones could not be enumerated.", error))
                    }),
            )
            .into_any_element()
    }

    fn capture_hotkey(&mut self, keystroke: &Keystroke) -> bool {
        let Some(target) = self.capturing_hotkey else {
            return false;
        };
        if keystroke.key == "escape" {
            self.capturing_hotkey = None;
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
        let result = match target {
            HotkeyTarget::Dictation => self
                .host
                .dispatch(DesktopAction::SetDictationShortcut(shortcut)),
            HotkeyTarget::VoiceAction => self.host.set_voice_action_hotkey(shortcut),
        };
        if result.is_ok() {
            self.capturing_hotkey = None;
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
            microphone: DesktopMicrophoneSnapshot {
                devices: self.microphones.clone(),
                error: self.microphone_error.clone(),
                selected: self.settings.microphone.clone(),
            },
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
            DesktopAction::CheckForUpdates => match &self.update {
                UpdateState::Unmanaged => {
                    return Err(eyre!("updates are unavailable for this Linux install"));
                }
                UpdateState::Checking(_) | UpdateState::Ready(_) => {}
                UpdateState::Failed(_) | UpdateState::Waiting(_) => {
                    self.update = UpdateState::Checking(start_update_check());
                }
            },
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
            }
            DesktopAction::RefreshMicrophones => self.refresh_microphones(),
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
            DesktopAction::SetMicrophone(microphone) => {
                self.set_microphone(microphone)?;
            }
            DesktopAction::StartListening => self.start(),
            DesktopAction::StopListening => self.stop(),
        }
        Ok(())
    }
}

impl LinuxApp {
    fn select_pane(&mut self, pane: DesktopPane, cx: &mut Context<Self>) {
        debug_assert!(DesktopPane::available(self.host.capabilities()).contains(&pane));
        self.close_popups();
        self.pane = pane;
        self.history_pane.disarm_clear();
        if pane == DesktopPane::History {
            self.history_pane.reload();
        }
        if matches!(pane, DesktopPane::Modes | DesktopPane::VoiceAction)
            && self.host.opencode_catalog.is_none()
            && !self.host.opencode_probe_complete
        {
            self.host.request_opencode_catalog();
        }
        cx.notify();
    }

    fn set_history_retention(&mut self, retention: HistoryRetention) {
        match self.host.set_history_retention(retention) {
            Ok(()) => self.history_pane.set_retention(retention),
            Err(error) => self.history_pane.set_error(Some(error.to_string())),
        }
    }

    fn sync_text_replacements(&mut self, cx: &mut Context<Self>) {
        let replacements = self
            .replacement_inputs
            .iter()
            .map(|inputs| inputs.value(cx))
            .collect();
        let _ = self.host.set_text_replacements(replacements);
        cx.notify();
    }

    fn sync_global_processing_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let mut processing = self.host.settings.dictation_post_processing.clone();
        processing.prompt = prompt;
        let _ = self
            .host
            .set_mode_post_processing(ModeTarget::Global, processing);
        cx.notify();
    }

    fn sync_global_processing_deadline(&mut self, deadline: String, cx: &mut Context<Self>) {
        let mut processing = self.host.settings.dictation_post_processing.clone();
        if processing.update_deadline_from_text(&deadline) {
            let _ = self
                .host
                .set_mode_post_processing(ModeTarget::Global, processing);
        }
        cx.notify();
    }

    fn mode_inputs(
        mode: &crate::linux_settings::LinuxMode,
        cx: &mut Context<Self>,
    ) -> LinuxModeInputs {
        let name = cx.new(|cx| TextInput::new(cx, "e.g. Coding", &mode.name));
        let applications = cx.new(|cx| {
            TextInput::new(
                cx,
                "e.g. code, firefox, slack",
                mode.applications.join(", "),
            )
        });
        let processing_prompt = cx.new(|cx| {
            TextInput::multiline_with_height(
                cx,
                "Tell OpenCode exactly how to transform the dictated text.",
                &mode.post_processing.prompt,
                px(92.0),
            )
        });
        let processing_deadline = cx.new(|cx| {
            TextInput::new(
                cx,
                "Seconds",
                mode.post_processing.deadline_seconds.to_string(),
            )
        });
        let name_changed = cx.subscribe(&name, |this, _, _: &TextChanged, cx| this.sync_modes(cx));
        let applications_changed = cx.subscribe(&applications, |this, _, _: &TextChanged, cx| {
            this.sync_modes(cx)
        });
        let processing_prompt_changed = cx
            .subscribe(&processing_prompt, |this, _, _: &TextChanged, cx| {
                this.sync_modes(cx)
            });
        let processing_deadline_changed = cx
            .subscribe(&processing_deadline, |this, _, _: &TextChanged, cx| {
                this.sync_modes(cx)
            });
        let corrections = mode
            .corrections
            .iter()
            .map(|correction| ReplacementEditorInput::new(correction, Self::sync_modes, cx))
            .collect();
        LinuxModeInputs {
            name,
            applications,
            processing_prompt,
            processing_deadline,
            corrections,
            _subscriptions: vec![
                name_changed,
                applications_changed,
                processing_prompt_changed,
                processing_deadline_changed,
            ],
        }
    }

    fn mode_values(
        &self,
        excluded: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<crate::linux_settings::LinuxMode> {
        self.mode_inputs
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != excluded)
            .map(|(index, inputs)| crate::linux_settings::LinuxMode {
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
                        .update_deadline_from_text(inputs.processing_deadline.read(cx).text());
                    processing
                },
                transformations: self
                    .host
                    .settings
                    .modes
                    .get(index)
                    .map_or_else(Vec::new, |mode| mode.transformations.clone()),
            })
            .collect()
    }

    fn sync_modes(&mut self, cx: &mut Context<Self>) {
        let modes = self.mode_values(None, cx);
        let _ = self.host.set_modes(modes);
        cx.notify();
    }

    fn add_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = crate::linux_settings::LinuxMode {
            name: format!("Mode {}", self.mode_inputs.len() + 1),
            ..Default::default()
        };
        self.mode_inputs.push(Self::mode_inputs(&mode, cx));
        self.selected_mode = ModeTarget::Mode(self.mode_inputs.len().saturating_sub(1));
        self.sync_modes(cx);
        window.blur();
    }

    fn remove_mode(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.mode_inputs.len() {
            return;
        }
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

    fn render_voice_action(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = if self.host.opencode_catalog_rx.is_some() || !self.host.opencode_probe_complete
        {
            VoiceActionView::Unavailable(VoiceActionUnavailableView {
                title: "Checking OpenCode",
                description: "Loading the local model catalog for Voice Action.",
                error: None,
                retry_label: None,
                setup_label: None,
            })
        } else if !self.host.opencode_installed {
            VoiceActionView::Unavailable(VoiceActionUnavailableView {
                title: "OpenCode is required",
                description: "Install and configure OpenCode to use Voice Action.",
                error: None,
                retry_label: Some("Check again"),
                setup_label: Some("Open OpenCode setup"),
            })
        } else if let Some(error) = self.host.opencode_catalog_error.clone() {
            VoiceActionView::Unavailable(VoiceActionUnavailableView {
                title: "Models could not be loaded",
                description: "Retry the OpenCode model catalog.",
                error: Some(VoiceActionError {
                    title: "OpenCode reported:",
                    detail: error,
                }),
                retry_label: Some("Retry"),
                setup_label: None,
            })
        } else {
            let enabled = self.host.settings.voice_action_hotkey.is_some();
            let running = self.host.is_running();
            let capturing = self.capturing_hotkey == Some(HotkeyTarget::VoiceAction);
            let shortcut_control = div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .id("linux-voice-action-binding")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(running, |control| control.opacity(0.5))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if running {
                                this.host.settings_error = Some(
                                    "Stop listening before changing the Voice Action shortcut"
                                        .into(),
                                );
                            } else {
                                this.capturing_hotkey =
                                    if this.capturing_hotkey == Some(HotkeyTarget::VoiceAction) {
                                        None
                                    } else {
                                        Some(HotkeyTarget::VoiceAction)
                                    };
                                this.host.settings_error = None;
                            }
                            cx.notify();
                        }))
                        .child(if capturing {
                            div()
                                .h(px(34.0))
                                .px_3()
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(rgb(TEXT_SOFT))
                                .bg(rgb(SURFACE))
                                .text_size(px(11.0))
                                .child(tr("Press a shortcut..."))
                                .into_any_element()
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
                                        .map(crate::linux_settings::LinuxHotkey::keycaps),
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
                    div()
                        .id("linux-voice-action-toggle")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.host.set_voice_action_enabled(!enabled);
                            if enabled {
                                this.capturing_hotkey = None;
                            }
                            cx.notify();
                        }))
                        .child(toggle(if enabled { 1.0 } else { 0.0 })),
                );
            let model_control =
                self.render_opencode_model_control(OpenCodeModelTarget::VoiceAction, cx);
            VoiceActionView::Ready(Box::new(VoiceActionReadyView {
                shortcut: VoiceActionSettingRow::translated(
                    "Shortcut",
                    "Hold to speak; selected text is included automatically",
                    shortcut_control,
                ),
                processing: vec![VoiceActionSettingRow::translated(
                    "Model",
                    "Fulfils each voice action; served by OpenCode",
                    model_control,
                )],
                error: self
                    .host
                    .settings_error
                    .clone()
                    .map(|detail| VoiceActionError {
                        title: "The Voice Action setting could not be saved.",
                        detail,
                    }),
            }))
        };
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

    fn render_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let ModeTarget::Mode(index) = self.selected_mode
            && index >= self.mode_inputs.len()
        {
            self.selected_mode = ModeTarget::Global;
        }
        let add = header_button(tr("Add mode"))
            .id("linux-add-mode")
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
                        .collect()
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
        let detail = self.render_linux_mode_detail(window, cx);

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

    fn render_linux_mode_detail(
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
                        self.render_linux_mode_processing(ModeTarget::Global, window, cx);
                    let transformations =
                        self.render_linux_mode_transformations(ModeTarget::Global, cx);
                    let basics = render_shared_mode_basics(
                        ModeBasicsView::Global {
                            title: "Global",
                            description: "Used unless a more specific application mode matches.",
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
                        .child(transformations)
                        .into_any_element()
                }
                ModeTarget::Mode(mode_index) => {
                    let processing = self.render_linux_mode_processing(
                        ModeTarget::Mode(mode_index),
                        window,
                        cx,
                    );
                    let transformations = self.render_linux_mode_transformations(
                        ModeTarget::Mode(mode_index),
                        cx,
                    );
                    let inputs = &self.mode_inputs[mode_index];
                    let basics = render_shared_mode_basics(
                        ModeBasicsView::Custom {
                            target: ModeTarget::Mode(mode_index),
                            name: inputs.name.clone(),
                            applications: Box::new(ModeApplicationEditorView::Freeform {
                                title: "Applications",
                                description:
                                    "Applies when the focused executable contains any of these names",
                                input: inputs.applications.clone(),
                            }),
                            websites: None,
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
                        .child(transformations)
                        .into_any_element()
                }
            });
        if let Some(error) = self.host.settings_error.clone() {
            detail = detail.child(error_message("Modes could not be saved.", error));
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

    fn mode_transformations(&self, target: ModeTarget) -> Option<&Vec<String>> {
        match target {
            ModeTarget::Global => Some(&self.host.settings.dictation_transformations),
            ModeTarget::Mode(index) => self
                .host
                .settings
                .modes
                .get(index)
                .map(|mode| &mode.transformations),
        }
    }

    fn render_linux_mode_transformations(
        &self,
        target: ModeTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .mode_transformations(target)
            .cloned()
            .unwrap_or_default();
        let catalog = self
            .host
            .personal_commands_status
            .transformations
            .iter()
            .map(|transformation| TransformationCatalogEntry {
                id: transformation.id.clone(),
                name: transformation.name.clone(),
                description: transformation.description.clone(),
            })
            .collect();
        let configured = crate::personal_commands::workspace_configured();
        let workspace = if self.host.personal_commands_setup.is_some() {
            Some(TransformationWorkspaceView {
                title: "Custom transformations",
                description: "Preparing the TypeScript workspace and installing its dependencies…",
                error: None,
                action_label: None,
                action: ModeTransformationsWorkspaceAction::Initialize,
            })
        } else if let Some(error) = self.host.personal_commands_setup_error.clone() {
            Some(TransformationWorkspaceView {
                title: "Custom transformations",
                description: "The TypeScript workspace could not be prepared.",
                error: Some(error),
                action_label: Some("Retry setup"),
                action: ModeTransformationsWorkspaceAction::Initialize,
            })
        } else {
            match self.host.personal_commands_status.host_state {
                crate::personal_commands::HostState::Active
                    if self
                        .host
                        .personal_commands_status
                        .last_reload_error
                        .is_none() =>
                {
                    None
                }
                crate::personal_commands::HostState::Starting => {
                    Some(TransformationWorkspaceView {
                        title: "Custom transformations",
                        description: "Loading registered TypeScript transformations…",
                        error: None,
                        action_label: None,
                        action: ModeTransformationsWorkspaceAction::Retry,
                    })
                }
                crate::personal_commands::HostState::NotConfigured => {
                    Some(TransformationWorkspaceView {
                        title: "Custom transformations",
                        description: "Create a TypeScript workspace to define reusable transformations.",
                        error: None,
                        action_label: Some("Set up"),
                        action: ModeTransformationsWorkspaceAction::Initialize,
                    })
                }
                _ => Some(TransformationWorkspaceView {
                    title: "Custom transformations",
                    description: "Registered TypeScript transformations are unavailable.",
                    error: self.host.personal_commands_status.last_reload_error.clone(),
                    action_label: Some(if configured { "Retry" } else { "Set up" }),
                    action: if configured {
                        ModeTransformationsWorkspaceAction::Retry
                    } else {
                        ModeTransformationsWorkspaceAction::Initialize
                    },
                }),
            }
        };
        render_shared_mode_transformations(
            ModeTransformationsView {
                target,
                selected,
                catalog,
                picker_open: self.transformation_picker_open,
                workspace,
            },
            cx,
        )
    }

    fn opencode_model_key(&self, target: OpenCodeModelTarget) -> Option<String> {
        match target {
            OpenCodeModelTarget::VoiceAction => self
                .host
                .settings
                .voice_action
                .model
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
                        "linux-opencode-model-{target_id}"
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

    fn set_mode_variant(&mut self, target: ModeTarget, variant: Option<String>) {
        let Some(mut processing) = self.mode_post_processing(target).cloned() else {
            return;
        };
        let default_model = self
            .host
            .opencode_catalog
            .as_ref()
            .and_then(|catalog| catalog.default_key.as_deref());
        processing.set_variant(variant, default_model);
        let _ = self.host.set_mode_post_processing(target, processing);
    }

    fn render_linux_mode_processing(
        &mut self,
        target: ModeTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(settings) = self.mode_post_processing(target).cloned() else {
            return div().into_any_element();
        };
        let installed = self.host.opencode_installed;
        let compact = window.viewport_size().width < px(980.0);
        let settings_view = if settings.enabled {
            let model_control =
                self.render_opencode_model_control(OpenCodeModelTarget::Mode(target), cx);
            let prompt = match target {
                ModeTarget::Global => self.global_processing_prompt.clone(),
                ModeTarget::Mode(index) => self.mode_inputs[index].processing_prompt.clone(),
            };
            let deadline = match target {
                ModeTarget::Global => self.global_processing_deadline.clone(),
                ModeTarget::Mode(index) => self.mode_inputs[index].processing_deadline.clone(),
            };
            let variants = self
                .host
                .opencode_catalog
                .as_ref()
                .map(|catalog| catalog.variants_for(settings.model.as_deref()).to_vec())
                .unwrap_or_default();
            let variant_control = (!variants.is_empty()).then(|| {
                render_mode_variant_picker(
                    ModeVariantPickerView {
                        target,
                        variants,
                        selected: settings.variant.clone(),
                        open: self.opencode_variant_picker_open == Some(target),
                    },
                    cx,
                )
            });
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
                            .when(compact, |row| row.flex_col().items_start().gap_3())
                            .when(!compact, |row| row.items_center().justify_between().gap_4())
                            .child(div().flex_1().min_w(px(0.0)).child(settings_copy(
                                "Deadline",
                                "Maximum time OpenCode may spend on one rewrite",
                            )))
                            .child(
                                div()
                                    .when(!compact, |control| control.w(px(120.0)).flex_none())
                                    .when(compact, |control| control.w_full())
                                    .child(deadline),
                            ),
                    )
                    .when_some(variant_control, |processing, variant_control| {
                        processing.child(
                            div()
                                .flex()
                                .when(compact, |row| row.flex_col().items_start().gap_3())
                                .when(!compact, |row| row.items_start().justify_between().gap_4())
                                .child(div().flex_1().min_w(px(0.0)).child(settings_copy(
                                    "Thinking",
                                    "Choose how much reasoning the model should use",
                                )))
                                .child(variant_control),
                        )
                    })
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
        let backdrop = dropdown_backdrop("linux-opencode-model-backdrop").on_click(cx.listener(
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
                            "linux-opencode-model-dropdown-{target_id}"
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
            let default_label = catalog.default_name.clone().map_or_else(
                || "OpenCode default".into(),
                |name| format!("{name} — OpenCode default"),
            );
            items.push(
                dropdown_item(
                    SharedString::from(format!("linux-opencode-model-default-{target_id}")),
                    default_label,
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
                SharedString::from(format!("linux-opencode-model-option-{target_id}-{index}")),
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
                        "linux-opencode-model-dropdown-{target_id}"
                    )))
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .into_any_element()
    }

    fn render_shared_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        debug_assert!(self.host.capabilities().listener_control);
        let items = render_navigation_items(
            self.pane,
            self.host.capabilities(),
            |label| tr(label).to_string(),
            Self::select_pane,
            cx,
        );
        sidebar_frame()
            .w(px(SIDEBAR_WIDTH))
            .px(px(14.0))
            .pt(px(52.0))
            .pb_4()
            .flex()
            .flex_col()
            .children(items)
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
        let shortcut = if self.capturing_hotkey == Some(HotkeyTarget::Dictation) {
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
                .child(tr("Press a shortcut..."))
                .into_any_element()
        } else {
            hotkey_keycaps(snapshot.dictation_shortcut.clone(), 1.0)
        };
        let update_ready = snapshot.update_status == DesktopUpdateStatus::ReadyToRestart;
        let transcription = &snapshot.transcription;
        let model_label = match transcription.preparing {
            Some(preparing) => tr_fill(
                "Preparing {}",
                crate::transcription_models::definition(preparing).name,
            ),
            None => crate::transcription_models::definition(transcription.selection.model)
                .name
                .to_string(),
        };
        let dictation_language_label = if transcription.selection.language == "auto" {
            tr("Auto").to_string()
        } else {
            crate::transcription_models::language_name(&transcription.selection.language)
                .to_string()
        };
        let microphone_label = snapshot
            .microphone
            .selected
            .clone()
            .unwrap_or_else(|| tr("Automatic").into());
        let ui_language_label = match self.host.settings.ui_language.as_deref() {
            None => tr("System").to_string(),
            Some(code) => crate::desktop_i18n::choice_name(Some(code)).to_string(),
        };
        let commands_enabled = self.host.settings.commands_enabled;
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
        let listener_label = tr(&listener_label).to_string();
        let device = snapshot
            .activity
            .device
            .clone()
            .unwrap_or_else(|| tr("Automatic microphone").into());
        let status_hint = format!(
            "{device} · {}",
            tr_fill("Hold {} to dictate", &snapshot.dictation_shortcut_label)
        );
        let listener_action = header_button(if running {
            tr("Stop listening")
        } else {
            tr("Start listening")
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
                                                    header_button(tr("Browse"))
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
                                                                    let _ = this.host.dispatch(
                                                                        DesktopAction::RefreshMicrophones,
                                                                    );
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
                                                            if this.capturing_hotkey
                                                                == Some(HotkeyTarget::Dictation)
                                                            {
                                                                None
                                                            } else {
                                                                Some(HotkeyTarget::Dictation)
                                                            };
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
                                    settings_panel()
                                        .child(
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
                                            .when(!crate::DEVELOPER_FEATURES_ENABLED, |row| {
                                                row.border_b_0()
                                            })
                                            .when(running, |row| row.opacity(0.5))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if !running {
                                                        let enabled =
                                                            !this.host.snapshot().double_tap_lock;
                                                        let _ = this.host.dispatch(
                                                            DesktopAction::SetDoubleTapLock(
                                                                enabled,
                                                            ),
                                                        );
                                                        cx.notify();
                                                    }
                                                }),
                                            ),
                                        )
                                        .when(crate::DEVELOPER_FEATURES_ENABLED, |panel| {
                                            panel.child(
                                                settings_row(
                                                    "Voice commands",
                                                    "Opt-in wake-word commands through the local Moonshine model; applies when listening starts",
                                                    toggle(if commands_enabled { 1.0 } else { 0.0 }),
                                                )
                                                .border_b_0()
                                                .id("linux-commands-setting")
                                                .on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        let enabled = !this
                                                            .host
                                                            .settings
                                                            .commands_enabled;
                                                        this.host.set_commands_enabled(enabled);
                                                        cx.notify();
                                                    },
                                                )),
                                            )
                                        }),
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
                                                        .id("linux-ui-language")
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
                                            "HEX",
                                            "Private local dictation for Linux",
                                            div().text_size(px(12.0)).text_color(rgb(MUTED)).child(
                                                tr_fill("Version {}", env!("CARGO_PKG_VERSION")),
                                            ),
                                        ))
                                        .when(update_ready, |panel| {
                                            panel.child(settings_row(
                                                "Update ready",
                                                "Restart into the verified Linux release.",
                                                compact_button(tr("Restart"))
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
                                                compact_button(tr("Dismiss"))
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

impl crate::desktop_onboarding::OnboardingDelegate for LinuxApp {
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
            self.host.settings_error = Some(format!("Could not save Linux settings: {error:#}"));
        }
        // The launch-time listener start was deferred while the dialog
        // held the screen.
        let _ = self.host.dispatch(DesktopAction::StartListening);
        cx.notify();
    }
}

impl crate::desktop_hud_lab::HudLabDelegate for LinuxApp {
    fn hud_lab(&self) -> &crate::desktop_hud_lab::HudLabState {
        &self.hud_lab
    }

    fn hud_lab_mut(&mut self) -> &mut crate::desktop_hud_lab::HudLabState {
        &mut self.hud_lab
    }

    fn configure_platform_hud(&mut self, _tuning: crate::desktop_hud_lab::HudTuning) {
        // Linux has no dictation HUD yet, so the lab only drives its
        // embedded preview; deliver the tuning here once the Linux shell
        // grows a HUD window.
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

impl ModeBasicsDelegate for LinuxApp {
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

impl ModeListDelegate for LinuxApp {
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

impl ReplacementEditorDelegate for LinuxApp {
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
            ReplacementEditorAction::Remove {
                target: ModeTarget::Global,
                index,
            } => {
                if index < self.replacement_inputs.len() {
                    self.replacement_inputs.remove(index);
                    self.sync_text_replacements(cx);
                }
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

impl HistoryPaneDelegate for LinuxApp {
    fn handle_history_action(&mut self, action: HistoryPaneAction, cx: &mut Context<Self>) {
        match action {
            HistoryPaneAction::SetRetention(retention) => self.set_history_retention(retention),
            HistoryPaneAction::Select(id) => self.history_pane.select(id),
            HistoryPaneAction::Copy(id) => self.history_pane.copy(id),
            HistoryPaneAction::Delete(id) => self.history_pane.delete(id),
            HistoryPaneAction::Clear => self.history_pane.clear(),
        }
        cx.notify();
    }
}

impl ModeTransformationsDelegate for LinuxApp {
    fn handle_mode_transformations_action(
        &mut self,
        action: ModeTransformationsAction,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, ModeTransformationsAction::TogglePicker) {
            self.transformation_picker_open = !self.transformation_picker_open;
            cx.notify();
            return;
        }
        if let ModeTransformationsAction::Workspace(action) = action {
            match action {
                ModeTransformationsWorkspaceAction::Initialize => {
                    self.host.initialize_personal_commands_workspace();
                }
                ModeTransformationsWorkspaceAction::Retry => {
                    self.host.retry_personal_commands_host();
                }
            }
            cx.notify();
            return;
        }
        let target = match &action {
            ModeTransformationsAction::Add { target, .. }
            | ModeTransformationsAction::Remove { target, .. }
            | ModeTransformationsAction::Move { target, .. } => *target,
            ModeTransformationsAction::TogglePicker => unreachable!(),
            ModeTransformationsAction::Workspace(_) => unreachable!(),
        };
        let Some(mut transformations) = self.mode_transformations(target).cloned() else {
            return;
        };
        let changed = match action {
            ModeTransformationsAction::Add { id, .. } => {
                let registered = self
                    .host
                    .personal_commands_status
                    .transformations
                    .iter()
                    .any(|transformation| transformation.id == id);
                if !registered || transformations.contains(&id) {
                    false
                } else {
                    transformations.push(id);
                    true
                }
            }
            ModeTransformationsAction::Remove { id, .. } => {
                let before = transformations.len();
                transformations.retain(|candidate| candidate != &id);
                transformations.len() != before
            }
            ModeTransformationsAction::Move {
                id, target_index, ..
            } => reorder_transformation(&mut transformations, &id, target_index),
            ModeTransformationsAction::TogglePicker => unreachable!(),
            ModeTransformationsAction::Workspace(_) => unreachable!(),
        };
        if changed {
            let _ = self.host.set_mode_transformations(target, transformations);
        }
        cx.notify();
    }
}

impl ModeProcessingDelegate for LinuxApp {
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
            ModeProcessingAction::ToggleVariantPicker { target } => {
                self.opencode_variant_picker_open =
                    if self.opencode_variant_picker_open == Some(target) {
                        None
                    } else {
                        Some(target)
                    };
            }
            ModeProcessingAction::SetVariant { target, variant } => {
                self.opencode_variant_picker_open = None;
                self.set_mode_variant(target, variant);
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

impl VoiceActionPaneDelegate for LinuxApp {
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

impl Render for LinuxApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let snapshot = self.host.snapshot();
        let content = match self.pane {
            DesktopPane::Settings => self.render_shared_settings(&snapshot, cx),
            DesktopPane::Modes => self.render_modes(window, cx),
            DesktopPane::VoiceAction => self.render_voice_action(window, cx),
            DesktopPane::HudLab => crate::desktop_hud_lab::render_hud_lab_pane(self, window, cx),
            DesktopPane::Activity => crate::desktop_activity_pane::render_activity_pane(&snapshot),
            DesktopPane::History => self.render_history(cx),
            DesktopPane::Commands | DesktopPane::Meetings => {
                unreachable!("capability-filtered Linux pane")
            }
        };
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
        let ui_language_dropdown = self
            .ui_language_dropdown_open
            .then(|| self.render_ui_language_dropdown(viewport.height, cx));
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
        window_frame()
            .child(self.render_shared_navigation(cx))
            .child(div().flex_1().h_full().overflow_hidden().child(content))
            .children(transcription_dropdown)
            .children(dictation_language_dropdown)
            .children(microphone_dropdown)
            .children(ui_language_dropdown)
            .children(opencode_model_dropdown)
            .children(model_picker)
            .children(catalog_filter_dropdown)
            .children(onboarding)
            .children(onboarding_language_dropdown)
    }
}
