use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Modifiers as GpuiModifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathPromptOptions, Pixels, Point, Render, ScrollHandle, SharedString,
    Subscription, Timer, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
    actions, deferred, div, img, prelude::*, px, relative, rgb, rgba, size,
};

use crate::app_settings::{
    AppSettings, DictationMode, HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide,
    RecordingAudioBehavior, TextReplacement,
};
use crate::application_catalog::InstalledApplication;
use crate::commands::{CommandConfig, CommandInfo, CommandScope};
use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopCapabilities, DesktopHost, DesktopSnapshot, DesktopTranscriptionSnapshot,
    DesktopUpdateStatus,
};
use crate::desktop_transcription_picker::{
    TranscriptionPickerDelegate, TranscriptionPickerModel, TranscriptionPickerProgress,
    TranscriptionPickerStatus, TranscriptionPickerView,
    render_transcription_picker as render_shared_transcription_picker,
    transcription_selection_is_active,
};
use crate::desktop_ui::{
    ACCENT, CANVAS, COMPACT_MULTILINE_INPUT_HEIGHT, CONTROL_HEIGHT, FAINT, LINE, MUTED, NEGATIVE,
    NavigationIcon, PANE_CONTENT_WIDTH, PANE_LIST_WIDTH, SECTION_GAP, SIDEBAR_WIDTH, SURFACE,
    SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_INPUT_HEIGHT, TEXT_SOFT, compact_button,
    compact_header_plus_button, compact_panel, compact_panel_header, compact_plus_button,
    compact_section_label, disclosure_button, empty_message, error_message, header_button,
    hotkey_keycaps, listener_status, mix_color, navigation_item, pane_body, pane_content,
    pane_header, pane_header_with_action, section_label, segmented_control, segmented_item,
    settings_copy, settings_panel, settings_row, settings_section_label, sidebar_frame, toggle,
    window_frame,
};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender, HudTuning};
use crate::dictation_processor::{ModelCatalog, ModelChoice};
use crate::events::{
    CommandOutcome, DictationPhase, EventReader, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention};
use crate::login_item::LoginItemStatus;
use crate::meeting::{self, MeetingManifest, MeetingRequest, MeetingStatus};
use crate::onboarding::{
    PermissionAction, PermissionKind, PermissionState, PermissionWarning, SetupStatus,
};
use crate::personal_commands::{HostState, StatusContext, StatusSnapshot, StatusTransformation};
use crate::recognition::CommandModelStatus;
use crate::text_input::{
    Changed as TextChanged, Dismissed as TextDismissed, Navigate as TextNavigate,
    Submitted as TextSubmitted, TextInput,
};
use crate::transcription_models::{
    ModelDefinition, ModelRuntime, TranscriptionModelId, TranscriptionSelection, definition,
    language_name, validate,
};

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;
const HOTKEY_MIN_WIDTH: f32 = 148.0;
const HOTKEY_SIDE_SELECTOR_WIDTH: f32 = 116.0;
const OPENCODE_INSTALL_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_OPENCODE_ERROR_BYTES: usize = 4 * 1024;
const ACTIVITY_LIMIT: usize = 100;
const OPENCODE_BETA_DOCS_URL: &str = "https://v2.opencode.ai/";

actions!(
    hex,
    [
        CloseWindow,
        CheckForUpdates,
        RetryCommands,
        HideApplication,
        MinimizeWindow,
        QuitApplication,
        ShowCommands,
        ShowHistory,
        ShowModes,
        ShowSettings,
        ShowVoiceAction,
        ToggleFullscreen,
    ]
);

/// Opens the app shell, or focuses and refreshes an existing shell window.
#[allow(clippy::too_many_arguments)]
pub fn open_or_focus(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    commands: CommandConfig,
    meeting_requests: SyncSender<MeetingRequest>,
    indicator: DictationIndicatorSender,
    recognition_start: Option<SyncSender<()>>,
    history: Option<History>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    if let Some(handle) = app_window.borrow().as_ref().copied()
        && handle
            .update(cx, |this, window, cx| {
                if this.event_reader.path() != event_path {
                    this.event_reader = EventReader::open(event_path.clone());
                }
                this.meeting_requests = meeting_requests.clone();
                this.indicator = indicator.clone();
                this.recognition_start = recognition_start.clone();
                this.history = history.clone();
                this.refresh(cx);
                window.activate_window();
            })
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }

    open_new(
        app_window,
        event_path,
        commands,
        meeting_requests,
        indicator,
        recognition_start,
        history,
        None,
        cx,
    )
}

pub fn open_preview(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    commands: CommandConfig,
    meeting_requests: SyncSender<MeetingRequest>,
    indicator: DictationIndicatorSender,
    preview: AppWindowPreview,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    if let Some(handle) = app_window.borrow().as_ref().copied()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }
    open_new(
        app_window,
        event_path,
        commands,
        meeting_requests,
        indicator,
        None,
        None,
        Some(preview),
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
fn open_new(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    commands: CommandConfig,
    meeting_requests: SyncSender<MeetingRequest>,
    indicator: DictationIndicatorSender,
    recognition_start: Option<SyncSender<()>>,
    history: Option<History>,
    preview: Option<AppWindowPreview>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    let preview_mode = preview.is_some();
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("HEX".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            is_resizable: !preview_mode,
            is_minimizable: true,
            window_min_size: Some(size(px(MINIMUM_WIDTH), px(MINIMUM_HEIGHT))),
            tabbing_identifier: preview_mode.then(|| "hex-preview".into()),
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                AppWindow::new(
                    event_path,
                    commands,
                    meeting_requests,
                    indicator,
                    recognition_start,
                    history,
                    preview,
                    window,
                    cx,
                )
            })
        },
    )?;
    *app_window.borrow_mut() = Some(handle);
    cx.activate(true);
    Ok(handle)
}

#[derive(Clone, Copy)]
pub enum PreviewPane {
    HudLab,
    Meetings,
    Commands,
    Activity,
    History,
    Modes,
    VoiceAction,
    Replacements,
    Settings,
}

#[derive(Clone, Copy)]
pub enum PreviewModelState {
    Actual,
    Installed,
    Missing,
    Downloading,
    Error,
}

#[derive(Clone)]
pub struct AppWindowPreview {
    pub pane: PreviewPane,
    pub transcription_picker: Option<(String, PreviewModelState)>,
    pub onboarding: bool,
    pub collapse_mode_processing: bool,
    pub open_transformation_picker: bool,
    pub select_global_mode: bool,
    pub voice_action_enabled: bool,
    pub opencode_unavailable: bool,
    pub permissions_missing: bool,
    pub model_missing: bool,
    pub command_model_missing: bool,
    pub open_history_retention: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Pane {
    HudLab,
    Meetings,
    Commands,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Activity,
    History,
    Modes,
    VoiceAction,
    #[default]
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudDemoState {
    Recording,
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudControl {
    LineCount,
    Curvature,
    Speed,
    Blur,
    Glow,
    Depth,
    LightAngle,
    Outline,
}

const HUD_MAX_SPEED: f32 = 24.0;

impl Pane {
    fn on_reopen(self, status: SetupStatus) -> Self {
        if crate::onboarding::permission_warnings(status).is_empty() {
            self
        } else {
            Self::Settings
        }
    }

    const ALL: [Self; 8] = [
        Self::Settings,
        Self::Modes,
        Self::Commands,
        Self::VoiceAction,
        Self::History,
        Self::HudLab,
        Self::Meetings,
        Self::Activity,
    ];

    fn all(capabilities: DesktopCapabilities) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|pane| match pane {
                Self::Settings => true,
                Self::Modes => capabilities.modes,
                Self::Commands => capabilities.commands,
                Self::VoiceAction => capabilities.voice_action,
                Self::History => capabilities.history,
                Self::HudLab => capabilities.hud_lab,
                Self::Meetings => capabilities.meetings,
                Self::Activity => capabilities.activity,
            })
            .collect()
    }

    fn label(self) -> &'static str {
        match self {
            Self::HudLab => "HUD Lab",
            Self::Meetings => "Meetings",
            Self::Commands => "Commands",
            Self::Activity => "Activity",
            Self::History => "History",
            Self::Modes => "Modes",
            Self::VoiceAction => "Voice Action",
            Self::Settings => "Settings",
        }
    }

    fn icon(self) -> NavigationIcon {
        match self {
            Self::Settings => NavigationIcon::Settings,
            Self::Modes => NavigationIcon::Modes,
            Self::VoiceAction => NavigationIcon::VoiceAction,
            Self::History => NavigationIcon::History,
            Self::Commands => NavigationIcon::Commands,
            Self::Meetings => NavigationIcon::Meetings,
            Self::Activity => NavigationIcon::Activity,
            Self::HudLab => NavigationIcon::HudLab,
        }
    }

    fn from_preview(pane: PreviewPane) -> Self {
        match pane {
            PreviewPane::HudLab => Self::HudLab,
            PreviewPane::Meetings => Self::Meetings,
            PreviewPane::Commands => Self::Commands,
            PreviewPane::Activity => Self::Activity,
            PreviewPane::History => Self::History,
            PreviewPane::Modes => Self::Modes,
            PreviewPane::VoiceAction => Self::VoiceAction,
            PreviewPane::Replacements => Self::Modes,
            PreviewPane::Settings => Self::Settings,
        }
    }

    fn from_developer(pane: crate::developer_control::DeveloperPane) -> Self {
        use crate::developer_control::DeveloperPane;
        match pane {
            DeveloperPane::Settings => Self::Settings,
            DeveloperPane::Modes => Self::Modes,
            DeveloperPane::VoiceAction => Self::VoiceAction,
            DeveloperPane::Replacements => Self::Modes,
            DeveloperPane::History => Self::History,
            DeveloperPane::HudLab => Self::HudLab,
            DeveloperPane::Commands => Self::Commands,
            DeveloperPane::Meetings => Self::Meetings,
            DeveloperPane::Activity => Self::Activity,
        }
    }

    fn developer(self) -> crate::developer_control::DeveloperPane {
        use crate::developer_control::DeveloperPane;
        match self {
            Self::Settings => DeveloperPane::Settings,
            Self::Modes => DeveloperPane::Modes,
            Self::VoiceAction => DeveloperPane::VoiceAction,
            Self::History => DeveloperPane::History,
            Self::HudLab => DeveloperPane::HudLab,
            Self::Commands => DeveloperPane::Commands,
            Self::Meetings => DeveloperPane::Meetings,
            Self::Activity => DeveloperPane::Activity,
        }
    }
}

#[derive(Clone)]
struct CatalogCommand {
    task: String,
    scope: CommandScope,
    phrase: String,
    aliases: Vec<String>,
    description: String,
    id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PersonalWorkspaceState {
    Missing,
    Creating,
    Ready,
    Error,
}

fn personal_workspace_state(
    config_exists: bool,
    creating: bool,
    status: &StatusSnapshot,
) -> PersonalWorkspaceState {
    if creating {
        PersonalWorkspaceState::Creating
    } else if config_exists {
        if status.host_state == HostState::Unavailable && status.active_generation.is_none() {
            PersonalWorkspaceState::Error
        } else {
            PersonalWorkspaceState::Ready
        }
    } else {
        PersonalWorkspaceState::Missing
    }
}

#[derive(Clone, PartialEq)]
struct TranscriptLine {
    source: String,
    text: String,
    pause_before_ms: Option<u64>,
}

#[derive(Clone)]
enum MeetingTranscript {
    Empty,
    Loaded(Vec<TranscriptLine>),
    Unavailable(String),
}

struct ToggleSpring {
    position: f32,
    velocity: f32,
    target: f32,
    last_frame: Instant,
}

struct ProcessingInput {
    entity: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

struct ModeInputs {
    name: ProcessingInput,
    browser_hosts: ProcessingInput,
    prompt: ProcessingInput,
    model: ProcessingInput,
    deadline: ProcessingInput,
    processing_toggle: ToggleSpring,
    replacements: Vec<ReplacementInputs>,
}

struct ProcessingInputs {
    default_mode: ModeInputs,
    modes: Vec<ModeInputs>,
}

struct VoiceActionInputs {
    model: ProcessingInput,
    enabled_toggle: ToggleSpring,
    error: Option<String>,
}

struct ReplacementInputs {
    matched_phrase: ProcessingInput,
    output: ProcessingInput,
}

#[derive(Clone)]
struct TransformationDrag {
    selection: ModeSelection,
    id: String,
    name: String,
    position: Point<Pixels>,
}

impl TransformationDrag {
    fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for TransformationDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.position.x - px(90.0))
            .pt(self.position.y - px(17.0))
            .child(
                div()
                    .w(px(180.0))
                    .h(px(34.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(rgb(0x505050))
                    .bg(rgb(SURFACE_SELECTED))
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT))
                    .child(self.name.clone()),
            )
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ModeSelection {
    Default,
    Custom(usize),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ModelPickerTarget {
    Mode(ModeSelection),
    VoiceAction,
}

/// Shared parts of the OpenCode model picker rendered by every surface that
/// selects a model: Modes processing and Voice Action.
struct ProcessingPicker {
    catalog_status: String,
    model_control: AnyElement,
    refresh_control: Option<AnyElement>,
    suggestions: Option<Div>,
    has_variants: bool,
    variant_control: Div,
}

enum ModelCatalogState {
    Loading,
    Loaded(ModelCatalog),
    Missing,
    Failed(String),
}

struct OpenCodeUnavailableCopy<'a> {
    title: &'static str,
    description: &'static str,
    error: Option<&'a str>,
    can_retry: bool,
    retry_label: &'static str,
    can_open_setup: bool,
}

fn opencode_unavailable_copy(state: &ModelCatalogState) -> Option<OpenCodeUnavailableCopy<'_>> {
    match state {
        ModelCatalogState::Loading => Some(OpenCodeUnavailableCopy {
            title: "Checking for OpenCode",
            description: "Voice Action will be available after HEX connects to OpenCode.",
            error: None,
            can_retry: false,
            retry_label: "Retry",
            can_open_setup: false,
        }),
        ModelCatalogState::Missing => Some(OpenCodeUnavailableCopy {
            title: "Voice Action requires OpenCode",
            description: "Install and configure OpenCode to turn spoken instructions into paste-ready text.",
            error: None,
            can_retry: true,
            retry_label: "Check again",
            can_open_setup: true,
        }),
        ModelCatalogState::Failed(error) => Some(OpenCodeUnavailableCopy {
            title: "OpenCode is unavailable",
            description: "HEX could not load the OpenCode model catalog.",
            error: Some(error),
            can_retry: true,
            retry_label: "Retry",
            can_open_setup: false,
        }),
        ModelCatalogState::Loaded(_) => None,
    }
}

fn bounded_opencode_error(error: &str) -> String {
    if error.len() <= MAX_OPENCODE_ERROR_BYTES {
        return error.into();
    }
    let mut end = MAX_OPENCODE_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &error[..end])
}

fn start_model_catalog_load() -> Receiver<ModelCatalogState> {
    let (sender, receiver) = sync_channel(1);
    thread::spawn(move || {
        let state = if crate::dictation_processor::opencode_installed() {
            match crate::dictation_processor::load_model_catalog() {
                Ok(catalog) => ModelCatalogState::Loaded(catalog),
                Err(error) => {
                    let error = bounded_opencode_error(&error.to_string());
                    tracing::warn!(%error, "could not load OpenCode model catalog");
                    ModelCatalogState::Failed(error)
                }
            }
        } else {
            ModelCatalogState::Missing
        };
        let _ = sender.send(state);
    });
    receiver
}

struct ModelPresentation {
    name: String,
    provider: String,
    key: String,
    is_default: bool,
}

enum ApplicationCatalogState {
    Loading,
    Loaded(Vec<InstalledApplication>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HotkeyKind {
    Dictation,
    Edit,
    PasteLast,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MicrophonePolicyChange {
    SetCommands(bool),
    SetReleaseWhileIdle(bool),
    KeepReadyAndEnableCommands,
    DisableCommandsAndRelease,
}

enum HotkeyCaptureState {
    Idle,
    Listening {
        kind: HotkeyKind,
        modifiers: HotkeyModifiers,
        message: Option<&'static str>,
        started_at: Instant,
    },
    Saved {
        kind: HotkeyKind,
        saved_at: Instant,
    },
}

impl HotkeyCaptureState {
    fn is_listening(&self) -> bool {
        matches!(self, Self::Listening { .. })
    }
}

impl ToggleSpring {
    fn new(enabled: bool) -> Self {
        let position = if enabled { 1.0 } else { 0.0 };
        Self::at(position)
    }

    fn at(position: f32) -> Self {
        Self {
            position,
            velocity: 0.0,
            target: position,
            last_frame: Instant::now(),
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.set_target(if enabled { 1.0 } else { 0.0 });
    }

    fn set_target(&mut self, target: f32) {
        if self.target == target {
            return;
        }
        self.target = target;
        self.last_frame = Instant::now();
    }

    fn advance(&mut self, elapsed: Duration) {
        /// Critically damped spring stiffness in rad/s; higher settles faster.
        const STIFFNESS: f32 = 40.0;
        let mut remaining = elapsed.as_secs_f32().min(0.1);
        while remaining > 0.0 {
            let dt = remaining.min(1.0 / 240.0);
            let acceleration = -STIFFNESS.powi(2) * (self.position - self.target)
                - 2.0 * STIFFNESS * self.velocity;
            self.velocity += acceleration * dt;
            self.position += self.velocity * dt;
            remaining -= dt;
        }
        if self.is_settled() {
            self.position = self.target;
            self.velocity = 0.0;
        }
    }

    fn render_position(&mut self, window: &mut Window) -> f32 {
        let now = Instant::now();
        self.advance(now.duration_since(self.last_frame));
        self.last_frame = now;
        if !self.is_settled() {
            window.request_animation_frame();
        }
        self.position
    }

    fn is_settled(&self) -> bool {
        (self.position - self.target).abs() < 0.001 && self.velocity.abs() < 0.01
    }

    fn enabled(&self) -> bool {
        self.target >= 0.5
    }
}

/// Root GPUI entity for the production app shell.
pub struct AppWindow {
    preview: bool,
    pane: Pane,
    event_reader: EventReader,
    activity: DesktopActivity,
    meeting_requests: SyncSender<MeetingRequest>,
    indicator: DictationIndicatorSender,
    hud_tuning: HudTuning,
    hud_demo: HudDemoState,
    hud_queued: usize,
    hud_dragging: Option<HudControl>,
    hud_copied: bool,
    recognition_start: Option<SyncSender<()>>,
    setup_status: SetupStatus,
    setup_visible: bool,
    onboarding_completed: bool,
    permission_refresh_at: Instant,
    settings: AppSettings,
    settings_load_error: Option<String>,
    microphone_devices: Vec<String>,
    microphone_picker_open: bool,
    microphone_picker_error: Option<String>,
    launch_at_login_status: LoginItemStatus,
    launch_at_login_error: Option<String>,
    launch_at_login_toggle: ToggleSpring,
    update_status: crate::sparkle::UpdateStatus,
    update_status_changed_at: Instant,
    commands_toggle: ToggleSpring,
    command_model_status: CommandModelStatus,
    release_microphone_toggle: ToggleSpring,
    pending_microphone_policy: Option<MicrophonePolicyChange>,
    double_tap_toggle: ToggleSpring,
    double_tap_only_visibility: ToggleSpring,
    dock_icon_toggle: ToggleSpring,
    sound_volume_spring: ToggleSpring,
    recording_audio_spring: ToggleSpring,
    variant_picker_open: Option<ModelPickerTarget>,
    transcription_hints: ProcessingInput,
    transcription_picker_language: Option<String>,
    transcription_picker_error: Option<String>,
    transcription_downloading: Option<TranscriptionModelId>,
    transcription_download_receiver: Option<Receiver<Result<TranscriptionSelection, String>>>,
    transcription_download_cancel: Option<Arc<AtomicBool>>,
    transcription_download_progress: Option<Arc<AtomicU64>>,
    transcription_downloaded_bytes: u64,
    transcription_activation_started: Option<Instant>,
    transcription_preview_installed: Option<bool>,
    processing_inputs: ProcessingInputs,
    voice_action_inputs: VoiceActionInputs,
    selected_mode: ModeSelection,
    model_catalog: ModelCatalogState,
    model_catalog_receiver: Option<Receiver<ModelCatalogState>>,
    model_catalog_retry_at: Instant,
    application_catalog: ApplicationCatalogState,
    application_catalog_receiver: Option<Receiver<Vec<InstalledApplication>>>,
    application_search: ProcessingInput,
    application_picker_open: bool,
    application_picker_error: Option<String>,
    transformation_picker_open: bool,
    application_picker_highlight: usize,
    model_picker_highlight: usize,
    mode_delete_armed: bool,
    mode_context_menu: Option<Point<Pixels>>,
    settings_save_generation: u64,
    settings_dirty: bool,
    hotkey_capture: HotkeyCaptureState,
    hotkey_capture_animation: ToggleSpring,
    hotkey_width_spring: ToggleSpring,
    hotkey_side_animations: [ToggleSpring; 3],
    hotkey_side_selection_springs: [ToggleSpring; 3],
    window_focus: FocusHandle,
    hotkey_focus: FocusHandle,
    _hotkey_blur_subscription: Subscription,
    _activation_subscription: Subscription,
    meetings: Vec<MeetingManifest>,
    meetings_error: Option<String>,
    meeting_runtime_error: Option<String>,
    meeting_refresh_started_ms: Option<u64>,
    meeting_stop_pending: bool,
    selected_meeting: Option<String>,
    meeting_transcript: MeetingTranscript,
    meeting_transcript_id: Option<String>,
    meeting_transcript_reader: meeting::TranscriptReader,
    meeting_transcript_scroll: ScrollHandle,
    compiled_commands: Vec<CatalogCommand>,
    commands: Vec<CatalogCommand>,
    personal_commands_status: StatusSnapshot,
    personal_workspace: PathBuf,
    personal_workspace_receiver: Option<Receiver<Result<PathBuf, String>>>,
    personal_workspace_error: Option<String>,
    personal_commands_prompt_copied: bool,
    command_search: ProcessingInput,
    selected_command: Option<String>,
    events: Vec<VoiceEvent>,
    current_context: (Option<String>, Option<String>),
    events_error: Option<String>,
    selected_event: Option<usize>,
    history: Option<History>,
    history_search: ProcessingInput,
    history_entries: Vec<HistoryEntry>,
    selected_history: Option<u64>,
    history_error: Option<String>,
    history_clear_armed: bool,
    history_retention_open: bool,
    history_copied: Option<u64>,
}

impl AppWindow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        event_path: PathBuf,
        commands: CommandConfig,
        meeting_requests: SyncSender<MeetingRequest>,
        indicator: DictationIndicatorSender,
        recognition_start: Option<SyncSender<()>>,
        history: Option<History>,
        preview: Option<AppWindowPreview>,
        native_window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |window, cx| {
            loop {
                Timer::after(Duration::from_millis(500)).await;
                if window
                    .update(cx, |window, cx| {
                        let model_catalog_changed = window.poll_model_catalog();
                        window.retry_model_catalog();
                        let application_catalog_changed = window.poll_application_catalog();
                        let transcription_changed = window.poll_transcription_download();
                        let setup_changed = window.poll_setup(false);
                        let command_model_status = if window.preview {
                            window.command_model_status
                        } else {
                            crate::recognition::command_model_status()
                        };
                        let command_model_changed =
                            window.command_model_status != command_model_status;
                        window.command_model_status = command_model_status;
                        let login_item_changed = window.poll_login_item();
                        let personal_commands_changed = window.poll_personal_commands();
                        let personal_workspace_changed = window.poll_personal_workspace();
                        let history_changed = window.poll_history(cx);
                        let update_status = crate::sparkle::status();
                        let update_status_changed = window.update_status != update_status;
                        if update_status_changed {
                            window.update_status = update_status;
                            window.update_status_changed_at = Instant::now();
                        }
                        let update_confirmation_expired = window.update_status
                            == crate::sparkle::UpdateStatus::UpToDate
                            && window.update_status_changed_at.elapsed() >= Duration::from_secs(4);
                        if update_confirmation_expired {
                            window.update_status = crate::sparkle::UpdateStatus::Idle;
                            crate::sparkle::clear_confirmation();
                        }
                        if crate::DEVELOPER_FEATURES_ENABLED
                            && (window.meeting_refresh_started_ms.is_some()
                                || meeting::active(&window.meetings).is_some())
                        {
                            window.reload_meetings();
                            cx.notify();
                        }
                        if model_catalog_changed
                            || application_catalog_changed
                            || transcription_changed
                            || setup_changed
                            || command_model_changed
                            || login_item_changed
                            || personal_commands_changed
                            || personal_workspace_changed
                            || history_changed
                            || update_status_changed
                            || update_confirmation_expired
                        {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let preview_mode = preview.is_some();
        let preview_choices = crate::transcription_models::choices_for_runtime;
        let (settings, settings_load_error) = if let Some(preview) = &preview {
            let mut settings = AppSettings {
                commands_enabled: preview.command_model_missing,
                ..AppSettings::default()
            };
            settings.voice_action.enabled = preview.voice_action_enabled;
            if let Some((language, _)) = &preview.transcription_picker
                && let Some(choice) = preview_choices(language).first()
            {
                settings.transcription.model = choice.model.id;
                settings.transcription.language = language.clone();
            }
            if matches!(preview.pane, PreviewPane::Modes | PreviewPane::Replacements) {
                let mut mode = DictationMode {
                    name: "Work notes".into(),
                    applications: vec!["Zed".into()],
                    browser_hosts: vec!["github.com".into()],
                    replacements: vec![TextReplacement {
                        matched_phrase: "open code".into(),
                        output: "OpenCode".into(),
                    }],
                    transformations: vec!["smart-punctuation".into(), "normalize-links".into()],
                    ..DictationMode::default()
                };
                mode.post_processing.enabled = !preview.collapse_mode_processing;
                mode.post_processing.prompt =
                    "Tighten prose and preserve technical terminology.".into();
                mode.post_processing.model = Some("anthropic/claude-sonnet-4".into());
                mode.post_processing.variant = Some("medium".into());
                settings.dictation_processing.modes.push(mode);
            }
            (settings, None)
        } else {
            match AppSettings::load() {
                Ok(settings) => (settings, None),
                Err(error) => {
                    tracing::error!(%error, "could not load app settings");
                    (
                        AppSettings::default(),
                        Some(format!("Could not load app settings: {error:#}")),
                    )
                }
            }
        };
        let transcription_hints =
            Self::transcription_hints_input(&settings.transcription.recognition_hints, cx);
        let processing_inputs = Self::processing_inputs(&settings, cx);
        let voice_action_inputs = Self::voice_action_inputs(&settings, cx);
        let (model_catalog, model_catalog_receiver) = if preview
            .as_ref()
            .is_some_and(|preview| preview.opencode_unavailable)
        {
            (ModelCatalogState::Missing, None)
        } else if preview_mode {
            (
                ModelCatalogState::Loaded(ModelCatalog {
                    models: vec![ModelChoice {
                        key: "anthropic/claude-sonnet-4".into(),
                        name: "Claude Sonnet 4".into(),
                        provider: "Anthropic".into(),
                        variants: vec!["low".into(), "medium".into(), "high".into()],
                    }],
                    default_key: Some("anthropic/claude-sonnet-4".into()),
                    default_name: Some("Claude Sonnet 4".into()),
                }),
                None,
            )
        } else {
            (ModelCatalogState::Loading, Some(start_model_catalog_load()))
        };
        let application_catalog = if preview_mode {
            ApplicationCatalogState::Loaded(Vec::new())
        } else {
            ApplicationCatalogState::Loading
        };
        let application_search = Self::application_search_input(cx);
        let window_focus = cx.focus_handle();
        window_focus.focus(native_window);
        let hotkey_focus = cx.focus_handle();
        let hotkey_blur_subscription = cx.on_blur(&hotkey_focus, native_window, |this, _, cx| {
            this.cancel_hotkey_capture(cx)
        });
        let activation_subscription =
            cx.observe_window_activation(native_window, |this, window, cx| {
                if window.is_window_active() {
                    this.permission_refresh_at = Instant::now();
                    if this.poll_setup(true) {
                        cx.notify();
                    }
                }
            });
        let compiled_commands = commands
            .catalog()
            .into_iter()
            .map(|command| CatalogCommand {
                task: command_task(&command),
                scope: command.scope,
                phrase: command.phrase,
                aliases: command.aliases,
                description: command.description,
                id: command.id,
            })
            .collect::<Vec<_>>();
        let personal_workspace = if preview_mode {
            PathBuf::from("~/.config/hex")
        } else {
            crate::app_paths::personal_commands_workspace()
                .unwrap_or_else(|_| PathBuf::from("~/.config/hex"))
        };
        let mut personal_commands_status = if preview_mode {
            StatusSnapshot {
                transformations: vec![
                    StatusTransformation {
                        id: "smart-punctuation".into(),
                        name: "Smart punctuation".into(),
                        description: None,
                    },
                    StatusTransformation {
                        id: "normalize-links".into(),
                        name: "Normalize links".into(),
                        description: None,
                    },
                ],
                ..StatusSnapshot::default()
            }
        } else {
            crate::personal_commands::load_status().unwrap_or_else(|error| StatusSnapshot {
                host_state: HostState::Unavailable,
                last_reload_error: Some(error.to_string()),
                ..StatusSnapshot::default()
            })
        };
        crate::personal_commands::include_builtin_transformations(&mut personal_commands_status);
        let commands = merged_catalog(&compiled_commands, &personal_commands_status);
        let preview_picker = preview
            .as_ref()
            .and_then(|preview| preview.transcription_picker.as_ref());
        let preview_model_missing = preview
            .as_ref()
            .is_some_and(|preview| preview.model_missing);
        let preview_model_state = preview_picker.map(|(_, state)| *state);
        let preview_downloading = preview_picker.and_then(|(language, state)| {
            matches!(state, PreviewModelState::Downloading)
                .then(|| preview_choices(language)[0].model.id)
        });
        let selected_model = validate(&settings.transcription).ok();
        let setup_status = if preview.as_ref().is_some_and(|preview| preview.onboarding) {
            SetupStatus {
                microphone: PermissionState::NeedsRequest,
                input_monitoring: PermissionState::NeedsRequest,
                accessibility: PermissionState::NeedsRequest,
                transcription_model: false,
            }
        } else if preview
            .as_ref()
            .is_some_and(|preview| preview.permissions_missing)
        {
            SetupStatus {
                microphone: PermissionState::NeedsRequest,
                input_monitoring: PermissionState::NeedsSettings,
                accessibility: PermissionState::NeedsSettings,
                transcription_model: !preview_model_missing,
            }
        } else if preview_mode {
            SetupStatus {
                microphone: PermissionState::Ready,
                input_monitoring: PermissionState::Ready,
                accessibility: PermissionState::Ready,
                transcription_model: !preview_model_missing,
            }
        } else {
            crate::onboarding::status_with_transcription_model(selected_model.is_some_and(
                |model| {
                    crate::transcription_models::is_installed(
                        model,
                        &settings.transcription.language,
                    )
                },
            ))
        };
        let onboarding_completed = preview_mode || crate::onboarding::completion_recorded();
        let setup_visible = preview.as_ref().is_some_and(|preview| preview.onboarding)
            || !onboarding_completed && !setup_status.ready();
        let microphone_devices = crate::audio::input_device_names().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not list microphones for settings");
            Vec::new()
        });
        let (launch_at_login_status, launch_at_login_error) = if preview_mode {
            (LoginItemStatus::Disabled, None)
        } else {
            match crate::login_item::status() {
                Ok(status) => (status, None),
                Err(error) => (LoginItemStatus::Disabled, Some(error.to_string())),
            }
        };
        let history = if preview_mode {
            preview
                .as_ref()
                .is_some_and(|preview| matches!(preview.pane, PreviewPane::History))
                .then(preview_history)
                .flatten()
        } else {
            history
        };
        let history_search = Self::history_search_input(cx);
        let command_search = Self::command_search_input(cx);
        let mut window = Self {
            preview: preview_mode,
            pane: preview
                .as_ref()
                .map_or(Pane::default(), |preview| Pane::from_preview(preview.pane)),
            event_reader: EventReader::open(event_path),
            activity: DesktopActivity::default(),
            meeting_requests,
            indicator,
            hud_tuning: HudTuning::default(),
            hud_demo: HudDemoState::Transcribing,
            hud_queued: 1,
            hud_dragging: None,
            hud_copied: false,
            recognition_start,
            setup_status,
            setup_visible,
            onboarding_completed,
            permission_refresh_at: Instant::now() + PERMISSION_REFRESH_INTERVAL,
            microphone_devices,
            microphone_picker_open: false,
            microphone_picker_error: None,
            launch_at_login_status,
            launch_at_login_error,
            launch_at_login_toggle: ToggleSpring::new(
                launch_at_login_status == LoginItemStatus::Enabled,
            ),
            update_status: crate::sparkle::status(),
            update_status_changed_at: Instant::now(),
            commands_toggle: ToggleSpring::new(settings.commands_enabled),
            command_model_status: if preview
                .as_ref()
                .is_some_and(|preview| preview.command_model_missing)
            {
                CommandModelStatus::Failed
            } else if preview_mode {
                CommandModelStatus::Disabled
            } else {
                crate::recognition::command_model_status()
            },
            release_microphone_toggle: ToggleSpring::new(settings.release_microphone_while_idle),
            pending_microphone_policy: None,
            double_tap_toggle: ToggleSpring::new(settings.double_tap_lock),
            double_tap_only_visibility: ToggleSpring::new(
                settings.double_tap_lock && settings.dictation_hotkey.key.is_some(),
            ),
            dock_icon_toggle: ToggleSpring::new(settings.show_dock_icon),
            sound_volume_spring: ToggleSpring::at(sound_volume_index(&settings) as f32),
            recording_audio_spring: ToggleSpring::at(recording_audio_index(
                settings.recording_audio_behavior,
            ) as f32),
            variant_picker_open: None,
            transcription_hints,
            transcription_picker_language: preview_picker.map(|(language, _)| language.clone()),
            transcription_picker_error: matches!(
                preview_model_state,
                Some(PreviewModelState::Error)
            )
            .then(|| "Preview download failed while verifying the model checksum.".into()),
            transcription_downloading: preview_downloading,
            transcription_download_receiver: None,
            transcription_download_cancel: None,
            transcription_download_progress: None,
            transcription_downloaded_bytes: preview_downloading
                .and_then(|model| definition(model).download_bytes())
                .map_or(0, |bytes| bytes * 37 / 100),
            transcription_activation_started: None,
            transcription_preview_installed: match preview_model_state {
                Some(PreviewModelState::Installed) => Some(true),
                Some(
                    PreviewModelState::Missing
                    | PreviewModelState::Downloading
                    | PreviewModelState::Error,
                ) => Some(false),
                Some(PreviewModelState::Actual) | None => preview_model_missing.then_some(false),
            },
            processing_inputs,
            voice_action_inputs,
            selected_mode: if preview
                .as_ref()
                .is_some_and(|preview| preview.select_global_mode)
            {
                ModeSelection::Default
            } else if preview.as_ref().is_some_and(|preview| {
                matches!(preview.pane, PreviewPane::Modes | PreviewPane::Replacements)
            }) {
                ModeSelection::Custom(0)
            } else {
                ModeSelection::Default
            },
            model_catalog,
            model_catalog_receiver,
            model_catalog_retry_at: Instant::now() + OPENCODE_INSTALL_RETRY_INTERVAL,
            application_catalog,
            application_catalog_receiver: None,
            application_search,
            application_picker_open: false,
            application_picker_error: None,
            transformation_picker_open: preview
                .as_ref()
                .is_some_and(|preview| preview.open_transformation_picker),
            application_picker_highlight: 0,
            model_picker_highlight: 0,
            mode_delete_armed: false,
            mode_context_menu: None,
            settings_save_generation: 0,
            settings_dirty: false,
            hotkey_capture: HotkeyCaptureState::Idle,
            hotkey_capture_animation: ToggleSpring::new(false),
            hotkey_width_spring: ToggleSpring::at(HOTKEY_MIN_WIDTH),
            hotkey_side_animations: [
                ToggleSpring::new(standalone_modifier_side(&settings.dictation_hotkey).is_some()),
                ToggleSpring::new(standalone_modifier_side(&settings.edit_hotkey).is_some()),
                ToggleSpring::new(
                    settings
                        .paste_last_hotkey
                        .as_ref()
                        .and_then(standalone_modifier_side)
                        .is_some(),
                ),
            ],
            hotkey_side_selection_springs: [
                ToggleSpring::at(hotkey_side_index(
                    standalone_modifier_side(&settings.dictation_hotkey)
                        .unwrap_or(ModifierSide::Either),
                ) as f32),
                ToggleSpring::at(hotkey_side_index(
                    standalone_modifier_side(&settings.edit_hotkey).unwrap_or(ModifierSide::Either),
                ) as f32),
                ToggleSpring::at(hotkey_side_index(
                    settings
                        .paste_last_hotkey
                        .as_ref()
                        .and_then(standalone_modifier_side)
                        .unwrap_or(ModifierSide::Either),
                ) as f32),
            ],
            window_focus,
            hotkey_focus,
            _hotkey_blur_subscription: hotkey_blur_subscription,
            _activation_subscription: activation_subscription,
            settings,
            settings_load_error,
            meetings: Vec::new(),
            meetings_error: None,
            meeting_runtime_error: None,
            meeting_refresh_started_ms: None,
            meeting_stop_pending: false,
            selected_meeting: None,
            meeting_transcript: MeetingTranscript::Empty,
            meeting_transcript_id: None,
            meeting_transcript_reader: meeting::TranscriptReader::default(),
            meeting_transcript_scroll: ScrollHandle::new(),
            compiled_commands,
            commands,
            personal_commands_status,
            personal_workspace,
            personal_workspace_receiver: None,
            personal_workspace_error: None,
            personal_commands_prompt_copied: false,
            command_search,
            selected_command: None,
            events: Vec::new(),
            current_context: (None, None),
            events_error: None,
            selected_event: None,
            history,
            history_search,
            history_entries: Vec::new(),
            selected_history: None,
            history_error: None,
            history_clear_armed: false,
            history_retention_open: preview
                .as_ref()
                .is_some_and(|preview| preview.open_history_retention),
            history_copied: None,
        };
        if !window.preview {
            if crate::DEVELOPER_FEATURES_ENABLED {
                window.reload_meetings();
                if meeting::active(&window.meetings).is_some() {
                    window.pane = Pane::Meetings;
                }
            }
            window.reload_events();
        }
        window.reload_history(cx);
        if window.preview {
            window.selected_history = window.history_entries.first().map(|entry| entry.id);
        }
        if window.pane == Pane::Modes {
            window.ensure_application_catalog_load();
        }
        if window.pane == Pane::HudLab {
            window.apply_hud_lab();
        }
        window
    }

    /// Refreshes disk-backed meeting and activity data while retaining valid
    /// selections. This is also called whenever an existing shell is reopened.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.preview {
            if crate::DEVELOPER_FEATURES_ENABLED {
                self.reload_meetings();
            }
            self.reload_events();
        }
        self.reload_history(cx);
        if self.pane == Pane::Modes {
            self.ensure_application_catalog_load();
        }
        self.permission_refresh_at = Instant::now();
        self.poll_setup(true);
        let pane = self.pane.on_reopen(self.setup_status);
        if pane != self.pane {
            self.select_pane(pane, cx);
        }
        cx.notify();
    }

    fn select_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.pane = pane;
        self.mode_context_menu = None;
        self.history_retention_open = false;
        match pane {
            Pane::HudLab => self.apply_hud_lab(),
            Pane::Meetings => self.reload_meetings(),
            Pane::Commands | Pane::Activity => self.reload_events(),
            Pane::History => self.reload_history(cx),
            Pane::Modes => self.ensure_application_catalog_load(),
            Pane::VoiceAction => {}
            Pane::Settings => self.permission_refresh_at = Instant::now(),
        }
        cx.notify();
    }

    pub(crate) fn show_settings(&mut self, cx: &mut Context<Self>) {
        self.select_pane(Pane::Settings, cx);
    }

    pub(crate) fn developer_select_pane(
        &mut self,
        pane: crate::developer_control::DeveloperPane,
        cx: &mut Context<Self>,
    ) {
        self.select_pane(Pane::from_developer(pane), cx);
    }

    pub(crate) fn developer_pane(&self) -> crate::developer_control::DeveloperPane {
        self.pane.developer()
    }

    pub(crate) fn developer_set_commands_enabled(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.update_microphone_policy(MicrophonePolicyChange::SetCommands(enabled), cx)
            .map_err(|error| error.to_string())
    }

    fn update_microphone_policy(
        &mut self,
        change: MicrophonePolicyChange,
        cx: &mut Context<Self>,
    ) -> color_eyre::Result<()> {
        let mut candidate = self.settings.clone();
        match change {
            MicrophonePolicyChange::SetCommands(enabled) => {
                candidate.set_commands_enabled(enabled)?
            }
            MicrophonePolicyChange::SetReleaseWhileIdle(enabled) => {
                candidate.set_release_microphone_while_idle(enabled)?
            }
            MicrophonePolicyChange::KeepReadyAndEnableCommands => {
                candidate.keep_microphone_ready_and_enable_commands()
            }
            MicrophonePolicyChange::DisableCommandsAndRelease => {
                candidate.disable_commands_and_release_microphone()
            }
        }
        if !self.preview {
            candidate.save()?;
        }
        self.settings = candidate;
        self.commands_toggle
            .set_enabled(self.settings.commands_enabled);
        self.release_microphone_toggle
            .set_enabled(self.settings.release_microphone_while_idle);
        self.pending_microphone_policy = None;
        self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
        self.settings_dirty = false;
        self.settings_load_error = None;
        cx.notify();
        Ok(())
    }

    fn poll_model_catalog(&mut self) -> bool {
        let Some(receiver) = &self.model_catalog_receiver else {
            return false;
        };
        let Ok(result) = receiver.try_recv() else {
            return false;
        };
        self.model_catalog = result;
        self.model_catalog_receiver = None;
        true
    }

    fn retry_model_catalog(&mut self) {
        if !matches!(&self.model_catalog, ModelCatalogState::Missing)
            || self.model_catalog_receiver.is_some()
            || Instant::now() < self.model_catalog_retry_at
        {
            return;
        }
        self.model_catalog_retry_at = Instant::now() + OPENCODE_INSTALL_RETRY_INTERVAL;
        self.model_catalog_receiver = Some(start_model_catalog_load());
    }

    fn reload_model_catalog(&mut self) {
        if self.preview || self.model_catalog_receiver.is_some() {
            return;
        }
        self.model_catalog = ModelCatalogState::Loading;
        self.model_catalog_retry_at = Instant::now() + OPENCODE_INSTALL_RETRY_INTERVAL;
        self.model_catalog_receiver = Some(start_model_catalog_load());
    }

    fn opencode_available(&self) -> bool {
        matches!(&self.model_catalog, ModelCatalogState::Loaded(_))
    }

    fn poll_application_catalog(&mut self) -> bool {
        let Some(receiver) = &self.application_catalog_receiver else {
            return false;
        };
        let Ok(result) = receiver.try_recv() else {
            return false;
        };
        let mut applications = result;
        if let ApplicationCatalogState::Loaded(existing) = &self.application_catalog {
            for application in existing.iter().cloned() {
                crate::application_catalog::insert(&mut applications, application);
            }
        }
        self.application_catalog = ApplicationCatalogState::Loaded(applications);
        self.application_catalog_receiver = None;
        true
    }

    fn ensure_application_catalog_load(&mut self) {
        if !application_catalog_load_needed(
            self.preview,
            &self.application_catalog,
            self.application_catalog_receiver.is_some(),
        ) {
            return;
        }
        let (sender, receiver) = sync_channel(1);
        thread::spawn(move || {
            let _ = sender.send(crate::application_catalog::discover());
        });
        self.application_catalog_receiver = Some(receiver);
    }

    fn poll_transcription_download(&mut self) -> bool {
        let mut changed = false;
        if let Some(progress) = &self.transcription_download_progress {
            let downloaded_bytes = progress.load(Ordering::Relaxed);
            if downloaded_bytes != self.transcription_downloaded_bytes {
                self.transcription_downloaded_bytes = downloaded_bytes;
                changed = true;
            }
        }
        let Some(receiver) = &self.transcription_download_receiver else {
            return changed;
        };
        let Ok(result) = receiver.try_recv() else {
            return self.transcription_activation_started.is_some() || changed;
        };
        self.transcription_download_receiver = None;
        self.transcription_download_cancel = None;
        self.transcription_download_progress = None;
        self.transcription_downloaded_bytes = 0;
        self.transcription_downloading = None;
        self.transcription_activation_started = None;
        match result {
            Ok(selection) => self.apply_transcription_selection(selection),
            Err(error) => self.transcription_picker_error = Some(error),
        }
        true
    }

    fn poll_setup(&mut self, force: bool) -> bool {
        if self.preview
            || !force
                && (Instant::now() < self.permission_refresh_at
                    || !self.setup_visible
                        && self.recognition_start.is_none()
                        && self.pane != Pane::Settings
                        && self.setup_status.transcription_model)
        {
            return false;
        }
        self.permission_refresh_at = Instant::now() + PERMISSION_REFRESH_INTERVAL;
        let mut changed = false;
        let transcription_model = validate(&self.settings.transcription).is_ok_and(|model| {
            crate::transcription_models::is_installed(model, &self.settings.transcription.language)
        });
        let status = crate::onboarding::status_with_transcription_model(transcription_model);
        if status != self.setup_status {
            self.setup_status = status;
            changed = true;
        }
        if self.setup_status.ready() {
            if !self.onboarding_completed {
                if let Err(error) = crate::onboarding::record_completion() {
                    tracing::warn!(%error, "could not record onboarding completion");
                }
                self.onboarding_completed = true;
            }
            if let Some(start) = self.recognition_start.take() {
                let _ = start.try_send(());
            }
            if self.setup_visible {
                self.setup_visible = false;
                changed = true;
            }
        }
        changed
    }

    fn poll_login_item(&mut self) -> bool {
        if self.preview {
            return false;
        }
        let Ok(status) = crate::login_item::status() else {
            return false;
        };
        if status == self.launch_at_login_status {
            return false;
        }
        self.launch_at_login_status = status;
        self.launch_at_login_toggle
            .set_enabled(status == LoginItemStatus::Enabled);
        if status == LoginItemStatus::Enabled {
            self.launch_at_login_error = None;
        }
        true
    }

    fn poll_personal_commands(&mut self) -> bool {
        if self.preview {
            return false;
        }
        let Ok(mut status) = crate::personal_commands::load_status() else {
            return false;
        };
        crate::personal_commands::include_builtin_transformations(&mut status);
        if status == self.personal_commands_status {
            return false;
        }
        self.personal_commands_status = status;
        self.commands = merged_catalog(&self.compiled_commands, &self.personal_commands_status);
        if self
            .selected_command
            .as_ref()
            .is_some_and(|id| !self.commands.iter().any(|command| &command.id == id))
        {
            self.selected_command = None;
        }
        true
    }

    fn poll_personal_workspace(&mut self) -> bool {
        let Some(receiver) = &self.personal_workspace_receiver else {
            return false;
        };
        let Ok(result) = receiver.try_recv() else {
            return false;
        };
        self.personal_workspace_receiver = None;
        match result {
            Ok(workspace) => {
                self.personal_workspace = workspace;
                self.personal_workspace_error = None;
            }
            Err(error) => self.personal_workspace_error = Some(error),
        }
        true
    }

    fn provision_personal_workspace(&mut self) {
        if self.preview || self.personal_workspace_receiver.is_some() {
            return;
        }
        let (sender, receiver) = sync_channel(1);
        thread::spawn(move || {
            let result =
                crate::personal_commands::initialize_workspace().map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.personal_workspace_error = None;
        self.personal_workspace_receiver = Some(receiver);
    }

    fn apply_transcription_selection(&mut self, selection: TranscriptionSelection) {
        if self.preview {
            self.settings.transcription = selection;
            self.transcription_preview_installed = Some(true);
            self.setup_status.transcription_model = true;
            self.transcription_picker_language = None;
            self.transcription_picker_error = None;
            return;
        }
        let previous = std::mem::replace(&mut self.settings.transcription, selection);
        match self.settings.save() {
            Ok(()) => {
                self.permission_refresh_at = Instant::now();
                self.transcription_picker_language = None;
                self.transcription_picker_error = None;
                self.settings_load_error = None;
            }
            Err(error) => {
                self.settings.transcription = previous;
                self.transcription_picker_error = Some(error.to_string());
            }
        }
    }

    fn choose_transcription_model(
        &mut self,
        model: &'static ModelDefinition,
        language: String,
        cx: &mut Context<Self>,
    ) {
        self.cancel_transcription_download();
        let selection = TranscriptionSelection {
            model: model.id,
            language,
            recognition_hints: if model.supports_recognition_hints {
                self.transcription_hints.entity.read(cx).text().to_string()
            } else {
                String::new()
            },
        };
        self.transcription_picker_error = None;
        if self.preview {
            self.apply_transcription_selection(selection);
            return;
        }
        if transcription_selection_is_active(
            &self.settings.transcription,
            model,
            &selection.language,
            self.transcription_model_installed(model, &selection.language),
        ) {
            self.transcription_picker_language = None;
            return;
        }
        let (sender, receiver) = sync_channel(1);
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        self.transcription_downloading = Some(model.id);
        self.transcription_download_receiver = Some(receiver);
        self.transcription_download_cancel = Some(canceled.clone());
        self.transcription_download_progress = Some(progress.clone());
        self.transcription_downloaded_bytes = 0;
        self.transcription_activation_started = Some(Instant::now());
        thread::spawn(move || {
            let result = (|| {
                if matches!(model.runtime, ModelRuntime::Gguf(_)) {
                    crate::transcription_models::download_with_progress(
                        model, &canceled, &progress,
                    )?;
                }
                if canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                crate::transcription::Transcriber::load_selection(&selection).map(drop)?;
                Ok(selection)
            })()
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
    }

    fn cancel_transcription_download(&mut self) {
        if let Some(canceled) = self.transcription_download_cancel.take() {
            canceled.store(true, Ordering::Relaxed);
        }
        self.transcription_downloading = None;
        self.transcription_download_receiver = None;
        self.transcription_download_progress = None;
        self.transcription_downloaded_bytes = 0;
        self.transcription_activation_started = None;
    }

    fn transcription_model_installed(&self, model: &ModelDefinition, language: &str) -> bool {
        self.transcription_preview_installed
            .unwrap_or_else(|| crate::transcription_models::is_installed(model, language))
    }

    pub fn meeting_starting(&mut self, cx: &mut Context<Self>) {
        self.meeting_runtime_error = None;
        self.meeting_refresh_started_ms = Some(now_ms());
        self.meeting_stop_pending = false;
        self.pane = Pane::Meetings;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_started(&mut self, cx: &mut Context<Self>) {
        self.meeting_refresh_started_ms = None;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_updated(&mut self, cx: &mut Context<Self>) {
        self.meeting_stop_pending = true;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_finished(&mut self, cx: &mut Context<Self>) {
        self.meeting_refresh_started_ms = None;
        self.meeting_stop_pending = false;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_failed(&mut self, title: String, error: String, cx: &mut Context<Self>) {
        self.meeting_runtime_error = Some(format!("{title}: {error}"));
        self.meeting_refresh_started_ms = None;
        self.meeting_stop_pending = false;
        self.pane = Pane::Meetings;
        self.reload_meetings();
        if let Some(failed) = self
            .meetings
            .iter()
            .find(|meeting| meeting.status == MeetingStatus::Failed && meeting.title == title)
        {
            self.selected_meeting = Some(failed.id.clone());
            self.load_selected_meeting();
        }
        cx.notify();
    }

    fn reload_meetings(&mut self) {
        match meeting::list() {
            Ok(meetings) => {
                let previous_active = meeting::active(&self.meetings).map(|meeting| &meeting.id);
                let next_active = meeting::active(&meetings).map(|meeting| meeting.id.clone());
                if next_active.as_deref() != previous_active.map(String::as_str)
                    && next_active.is_some()
                {
                    self.selected_meeting = next_active;
                }
                if meeting::active(&meetings).is_none()
                    && (previous_active.is_some()
                        || self.meeting_refresh_started_ms.is_some_and(|started| {
                            meetings.iter().any(|meeting| {
                                meeting.created_at_ms >= started
                                    && matches!(
                                        meeting.status,
                                        MeetingStatus::Complete
                                            | MeetingStatus::Interrupted
                                            | MeetingStatus::Failed
                                    )
                            })
                        }))
                {
                    self.meeting_refresh_started_ms = None;
                }
                self.meetings = meetings;
                if meeting::active(&self.meetings)
                    .is_none_or(|meeting| meeting.status == MeetingStatus::Transcribing)
                {
                    self.meeting_stop_pending = false;
                }
                self.meetings_error = None;
                if self
                    .selected_meeting
                    .as_ref()
                    .is_some_and(|selected| !self.meetings.iter().any(|item| &item.id == selected))
                {
                    self.selected_meeting = None;
                    self.meeting_transcript = MeetingTranscript::Empty;
                } else if self.selected_meeting.is_some() {
                    self.load_selected_meeting();
                }
            }
            Err(error) => {
                tracing::error!(%error, "could not list meetings");
                self.meetings_error = Some(error.to_string());
            }
        }
    }

    fn load_selected_meeting(&mut self) {
        let Some(id) = self.selected_meeting.clone() else {
            self.meeting_transcript = MeetingTranscript::Empty;
            self.meeting_transcript_id = None;
            return;
        };
        let selection_changed = self.meeting_transcript_id.as_deref() != Some(&id);
        if selection_changed {
            self.meeting_transcript_id = Some(id.clone());
            self.meeting_transcript_scroll = ScrollHandle::new();
        }
        let previous_lines = match &self.meeting_transcript {
            MeetingTranscript::Loaded(lines) if !selection_changed => Some(lines.clone()),
            MeetingTranscript::Empty
            | MeetingTranscript::Loaded(_)
            | MeetingTranscript::Unavailable(_) => None,
        };
        let was_following = previous_lines.as_ref().is_none_or(|lines| {
            lines.is_empty()
                || self.meeting_transcript_scroll.max_offset().height
                    + self.meeting_transcript_scroll.offset().y
                    <= px(48.0)
        });
        self.meeting_transcript = match self.meeting_transcript_reader.read(&id) {
            Ok(entries) => MeetingTranscript::Loaded(
                meeting::coalesce_transcript(entries)
                    .into_iter()
                    .scan(None, |previous_end, entry| {
                        let pause_before_ms = previous_end
                            .map(|end| entry.start_ms.saturating_sub(end))
                            .filter(|pause| *pause >= 8_000);
                        *previous_end =
                            Some(previous_end.map_or(entry.end_ms, |end| end.max(entry.end_ms)));
                        Some(TranscriptLine {
                            source: format!(
                                "{} {}",
                                meeting::format_duration(entry.start_ms),
                                entry.source.label()
                            ),
                            text: entry.text,
                            pause_before_ms,
                        })
                    })
                    .collect(),
            ),
            Err(error) => MeetingTranscript::Unavailable(error.to_string()),
        };
        if !selection_changed
            && matches!(
                &self.meeting_transcript,
                MeetingTranscript::Loaded(lines) if was_following && previous_lines.as_ref() != Some(lines)
            )
        {
            self.meeting_transcript_scroll.scroll_to_bottom();
        }
    }

    fn reload_events(&mut self) {
        let selected = self
            .selected_event
            .and_then(|index| self.events.get(index))
            .and_then(|event| serde_json::to_string(event).ok());
        self.activity.refresh(&mut self.event_reader);
        self.events_error = self.activity.error.clone();
        if self.events_error.is_some() {
            return;
        }
        self.events = self.event_reader.recent(ACTIVITY_LIMIT);
        self.current_context = activity_context(&self.event_reader);
        self.selected_event = selected.and_then(|selected| {
            self.events.iter().position(|event| {
                serde_json::to_string(event).ok().as_deref() == Some(selected.as_str())
            })
        });
    }

    fn history_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, "Search history", ""));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.reload_history(cx);
            cx.notify();
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed],
        }
    }

    /// Refresh the bounded history snapshot for the current search query.
    fn reload_history(&mut self, cx: &App) {
        let Some(history) = &self.history else {
            self.history_entries.clear();
            self.selected_history = None;
            return;
        };
        let query = self.history_search.entity.read(cx).text().to_string();
        self.history_entries = history.search(&query);
        if self
            .selected_history
            .is_some_and(|id| !self.history_entries.iter().any(|entry| entry.id == id))
        {
            self.selected_history = None;
        }
    }

    /// Refresh the visible history while the pane is open so entries recorded
    /// by the recognition side appear without reopening the pane.
    fn poll_history(&mut self, cx: &App) -> bool {
        if self.preview || self.pane != Pane::History || self.history.is_none() {
            return false;
        }
        let previous = std::mem::take(&mut self.history_entries);
        self.reload_history(cx);
        self.history_entries != previous
    }

    fn set_history_retention(&mut self, retention: HistoryRetention, cx: &mut Context<Self>) {
        if self.settings.history_retention == retention {
            return;
        }
        self.settings.history_retention = retention;
        if let Some(history) = &self.history
            && let Err(error) = history.set_retention(retention)
        {
            self.history_error = Some(error.to_string());
        }
        self.save_settings(cx);
        self.reload_history(cx);
        cx.notify();
    }

    fn copy_history_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.history_entries.iter().find(|entry| entry.id == id) else {
            return;
        };
        match arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(entry.final_text.clone()))
        {
            Ok(()) => {
                self.history_copied = Some(id);
                self.history_error = None;
            }
            Err(error) => self.history_error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn delete_history_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(history) = &self.history {
            if let Err(error) = history.delete(id) {
                self.history_error = Some(error.to_string());
            }
            self.reload_history(cx);
        }
        cx.notify();
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        if !self.history_clear_armed {
            self.history_clear_armed = true;
            cx.notify();
            return;
        }
        self.history_clear_armed = false;
        if let Some(history) = &self.history {
            if let Err(error) = history.clear() {
                self.history_error = Some(error.to_string());
            }
            self.reload_history(cx);
        }
        cx.notify();
    }

    fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let retention = self.settings.history_retention;
        let search = div().w(px(220.0)).child(self.history_search.entity.clone());
        let retention_control = header_button(format!("Keep: {}", retention.label()))
            .id("history-retention")
            .on_click(cx.listener(|this, _, _, cx| {
                this.history_retention_open = true;
                cx.notify();
            }));
        let retention_control = div().relative().child(retention_control).when(
            self.history_retention_open,
            |control| {
                control.child(deferred(
                    div()
                        .id("history-retention-menu")
                        .absolute()
                        .top(px(36.0))
                        .right_0()
                        .w(px(160.0))
                        .p_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(LINE))
                        .bg(rgb(SURFACE))
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.history_retention_open = false;
                            cx.notify();
                        }))
                        .children(HistoryRetention::ALL.into_iter().enumerate().map(
                            |(index, choice)| {
                                compact_button(choice.label())
                                    .id(("history-retention-choice", index))
                                    .w_full()
                                    .when(choice == retention, |item| {
                                        item.bg(rgb(SURFACE_SELECTED))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.history_retention_open = false;
                                        this.set_history_retention(choice, cx);
                                        cx.notify();
                                    }))
                            },
                        )),
                ))
            },
        );
        let clear = header_button(if self.history_clear_armed {
            "Really clear all?"
        } else {
            "Clear all"
        })
        .id("history-clear")
        .when(self.history_clear_armed, |button| {
            button.text_color(rgb(0xff8a80))
        })
        .on_click(cx.listener(|this, _, _, cx| this.clear_history(cx)))
        .into_any_element();
        let header_action = div()
            .flex()
            .items_center()
            .gap_3()
            .child(search)
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
                let mut meta = entry.kind.label().to_string();
                if let Some(application) = &entry.application {
                    meta.push_str(" · ");
                    meta.push_str(application);
                }
                div()
                    .id(("history-entry", index))
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
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
                pane_body().p_5().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(
                            compact_panel()
                                .id("history-list")
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
                                        && self.history_error.is_none(),
                                    |list| list.child(empty_message("No dictations retained yet.")),
                                )
                                .when_some(self.history_error.clone(), |list, error| {
                                    list.child(error_message("History could not be loaded.", error))
                                })
                                .child(
                                    div()
                                        .w(px(PANE_LIST_WIDTH - 2.0))
                                        .flex()
                                        .flex_col()
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
        let action_button = |label: &'static str, id_suffix: &'static str| {
            div()
                .id(SharedString::from(format!("history-action-{id_suffix}")))
                .h(px(30.0))
                .px_3()
                .flex()
                .items_center()
                .rounded_sm()
                .bg(rgb(SURFACE))
                .text_size(px(12.0))
                .text_color(rgb(TEXT_SOFT))
                .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT)))
                .child(label)
        };
        let mut latency = format!(
            "{} ms audio · {} ms inference",
            entry.audio_ms, entry.inference_ms
        );
        if entry.total_ms > 0 {
            latency.push_str(&format!(" · {} ms total", entry.total_ms));
        }
        div()
            .id("history-detail")
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
                                    .child(entry.kind.label()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(FAINT))
                                    .child(event_age(entry.timestamp_ms)),
                            ),
                    )
                    .child(
                        div()
                            .pt_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                action_button(if copied { "Copied" } else { "Copy" }, "copy")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy_history_entry(id, cx)
                                    })),
                            )
                            .child(action_button("Delete", "delete").on_click(cx.listener(
                                move |this, _, _, cx| this.delete_history_entry(id, cx),
                            ))),
                    )
                    .child(
                        div()
                            .mt_5()
                            .pt_5()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .child(section_label("Final text"))
                            .child(
                                div()
                                    .pt_3()
                                    .text_size(px(12.0))
                                    .line_height(px(19.0))
                                    .text_color(rgb(TEXT))
                                    .child(entry.final_text.clone()),
                            ),
                    )
                    .when(show_raw, |detail| {
                        detail.child(
                            div()
                                .mt_5()
                                .pt_5()
                                .border_t_1()
                                .border_color(rgb(LINE))
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
                    })
                    .child(
                        div()
                            .mt_5()
                            .pt_5()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .when_some(entry.application.clone(), |detail, application| {
                                detail.child(detail_row("Application", application))
                            })
                            .when_some(entry.processing.clone(), |detail, processing| {
                                let mut summary = format!(
                                    "{} · {} ms",
                                    processing.profile, processing.latency_ms
                                );
                                if let Some(fallback) = &processing.fallback {
                                    summary.push_str(&format!(" · fell back: {fallback}"));
                                }
                                detail.child(detail_row("Processing", summary))
                            })
                            .child(detail_row("Latency", latency)),
                    ),
            )
            .into_any_element()
    }

    fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let items = Pane::all(self.capabilities())
            .into_iter()
            .enumerate()
            .map(|(index, pane)| {
                let selected = self.pane == pane;
                navigation_item(pane.icon(), selected)
                    .id(("app-nav", index))
                    .child(pane.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_pane(pane, cx);
                    }))
            });

        sidebar_frame()
            .w(px(SIDEBAR_WIDTH))
            .px(px(14.0))
            .pt(px(52.0))
            .pb_4()
            .flex()
            .flex_col()
            .child(div().flex().flex_col().gap(px(2.0)).children(items))
            .child(div().flex_1())
            .into_any_element()
    }

    fn render_hotkey_control(
        &mut self,
        kind: HotkeyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let binding = match kind {
            HotkeyKind::Dictation => &self.settings.dictation_hotkey,
            HotkeyKind::Edit => &self.settings.edit_hotkey,
            HotkeyKind::PasteLast => self
                .settings
                .paste_last_hotkey
                .as_ref()
                .unwrap_or(&self.settings.dictation_hotkey),
        };
        let binding_keycaps = match kind {
            HotkeyKind::Dictation => self.snapshot().dictation_shortcut,
            HotkeyKind::Edit => binding.keycaps(),
            HotkeyKind::PasteLast => self
                .settings
                .paste_last_hotkey
                .as_ref()
                .map_or_else(|| vec!["Off".into()], HotkeyBinding::keycaps),
        };
        let idle_width = hotkey_idle_width(binding_keycaps.len());
        if matches!(
            self.hotkey_capture,
            HotkeyCaptureState::Saved { saved_at, .. }
                if saved_at.elapsed() >= Duration::from_millis(700)
        ) {
            self.hotkey_capture = HotkeyCaptureState::Idle;
            self.hotkey_capture_animation.set_enabled(false);
            self.hotkey_width_spring.set_target(idle_width);
        } else if matches!(self.hotkey_capture, HotkeyCaptureState::Idle) {
            self.hotkey_width_spring.set_target(idle_width);
        }
        let intensity = self.hotkey_capture_animation.render_position(window);
        let animated_control_width = self.hotkey_width_spring.render_position(window);
        let this_capture = matches!(
            self.hotkey_capture,
            HotkeyCaptureState::Listening { kind: active, .. }
                | HotkeyCaptureState::Saved { kind: active, .. }
                if active == kind
        );
        let control_width = hotkey_control_width(this_capture, idle_width, animated_control_width);
        let capture_active = !matches!(self.hotkey_capture, HotkeyCaptureState::Idle);
        let control_intensity = hotkey_control_intensity(this_capture, intensity);
        let capture_color = if matches!(
            self.hotkey_capture,
            HotkeyCaptureState::Saved { kind: active, .. } if active == kind
        ) {
            rgb(0x1b2420)
        } else {
            rgb(0x251c1b)
        };
        let pulse = match &self.hotkey_capture {
            HotkeyCaptureState::Listening {
                kind: active,
                started_at,
                ..
            } if *active == kind => {
                window.request_animation_frame();
                (started_at.elapsed().as_secs_f32() * 4.5).sin() * 0.5 + 0.5
            }
            HotkeyCaptureState::Saved { kind: active, .. } if *active == kind => {
                window.request_animation_frame();
                1.0
            }
            HotkeyCaptureState::Idle
            | HotkeyCaptureState::Listening { .. }
            | HotkeyCaptureState::Saved { .. } => 0.0,
        };
        let content = match &self.hotkey_capture {
            HotkeyCaptureState::Listening {
                kind: active,
                modifiers,
                message,
                ..
            } if *active == kind => {
                let label = message.unwrap_or(if modifiers.is_empty() {
                    "Press shortcut"
                } else {
                    "Release to save or add key"
                });
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(5.0 + pulse * 3.0))
                            .ml_2()
                            .rounded_full()
                            .bg(rgb(NEGATIVE))
                            .opacity(0.6 + pulse * 0.4),
                    )
                    .when(!modifiers.is_empty() && message.is_none(), |content| {
                        content.child(hotkey_keycaps(
                            HotkeyBinding {
                                modifiers: *modifiers,
                                key: None,
                            }
                            .keycaps(),
                            0.85,
                        ))
                    })
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_SOFT))
                            .child(label),
                    )
                    .child(
                        div()
                            .id(match kind {
                                HotkeyKind::Dictation => "cancel-dictation-hotkey-capture",
                                HotkeyKind::Edit => "cancel-edit-hotkey-capture",
                                HotkeyKind::PasteLast => "cancel-paste-last-hotkey-capture",
                            })
                            .h(px(26.0))
                            .ml_1()
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(4.0))
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .hover(|button| {
                                button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT))
                            })
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.cancel_hotkey_capture(cx);
                            })),
                    )
                    .into_any_element()
            }
            HotkeyCaptureState::Saved { kind: active, .. } if *active == kind => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_SOFT))
                        .child("Saved"),
                )
                .child(hotkey_keycaps(binding_keycaps.clone(), 1.0))
                .into_any_element(),
            HotkeyCaptureState::Idle
            | HotkeyCaptureState::Listening { .. }
            | HotkeyCaptureState::Saved { .. } => div()
                .flex()
                .items_center()
                .gap_2()
                .child(hotkey_keycaps(binding_keycaps, 1.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(TEXT_SOFT))
                        .child("Change shortcut"),
                )
                .into_any_element(),
        };
        div()
            .id(match kind {
                HotkeyKind::Dictation => "dictation-hotkey-control",
                HotkeyKind::Edit => "edit-hotkey-control",
                HotkeyKind::PasteLast => "paste-last-hotkey-control",
            })
            .track_focus(&self.hotkey_focus)
            .w(px(control_width))
            .min_w(px(HOTKEY_MIN_WIDTH))
            .h(px(32.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(px(6.0))
            .border_1()
            .border_color(mix_color(
                rgb(LINE),
                rgb(TEXT_SOFT),
                control_intensity * 0.55,
            ))
            .bg(mix_color(
                rgb(CANVAS),
                capture_color,
                control_intensity * (0.55 + pulse * 0.15),
            ))
            .when(!capture_active, |control| {
                control.hover(|control| control.bg(rgb(SURFACE_HOVER)))
            })
            .child(content)
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.hotkey_capture.is_listening() && this_capture {
                    this.cancel_hotkey_capture(cx);
                } else {
                    this.begin_hotkey_capture(kind, window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.capture_hotkey_key(event, cx);
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.capture_hotkey_modifiers(event, cx);
            }))
            .into_any_element()
    }

    fn render_hotkey_setting_control(
        &mut self,
        kind: HotkeyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hotkey = self.render_hotkey_control(kind, window, cx);
        let side = self.hotkey_binding(kind).and_then(standalone_modifier_side);
        let side_animation = &mut self.hotkey_side_animations[hotkey_kind_index(kind)];
        side_animation.set_enabled(side.is_some());
        let side_position = side_animation.render_position(window).clamp(0.0, 1.0);
        let selected = side.unwrap_or(ModifierSide::Either);
        let side_widths = [34.0, 44.0, 34.0];
        let side_selection_spring =
            &mut self.hotkey_side_selection_springs[hotkey_kind_index(kind)];
        side_selection_spring.set_target(hotkey_side_index(selected) as f32);
        let selection_position = side_selection_spring.render_position(window);
        let (selection_left, selection_width) = segmented_geometry(selection_position, side_widths);
        let side_selector = div()
            .w(px(HOTKEY_SIDE_SELECTOR_WIDTH * side_position))
            .mr(px(8.0 * side_position))
            .flex_none()
            .overflow_hidden()
            .opacity(side_position)
            .child(
                segmented_control()
                    .w(px(HOTKEY_SIDE_SELECTOR_WIDTH))
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .left(px(selection_left))
                            .top(px(2.0))
                            .w(px(selection_width))
                            .h(px(26.0))
                            .rounded(px(4.0))
                            .bg(rgb(SURFACE_SELECTED)),
                    )
                    .children(
                        [
                            ("Left", ModifierSide::Left),
                            ("Either", ModifierSide::Either),
                            ("Right", ModifierSide::Right),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, side))| {
                            let candidate = hotkey_side_binding(&self.settings, kind, side);
                            segmented_item(selected == side)
                                .id(("hotkey-side", hotkey_kind_index(kind) * 3 + index))
                                .w(px(side_widths[index]))
                                .h(px(26.0))
                                .px(px(0.0))
                                .justify_center()
                                .bg(rgba(0x00000000))
                                .text_size(px(9.0))
                                .when(candidate.is_none(), |item| item.opacity(0.35))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let Some(candidate) =
                                        hotkey_side_binding(&this.settings, kind, side)
                                    else {
                                        return;
                                    };
                                    let Some(binding) = this.hotkey_binding_mut(kind) else {
                                        return;
                                    };
                                    *binding = candidate;
                                    if kind == HotkeyKind::Edit {
                                        this.voice_action_inputs.error = None;
                                    }
                                    this.hotkey_side_selection_springs[hotkey_kind_index(kind)]
                                        .set_target(index as f32);
                                    this.save_settings(cx);
                                }))
                        }),
                    ),
            );
        div()
            .flex()
            .items_center()
            .child(side_selector)
            .child(hotkey)
            .into_any_element()
    }

    fn hotkey_binding(&self, kind: HotkeyKind) -> Option<&HotkeyBinding> {
        match kind {
            HotkeyKind::Dictation => Some(&self.settings.dictation_hotkey),
            HotkeyKind::Edit => Some(&self.settings.edit_hotkey),
            HotkeyKind::PasteLast => self.settings.paste_last_hotkey.as_ref(),
        }
    }

    fn hotkey_binding_mut(&mut self, kind: HotkeyKind) -> Option<&mut HotkeyBinding> {
        match kind {
            HotkeyKind::Dictation => Some(&mut self.settings.dictation_hotkey),
            HotkeyKind::Edit => Some(&mut self.settings.edit_hotkey),
            HotkeyKind::PasteLast => self.settings.paste_last_hotkey.as_mut(),
        }
    }

    fn begin_hotkey_capture(
        &mut self,
        kind: HotkeyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_settings::set_hotkey_capture_active(true);
        self.hotkey_width_spring.set_target(hotkey_capture_width(0));
        self.hotkey_capture = HotkeyCaptureState::Listening {
            kind,
            modifiers: HotkeyModifiers::default(),
            message: None,
            started_at: Instant::now(),
        };
        self.hotkey_capture_animation.set_enabled(true);
        self.hotkey_focus.focus(window);
        cx.notify();
    }

    fn cancel_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.hotkey_capture, HotkeyCaptureState::Idle) {
            crate::app_settings::set_hotkey_capture_active(false);
            self.hotkey_capture = HotkeyCaptureState::Idle;
            self.hotkey_capture_animation.set_enabled(false);
            self.hotkey_width_spring.set_target(HOTKEY_MIN_WIDTH);
            cx.notify();
        }
    }

    fn capture_hotkey_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if !self.hotkey_capture.is_listening() {
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        if event.keystroke.key == "escape" {
            self.cancel_hotkey_capture(cx);
            return;
        }
        let modifiers = hotkey_modifiers(event.keystroke.modifiers).without_side_constraints();
        let key = match hotkey_key(&event.keystroke.key) {
            Ok(key) => key,
            Err(message) => {
                self.set_hotkey_capture_message(message, cx);
                return;
            }
        };
        if modifiers.is_empty() && !is_function_key(&key.label) {
            self.set_hotkey_capture_message("Add a modifier", cx);
            return;
        }
        let binding = HotkeyBinding {
            modifiers,
            key: Some(key),
        };
        self.hotkey_width_spring
            .set_target(hotkey_capture_width(binding.keycaps().len()));
        self.save_hotkey_binding(binding, cx);
    }

    fn capture_hotkey_modifiers(&mut self, event: &ModifiersChangedEvent, cx: &mut Context<Self>) {
        let current = hotkey_modifiers(event.modifiers);
        if !current.is_empty() {
            self.hotkey_width_spring.set_target(hotkey_capture_width(
                HotkeyBinding {
                    modifiers: current,
                    key: None,
                }
                .keycaps()
                .len(),
            ));
        }
        let released = {
            let HotkeyCaptureState::Listening {
                modifiers, message, ..
            } = &mut self.hotkey_capture
            else {
                return;
            };
            if !current.is_empty() {
                if current.count() >= modifiers.count() {
                    *modifiers = current;
                    *message = None;
                }
                cx.notify();
                None
            } else if message.is_some() {
                *modifiers = HotkeyModifiers::default();
                *message = None;
                cx.notify();
                None
            } else {
                (!modifiers.is_empty()).then_some(*modifiers)
            }
        };
        if let Some(modifiers) = released {
            let modifiers = if modifiers.count() == 1 {
                modifiers
            } else {
                modifiers.without_side_constraints()
            };
            self.save_hotkey_binding(
                HotkeyBinding {
                    modifiers,
                    key: None,
                },
                cx,
            );
        }
    }

    fn set_hotkey_capture_message(&mut self, next_message: &'static str, cx: &mut Context<Self>) {
        if let HotkeyCaptureState::Listening { message, .. } = &mut self.hotkey_capture {
            *message = Some(next_message);
            cx.notify();
        }
    }

    fn save_hotkey_binding(&mut self, binding: HotkeyBinding, cx: &mut Context<Self>) {
        if binding.is_empty() {
            self.set_hotkey_capture_message("Press a shortcut", cx);
            return;
        }
        let kind = match self.hotkey_capture {
            HotkeyCaptureState::Listening { kind, .. } => kind,
            HotkeyCaptureState::Idle | HotkeyCaptureState::Saved { .. } => return,
        };
        if hotkey_binding_conflicts(&self.settings, kind, &binding) {
            self.set_hotkey_capture_message("Already in use", cx);
            return;
        }
        let key_based = binding.key.is_some();
        self.hotkey_width_spring
            .set_target(hotkey_saved_width(binding.keycaps().len()));
        match kind {
            HotkeyKind::Dictation => self.settings.dictation_hotkey = binding,
            HotkeyKind::Edit => {
                self.settings.edit_hotkey = binding;
                self.voice_action_inputs.error = None;
            }
            HotkeyKind::PasteLast => self.settings.paste_last_hotkey = Some(binding),
        }
        if kind == HotkeyKind::Dictation && !key_based {
            self.settings.double_tap_only = false;
        }
        self.save_settings(cx);
        crate::app_settings::set_hotkey_capture_active(false);
        self.hotkey_capture = HotkeyCaptureState::Saved {
            kind,
            saved_at: Instant::now(),
        };
        cx.notify();
    }

    fn render_mode_replacements(
        &mut self,
        selection: ModeSelection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let replacement_count = self.mode_inputs_for(selection).replacements.len();
        let replacement_rows = self
            .mode_inputs_for(selection)
            .replacements
            .iter()
            .enumerate()
            .map(|(index, inputs)| {
                div()
                    .id(("text-replacement", index))
                    .w_full()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_end()
                    .gap_2()
                    .when(index + 1 < replacement_count, |row| {
                        row.border_b_1().border_color(rgb(LINE))
                    })
                    .child(compact_mode_field(
                        "Transcription",
                        inputs.matched_phrase.entity.clone(),
                    ))
                    .child(
                        div()
                            .h(px(34.0))
                            .flex()
                            .items_center()
                            .text_size(px(11.0))
                            .text_color(rgb(FAINT))
                            .child("→"),
                    )
                    .child(compact_mode_field("Output", inputs.output.entity.clone()))
                    .child(
                        compact_button("×")
                            .id(("remove-text-replacement", index))
                            .size(px(34.0))
                            .px_0()
                            .justify_center()
                            .flex_none()
                            .text_size(px(14.0))
                            .text_color(rgb(MUTED))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mode_inputs_mut(selection).replacements.remove(index);
                                this.mode_settings_mut(selection).replacements.remove(index);
                                this.save_settings(cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let add = compact_header_plus_button()
            .id("add-mode-replacement")
            .on_click(cx.listener(move |this, _, window, cx| {
                let replacement = TextReplacement::default();
                let inputs = Self::replacement_inputs(&replacement, cx);
                let focus = inputs.matched_phrase.entity.focus_handle(cx);
                this.mode_inputs_mut(selection).replacements.push(inputs);
                this.mode_settings_mut(selection)
                    .replacements
                    .push(replacement);
                this.save_settings(cx);
                focus.focus(window);
            }))
            .into_any_element();

        compact_panel()
            .child(compact_panel_header("Corrections", Some(add)))
            .when(replacement_count == 0, |panel| {
                panel.child(
                    div()
                        .px_3()
                        .py_3()
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child("No corrections in this mode."),
                )
            })
            .children(replacement_rows)
            .into_any_element()
    }

    fn transcription_picker_view(&mut self, selected_language: &str) -> TranscriptionPickerView {
        let models = crate::transcription_models::choices_for_runtime(selected_language)
            .into_iter()
            .map(|choice| {
                let model = choice.model;
                let installed = self.transcription_model_installed(model, selected_language);
                let downloading = self.transcription_downloading == Some(model.id);
                let active = transcription_selection_is_active(
                    &self.settings.transcription,
                    model,
                    selected_language,
                    installed,
                );
                let progress = if downloading {
                    model.download_bytes().map_or(0.0, |bytes| {
                        (self.transcription_downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
                    })
                } else {
                    0.0
                };
                let status = if downloading {
                    if installed {
                        let elapsed = self
                            .transcription_activation_started
                            .map_or(0, |started| started.elapsed().as_secs());
                        let progress = self
                            .transcription_activation_started
                            .map_or(0.0, |started| (started.elapsed().as_secs_f32() % 1.6) / 1.6);
                        TranscriptionPickerStatus::Preparing {
                            label: format!("Loading model · {elapsed}s"),
                            progress: Some(TranscriptionPickerProgress::Loading(progress)),
                        }
                    } else {
                        TranscriptionPickerStatus::Preparing {
                            label: format!("Downloading {:.0}%", progress * 100.0),
                            progress: matches!(model.runtime, ModelRuntime::Gguf(_))
                                .then_some(TranscriptionPickerProgress::Downloading(progress)),
                        }
                    }
                } else if active {
                    TranscriptionPickerStatus::Active
                } else {
                    TranscriptionPickerStatus::Available { installed }
                };
                TranscriptionPickerModel { choice, status }
            })
            .collect();
        TranscriptionPickerView {
            error: self.transcription_picker_error.clone(),
            language: selected_language.to_string(),
            models,
        }
    }

    fn render_transcription_picker(
        &mut self,
        selected_language: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        render_shared_transcription_picker(self.transcription_picker_view(selected_language), cx)
    }

    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>) {
        self.transcription_picker_language = None;
        self.transcription_picker_error = None;
        cx.notify();
    }

    fn render_microphone_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let choices = std::iter::once(None)
            .chain(self.microphone_devices.iter().cloned().map(Some))
            .collect::<Vec<_>>();
        div()
            .id("microphone-picker")
            .absolute()
            .top(px(170.0))
            .right(px(16.0))
            .w(px(220.0))
            .max_h(px(240.0))
            .p_2()
            .overflow_y_scroll()
            .rounded_sm()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .shadow_lg()
            .children(choices.into_iter().enumerate().map(|(index, device)| {
                let selected = self.settings.microphone == device;
                let label = device.clone().unwrap_or_else(|| "Automatic".into());
                div()
                    .id(("microphone-choice", index))
                    .w_full()
                    .h(px(34.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_sm()
                    .text_size(px(12.0))
                    .text_color(if selected { rgb(TEXT) } else { rgb(TEXT_SOFT) })
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.microphone = device.clone();
                        this.microphone_picker_open = false;
                        this.microphone_picker_error = None;
                        this.save_settings(cx);
                    }))
            }))
            .when_some(self.microphone_picker_error.clone(), |picker, error| {
                picker.child(
                    div()
                        .px_3()
                        .pt_2()
                        .text_size(px(11.0))
                        .text_color(rgb(NEGATIVE))
                        .child(error),
                )
            })
            .into_any_element()
    }

    fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preview {
            self.launch_at_login_status = if enabled {
                LoginItemStatus::Enabled
            } else {
                LoginItemStatus::Disabled
            };
            self.launch_at_login_toggle.set_enabled(enabled);
            cx.notify();
            return;
        }
        match crate::login_item::set_enabled(enabled) {
            Ok(status) => {
                self.launch_at_login_status = status;
                self.launch_at_login_error = None;
                self.launch_at_login_toggle
                    .set_enabled(status == LoginItemStatus::Enabled);
            }
            Err(error) => {
                self.launch_at_login_error = Some(error.to_string());
                if let Ok(status) = crate::login_item::status() {
                    self.launch_at_login_status = status;
                    self.launch_at_login_toggle
                        .set_enabled(status == LoginItemStatus::Enabled);
                }
            }
        }
        cx.notify();
    }

    fn render_permission_warnings(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let warnings = crate::onboarding::permission_warnings(self.setup_status);
        if self.setup_visible || warnings.is_empty() {
            return None;
        }
        Some(
            div()
                .child(settings_section_label("PERMISSIONS NEEDED"))
                .child(
                    settings_panel().children(warnings.into_iter().map(|warning| {
                        let (name, description) = permission_warning_copy(warning.kind);
                        let action = compact_button(permission_action_label(warning.action))
                            .id(permission_warning_id(warning.kind))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.perform_permission_action(warning);
                                cx.notify();
                            }));
                        settings_row(name, description, action)
                    })),
                )
                .into_any_element(),
        )
    }

    fn render_model_notice(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.setup_visible {
            return None;
        }
        let dictation_notice =
            dictation_model_notice(self.setup_status, self.transcription_downloading.is_some());
        let (title, description) = dictation_notice.or_else(|| {
            self.settings
                .commands_enabled
                .then(|| command_model_notice(&self.command_model_status))
                .flatten()
        })?;
        let action = if dictation_notice.is_some() {
            compact_button("Choose model")
                .id("choose-required-model")
                .bg(rgb(ACCENT))
                .text_color(rgb(TEXT))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.transcription_picker_language =
                        Some(this.settings.transcription.language.clone());
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .when(
                    matches!(self.command_model_status, CommandModelStatus::Failed),
                    |controls| {
                        controls.child(compact_button("Retry").id("retry-command-model").on_click(
                            cx.listener(|this, _, _, cx| {
                                if !this.preview {
                                    cx.dispatch_action(&RetryCommands);
                                }
                            }),
                        ))
                    },
                )
                .child(
                    compact_button("Turn off commands")
                        .id("disable-unavailable-commands")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Err(error) = this.developer_set_commands_enabled(false, cx) {
                                tracing::warn!(%error, "could not disable voice commands");
                            }
                        })),
                )
                .into_any_element()
        };
        Some(
            div()
                .id("dictation-model-notice")
                .w_full()
                .flex()
                .justify_center()
                .flex_shrink_0()
                .px_8()
                .py_4()
                .bg(rgb(SURFACE))
                .border_b_1()
                .border_color(rgb(LINE))
                .child(
                    div()
                        .w_full()
                        .max_w(px(PANE_CONTENT_WIDTH))
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .border_l_2()
                                .border_color(rgb(ACCENT))
                                .pl_3()
                                .child(settings_copy(title, description)),
                        )
                        .child(div().flex_shrink_0().child(action)),
                )
                .into_any_element(),
        )
    }

    fn perform_permission_action(&mut self, warning: PermissionWarning) {
        match warning.action {
            PermissionAction::RequestMicrophone => crate::onboarding::request_microphone(),
            PermissionAction::OpenMicrophoneSettings => {
                crate::onboarding::open_permission_settings("microphone");
            }
            PermissionAction::OpenInputMonitoringSettings => {
                crate::permission_guide::show_input_monitoring();
            }
            PermissionAction::OpenAccessibilitySettings => {
                crate::permission_guide::show_accessibility();
            }
        }
        self.permission_refresh_at = Instant::now() + PERMISSION_REFRESH_INTERVAL;
    }

    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let permission_warnings = self.render_permission_warnings(cx);
        let hotkey_control = self.render_hotkey_setting_control(HotkeyKind::Dictation, window, cx);
        let paste_last_control = self.render_hotkey_control(HotkeyKind::PasteLast, window, cx);
        let transcription_model = definition(self.settings.transcription.model);
        let transcription_label = format!(
            "{} · {}",
            language_name(&self.settings.transcription.language),
            transcription_model.name
        );
        let transcription_control = disclosure_button(transcription_label)
            .id("transcription-model-setting")
            .on_click(cx.listener(|this, _, _, cx| {
                this.transcription_picker_language =
                    Some(this.settings.transcription.language.clone());
                this.transcription_picker_error = None;
                cx.notify();
            }));
        let double_tap_position = self.double_tap_toggle.render_position(window);
        let release_microphone_position = self.release_microphone_toggle.render_position(window);
        self.double_tap_only_visibility.set_enabled(
            self.settings.double_tap_lock && self.settings.dictation_hotkey.key.is_some(),
        );
        let double_tap_only_visibility = self
            .double_tap_only_visibility
            .render_position(window)
            .clamp(0.0, 1.0);
        let dock_icon_position = self.dock_icon_toggle.render_position(window);
        let launch_at_login_position = self.launch_at_login_toggle.render_position(window);
        let microphone_label = self
            .settings
            .microphone
            .clone()
            .unwrap_or_else(|| "Automatic".into());
        let microphone_picker = self
            .microphone_picker_open
            .then(|| self.render_microphone_picker(cx));
        let sound_volume_position = self.sound_volume_spring.render_position(window);
        let sound_volume = segmented_control()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(2.0 + sound_volume_position * 34.0))
                    .top(px(2.0))
                    .w(px(34.0))
                    .h(px(26.0))
                    .rounded(px(4.0))
                    .bg(rgb(SURFACE_SELECTED)),
            )
            .children(
                [
                    ("Off", 0.0_f32),
                    ("25%", 0.25),
                    ("50%", 0.5),
                    ("75%", 0.75),
                    ("100%", 1.0),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (label, volume))| {
                    let selected = if volume == 0.0 {
                        !self.settings.sound_effects
                    } else {
                        self.settings.sound_effects
                            && (self.settings.sound_effect_volume - volume).abs() < 0.01
                    };
                    segmented_item(selected)
                        .id(("sound-volume", index))
                        .w(px(34.0))
                        .px(px(0.0))
                        .justify_center()
                        .text_size(px(9.0))
                        .bg(rgba(0x00000000))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings.sound_effects = volume > 0.0;
                            if volume > 0.0 {
                                this.settings.sound_effect_volume = volume;
                            }
                            this.sound_volume_spring.set_target(index as f32);
                            this.save_settings(cx);
                            if volume > 0.0 {
                                crate::feedback::play(crate::feedback::Tone::DictationStart);
                            }
                        }))
                }),
            );
        let launch_at_login_control =
            if self.launch_at_login_status == LoginItemStatus::RequiresApproval {
                compact_button("Open Settings")
                    .id("launch-at-login-approval")
                    .on_click(|_, _, _| crate::login_item::open_settings())
                    .into_any_element()
            } else {
                toggle(launch_at_login_position)
            };
        let recording_audio_position = self.recording_audio_spring.render_position(window);
        let audio_widths = [50.0, 90.0, 80.0];
        let (audio_left, audio_width) = segmented_geometry(recording_audio_position, audio_widths);
        let update_button_label = match self.update_status {
            crate::sparkle::UpdateStatus::Checking => "Checking…",
            crate::sparkle::UpdateStatus::UpToDate
                if self.update_status_changed_at.elapsed() < Duration::from_secs(4) =>
            {
                "Up to date"
            }
            crate::sparkle::UpdateStatus::UpdateAvailable => "Update available",
            crate::sparkle::UpdateStatus::Unavailable
            | crate::sparkle::UpdateStatus::Idle
            | crate::sparkle::UpdateStatus::UpToDate => "Check now",
        };
        let audio_behavior = segmented_control()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(audio_left))
                    .top(px(2.0))
                    .w(px(audio_width))
                    .h(px(26.0))
                    .rounded(px(4.0))
                    .bg(rgb(SURFACE_SELECTED)),
            )
            .children(
                [
                    RecordingAudioBehavior::ALL[1],
                    RecordingAudioBehavior::ALL[2],
                    RecordingAudioBehavior::ALL[0],
                ]
                .into_iter()
                .enumerate()
                .map(|(index, behavior)| {
                    let selected = self.settings.recording_audio_behavior == behavior;
                    segmented_item(selected)
                        .id(("recording-audio-behavior", index))
                        .w(px(audio_widths[index]))
                        .px(px(0.0))
                        .justify_center()
                        .bg(rgba(0x00000000))
                        .child(behavior.label())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.settings.recording_audio_behavior == behavior {
                                return;
                            }
                            this.settings.recording_audio_behavior = behavior;
                            this.recording_audio_spring.set_target(index as f32);
                            this.save_settings(cx);
                        }))
                }),
            );
        let release_microphone_control = if self.pending_microphone_policy
            == Some(MicrophonePolicyChange::DisableCommandsAndRelease)
        {
            compact_button("Disable Commands & Release Microphone")
                .id("confirm-release-microphone")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Err(error) = this.update_microphone_policy(
                        MicrophonePolicyChange::DisableCommandsAndRelease,
                        cx,
                    ) {
                        tracing::error!(%error, "could not update microphone policy");
                    }
                }))
                .into_any_element()
        } else {
            div()
                .id("release-microphone-while-idle")
                .child(toggle(release_microphone_position))
                .on_click(cx.listener(|this, _, _, cx| {
                    let enabled = !this.settings.release_microphone_while_idle;
                    if enabled && this.settings.commands_enabled {
                        this.pending_microphone_policy =
                            Some(MicrophonePolicyChange::DisableCommandsAndRelease);
                        cx.notify();
                        return;
                    }
                    if let Err(error) = this.update_microphone_policy(
                        MicrophonePolicyChange::SetReleaseWhileIdle(enabled),
                        cx,
                    ) {
                        tracing::error!(%error, "could not update microphone policy");
                    }
                }))
                .into_any_element()
        };
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
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(PANE_CONTENT_WIDTH))
                                    .relative()
                                    .children(permission_warnings)
                                    .child(settings_section_label("DICTATION"))
                                    .child(
                                        settings_panel()
                                    .child(settings_row(
                                        "Local transcription",
                                        "Language and on-device speech model",
                                        transcription_control,
                                    ))
                                    .child(settings_row(
                                        "Microphone",
                                        "Automatically chooses the preferred available input",
                                        disclosure_button(microphone_label)
                                            .id("microphone-setting")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.microphone_picker_open =
                                                    !this.microphone_picker_open;
                                                if this.microphone_picker_open {
                                                    match crate::audio::input_device_names() {
                                                        Ok(devices) => {
                                                            this.microphone_devices = devices;
                                                            this.microphone_picker_error = None;
                                                        }
                                                        Err(error) => {
                                                            this.microphone_picker_error =
                                                                Some(error.to_string());
                                                        }
                                                    }
                                                }
                                                cx.notify();
                                            })),
                                    ))
                                    .when(
                                        transcription_model.supports_recognition_hints,
                                        |settings| {
                                            settings.child(settings_row(
                                                "Recognition hints",
                                                "Names and terms to softly prime the speech model",
                                                div()
                                                    .w(px(320.0))
                                                    .h(px(76.0))
                                                    .child(
                                                        self.transcription_hints.entity.clone(),
                                                    ),
                                            ))
                                        },
                                    )
                                    .child(
                                        settings_row(
                                            "Dictation shortcut",
                                            "Hold to dictate, release to transcribe",
                                            hotkey_control,
                                        )
                                        .border_b_0()
                                        .id("dictation-hotkey-setting"),
                                    ),
                            )
                            .child(settings_section_label("BEHAVIOR"))
                            .child(
                                settings_panel()
                                    .child(
                                        settings_row(
                                            "While dictating",
                                            "Control other audio once a shortcut hold becomes intentional",
                                            audio_behavior,
                                        )
                                        .id("recording-audio-setting"),
                                    )
                                    .child(settings_row(
                                        "Release microphone while idle",
                                        "Adds first-capture latency and disables audio pre-roll",
                                        release_microphone_control,
                                    ))
                                    .child(
                                        settings_row(
                                            "Double-tap to lock",
                                            "Double-tap the shortcut for hands-free dictation",
                                            toggle(double_tap_position),
                                        )
                                        .border_b_0()
                                        .id("double-tap-setting")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let enabled = !this.snapshot().double_tap_lock;
                                            if let Err(error) = this.dispatch(
                                                DesktopAction::SetDoubleTapLock(enabled),
                                            ) {
                                                tracing::error!(%error, "could not update double-tap setting");
                                            }
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        div()
                                            .h(px(72.0 * double_tap_only_visibility))
                                            .overflow_hidden()
                                            .opacity(double_tap_only_visibility)
                                            .child(
                                                settings_row(
                                                    "Double-tap only",
                                                    "Wait for two complete shortcut taps before recording",
                                                    toggle(if self.settings.double_tap_only { 1.0 } else { 0.0 }),
                                                )
                                                .id("double-tap-only-setting")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let enabled = !this.snapshot().double_tap_only;
                                                    if let Err(error) = this.dispatch(
                                                        DesktopAction::SetDoubleTapOnly(enabled),
                                                    ) {
                                                        tracing::error!(%error, "could not update double-tap-only setting");
                                                    }
                                                    cx.notify();
                                                })),
                                            ),
                                    )
                                    .child(
                                        settings_row(
                                            "Paste last dictation",
                                            "Pastes the last ordinary dictation from this app session",
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(paste_last_control)
                                                .child(
                                                    compact_button("Disable")
                                                        .id("disable-paste-last-hotkey")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.settings.paste_last_hotkey = None;
                                                            this.save_settings(cx);
                                                        })),
                                                ),
                                        )
                                        .border_b_0()
                                        .id("paste-last-hotkey-setting"),
                                    )
                            )
                            .child(settings_section_label("APPLICATION"))
                            .child(
                                settings_panel()
                                    .child(settings_row(
                                        "HEX",
                                        if self.settings.commands_enabled {
                                            "Local voice commands and private dictation"
                                        } else {
                                            "Private local dictation for your Mac"
                                        },
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(rgb(MUTED))
                                            .child(format!(
                                                "Version {}",
                                                env!("CARGO_PKG_VERSION")
                                            )),
                                    ))
                                    .child(settings_row(
                                        "Software updates",
                                        "Check for signed updates automatically in the background",
                                        compact_button(update_button_label)
                                            .id("check-for-updates-setting")
                                            .h(px(32.0))
                                            .border_1()
                                            .border_color(rgb(LINE))
                                            .bg(rgb(CANVAS))
                                            .on_click(cx.listener(|_this, _, _, cx| {
                                                cx.dispatch_action(&CheckForUpdates);
                                                cx.notify();
                                            })),
                                    ))
                                    .child(
                                        settings_row(
                                            "Launch at login",
                                            "Start HEX automatically when you sign in to your Mac",
                                            launch_at_login_control,
                                        )
                                        .id("launch-at-login-setting")
                                        .when(
                                            self.launch_at_login_status
                                                != LoginItemStatus::RequiresApproval,
                                            |row| {
                                                row.on_click(cx.listener(
                                                    |this, _, _, cx| {
                                                        let enabled = this.launch_at_login_status
                                                            != LoginItemStatus::Enabled;
                                                        this.set_launch_at_login(enabled, cx);
                                                    },
                                                ))
                                            },
                                        ),
                                    )
                                    .when_some(
                                        self.launch_at_login_error.clone(),
                                        |settings, error| {
                                            settings.child(
                                                div()
                                                    .px_4()
                                                    .py_2()
                                                    .text_size(px(11.0))
                                                    .text_color(rgb(NEGATIVE))
                                                    .child(error),
                                            )
                                        },
                                    )
                                    .child(
                                        settings_row(
                                            "Show Dock icon",
                                            "Keep HEX visible in the Dock while it is running",
                                            toggle(dock_icon_position),
                                        )
                                        .id("dock-icon-setting")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.settings.show_dock_icon =
                                                !this.settings.show_dock_icon;
                                            this.dock_icon_toggle
                                                .set_enabled(this.settings.show_dock_icon);
                                            crate::app_settings::set_dock_icon_visible(
                                                this.settings.show_dock_icon,
                                            );
                                            if this.settings.show_dock_icon {
                                                cx.activate(true);
                                            }
                                            this.save_settings(cx);
                                        })),
                                    )
                                    .child(
                                        settings_row(
                                            "Sound volume",
                                            "Recording, cancellation, and error tones",
                                            sound_volume,
                                        )
                                        .border_b_0()
                                        .id("sound-effects-setting"),
                                    ),
                            )
                            .children(microphone_picker),
                    ),
            )
            )
            .into_any_element()
    }

    fn render_setup(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.setup_status;
        let microphone = match status.microphone {
            PermissionState::Ready => setup_ready_badge(),
            PermissionState::NeedsRequest => compact_button("Allow")
                .id("setup-microphone")
                .bg(rgb(SURFACE_SELECTED))
                .on_click(cx.listener(|_this, _, _, cx| {
                    crate::onboarding::request_microphone();
                    cx.notify();
                }))
                .into_any_element(),
            PermissionState::NeedsSettings => compact_button("Open Settings")
                .id("setup-microphone")
                .bg(rgb(SURFACE_SELECTED))
                .on_click(cx.listener(|_this, _, _, cx| {
                    crate::onboarding::open_permission_settings("microphone");
                    cx.notify();
                }))
                .into_any_element(),
        };
        let mut permission_rows = Vec::new();
        if status.microphone != PermissionState::Ready {
            permission_rows.push(setup_row(
                "Microphone",
                if self.settings.commands_enabled {
                    "Hear commands and dictation."
                } else {
                    "Record dictation while you hold the shortcut."
                },
                microphone,
            ));
        }
        if status.input_monitoring != PermissionState::Ready {
            let input_monitoring = compact_button("Grant Access")
                .id("setup-input-monitoring")
                .bg(rgb(SURFACE_SELECTED))
                .on_click(cx.listener(|_this, _, _, cx| {
                    crate::permission_guide::show_input_monitoring();
                    cx.notify();
                }))
                .into_any_element();
            permission_rows.push(setup_row(
                "Input Monitoring",
                "Recognize the dictation shortcut anywhere.",
                input_monitoring,
            ));
        }
        if status.accessibility != PermissionState::Ready {
            let accessibility = compact_button("Grant Access")
                .id("setup-accessibility")
                .bg(rgb(SURFACE_SELECTED))
                .on_click(cx.listener(|_this, _, _, cx| {
                    crate::permission_guide::show_accessibility();
                    cx.notify();
                }))
                .into_any_element();
            permission_rows.push(setup_row(
                "Accessibility",
                if self.settings.commands_enabled {
                    "Paste text and run keyboard commands."
                } else {
                    "Paste completed dictation into the foreground app."
                },
                accessibility,
            ));
        }
        let models_ready = status.transcription_model;
        let model_notice = dictation_model_notice(status, self.transcription_downloading.is_some());
        let models = if models_ready {
            setup_ready_badge()
        } else {
            compact_button("Choose model")
                .id("setup-models")
                .bg(rgb(TEXT))
                .text_color(rgb(CANVAS))
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.transcription_picker_language =
                        Some(this.settings.transcription.language.clone());
                    this.transcription_picker_error = None;
                    cx.notify();
                }))
                .into_any_element()
        };

        div()
            .id("setup-backdrop")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000dd))
            .child(
                div()
                    .id("setup")
                    .w(px(640.0))
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(CANVAS))
                    .child(
                        div()
                            .px_7()
                            .pt_7()
                            .pb_5()
                            .child(
                                div()
                                    .text_size(px(22.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Set up HEX"),
                            )
                            .child(
                                div()
                                    .pt_2()
                                    .text_size(px(12.0))
                                    .line_height(px(19.0))
                                    .text_color(rgb(MUTED))
                                    .child("Everything stays on this Mac. HEX is ready once permissions and your local dictation model are set up."),
                            ),
                    )
                    .when(!permission_rows.is_empty(), |setup| {
                        setup.child(
                            div()
                                .mx_7()
                                .pb_5()
                                .child(setup_group_label("PERMISSIONS NEEDED"))
                                .child(div().border_t_1().border_color(rgb(LINE)).children(permission_rows)),
                        )
                    })
                    .child(
                        div()
                            .mx_7()
                            .child(setup_group_label("LOCAL MODELS"))
                            .child(
                                div().border_t_1().border_color(rgb(LINE)).child(setup_row(
                                    if let Some((title, _)) = model_notice {
                                        title
                                    } else {
                                        "Local dictation model"
                                    },
                                    if let Some((_, description)) = model_notice {
                                        description
                                    } else {
                                        "The selected model transcribes speech entirely on this Mac."
                                    },
                                    models,
                                )),
                            ),
                    )
                    .when(crate::DEVELOPER_FEATURES_ENABLED, |setup| {
                        setup.child(
                            div()
                                .px_7()
                                .py_5()
                                .text_size(px(10.0))
                                .line_height(px(16.0))
                                .text_color(rgb(FAINT))
                                .child("Meeting recording requests Screen & System Audio Recording separately when you use it."),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_voice_action(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.voice_action.enabled;
        let compact = window.viewport_size().width < px(980.0);
        let (status, description) = voice_action_status_copy(enabled);
        let position = self
            .voice_action_inputs
            .enabled_toggle
            .render_position(window);
        let enabled_control = div()
            .id("voice-action-enabled")
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(status),
            )
            .child(toggle(position))
            .on_click(cx.listener(|this, _, _, cx| {
                this.cancel_hotkey_capture(cx);
                let enabled = !this.settings.voice_action.enabled;
                let result = set_voice_action_enabled(&mut this.settings, enabled)
                    .map_err(|message| color_eyre::eyre::eyre!(message))
                    .and_then(|()| this.persist_settings());
                match result {
                    Ok(()) => {
                        this.voice_action_inputs.enabled_toggle.set_enabled(enabled);
                        this.voice_action_inputs.error = None;
                    }
                    Err(error) => {
                        this.settings.voice_action.enabled = !enabled;
                        this.voice_action_inputs.error = Some(error.to_string());
                    }
                }
                cx.notify();
            }));
        let hotkey = self.render_hotkey_setting_control(HotkeyKind::Edit, window, cx);
        let processing = if !enabled {
            None
        } else if let Some(copy) = opencode_unavailable_copy(&self.model_catalog) {
            let error = copy.error.map(str::to_owned);
            let show_actions = copy.can_retry || copy.can_open_setup;
            let retry_label = copy.retry_label;
            let actions = div()
                .mt_4()
                .flex()
                .gap_2()
                .when(copy.can_retry, |actions| {
                    actions.child(header_button(retry_label).id("retry-opencode").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.reload_model_catalog();
                            cx.notify();
                        }),
                    ))
                })
                .when(copy.can_open_setup, |actions| {
                    actions.child(
                        header_button("Open OpenCode setup")
                            .id("open-opencode-setup")
                            .on_click(|_, _, _| open_opencode_beta_docs()),
                    )
                });
            Some(
                compact_panel()
                    .p_5()
                    .child(settings_copy(copy.title, copy.description))
                    .when_some(error, |card, error| {
                        card.child(
                            div()
                                .mt_3()
                                .rounded_sm()
                                .border_1()
                                .border_color(rgb(0x613b3b))
                                .bg(rgb(0x271b1b))
                                .child(error_message("OpenCode reported:", error)),
                        )
                    })
                    .when(show_actions, |card| card.child(actions))
                    .into_any_element(),
            )
        } else {
            Some(self.render_voice_action_processing(window, cx))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header("Voice Action"))
            .child(
                pane_body()
                    .px(if compact { px(20.0) } else { px(32.0) })
                    .py(px(22.0))
                    .child(
                        pane_content()
                            .id("voice-action-scroll")
                            .overflow_y_scroll()
                            .gap(px(18.0))
                            .child(
                                div()
                                    .px_1()
                                    .pt_1()
                                    .text_size(px(11.0))
                                    .line_height(px(17.0))
                                    .text_color(rgb(MUTED))
                                    .child(
                                        "Voice Action is an optional OpenCode feature, separate \
                                         from dictation. Speak an instruction, optionally with \
                                         text selected, and HEX pastes the model's reply at your \
                                         cursor. The default shortcut is Command-Option. Changing \
                                         your dictation shortcut does not change this shortcut.",
                                    ),
                            )
                            .child(compact_panel().child(voice_action_setting_row(
                                "Enable Voice Action",
                                description,
                                enabled_control,
                                false,
                                compact,
                            )))
                            .when_some(self.voice_action_inputs.error.clone(), |column, error| {
                                column.child(error_message("Could not update Voice Action:", error))
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(compact_section_label("CAPTURE"))
                                    .child(
                                        compact_panel().child(
                                            voice_action_setting_row(
                                                "Shortcut",
                                                if enabled {
                                                    "Hold to speak; selected text is included automatically."
                                                } else {
                                                    "Saved for when Voice Action is enabled."
                                                },
                                                hotkey,
                                                false,
                                                compact,
                                            )
                                            .id("voice-action-hotkey-setting"),
                                        ),
                                    ),
                            )
                            .when_some(processing, |column, processing| column.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(compact_section_label("PROCESSING"))
                                    .child(processing),
                            )),
                    ),
            )
            .into_any_element()
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.persist_settings() {
            tracing::error!(%error, "could not save app settings");
        }
        cx.notify();
    }

    fn persist_settings(&mut self) -> color_eyre::Result<()> {
        self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
        if self.preview {
            self.settings_dirty = false;
            return Ok(());
        }
        self.settings.save()?;
        self.settings_dirty = false;
        self.settings_load_error = None;
        Ok(())
    }

    fn processing_input(
        placeholder: &'static str,
        initial: &str,
        cx: &mut Context<Self>,
    ) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::new(cx, placeholder, initial));
        Self::synchronized_processing_input(entity, cx)
    }

    fn replacement_inputs(
        replacement: &TextReplacement,
        cx: &mut Context<Self>,
    ) -> ReplacementInputs {
        let matched_phrase =
            cx.new(|cx| TextInput::new(cx, "e.g. open code", &replacement.matched_phrase));
        let output = cx.new(|cx| TextInput::new(cx, "e.g. OpenCode", &replacement.output));
        let matched_subscription = cx.subscribe(&matched_phrase, |this, _, _: &TextChanged, cx| {
            this.sync_processing_settings(cx)
        });
        let output_subscription = cx.subscribe(&output, |this, _, _: &TextChanged, cx| {
            this.sync_processing_settings(cx)
        });
        ReplacementInputs {
            matched_phrase: ProcessingInput {
                entity: matched_phrase,
                _subscriptions: vec![matched_subscription],
            },
            output: ProcessingInput {
                entity: output,
                _subscriptions: vec![output_subscription],
            },
        }
    }

    fn transcription_hints_input(initial: &str, cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| {
            TextInput::multiline(
                cx,
                "Names and terms Whisper should expect, e.g. OpenCode, Effect...",
                initial,
            )
        });
        let subscription = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.settings.transcription.recognition_hints =
                this.transcription_hints.entity.read(cx).text().to_string();
            this.schedule_settings_save(cx);
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![subscription],
        }
    }

    fn voice_action_inputs(settings: &AppSettings, cx: &mut Context<Self>) -> VoiceActionInputs {
        let model = Self::voice_action_model_input(
            settings.voice_action.model.as_deref().unwrap_or_default(),
            cx,
        );
        VoiceActionInputs {
            model,
            enabled_toggle: ToggleSpring::new(settings.voice_action.enabled),
            error: None,
        }
    }

    fn synchronized_processing_input(
        entity: Entity<TextInput>,
        cx: &mut Context<Self>,
    ) -> ProcessingInput {
        let subscription = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.sync_processing_settings(cx)
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![subscription],
        }
    }

    fn model_input(initial: &str, cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, "Search available models", initial));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.model_picker_highlight = 0;
            cx.notify();
        });
        let navigate = cx.subscribe(&entity, |this, _, event: &TextNavigate, cx| {
            let count = this
                .model_choice_keys(ModelPickerTarget::Mode(this.selected_mode), cx)
                .len();
            if count > 0 {
                this.model_picker_highlight =
                    advance_highlight(this.model_picker_highlight, event.0, count);
                cx.notify();
            }
        });
        let submitted = cx.subscribe(&entity, |this, _, _: &TextSubmitted, cx| {
            if let Some(model) = this
                .model_choice_keys(ModelPickerTarget::Mode(this.selected_mode), cx)
                .get(this.model_picker_highlight)
                .cloned()
            {
                this.select_model(ModelPickerTarget::Mode(this.selected_mode), model, cx);
            }
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed, navigate, submitted],
        }
    }

    fn voice_action_model_input(initial: &str, cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, "Search available models", initial));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.model_picker_highlight = 0;
            cx.notify();
        });
        let navigate = cx.subscribe(&entity, |this, _, event: &TextNavigate, cx| {
            let count = this
                .model_choice_keys(ModelPickerTarget::VoiceAction, cx)
                .len();
            if count > 0 {
                this.model_picker_highlight =
                    advance_highlight(this.model_picker_highlight, event.0, count);
                cx.notify();
            }
        });
        let submitted = cx.subscribe(&entity, |this, _, _: &TextSubmitted, cx| {
            if let Some(model) = this
                .model_choice_keys(ModelPickerTarget::VoiceAction, cx)
                .get(this.model_picker_highlight)
                .cloned()
            {
                this.select_model(ModelPickerTarget::VoiceAction, model, cx);
            }
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed, navigate, submitted],
        }
    }

    fn application_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, "Search applications", ""));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.application_picker_highlight = 0;
            cx.notify();
        });
        let navigate = cx.subscribe(&entity, |this, _, event: &TextNavigate, cx| {
            let count = this.application_choice_names(cx).len();
            if count > 0 {
                this.application_picker_highlight =
                    advance_highlight(this.application_picker_highlight, event.0, count);
                cx.notify();
            }
        });
        let submitted = cx.subscribe(&entity, |this, _, _: &TextSubmitted, cx| {
            if let Some(name) = this
                .application_choice_names(cx)
                .get(this.application_picker_highlight)
                .cloned()
            {
                this.add_mode_application(this.selected_mode, name, cx);
            }
        });
        let dismissed = cx.subscribe(&entity, |this, _, _: &TextDismissed, cx| {
            this.application_picker_open = false;
            this.application_picker_highlight = 0;
            this.application_search
                .entity
                .update(cx, |input, cx| input.set_text("", cx));
            cx.notify();
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed, navigate, submitted, dismissed],
        }
    }

    fn multiline_processing_input(
        placeholder: &'static str,
        initial: &str,
        cx: &mut Context<Self>,
    ) -> ProcessingInput {
        let entity = cx.new(|cx| {
            TextInput::multiline_with_height(
                cx,
                placeholder,
                initial,
                px(COMPACT_MULTILINE_INPUT_HEIGHT),
            )
        });
        Self::synchronized_processing_input(entity, cx)
    }

    fn mode_inputs(mode: &DictationMode, cx: &mut Context<Self>) -> ModeInputs {
        let processing = &mode.post_processing;
        ModeInputs {
            name: Self::processing_input("Mode name", &mode.name, cx),
            browser_hosts: Self::processing_input(
                "github.com, x.com",
                &mode.browser_hosts.join(", "),
                cx,
            ),
            prompt: Self::multiline_processing_input(
                "Describe how OpenCode should rewrite your dictation...",
                &processing.prompt,
                cx,
            ),
            model: Self::model_input(processing.model.as_deref().unwrap_or_default(), cx),
            deadline: Self::processing_input("30", &processing.deadline_seconds.to_string(), cx),
            processing_toggle: ToggleSpring::new(processing.enabled),
            replacements: mode
                .replacements
                .iter()
                .map(|replacement| Self::replacement_inputs(replacement, cx))
                .collect(),
        }
    }

    fn processing_inputs(settings: &AppSettings, cx: &mut Context<Self>) -> ProcessingInputs {
        let processing = &settings.dictation_processing;
        ProcessingInputs {
            default_mode: Self::mode_inputs(&processing.default_mode, cx),
            modes: processing
                .modes
                .iter()
                .map(|mode| Self::mode_inputs(mode, cx))
                .collect(),
        }
    }

    fn sync_processing_settings(&mut self, cx: &mut Context<Self>) {
        apply_mode_inputs(
            &self.processing_inputs.default_mode,
            &mut self.settings.dictation_processing.default_mode,
            true,
            cx,
        );
        for (inputs, mode) in self
            .processing_inputs
            .modes
            .iter()
            .zip(&mut self.settings.dictation_processing.modes)
        {
            apply_mode_inputs(inputs, mode, false, cx);
        }
        self.schedule_settings_save(cx);
    }

    fn schedule_settings_save(&mut self, cx: &mut Context<Self>) {
        self.settings_dirty = true;
        self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
        let generation = self.settings_save_generation;
        cx.spawn(async move |window, cx| {
            Timer::after(Duration::from_millis(250)).await;
            let _ = window.update(cx, |window, cx| {
                if window.settings_save_generation == generation {
                    window.save_settings(cx);
                }
            });
        })
        .detach();
    }

    fn render_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let compact = window.viewport_size().width < px(980.0);
        let mode_list_width = if compact { 196.0 } else { 220.0 };
        let add = compact_header_plus_button()
            .id("add-dictation-mode")
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(SURFACE))
            .on_click(cx.listener(|this, _, window, cx| {
                let mut mode = this.settings.dictation_processing.default_mode.clone();
                mode.name = format!("Mode {}", this.processing_inputs.modes.len() + 1);
                mode.applications.clear();
                mode.browser_hosts.clear();
                let mode = DictationMode { ..mode };
                this.processing_inputs
                    .modes
                    .push(Self::mode_inputs(&mode, cx));
                this.settings.dictation_processing.modes.push(mode);
                let selection =
                    ModeSelection::Custom(this.processing_inputs.modes.len().saturating_sub(1));
                this.select_mode(selection, window, cx);
                this.sync_processing_settings(cx);
            }));
        let default_selected = self.selected_mode == ModeSelection::Default;
        let mut rows = vec![
            mode_row(
                "Global",
                "Fallback for everything",
                default_selected,
                &[],
                &[],
                &self.application_catalog,
            )
            .id("default-dictation-mode")
            .on_click(cx.listener(|this, _, window, cx| {
                this.select_mode(ModeSelection::Default, window, cx);
            }))
            .into_any_element(),
        ];
        rows.extend(
            self.processing_inputs
                .modes
                .iter()
                .enumerate()
                .map(|(index, inputs)| {
                    let name = input_text(&inputs.name, cx);
                    let mode = &self.settings.dictation_processing.modes[index];
                    let selected = self.selected_mode == ModeSelection::Custom(index);
                    mode_row(
                        name,
                        "No activation rules",
                        selected,
                        &mode.applications,
                        &mode.browser_hosts,
                        &self.application_catalog,
                    )
                    .id(("dictation-mode", index))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.select_mode(ModeSelection::Custom(index), window, cx);
                            this.mode_context_menu = Some(event.position);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_mode(ModeSelection::Custom(index), window, cx);
                    }))
                    .into_any_element()
                })
                .collect::<Vec<_>>(),
        );
        let detail = self.render_mode_detail(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action(
                "Modes",
                Some(add.into_any_element()),
            ))
            .child(
                pane_body().p_5().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(
                            div()
                                .id("modes-list")
                                .w(px(mode_list_width))
                                .h_full()
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    compact_panel()
                                        .id("mode-list-card")
                                        .overflow_y_scroll()
                                        .child(
                                            div().p_2().flex().flex_col().gap_1().children(rows),
                                        ),
                                ),
                        )
                        .child(detail),
                ),
            )
            .into_any_element()
    }

    fn render_mode_context_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.mode_context_menu?;
        Some(
            div()
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(148.0))
                .p_1()
                .rounded(px(8.0))
                .border_1()
                .border_color(rgb(0x444444))
                .bg(rgb(0x252525))
                .child(
                    div()
                        .id("context-delete-mode")
                        .h(px(30.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .text_size(px(11.0))
                        .text_color(rgb(NEGATIVE))
                        .hover(|item| item.bg(rgb(SURFACE_HOVER)))
                        .child("Delete mode…")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.mode_delete_armed = true;
                                this.mode_context_menu = None;
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_mode_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selection = self.selected_mode;
        let is_default = selection == ModeSelection::Default;
        let (name, browser_hosts, prompt, model, deadline, processing_position) = {
            let inputs = self.selected_mode_inputs_mut();
            (
                inputs.name.entity.clone(),
                inputs.browser_hosts.entity.clone(),
                inputs.prompt.entity.clone(),
                inputs.model.entity.clone(),
                inputs.deadline.entity.clone(),
                inputs.processing_toggle.render_position(window),
            )
        };
        let processing_enabled = self.selected_mode_settings().post_processing.enabled;
        let processing_can_toggle = processing_enabled || self.opencode_available();
        let processing_unavailable =
            opencode_unavailable_copy(&self.model_catalog).map(|mut copy| {
                if matches!(self.model_catalog, ModelCatalogState::Loading) {
                    copy.description =
                        "OpenCode transformation will be available after HEX connects to OpenCode.";
                } else if matches!(self.model_catalog, ModelCatalogState::Missing) {
                    copy.title = "Transformation requires OpenCode";
                    copy.description = "Install and configure OpenCode to transform dictated text.";
                }
                (
                    copy.title,
                    copy.description,
                    copy.error.map(str::to_owned),
                    copy.can_retry,
                    copy.retry_label,
                    copy.can_open_setup,
                )
            });
        let corrections = self.render_mode_replacements(selection, cx);
        let transformations = self.render_mode_transformations(selection, cx);
        let application_picker =
            (!is_default).then(|| self.render_application_picker(selection, cx));
        let basics = if is_default {
            compact_panel()
                .child(
                    div()
                        .min_h(px(64.0))
                        .px_3()
                        .py_2()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(settings_copy(
                            "Global",
                            "Used unless a more specific mode matches.",
                        )),
                )
                .into_any_element()
        } else {
            compact_panel()
                .child(
                    div()
                        .min_h(px(54.0))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child("Name"),
                        )
                        .child(div().w(px(300.0)).flex_none().child(name)),
                )
                .when_some(application_picker, |panel, picker| {
                    panel.child(div().border_b_1().border_color(rgb(LINE)).child(picker))
                })
                .child(
                    div()
                        .min_h(px(64.0))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(settings_copy(
                            "Web pages",
                            "Switch when a domain is active in Brave",
                        ))
                        .child(div().max_w(px(300.0)).flex_1().child(browser_hosts)),
                )
                .into_any_element()
        };

        let processing_toggle = div()
            .id("mode-processing-toggle")
            .h(px(28.0))
            .flex()
            .items_center()
            .child(toggle(processing_position))
            .when(!processing_can_toggle, |control| control.opacity(0.42))
            .when(processing_can_toggle, |control| {
                control.on_click(cx.listener(move |this, _, _, cx| {
                    let enabled = !this.selected_mode_settings().post_processing.enabled;
                    this.selected_mode_inputs_mut()
                        .processing_toggle
                        .set_enabled(enabled);
                    this.selected_mode_settings_mut().post_processing.enabled = enabled;
                    this.save_settings(cx);
                }))
            })
            .into_any_element();
        let processing_settings = processing_enabled.then(|| {
            self.render_processing_settings(
                ModelPickerTarget::Mode(selection),
                Some(prompt),
                model,
                deadline,
                window,
                cx,
            )
        });
        let processing_unavailable = processing_unavailable.map(
            |(title, description, error, can_retry, retry_label, can_open_setup)| {
                let actions = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(can_retry, |actions| {
                        actions.child(
                            header_button(retry_label)
                                .id("retry-mode-opencode")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.reload_model_catalog();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(can_open_setup, |actions| {
                        actions.child(
                            header_button("Open setup")
                                .id("open-mode-opencode-setup")
                                .on_click(|_, _, _| open_opencode_beta_docs()),
                        )
                    });
                div()
                    .px_3()
                    .py_3()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_SOFT))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .line_height(px(16.0))
                                    .text_color(rgb(if error.is_some() { NEGATIVE } else { FAINT }))
                                    .child(error.unwrap_or_else(|| description.into())),
                            ),
                    )
                    .when(can_retry || can_open_setup, |notice| notice.child(actions))
            },
        );
        let processing = compact_panel()
            .child(
                compact_panel_header("OpenCode transformation", Some(processing_toggle)).when(
                    !processing_enabled && processing_unavailable.is_none(),
                    |header| header.border_b_0(),
                ),
            )
            .when_some(processing_settings, |panel, settings| {
                panel.child(div().px_3().pb_3().child(settings))
            })
            .when_some(processing_unavailable, |panel, unavailable| {
                panel.child(unavailable)
            })
            .into_any_element();

        let remove = (!is_default).then(|| {
            let action =
                if self.mode_delete_armed {
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(compact_button("Cancel").id("cancel-remove-mode").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.mode_delete_armed = false;
                                cx.notify();
                            }),
                        ))
                        .child(
                            compact_button("Confirm delete")
                                .id("confirm-remove-mode")
                                .border_1()
                                .border_color(rgb(0x613b3b))
                                .bg(rgb(0x271b1b))
                                .text_color(rgb(NEGATIVE))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let ModeSelection::Custom(index) = this.selected_mode else {
                                        return;
                                    };
                                    if index < this.processing_inputs.modes.len() {
                                        this.processing_inputs.modes.remove(index);
                                        this.settings.dictation_processing.modes.remove(index);
                                        this.selected_mode = ModeSelection::Default;
                                        this.mode_delete_armed = false;
                                        this.save_settings(cx);
                                    }
                                })),
                        )
                        .into_any_element()
                } else {
                    compact_button("Delete mode")
                        .id("remove-selected-mode")
                        .border_1()
                        .border_color(rgb(0x613b3b))
                        .bg(rgb(0x271b1b))
                        .text_color(rgb(NEGATIVE))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mode_delete_armed = true;
                            cx.notify();
                        }))
                        .into_any_element()
                };
            div()
                .pt_3()
                .flex()
                .items_center()
                .justify_between()
                .border_t_1()
                .border_color(rgb(LINE))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(FAINT))
                        .child("This cannot be undone."),
                )
                .child(action)
                .into_any_element()
        });

        div()
            .id("mode-detail")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(700.0))
                    .flex()
                    .flex_col()
                    .gap(px(SECTION_GAP))
                    .pb_5()
                    .child(basics)
                    .child(div().h(px(4.0)))
                    .child(compact_section_label("TEXT PROCESSING"))
                    .child(corrections)
                    .child(processing)
                    .child(transformations)
                    .when_some(remove, |detail, remove| {
                        detail.child(div().pt_1().child(remove))
                    }),
            )
            .into_any_element()
    }

    fn render_mode_transformations(
        &self,
        selection: ModeSelection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.mode_settings(selection).transformations.clone();
        let catalog = &self.personal_commands_status.transformations;
        let selected_rows = selected
            .iter()
            .enumerate()
            .map(|(target_index, id)| {
                let name = catalog
                    .iter()
                    .find(|transformation| &transformation.id == id)
                    .map_or_else(|| id.clone(), |transformation| transformation.name.clone());
                let drag = TransformationDrag {
                    selection,
                    id: id.clone(),
                    name: name.clone(),
                    position: Point::default(),
                };
                let dragged_id = id.clone();
                let drop_id = id.clone();
                div()
                    .id(("mode-transformation", target_index))
                    .h(px(36.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(target_index + 1 < selected.len(), |row| {
                        row.border_b_1().border_color(rgb(LINE))
                    })
                    .can_drop(move |value, _, _| {
                        value
                            .downcast_ref::<TransformationDrag>()
                            .is_some_and(|drag| drag.selection == selection && drag.id != drop_id)
                    })
                    .on_drop(cx.listener(move |this, drag: &TransformationDrag, _, cx| {
                        if drag.selection != selection {
                            return;
                        }
                        let transformations =
                            &mut this.mode_settings_mut(selection).transformations;
                        if !reorder_transformation(transformations, &drag.id, target_index) {
                            return;
                        }
                        this.save_settings(cx);
                    }))
                    .child(
                        div()
                            .id(("drag-mode-transformation", target_index))
                            .w(px(20.0))
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.0))
                            .text_color(rgb(FAINT))
                            .child("⠿")
                            .on_drag(drag, |drag, position, _, cx| {
                                cx.new(|_| drag.clone().at(position))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_SOFT))
                            .truncate()
                            .child(name),
                    )
                    .child(
                        compact_button("×")
                            .id(("remove-mode-transformation", target_index))
                            .size(px(28.0))
                            .px_0()
                            .justify_center()
                            .text_size(px(14.0))
                            .text_color(rgb(MUTED))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mode_settings_mut(selection)
                                    .transformations
                                    .retain(|candidate| candidate != &dragged_id);
                                this.save_settings(cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let available_rows = if self.transformation_picker_open {
            catalog
                .iter()
                .enumerate()
                .filter(|(_, transformation)| !selected.contains(&transformation.id))
                .map(|(index, transformation)| {
                    let id = transformation.id.clone();
                    let name = transformation.name.clone();
                    let description = transformation.description.clone();
                    div()
                        .id(("available-mode-transformation", index))
                        .min_h(px(40.0))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT_SOFT))
                                        .child(name),
                                )
                                .when_some(description, |copy, description| {
                                    copy.child(
                                        div()
                                            .pt_1()
                                            .text_size(px(10.0))
                                            .text_color(rgb(FAINT))
                                            .child(description),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(TEXT_SOFT))
                                .child("Add"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mode_settings_mut(selection)
                                .transformations
                                .push(id.clone());
                            this.save_settings(cx);
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let available_empty = available_rows.is_empty();
        let catalog_empty = catalog.is_empty();
        let add = (!catalog_empty).then(|| {
            let button = if self.transformation_picker_open {
                compact_button("Done")
                    .h(px(28.0))
                    .mr(px(-7.0))
                    .text_size(px(10.0))
            } else {
                compact_header_plus_button()
            };
            button
                .id("toggle-transformation-picker")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.transformation_picker_open = !this.transformation_picker_open;
                    cx.notify();
                }))
                .into_any_element()
        });

        compact_panel()
            .child(compact_panel_header("Transformations", add).when(
                selected.is_empty() && !self.transformation_picker_open,
                |header| header.border_b_0(),
            ))
            .children(selected_rows)
            .when(self.transformation_picker_open && !catalog_empty, |panel| {
                panel
                    .when(available_empty, |list| {
                        list.child(
                            div()
                                .border_t_1()
                                .border_color(rgb(LINE))
                                .px_3()
                                .py_3()
                                .text_size(px(10.0))
                                .text_color(rgb(MUTED))
                                .child("Every registered transformation is already included."),
                        )
                    })
                    .children(available_rows)
            })
            .into_any_element()
    }

    fn render_application_picker(
        &self,
        selection: ModeSelection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.mode_settings(selection).applications.clone();
        let selected_rows = selected.iter().enumerate().map(|(index, name)| {
            let application = self.application_named(name);
            let name = name.clone();
            div()
                .id(("selected-application", index))
                .h(px(30.0))
                .pl_1()
                .pr_1()
                .flex()
                .items_center()
                .gap_1()
                .rounded_sm()
                .bg(rgb(SURFACE_SELECTED))
                .border_1()
                .border_color(rgb(LINE))
                .child(application_icon(application, Some(name.as_str()), 20.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(TEXT_SOFT))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .id(("remove-selected-application", index))
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .text_size(px(14.0))
                        .text_color(rgb(FAINT))
                        .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                        .child("×")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_mode_application(selection, &name, cx);
                        })),
                )
        });
        let query = self
            .application_search
            .entity
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        let available = if self.application_picker_open {
            self.application_choices(selection, cx)
        } else {
            Vec::new()
        };
        let no_matches = available.is_empty();
        let available_rows =
            available
                .into_iter()
                .cloned()
                .enumerate()
                .map(|(index, application)| {
                    let name = application.name.clone();
                    let location = application
                        .path
                        .parent()
                        .map_or_else(String::new, |path| path.display().to_string());
                    div()
                        .id(("available-application", index))
                        .w_full()
                        .px_2()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .when(index == self.application_picker_highlight, |row| {
                            row.bg(rgb(SURFACE_SELECTED))
                        })
                        .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                        .child(application_icon(Some(&application), Some(&name), 30.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT_SOFT))
                                        .child(name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(rgb(FAINT))
                                        .overflow_hidden()
                                        .child(location),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.add_mode_application(selection, name.clone(), cx);
                            }),
                        )
                });
        let picker_status = match &self.application_catalog {
            ApplicationCatalogState::Loading => Some("Finding applications on this Mac...".into()),
            ApplicationCatalogState::Loaded(applications) if applications.is_empty() => {
                Some("No applications found.".into())
            }
            ApplicationCatalogState::Loaded(_) if no_matches && !query.is_empty() => {
                Some(format!("No applications match \"{query}\"."))
            }
            _ => None,
        };
        let tray = self.application_picker_open.then(|| {
            div()
                .mt_2()
                .p_2()
                .rounded_sm()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(SURFACE))
                .child(self.application_search.entity.clone())
                .when_some(picker_status, |tray, status| {
                    tray.child(
                        div()
                            .px_2()
                            .py_3()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(status),
                    )
                })
                .child(
                    div()
                        .id("application-picker-results")
                        .max_h(px(300.0))
                        .overflow_y_scroll()
                        .children(available_rows),
                )
                .child(
                    div()
                        .pt_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("choose-application-bundle")
                                .h(px(28.0))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_sm()
                                .text_size(px(11.0))
                                .text_color(rgb(TEXT_SOFT))
                                .hover(|button| button.bg(rgb(SURFACE_HOVER)))
                                .child("Choose from Finder...")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.choose_application(selection, cx);
                                })),
                        )
                        .child(
                            div()
                                .id("close-application-picker")
                                .h(px(28.0))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_sm()
                                .text_size(px(11.0))
                                .text_color(rgb(MUTED))
                                .hover(|button| button.bg(rgb(SURFACE_HOVER)))
                                .child("Done")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.application_picker_open = false;
                                    this.application_picker_highlight = 0;
                                    this.application_search
                                        .entity
                                        .update(cx, |input, cx| input.set_text("", cx));
                                    window.blur();
                                    cx.notify();
                                })),
                        ),
                )
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(64.0))
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(settings_copy(
                        "Applications",
                        "Switch when an app is frontmost",
                    ))
                    .child(
                        div()
                            .max_w(px(300.0))
                            .min_h(px(CONTROL_HEIGHT))
                            .flex_1()
                            .p_1()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .children(selected_rows)
                            .child(
                                compact_plus_button()
                                    .id("open-application-picker")
                                    .size(px(30.0))
                                    .border_1()
                                    .border_color(rgb(LINE))
                                    .bg(rgb(CANVAS))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.application_picker_highlight = 0;
                                        this.application_picker_open = true;
                                        this.application_picker_error = None;
                                        this.ensure_application_catalog_load();
                                        this.application_search
                                            .entity
                                            .update(cx, |input, cx| input.set_text("", cx));
                                        this.application_search
                                            .entity
                                            .focus_handle(cx)
                                            .focus(window);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when_some(self.application_picker_error.clone(), |picker, error| {
                picker.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(NEGATIVE))
                        .child(error),
                )
            })
            .when_some(tray, |picker, tray| picker.child(tray))
            .into_any_element()
    }

    fn application_named(&self, name: &str) -> Option<&InstalledApplication> {
        let ApplicationCatalogState::Loaded(applications) = &self.application_catalog else {
            return None;
        };
        applications
            .iter()
            .find(|application| application.name == name)
    }

    fn application_choice_names(&self, cx: &App) -> Vec<String> {
        self.application_choices(self.selected_mode, cx)
            .into_iter()
            .map(|application| application.name.clone())
            .collect()
    }

    fn application_choices<'a>(
        &'a self,
        selection: ModeSelection,
        cx: &App,
    ) -> Vec<&'a InstalledApplication> {
        let ApplicationCatalogState::Loaded(applications) = &self.application_catalog else {
            return Vec::new();
        };
        let selected = &self.mode_settings(selection).applications;
        let query = self
            .application_search
            .entity
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        applications
            .iter()
            .filter(|application| {
                !selected.contains(&application.name)
                    && (query.is_empty()
                        || application.name.to_lowercase().contains(&query)
                        || application
                            .bundle_id
                            .as_deref()
                            .is_some_and(|bundle| bundle.to_lowercase().contains(&query)))
            })
            .take(10)
            .collect()
    }

    fn add_mode_application(
        &mut self,
        selection: ModeSelection,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(conflict) = self
            .settings
            .dictation_processing
            .modes
            .iter()
            .enumerate()
            .find(|(index, mode)| {
                selection != ModeSelection::Custom(*index) && mode.applications.contains(&name)
            })
            .map(|(_, mode)| mode.name.clone())
        {
            self.application_picker_error = Some(format!(
                "{name} already activates {conflict}. Remove it there before reassigning it."
            ));
            cx.notify();
            return;
        }
        let applications = &mut self.mode_settings_mut(selection).applications;
        if !applications.contains(&name) {
            applications.push(name);
            applications.sort_by_key(|name| name.to_lowercase());
            self.application_picker_highlight = 0;
            self.application_search
                .entity
                .update(cx, |input, cx| input.set_text("", cx));
            self.application_picker_error = None;
            self.save_settings(cx);
        }
    }

    fn remove_mode_application(
        &mut self,
        selection: ModeSelection,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        self.mode_settings_mut(selection)
            .applications
            .retain(|application| application != name);
        self.save_settings(cx);
    }

    fn choose_application(&mut self, selection: ModeSelection, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add Application".into()),
        });
        cx.spawn(async move |window, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = window.update(cx, |window, cx| {
                        window.application_picker_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = window.update(cx, |window, cx| {
                        window.application_picker_error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { crate::application_catalog::metadata(&path) })
                .await;
            let _ = window.update(cx, |window, cx| match result {
                Ok(application) => {
                    let name = application.name.clone();
                    match &mut window.application_catalog {
                        ApplicationCatalogState::Loading => {
                            window.application_catalog =
                                ApplicationCatalogState::Loaded(vec![application]);
                        }
                        ApplicationCatalogState::Loaded(applications) => {
                            crate::application_catalog::insert(applications, application);
                        }
                    }
                    window.application_picker_error = None;
                    window.add_mode_application(selection, name, cx);
                }
                Err(error) => {
                    window.application_picker_error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Build the shared OpenCode model picker: the collapsed selected-model
    /// card, the type-to-filter search input with bounded suggestions, the
    /// catalog status line, and the thinking-variant control. Every surface
    /// that selects a model renders these same parts.
    fn processing_picker(
        &mut self,
        target: ModelPickerTarget,
        model: Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ProcessingPicker {
        let focused = model.read(cx).is_focused(window);
        let (selected_model, selected_variant) = self.model_settings_for(target);
        let selected_model_text = selected_model.unwrap_or_default();
        if !focused && model.read(cx).text() != selected_model_text {
            model.update(cx, |input, cx| input.set_text(selected_model_text, cx));
        }
        let query = model.read(cx).text().trim().to_lowercase();
        let presentation = model_presentation(&self.model_catalog, selected_model);
        let (catalog_status, default_label, matches, variants) = match &self.model_catalog {
            ModelCatalogState::Loading => {
                ("Loading OpenCode...".into(), None, Vec::new(), Vec::new())
            }
            ModelCatalogState::Missing => (
                "OpenCode beta required".into(),
                None,
                Vec::new(),
                Vec::new(),
            ),
            ModelCatalogState::Failed(error) => (
                format!("OpenCode models unavailable: {error}"),
                None,
                Vec::new(),
                Vec::new(),
            ),
            ModelCatalogState::Loaded(catalog) => {
                let choice_keys = if focused {
                    filtered_model_keys(catalog, &query)
                } else {
                    Vec::new()
                };
                let matches = choice_keys
                    .iter()
                    .filter_map(Option::as_deref)
                    .filter_map(|key| catalog.models.iter().find(|choice| choice.key == key))
                    .cloned()
                    .collect();
                let variants = model_variants(catalog, selected_model);
                let default_label = choice_keys
                    .first()
                    .is_some_and(Option::is_none)
                    .then(|| {
                        catalog
                            .default_name
                            .as_ref()
                            .map(|name| match &catalog.default_key {
                                Some(key) => format!("OpenCode default: {name} ({key})"),
                                None => format!("OpenCode default: {name}"),
                            })
                    })
                    .flatten();
                (
                    format!("{} available models", catalog.models.len()),
                    default_label,
                    matches,
                    variants,
                )
            }
        };
        let suggestions =
            (focused && (!matches.is_empty() || default_label.is_some())).then(|| {
                div()
                    .mt_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(SURFACE))
                    .when_some(default_label.clone(), |list, label| {
                        list.child(
                            model_choice_row(
                                "Use OpenCode default",
                                label,
                                self.model_picker_highlight == 0,
                            )
                            .id("opencode-default")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.select_model(target, None, cx);
                                    window.blur();
                                }),
                            ),
                        )
                    })
                    .children(matches.into_iter().enumerate().map(|(index, choice)| {
                        let key = choice.key.clone();
                        let row_index = index + usize::from(default_label.is_some());
                        model_choice_row(
                            choice.name,
                            choice.key,
                            self.model_picker_highlight == row_index,
                        )
                        .id(("model-choice", index))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.select_model(target, Some(key.clone()), cx);
                                window.blur();
                            }),
                        )
                    }))
            });
        let has_variants = !variants.is_empty();
        let variant_open = self.variant_picker_open == Some(target);
        let variant_list = (has_variants && variant_open).then(|| {
            let choices = std::iter::once(None)
                .chain(variants.iter().cloned().map(Some))
                .collect::<Vec<_>>();
            div()
                .mt_1()
                .p_1()
                .rounded_sm()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(SURFACE))
                .children(choices.into_iter().enumerate().map(|(index, variant)| {
                    let selected = selected_variant == variant.as_deref();
                    let label = variant.clone().unwrap_or_else(|| "Default".into());
                    div()
                        .id(("model-variant", index))
                        .w_full()
                        .h(px(28.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .text_size(px(11.0))
                        .text_color(if selected { rgb(TEXT) } else { rgb(TEXT_SOFT) })
                        .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                        .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.variant_picker_open = None;
                            this.set_model_variant(target, variant.clone(), cx);
                        }))
                }))
        });
        let variant_control = div()
            .w(px(160.0))
            .flex_none()
            .child(
                disclosure_button(selected_variant.unwrap_or("Default").to_string())
                    .id("model-variant-trigger")
                    .w(px(160.0))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.variant_picker_open = if this.variant_picker_open == Some(target) {
                            None
                        } else {
                            Some(target)
                        };
                        cx.notify();
                    })),
            )
            .when_some(variant_list, |control, list| control.child(list));
        let model_control = if let Some(presentation) = presentation.filter(|_| !focused) {
            let model_input = model.clone();
            {
                div()
                    .id("selected-model")
                    .h(px(54.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(SURFACE))
                    .hover(|control| control.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(presentation.name),
                            )
                            .child(div().text_size(px(10.0)).text_color(rgb(FAINT)).child(
                                format!("{} · {}", presentation.provider, presentation.key),
                            )),
                    )
                    .when(presentation.is_default, |control| {
                        control.child(
                            div()
                                .flex_none()
                                .text_size(px(9.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(MUTED))
                                .child("DEFAULT"),
                        )
                    })
                    .on_click(cx.listener(move |_, _, window, cx| {
                        model_input.focus_handle(cx).focus(window);
                        cx.notify();
                    }))
                    .into_any_element()
            }
        } else {
            model.clone().into_any_element()
        };
        let refresh_control =
            matches!(&self.model_catalog, ModelCatalogState::Loaded(_)).then(|| {
                compact_button("Refresh")
                    .id("refresh-opencode-models")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reload_model_catalog();
                        cx.notify();
                    }))
                    .into_any_element()
            });
        ProcessingPicker {
            catalog_status,
            model_control,
            refresh_control,
            suggestions,
            has_variants,
            variant_control,
        }
    }

    /// The Voice Action processing panel: the shared model and thinking
    /// pickers presented in the pane's setting-row language. The processing
    /// deadline is not configurable here; Voice Action uses the persisted
    /// default.
    fn render_voice_action_processing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = window.viewport_size().width < px(980.0);
        let model = self.voice_action_inputs.model.entity.clone();
        let ProcessingPicker {
            catalog_status,
            model_control,
            refresh_control,
            suggestions,
            has_variants,
            variant_control,
        } = self.processing_picker(ModelPickerTarget::VoiceAction, model, window, cx);
        let model_field = div()
            .when(!compact, |field| field.w(px(380.0)).flex_none())
            .when(compact, |field| field.w_full())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().min_w(px(0.0)).flex_1().child(model_control))
                    .when_some(refresh_control, |row, refresh| row.child(refresh)),
            )
            .when_some(suggestions, |field, suggestions| field.child(suggestions));
        compact_panel()
            .child(voice_action_setting_row(
                "OpenCode model",
                catalog_status,
                model_field,
                has_variants,
                compact,
            ))
            .when(has_variants, |panel| {
                panel.child(voice_action_setting_row(
                    "Thinking",
                    "Choose how much reasoning the model should use",
                    div().w(px(160.0)).flex_none().child(variant_control),
                    false,
                    compact,
                ))
            })
            .into_any_element()
    }

    fn render_processing_settings(
        &mut self,
        target: ModelPickerTarget,
        prompt: Option<Entity<TextInput>>,
        model: Entity<TextInput>,
        deadline: Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = window.viewport_size().width < px(980.0);
        let ProcessingPicker {
            catalog_status,
            model_control,
            refresh_control,
            suggestions,
            has_variants,
            variant_control,
        } = self.processing_picker(target, model, window, cx);
        div()
            .pt_5()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .flex()
                    .when(compact, |fields| fields.flex_col())
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(settings_control(
                                "Model",
                                &catalog_status,
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().min_w(px(0.0)).flex_1().child(model_control))
                                    .when_some(refresh_control, |row, refresh| row.child(refresh))
                                    .into_any_element(),
                            ))
                            .when(
                                matches!(&self.model_catalog, ModelCatalogState::Missing),
                                |field| {
                                    field.child(
                                        compact_button("Install OpenCode")
                                            .id("install-opencode-beta")
                                            .on_click(|_, _, _| open_opencode_beta_docs()),
                                    )
                                },
                            )
                            .when_some(suggestions, |field, suggestions| field.child(suggestions)),
                    )
                    .child(
                        div()
                            .when(!compact, |field| field.w(px(150.0)).flex_none())
                            .when(compact, |field| field.w_full())
                            .child(settings_input("Deadline", "Seconds", deadline)),
                    ),
            )
            .when(has_variants, |processing| {
                processing.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_SOFT))
                                .child("Thinking"),
                        )
                        .child(div().flex().child(variant_control)),
                )
            })
            .when_some(prompt, |processing, prompt| {
                processing.child(settings_input(
                    "Instructions",
                    "Tell OpenCode exactly how to transform the dictated text.",
                    prompt,
                ))
            })
            .into_any_element()
    }

    fn model_choice_keys(&self, target: ModelPickerTarget, cx: &App) -> Vec<Option<String>> {
        let ModelCatalogState::Loaded(catalog) = &self.model_catalog else {
            return Vec::new();
        };
        let query = self
            .model_input_for(target)
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        filtered_model_keys(catalog, &query)
    }

    fn select_model(
        &mut self,
        target: ModelPickerTarget,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let text = model.clone().unwrap_or_default();
        self.model_input_for(target)
            .update(cx, |input, cx| input.set_text(text, cx));
        match target {
            ModelPickerTarget::Mode(selection) => {
                let processing = &mut self.mode_settings_mut(selection).post_processing;
                processing.model = model;
                processing.variant = None;
            }
            ModelPickerTarget::VoiceAction => {
                self.settings.voice_action.model = model;
                self.settings.voice_action.variant = None;
            }
        }
        self.save_settings(cx);
    }

    fn set_model_variant(
        &mut self,
        target: ModelPickerTarget,
        variant: Option<String>,
        cx: &mut Context<Self>,
    ) {
        apply_model_variant(&mut self.settings, target, variant, &self.model_catalog);
        self.save_settings(cx);
    }

    fn model_input_for(&self, target: ModelPickerTarget) -> Entity<TextInput> {
        match target {
            ModelPickerTarget::Mode(selection) => {
                self.mode_inputs_for(selection).model.entity.clone()
            }
            ModelPickerTarget::VoiceAction => self.voice_action_inputs.model.entity.clone(),
        }
    }

    fn model_settings_for(&self, target: ModelPickerTarget) -> (Option<&str>, Option<&str>) {
        match target {
            ModelPickerTarget::Mode(selection) => {
                let processing = &self.mode_settings(selection).post_processing;
                (processing.model.as_deref(), processing.variant.as_deref())
            }
            ModelPickerTarget::VoiceAction => (
                self.settings.voice_action.model.as_deref(),
                self.settings.voice_action.variant.as_deref(),
            ),
        }
    }

    fn selected_mode_inputs_mut(&mut self) -> &mut ModeInputs {
        self.mode_inputs_mut(self.selected_mode)
    }

    fn select_mode(
        &mut self,
        selection: ModeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_mode = selection;
        self.application_picker_open = false;
        self.application_picker_error = None;
        self.transformation_picker_open = false;
        self.variant_picker_open = None;
        self.application_picker_highlight = 0;
        self.model_picker_highlight = 0;
        self.mode_delete_armed = false;
        self.mode_context_menu = None;
        self.application_search
            .entity
            .update(cx, |input, cx| input.set_text("", cx));
        window.blur();
        cx.notify();
    }

    fn mode_inputs_mut(&mut self, selection: ModeSelection) -> &mut ModeInputs {
        match selection {
            ModeSelection::Default => &mut self.processing_inputs.default_mode,
            ModeSelection::Custom(index) => &mut self.processing_inputs.modes[index],
        }
    }

    fn mode_inputs_for(&self, selection: ModeSelection) -> &ModeInputs {
        match selection {
            ModeSelection::Default => &self.processing_inputs.default_mode,
            ModeSelection::Custom(index) => &self.processing_inputs.modes[index],
        }
    }

    fn selected_mode_settings(&self) -> &DictationMode {
        self.mode_settings(self.selected_mode)
    }

    fn selected_mode_settings_mut(&mut self) -> &mut DictationMode {
        self.mode_settings_mut(self.selected_mode)
    }

    fn mode_settings(&self, selection: ModeSelection) -> &DictationMode {
        match selection {
            ModeSelection::Default => &self.settings.dictation_processing.default_mode,
            ModeSelection::Custom(index) => &self.settings.dictation_processing.modes[index],
        }
    }

    fn mode_settings_mut(&mut self, selection: ModeSelection) -> &mut DictationMode {
        match selection {
            ModeSelection::Default => &mut self.settings.dictation_processing.default_mode,
            ModeSelection::Custom(index) => &mut self.settings.dictation_processing.modes[index],
        }
    }

    fn render_meetings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let action = if self.meeting_stop_pending {
            meeting_progress_action("Finalizing...")
        } else {
            match meeting::active(&self.meetings).map(|meeting| meeting.status) {
                Some(MeetingStatus::Starting) => meeting_progress_action("Starting..."),
                Some(MeetingStatus::Recording) => div()
                    .id("stop-meeting")
                    .h(px(30.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .bg(rgb(0xe8e8e8))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0x171717))
                    .hover(|button| button.bg(rgb(0xffffff)))
                    .child("Stop & Transcribe")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.meeting_stop_pending = true;
                        if let Err(error) = this.meeting_requests.try_send(MeetingRequest::Stop) {
                            this.meeting_stop_pending = false;
                            log_meeting_request_error(error);
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                Some(MeetingStatus::Transcribing) => meeting_progress_action("Finalizing..."),
                _ if self.meeting_refresh_started_ms.is_some() => {
                    meeting_progress_action("Starting...")
                }
                _ => div()
                    .id("start-meeting")
                    .h(px(30.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .bg(rgb(0xe8e8e8))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0x171717))
                    .hover(|button| button.bg(rgb(0xffffff)))
                    .child("Start Recording")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.meeting_refresh_started_ms = Some(now_ms());
                        if let Err(error) = this.meeting_requests.try_send(MeetingRequest::Start) {
                            this.meeting_refresh_started_ms = None;
                            log_meeting_request_error(error);
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            }
        };

        let rows: Vec<AnyElement> = self
            .meetings
            .iter()
            .enumerate()
            .map(|(index, meeting)| {
                let id = meeting.id.clone();
                let selected = self.selected_meeting.as_deref() == Some(id.as_str());
                let title = meeting.title.clone();
                let duration = meeting::format_duration(meeting_duration(meeting));
                let status = meeting_status(meeting.status);
                div()
                    .id(("meeting", index))
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.0))
                            .text_color(rgb(FAINT))
                            .child(status)
                            .child(duration),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_meeting = Some(id.clone());
                        this.load_selected_meeting();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        let list = div()
            .id("meetings-list")
            .w(px(PANE_LIST_WIDTH))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(rgb(LINE))
            .when(
                self.meetings.is_empty() && self.meetings_error.is_none(),
                |list| list.child(empty_message("No meetings yet.")),
            )
            .when_some(self.meetings_error.clone(), |list, error| {
                list.child(error_message("Meetings could not be loaded.", error))
            })
            .children(rows);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Meetings", Some(action)))
            .when_some(self.meeting_runtime_error.clone(), |pane, error| {
                pane.child(
                    div()
                        .px_6()
                        .py_3()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .text_size(px(12.0))
                        .text_color(rgb(NEGATIVE))
                        .child(error),
                )
            })
            .child(
                pane_body().child(
                    pane_content()
                        .id("meetings-workspace")
                        .flex_row()
                        .child(list)
                        .child(self.render_meeting_detail(cx)),
                ),
            )
            .into_any_element()
    }

    fn render_meeting_detail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(meeting) = self
            .selected_meeting
            .as_deref()
            .and_then(|id| self.meetings.iter().find(|meeting| meeting.id == id))
            .cloned()
        else {
            return detail_placeholder("Select a meeting.");
        };
        let id = meeting.id.clone();
        let reveal = div()
            .id("reveal-meeting-files")
            .h(px(30.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_sm()
            .text_size(px(12.0))
            .text_color(rgb(MUTED))
            .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
            .child("Reveal Files")
            .on_click(cx.listener(move |_, _, _, _| {
                match meeting::root() {
                    Ok(root) => {
                        match ProcessCommand::new("/usr/bin/open")
                            .arg(root.join(&id))
                            .status()
                        {
                            Ok(status) if status.success() => {}
                            Ok(status) => {
                                tracing::error!(%status, "could not reveal meeting files")
                            }
                            Err(error) => {
                                tracing::error!(%error, "could not reveal meeting files")
                            }
                        }
                    }
                    Err(error) => tracing::error!(%error, "could not locate meeting files"),
                }
            }));

        let transcript = match &self.meeting_transcript {
            MeetingTranscript::Empty => empty_message("No transcript selected."),
            MeetingTranscript::Loaded(lines) if lines.is_empty() => {
                empty_message(match meeting.status {
                    MeetingStatus::Starting => "Preparing local audio capture...",
                    MeetingStatus::Recording if meeting.live_transcription_error.is_some() => {
                        "Live transcription is unavailable. Audio is still recording safely."
                    }
                    MeetingStatus::Recording if self.meeting_stop_pending => {
                        "Building the final transcript..."
                    }
                    MeetingStatus::Recording => "Listening for speech...",
                    MeetingStatus::Transcribing => {
                        "The live draft is empty. Building the final transcript..."
                    }
                    MeetingStatus::Complete => "The final transcript is empty.",
                    MeetingStatus::Interrupted | MeetingStatus::Failed => {
                        "No recoverable transcript is available."
                    }
                })
            }
            MeetingTranscript::Loaded(lines) => div()
                .children(lines.iter().enumerate().map(|(index, line)| {
                    div()
                        .id(("transcript-line", index))
                        .flex()
                        .flex_col()
                        .when_some(line.pause_before_ms, |row, pause| {
                            row.child(
                                div()
                                    .py_3()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_size(px(10.0))
                                    .text_color(rgb(FAINT))
                                    .child(div().h(px(1.0)).flex_1().bg(rgb(LINE)))
                                    .child(format!("{} pause", meeting::format_duration(pause)))
                                    .child(div().h(px(1.0)).flex_1().bg(rgb(LINE))),
                            )
                        })
                        .child(
                            div()
                                .py_4()
                                .flex()
                                .items_start()
                                .gap_4()
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .child(
                                    div()
                                        .w(px(116.0))
                                        .flex_none()
                                        .text_size(px(11.0))
                                        .text_color(rgb(MUTED))
                                        .child(line.source.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(13.0))
                                        .line_height(px(20.0))
                                        .text_color(rgb(TEXT_SOFT))
                                        .child(line.text.clone()),
                                ),
                        )
                }))
                .into_any_element(),
            MeetingTranscript::Unavailable(error) => {
                error_message("Transcript unavailable.", error.clone())
            }
        };
        let transcript_state = match meeting.status {
            MeetingStatus::Starting => "STARTING CAPTURE",
            MeetingStatus::Recording if meeting.live_transcription_error.is_some() => {
                "LIVE DRAFT UNAVAILABLE"
            }
            MeetingStatus::Recording if self.meeting_stop_pending => "FINALIZING WITH PARAKEET V2",
            MeetingStatus::Recording => "LIVE DRAFT",
            MeetingStatus::Transcribing => "FINALIZING WITH PARAKEET V2",
            MeetingStatus::Complete => "FINAL TRANSCRIPT",
            MeetingStatus::Interrupted | MeetingStatus::Failed => "RECOVERED DRAFT",
        };

        div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_6()
                    .py_5()
                    .flex()
                    .items_start()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(meeting.title.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_4()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(
                                        if self.meeting_stop_pending
                                            && meeting.status == MeetingStatus::Recording
                                        {
                                            "Finalizing"
                                        } else {
                                            meeting_status(meeting.status)
                                        },
                                    )
                                    .child(meeting::format_duration(meeting_duration(&meeting))),
                            )
                            .when_some(meeting.error.clone(), |header, error| {
                                header.child(
                                    div()
                                        .max_w(px(620.0))
                                        .text_size(px(11.0))
                                        .line_height(px(17.0))
                                        .text_color(rgb(NEGATIVE))
                                        .child(error),
                                )
                            }),
                    )
                    .child(reveal),
            )
            .child(
                div()
                    .id("meeting-transcript")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.meeting_transcript_scroll)
                    .px_6()
                    .pb_6()
                    .child(
                        div()
                            .h(px(42.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(10.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(MUTED))
                            .when(
                                meeting.status == MeetingStatus::Recording
                                    && !self.meeting_stop_pending,
                                |label| {
                                    label
                                        .child(div().size(px(6.0)).rounded_full().bg(rgb(0xd64f4f)))
                                },
                            )
                            .child(transcript_state)
                            .when_some(meeting.live_transcription_error.clone(), |label, error| {
                                label.child(
                                    div().text_color(rgb(NEGATIVE)).child(format!("  {error}")),
                                )
                            }),
                    )
                    .child(transcript),
            )
            .into_any_element()
    }

    fn render_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let config_path = self.personal_workspace.join("hex.config.ts");
        let workspace_state = personal_workspace_state(
            !self.preview && config_path.is_file(),
            self.personal_workspace_receiver.is_some(),
            &self.personal_commands_status,
        );
        let last_error = self
            .personal_workspace_error
            .clone()
            .or_else(|| self.personal_commands_status.last_reload_error.clone());
        let open_config = config_path.clone();
        let config_control = match workspace_state {
            PersonalWorkspaceState::Missing => header_button("Create Config")
                .id("personal-commands-create")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.provision_personal_workspace();
                    cx.notify();
                }))
                .into_any_element(),
            PersonalWorkspaceState::Creating => div()
                .h(px(30.0))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child("Creating Config...")
                .into_any_element(),
            PersonalWorkspaceState::Ready | PersonalWorkspaceState::Error => {
                header_button("Edit Config")
                    .id("personal-commands-open-config")
                    .on_click(move |_, _, _| open_path(&open_config))
                    .into_any_element()
            }
        };
        let copy_prompt = matches!(
            workspace_state,
            PersonalWorkspaceState::Ready | PersonalWorkspaceState::Error
        )
        .then(|| {
            let copied = self.personal_commands_prompt_copied;
            let workspace = self.personal_workspace.display();
            let config = config_path.display();
            let prompt = format!(
                "Help me configure my HEX custom voice commands.\n\n\
                 Workspace: {workspace}\n\
                 Config: {config}\n\n\
                 Read AGENTS.md and .agents/skills/personal-commands/SKILL.md in the workspace \
                 before editing. Ask me what commands or dictation phrases I want, update \
                 hex.config.ts, and run `bun run check` from the workspace when finished. You \
                 may add npm packages when a command needs them."
            );
            header_button(if copied {
                "Copied"
            } else {
                "Copy agent prompt"
            })
            .id("personal-commands-copy-prompt")
            .on_click(cx.listener(move |this, _, _, cx| {
                match arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set_text(prompt.clone()))
                {
                    Ok(()) => {
                        this.personal_commands_prompt_copied = true;
                        cx.notify();
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not copy personal command agent prompt")
                    }
                }
            }))
        });
        let commands_position = self.commands_toggle.render_position(window);
        let commands_toggle_control = if self.pending_microphone_policy
            == Some(MicrophonePolicyChange::KeepReadyAndEnableCommands)
        {
            compact_button("Keep Microphone Ready & Enable Commands")
                .id("confirm-enable-commands")
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Err(error) = this.update_microphone_policy(
                        MicrophonePolicyChange::KeepReadyAndEnableCommands,
                        cx,
                    ) {
                        tracing::error!(%error, "could not update microphone policy");
                    }
                }))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(
                    if self.settings.commands_enabled {
                        "Enabled"
                    } else {
                        "Off"
                    },
                ))
                .child(toggle(commands_position))
                .into_any_element()
        };
        let commands_control = div()
            .flex()
            .items_center()
            .gap_2()
            .child(config_control)
            .when_some(copy_prompt, |controls, prompt| controls.child(prompt))
            .child(
                div()
                    .id("commands-enabled")
                    .flex()
                    .items_center()
                    .child(commands_toggle_control)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let enabled = !this.settings.commands_enabled;
                        if enabled && this.settings.release_microphone_while_idle {
                            this.pending_microphone_policy =
                                Some(MicrophonePolicyChange::KeepReadyAndEnableCommands);
                            cx.notify();
                            return;
                        }
                        if let Err(error) = this.developer_set_commands_enabled(enabled, cx) {
                            tracing::error!(%error, "could not save app settings");
                        }
                    })),
            )
            .into_any_element();
        let context_label = self.current_context_label();
        let query = self
            .command_search
            .entity
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        let visible: Vec<usize> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| Self::command_matches(command, &query).then_some(index))
            .collect();
        let mut tasks = Vec::new();
        for index in &visible {
            let task = self.commands[*index].task.clone();
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
        let effective_selected = self
            .selected_command
            .clone()
            .filter(|id| visible.iter().any(|index| self.commands[*index].id == *id))
            .or_else(|| {
                visible
                    .first()
                    .map(|index| self.commands[*index].id.clone())
            });
        let mut groups: Vec<AnyElement> = Vec::new();
        for task in tasks {
            groups.push(
                compact_section_label(task.clone())
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_color(rgb(MUTED))
                    .into_any_element(),
            );
            groups.extend(
                visible
                    .iter()
                    .copied()
                    .filter(|index| self.commands[*index].task == task)
                    .map(|index| {
                        let command = self.commands[index].clone();
                        let command_id = command.id.clone();
                        let selected = effective_selected.as_deref() == Some(command_id.as_str());
                        div()
                            .id(("command", index))
                            .w_full()
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .rounded(px(6.0))
                            .when(selected, |row| {
                                row.bg(rgb(0x292929)).hover(|row| row.bg(rgb(0x303030)))
                            })
                            .when(!selected, |row| row.hover(|row| row.bg(rgb(SURFACE_HOVER))))
                            .child(
                                div()
                                    .w_full()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(command.phrase),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_size(px(10.0))
                                    .text_color(rgb(if selected { TEXT_SOFT } else { FAINT }))
                                    .truncate()
                                    .child(command.description),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_command = Some(command_id.clone());
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            );
        }

        let no_commands = groups.is_empty();
        let commands_list_width = PANE_LIST_WIDTH;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Commands", Some(commands_control)))
            .child(
                pane_body().p_5().child(
                    pane_content()
                        .gap_3()
                        .when_some(last_error, |column, error| {
                            column.child(
                                compact_panel().flex_none().child(error_message(
                                    "Personal commands are not loading.",
                                    error,
                                )),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(0.0))
                                .flex()
                                .gap_5()
                                .child(
                                    div()
                                        .w(px(commands_list_width))
                                        .h_full()
                                        .flex_none()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_none()
                                                .child(self.command_search.entity.clone()),
                                        )
                                        .child(
                                            compact_panel()
                                                .id("commands-list")
                                                .flex_1()
                                                .min_h(px(0.0))
                                                .overflow_y_scroll()
                                                .when(no_commands, |card| {
                                                    card.child(empty_message(if query.is_empty() {
                                                        "No commands yet."
                                                    } else {
                                                        "No matching commands."
                                                    }))
                                                })
                                                .child(
                                                    div()
                                                        .w(px(commands_list_width - 2.0))
                                                        .p_2()
                                                        .flex()
                                                        .flex_col()
                                                        .gap_1()
                                                        .children(groups),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .h_full()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .h(px(TEXT_INPUT_HEIGHT))
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_end()
                                                .child(
                                                    div()
                                                        .text_size(px(10.0))
                                                        .text_color(rgb(FAINT))
                                                        .child(format!("Context: {context_label}")),
                                                ),
                                        )
                                        .child(
                                            compact_panel()
                                                .id("command-detail")
                                                .flex_1()
                                                .min_h(px(0.0))
                                                .overflow_y_scroll()
                                                .child(self.render_command_detail(
                                                    effective_selected.as_deref(),
                                                )),
                                        ),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_command_detail(&self, selected: Option<&str>) -> AnyElement {
        let Some(command) =
            selected.and_then(|id| self.commands.iter().find(|command| command.id == id))
        else {
            return div()
                .py(px(120.0))
                .flex()
                .justify_center()
                .text_size(px(12.0))
                .text_color(rgb(FAINT))
                .child("Select a command.")
                .into_any_element();
        };
        let aliases = if command.aliases.is_empty() {
            "None".into()
        } else {
            command.aliases.join("\n")
        };
        div()
            .p_5()
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(command.phrase.clone()),
            )
            .child(
                div()
                    .pt_2()
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(command.description.clone()),
            )
            .child(
                div()
                    .pt_4()
                    .child(detail_row("Task", command.task.clone()))
                    .child(detail_row("Context", command_scope(&command.scope)))
                    .child(detail_row("Aliases", aliases)),
            )
            .into_any_element()
    }

    fn command_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, "Search commands", ""));
        let changed = cx.subscribe(&entity, |_, _, _: &TextChanged, cx| {
            cx.notify();
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed],
        }
    }

    /// Case-insensitive match over everything a person might remember about a
    /// command: its phrase, aliases, description, and group.
    fn command_matches(command: &CatalogCommand, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let scope = match &command.scope {
            CommandScope::Sleeping | CommandScope::Global => "global",
            CommandScope::Application(name) => name,
            CommandScope::Browser(host) => host,
        };
        command.phrase.to_lowercase().contains(query)
            || command.description.to_lowercase().contains(query)
            || command.task.to_lowercase().contains(query)
            || scope.to_lowercase().contains(query)
            || command
                .aliases
                .iter()
                .any(|alias| alias.to_lowercase().contains(query))
    }

    fn current_context(&self) -> (Option<String>, Option<String>) {
        self.current_context.clone()
    }

    fn current_context_label(&self) -> String {
        let (application, browser_host) = self.current_context();
        match (application, browser_host) {
            (Some(application), Some(host)) => format!("{application} / {host}"),
            (Some(application), None) => application,
            _ => "Unavailable".into(),
        }
    }

    fn render_activity(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self.snapshot();
        let refresh = header_button("Refresh")
            .id("refresh-activity")
            .on_click(cx.listener(|this, _, _, cx| {
                this.reload_events();
                cx.notify();
            }))
            .into_any_element();
        let header_action = div()
            .flex()
            .items_center()
            .gap_4()
            .when_some(snapshot.activity.state_label(), |actions, status| {
                let active = snapshot.activity.state != Some(VoiceState::Stopping);
                actions.child(listener_status(
                    status,
                    snapshot.activity.device.clone().unwrap_or_default(),
                    active,
                ))
            })
            .child(refresh)
            .into_any_element();
        let rows: Vec<AnyElement> = self
            .events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                let selected = self.selected_event == Some(index);
                let summary = event_summary(event);
                let age = event_age(event.timestamp_ms());
                let boundary = matches!(event, VoiceEvent::SessionStarted { .. });
                div()
                    .id(("activity-event", index))
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                    .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(if boundary { px(11.0) } else { px(12.0) })
                            .font_weight(if boundary {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if boundary { rgb(FAINT) } else { rgb(TEXT_SOFT) })
                            .line_height(px(18.0))
                            .child(summary),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.0))
                            .text_color(rgb(FAINT))
                            .child(age),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_event = Some(index);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action("Activity", Some(header_action)))
            .child(
                pane_body().child(
                    pane_content()
                        .flex_row()
                        .child(
                            div()
                                .id("activity-list")
                                .w(px(PANE_LIST_WIDTH))
                                .h_full()
                                .flex_none()
                                .overflow_y_scroll()
                                .border_r_1()
                                .border_color(rgb(LINE))
                                .when(
                                    self.events.is_empty() && self.events_error.is_none(),
                                    |list| list.child(empty_message("No activity yet.")),
                                )
                                .when_some(self.events_error.clone(), |list, error| {
                                    list.child(error_message(
                                        "Activity could not be loaded.",
                                        error,
                                    ))
                                })
                                .children(rows),
                        )
                        .child(self.render_event_detail()),
                ),
            )
            .into_any_element()
    }

    fn render_event_detail(&self) -> AnyElement {
        let Some(event) = self.selected_event.and_then(|index| self.events.get(index)) else {
            return detail_placeholder("Select an activity row.");
        };
        let raw = serde_json::to_string(event).unwrap_or_else(|error| error.to_string());
        div()
            .id("activity-detail")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_y_scroll()
            .px_6()
            .py_6()
            .child(
                div()
                    .text_size(px(18.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(24.0))
                    .child(event_summary(event)),
            )
            .child(
                div()
                    .pt_6()
                    .child(detail_row("Event", event.kind()))
                    .child(detail_row(
                        "Timestamp",
                        format!("{} ms", event.timestamp_ms()),
                    )),
            )
            .child(
                div()
                    .mt_6()
                    .pt_5()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(section_label("Raw event"))
                    .child(
                        div()
                            .pt_3()
                            .text_size(px(11.0))
                            .line_height(px(18.0))
                            .text_color(rgb(MUTED))
                            .child(raw),
                    ),
            )
            .into_any_element()
    }
    fn apply_hud_lab(&self) {
        self.indicator
            .send(DictationIndicatorEvent::Configure(self.hud_tuning));
        self.indicator.send(DictationIndicatorEvent::Reset);
        match self.hud_demo {
            HudDemoState::Recording => {
                for job_id in 0..self.hud_queued as u64 {
                    self.indicator
                        .send(DictationIndicatorEvent::Submitted { job_id });
                    self.indicator
                        .send(DictationIndicatorEvent::Transcribing { job_id });
                }
                self.indicator.send(DictationIndicatorEvent::Started);
                self.indicator.send(DictationIndicatorEvent::Meter {
                    average: 0.08,
                    peak: 0.42,
                });
            }
            HudDemoState::Transcribing | HudDemoState::Processing => {
                self.indicator
                    .send(DictationIndicatorEvent::Submitted { job_id: 0 });
                self.indicator.send(match self.hud_demo {
                    HudDemoState::Transcribing => {
                        DictationIndicatorEvent::Transcribing { job_id: 0 }
                    }
                    HudDemoState::Processing => DictationIndicatorEvent::Processing { job_id: 0 },
                    HudDemoState::Recording => unreachable!(),
                });
                for job_id in 1..=self.hud_queued as u64 {
                    self.indicator
                        .send(DictationIndicatorEvent::Submitted { job_id });
                    self.indicator
                        .send(DictationIndicatorEvent::Transcribing { job_id });
                }
            }
        }
    }

    fn hud_control_value(&self, control: HudControl) -> f32 {
        match control {
            HudControl::LineCount => (self.hud_tuning.line_count - 1.0) / 5.0,
            HudControl::Curvature => self.hud_tuning.curvature,
            HudControl::Speed => self.hud_tuning.speed / HUD_MAX_SPEED,
            HudControl::Blur => 1.0 - self.hud_tuning.sharpness,
            HudControl::Glow => self.hud_tuning.glow,
            HudControl::Depth => self.hud_tuning.depth,
            HudControl::LightAngle => self.hud_tuning.light_angle,
            HudControl::Outline => self.hud_tuning.outline,
        }
        .clamp(0.0, 1.0)
    }

    fn set_hud_control(&mut self, control: HudControl, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.0);
        match control {
            HudControl::LineCount => self.hud_tuning.line_count = (1.0 + value * 5.0).round(),
            HudControl::Curvature => self.hud_tuning.curvature = value,
            HudControl::Speed => self.hud_tuning.speed = value * HUD_MAX_SPEED,
            HudControl::Blur => self.hud_tuning.sharpness = 1.0 - value,
            HudControl::Glow => self.hud_tuning.glow = value,
            HudControl::Depth => self.hud_tuning.depth = value,
            HudControl::LightAngle => self.hud_tuning.light_angle = value,
            HudControl::Outline => self.hud_tuning.outline = value,
        }
        self.hud_copied = false;
        self.indicator
            .send(DictationIndicatorEvent::Configure(self.hud_tuning));
        cx.notify();
    }

    fn update_hud_from_pointer(
        &mut self,
        control: HudControl,
        pointer_x: f32,
        cx: &mut Context<Self>,
    ) {
        const SLIDER_LEFT: f32 = SIDEBAR_WIDTH + 24.0;
        const SLIDER_WIDTH: f32 = 360.0;
        self.set_hud_control(control, (pointer_x - SLIDER_LEFT) / SLIDER_WIDTH, cx);
    }

    fn select_hud_style(&mut self, style: usize, cx: &mut Context<Self>) {
        self.hud_tuning = match style {
            0 => HudTuning {
                style: 0.0,
                line_count: 1.0,
                curvature: 0.5,
                speed: 0.7,
                sharpness: 0.42,
                glow: 0.4,
                depth: 0.75,
                light_angle: 0.35,
                outline: 0.25,
            },
            1 => HudTuning {
                style: 1.0,
                line_count: 2.0,
                curvature: 0.47,
                speed: 17.08,
                sharpness: 0.29,
                glow: 0.77,
                depth: 0.65,
                light_angle: 0.35,
                outline: 1.0,
            },
            _ => HudTuning {
                style: 2.0,
                line_count: 3.0,
                curvature: 0.7,
                speed: 0.9,
                sharpness: 0.64,
                glow: 0.28,
                depth: 0.74,
                light_angle: 0.35,
                outline: 0.3,
            },
        };
        self.hud_copied = false;
        self.apply_hud_lab();
        cx.notify();
    }

    fn hud_preset(&self) -> String {
        format!(
            "hud-v1 style={} lines={:.0} curvature={:.2} speed={:.2} blur={:.2} glow={:.2} depth={:.2} light={:.2} outline={:.2}",
            self.hud_tuning.style as usize,
            self.hud_tuning.line_count,
            self.hud_tuning.curvature,
            self.hud_tuning.speed,
            1.0 - self.hud_tuning.sharpness,
            self.hud_tuning.glow,
            self.hud_tuning.depth,
            self.hud_tuning.light_angle,
            self.hud_tuning.outline,
        )
    }

    fn render_hud_slider(
        &self,
        control: HudControl,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = self.hud_control_value(control);
        let display = match control {
            HudControl::LineCount => format!("{:.0}", self.hud_tuning.line_count),
            HudControl::Speed => format!("{:.2}x", self.hud_tuning.speed),
            HudControl::LightAngle => format!("{:.0}°", self.hud_tuning.light_angle * 360.0),
            _ => format!("{:.2}", value),
        };
        div()
            .w(px(360.0))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.0))
                    .child(label)
                    .child(div().text_color(rgb(MUTED)).child(display)),
            )
            .child(
                div()
                    .id(("hud-slider", control as usize))
                    .relative()
                    .w_full()
                    .h(px(18.0))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(px(7.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgb(0x303030)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(7.0))
                            .w(relative(value))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgb(0x6f7fff)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(relative(value))
                            .top(px(2.0))
                            .ml(px(-7.0))
                            .size(px(14.0))
                            .rounded_full()
                            .border_2()
                            .border_color(rgb(0xe8e8e8))
                            .bg(rgb(0x333333)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.hud_dragging = Some(control);
                            this.update_hud_from_pointer(control, f32::from(event.position.x), cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if event.dragging() && this.hud_dragging == Some(control) {
                            this.update_hud_from_pointer(control, f32::from(event.position.x), cx);
                        }
                    })),
            )
            .into_any_element()
    }

    fn render_hud_lab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let styles = [
            (
                "A",
                "Moving light",
                "A line-free area light travels around the orb.",
            ),
            (
                "B",
                "Meridians",
                "Highlights wrap around the sphere instead of crossing it.",
            ),
            (
                "C",
                "Ribbons",
                "Curved bands retained for comparison and tuning.",
            ),
        ];
        let style_cards =
            styles
                .into_iter()
                .enumerate()
                .map(|(index, (key, name, description))| {
                    let selected = self.hud_tuning.style as usize == index;
                    div()
                        .id(("hud-style", index))
                        .w(px(190.0))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(if selected { 0x697cff } else { LINE }))
                        .bg(rgb(if selected { 0x1c2030 } else { SURFACE }))
                        .hover(|card| card.bg(rgb(SURFACE_HOVER)))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(0x8390ff))
                                .child(key),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(MUTED))
                                .child(description),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select_hud_style(index, cx)),
                        )
                });
        let state_button = |state, label: &'static str, index: usize| {
            compact_button(label)
                .id(("hud-state", index))
                .when(self.hud_demo == state, |button| {
                    button.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.hud_demo = state;
                    this.apply_hud_lab();
                    cx.notify();
                }))
        };
        let queue_buttons = (0..=3).map(|count| {
            compact_button(count.to_string())
                .id(("hud-queue", count))
                .when(self.hud_queued == count, |button| {
                    button.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.hud_queued = count;
                    this.apply_hud_lab();
                    cx.notify();
                }))
        });
        let preset = self.hud_preset();
        let copied = self.hud_copied;

        div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.hud_dragging = None;
                }),
            )
            .child(pane_header_with_action(
                "HUD Lab",
                Some(
                    header_button(if copied { "Copied" } else { "Copy preset" })
                        .id("copy-hud-preset")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(this.hud_preset());
                                this.hud_copied = true;
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                ),
            ))
            .child(
                pane_content()
                    .id("hud-lab-scroll")
                    .flex_1()
                    .mx_auto()
                    .overflow_y_scroll()
                    .p_6()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children([
                                state_button(HudDemoState::Recording, "Recording", 0),
                                state_button(HudDemoState::Transcribing, "Transcribing", 1),
                                state_button(HudDemoState::Processing, "Processing", 2),
                            ])
                            .child(div().w_4())
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child("Queued"),
                            )
                            .children(queue_buttons),
                    )
                    .child(div().flex().gap_3().children(style_cards))
                    .child(
                        div()
                            .flex()
                            .child(div().flex().flex_col().gap_4().children([
                                self.render_hud_slider(HudControl::LineCount, "Line count", cx),
                                self.render_hud_slider(HudControl::Curvature, "Curvature", cx),
                                self.render_hud_slider(HudControl::Speed, "Speed", cx),
                                self.render_hud_slider(HudControl::Blur, "Highlight blur", cx),
                                self.render_hud_slider(HudControl::Glow, "Line glow", cx),
                                self.render_hud_slider(HudControl::Depth, "Sphere depth", cx),
                                self.render_hud_slider(HudControl::LightAngle, "Light angle", cx),
                                self.render_hud_slider(HudControl::Outline, "Sphere outline", cx),
                            ])),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(LINE))
                            .bg(rgb(0x0d0d0d))
                            .font_family("SF Mono")
                            .text_size(px(10.0))
                            .text_color(rgb(TEXT_SOFT))
                            .child(preset),
                    ),
            )
            .into_any_element()
    }
}

fn application_catalog_load_needed(
    preview: bool,
    catalog: &ApplicationCatalogState,
    receiver_pending: bool,
) -> bool {
    !preview && matches!(catalog, ApplicationCatalogState::Loading) && !receiver_pending
}

impl Drop for AppWindow {
    fn drop(&mut self) {
        if !self.preview
            && self.settings_dirty
            && let Err(error) = self.settings.save()
        {
            tracing::error!(%error, "could not flush app settings while closing window");
        }
        crate::app_settings::set_hotkey_capture_active(false);
    }
}

impl TranscriptionPickerDelegate for AppWindow {
    fn cancel_transcription_preparation(&mut self) {
        self.cancel_transcription_download();
    }

    fn choose_transcription_model(
        &mut self,
        model: TranscriptionModelId,
        language: String,
        cx: &mut Context<Self>,
    ) {
        AppWindow::choose_transcription_model(self, definition(model), language, cx);
    }

    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>) {
        AppWindow::dismiss_transcription_picker(self, cx);
    }

    fn select_transcription_language(&mut self, language: String, cx: &mut Context<Self>) {
        if self.transcription_picker_language.as_deref() != Some(&language) {
            self.cancel_transcription_download();
            self.transcription_picker_language = Some(language);
            self.transcription_picker_error = None;
            cx.notify();
        }
    }
}

impl DesktopHost for AppWindow {
    fn capabilities(&self) -> DesktopCapabilities {
        DesktopCapabilities::macos(crate::DEVELOPER_FEATURES_ENABLED)
    }

    fn snapshot(&self) -> DesktopSnapshot {
        let dictation_shortcut = self.settings.dictation_hotkey.keycaps();
        let update_status = match self.update_status {
            crate::sparkle::UpdateStatus::Unavailable => DesktopUpdateStatus::Unavailable,
            crate::sparkle::UpdateStatus::Checking => DesktopUpdateStatus::Checking,
            crate::sparkle::UpdateStatus::Idle | crate::sparkle::UpdateStatus::UpToDate => {
                DesktopUpdateStatus::Current
            }
            crate::sparkle::UpdateStatus::UpdateAvailable => DesktopUpdateStatus::Available,
        };
        DesktopSnapshot {
            activity: self.activity.clone(),
            dictation_shortcut_label: dictation_shortcut.join("+"),
            dictation_shortcut,
            double_tap_lock: self.settings.double_tap_lock,
            double_tap_only: self.settings.double_tap_only,
            paste_last_shortcut: self
                .settings
                .paste_last_hotkey
                .as_ref()
                .map(HotkeyBinding::keycaps),
            listener: None,
            operation_error: self.settings_load_error.clone(),
            observations_path: self.event_reader.path().display().to_string(),
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: self.transcription_downloaded_bytes,
                error: self.transcription_picker_error.clone(),
                preparation_stage: None,
                selection: self.settings.transcription.clone(),
                preparing: self.transcription_downloading,
            },
            update_status,
        }
    }

    fn dispatch(&mut self, action: DesktopAction) -> color_eyre::Result<()> {
        match action {
            DesktopAction::ClearError => {}
            DesktopAction::RestartIntoUpdate
            | DesktopAction::StartListening
            | DesktopAction::StopListening => {
                return Err(color_eyre::eyre::eyre!(
                    "desktop action is unavailable on this host"
                ));
            }
            DesktopAction::SetDictationShortcut(shortcut) => {
                let modifiers = HotkeyModifiers {
                    control: shortcut.control.then_some(Default::default()),
                    option: shortcut.alt.then_some(Default::default()),
                    shift: shortcut.shift.then_some(Default::default()),
                    command: shortcut.platform.then_some(Default::default()),
                    function: shortcut.function,
                };
                let key = if shortcut.key.is_empty() {
                    None
                } else {
                    Some(
                        hotkey_key(&shortcut.key)
                            .map_err(|message| color_eyre::eyre::eyre!(message))?,
                    )
                };
                let binding = HotkeyBinding { modifiers, key };
                if binding.is_empty() {
                    return Err(color_eyre::eyre::eyre!("shortcut cannot be empty"));
                }
                if binding.key.is_some()
                    && binding.modifiers.is_empty()
                    && !binding
                        .key
                        .as_ref()
                        .is_some_and(|key| is_function_key(&key.label))
                {
                    return Err(color_eyre::eyre::eyre!("shortcut requires a modifier"));
                }
                if hotkey_binding_conflicts(&self.settings, HotkeyKind::Dictation, &binding) {
                    return Err(color_eyre::eyre::eyre!("shortcut is already in use"));
                }
                let mut candidate = self.settings.clone();
                candidate.dictation_hotkey = binding;
                if !self.preview {
                    candidate.save()?;
                }
                self.settings = candidate;
                self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
                self.settings_dirty = false;
                self.settings_load_error = None;
            }
            DesktopAction::SetDoubleTapLock(enabled) => {
                let mut candidate = self.settings.clone();
                candidate.double_tap_lock = enabled;
                if !enabled {
                    candidate.double_tap_only = false;
                }
                if !self.preview {
                    candidate.save()?;
                }
                self.settings = candidate;
                self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
                self.settings_dirty = false;
                self.settings_load_error = None;
                self.double_tap_toggle.set_enabled(enabled);
            }
            DesktopAction::SetDoubleTapOnly(enabled) => {
                let mut candidate = self.settings.clone();
                candidate.double_tap_only = enabled
                    && candidate.double_tap_lock
                    && candidate.dictation_hotkey.key.is_some();
                if !self.preview {
                    candidate.save()?;
                }
                self.settings = candidate;
                self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
                self.settings_dirty = false;
                self.settings_load_error = None;
            }
        }
        Ok(())
    }
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model_notice = self.render_model_notice(cx);
        let content = match self.pane {
            Pane::HudLab => self.render_hud_lab(cx),
            Pane::Meetings => self.render_meetings(cx),
            Pane::Commands => self.render_commands(window, cx),
            Pane::Activity => self.render_activity(cx),
            Pane::History => self.render_history(cx),
            Pane::Modes => self.render_modes(window, cx),
            Pane::VoiceAction => self.render_voice_action(window, cx),
            Pane::Settings => self.render_settings(window, cx),
        };
        let transcription_picker = self
            .transcription_picker_language
            .clone()
            .map(|language| self.render_transcription_picker(&language, cx));
        let setup = self.setup_visible.then(|| self.render_setup(cx));
        let mode_context_menu = (self.pane == Pane::Modes)
            .then(|| self.render_mode_context_menu(cx))
            .flatten();
        window_frame()
            .track_focus(&self.window_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.history_retention_open {
                    this.history_retention_open = false;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.mode_context_menu.take().is_some() {
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.transcription_picker_language.is_some() {
                    cx.stop_propagation();
                    this.dismiss_transcription_picker(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.mode_context_menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.select_pane(Pane::Settings, cx);
                window.activate_window();
            }))
            .on_action(cx.listener(|this, _: &ShowModes, window, cx| {
                this.select_pane(Pane::Modes, cx);
                window.activate_window();
            }))
            .on_action(cx.listener(|this, _: &ShowVoiceAction, window, cx| {
                this.select_pane(Pane::VoiceAction, cx);
                window.activate_window();
            }))
            .on_action(cx.listener(|this, _: &ShowCommands, window, cx| {
                this.select_pane(Pane::Commands, cx);
                window.activate_window();
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                this.select_pane(Pane::History, cx);
                window.activate_window();
            }))
            .child(self.render_navigation(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .children(model_notice)
                    .child(div().flex_1().min_h_0().child(content)),
            )
            .children(setup)
            .children(transcription_picker)
            .children(mode_context_menu)
    }
}

/// Deterministic retained-history fixtures for the History pane preview.
fn preview_history() -> Option<History> {
    use crate::history::{HistoryDraft, HistoryProcessing, HistoryStore, now_ms};
    let directory =
        std::env::temp_dir().join(format!("hex-history-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).ok()?;
    let mut store = HistoryStore::open(
        directory.join("history.json"),
        HistoryRetention::Week,
        now_ms(),
    );
    let now = now_ms();
    let fixtures = [
        (
            26 * 60 * 60 * 1_000,
            HistoryKind::Dictation,
            "so um the parser needs a retry when the socket drops",
            "The parser needs a retry when the socket drops.",
            Some("Zed"),
            Some(("Work notes", 940)),
        ),
        (
            3 * 60 * 60 * 1_000,
            HistoryKind::Send,
            "sounds good shipping it after lunch",
            "Sounds good — shipping it after lunch.",
            Some("Slack"),
            Some(("Messages", 610)),
        ),
        (
            42 * 60 * 1_000,
            HistoryKind::VoiceAction,
            "write a haiku about warm tea",
            "Steam curls from the cup—\nafternoon light through the glass,\nthe first sip of calm.",
            Some("Brave Browser"),
            None,
        ),
        (
            4 * 60 * 1_000,
            HistoryKind::Dictation,
            "so um remember to publish the appcast after validating the artifact and uh \
             double check that the release notes mention the new capture placeholders \
             before anyone downloads it",
            "Remember to publish the appcast after validating the artifact, and double-check \
             that the release notes mention the new capture placeholders before anyone \
             downloads it.",
            Some("Messages"),
            None,
        ),
    ];
    for (age_ms, kind, raw, final_text, application, processing) in fixtures {
        let _ = store.record(
            HistoryDraft {
                kind,
                raw_text: raw.into(),
                final_text: final_text.into(),
                application: application.map(Into::into),
                processing: processing.map(|(profile, latency_ms)| HistoryProcessing {
                    profile: profile.into(),
                    latency_ms,
                    fallback: None,
                }),
                audio_ms: 2_800,
                inference_ms: 96,
                total_ms: 3_400,
            },
            now.saturating_sub(age_ms),
        );
    }
    Some(History::new(store))
}

fn voice_action_status_copy(enabled: bool) -> (&'static str, &'static str) {
    if enabled {
        (
            "Enabled",
            "Use the shortcut below with OpenCode to run spoken instructions.",
        )
    } else {
        (
            "Off",
            "Off by default. Its saved shortcut is not reserved until you enable Voice Action.",
        )
    }
}

fn set_voice_action_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled && hotkey_binding_conflicts(settings, HotkeyKind::Edit, &settings.edit_hotkey) {
        return Err(
            "The Voice Action shortcut is already in use. Change it below or change the other shortcut before enabling Voice Action.",
        );
    }
    settings.voice_action.enabled = enabled;
    Ok(())
}

fn voice_action_setting_row(
    title: &'static str,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    divider: bool,
    compact: bool,
) -> Div {
    div()
        .w_full()
        .min_h(px(64.0))
        .px_3()
        .py_3()
        .flex()
        .when(compact, |row| row.flex_col().items_start().gap_3())
        .when(!compact, |row| row.items_center().justify_between().gap_4())
        .when(divider, |row| row.border_b_1().border_color(rgb(LINE)))
        .child(
            div()
                .min_w(px(0.0))
                .when(!compact, |copy| copy.flex_1())
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .line_height(px(14.0))
                        .text_color(rgb(MUTED))
                        .child(description.into()),
                ),
        )
        .child(
            div()
                .when(compact, |field| field.w_full())
                .when(!compact, |field| field.flex_none())
                .child(control),
        )
}

fn hotkey_idle_width(keycap_count: usize) -> f32 {
    (112.0 + hotkey_keycaps_width(keycap_count)).max(HOTKEY_MIN_WIDTH)
}

const fn hotkey_kind_index(kind: HotkeyKind) -> usize {
    match kind {
        HotkeyKind::Dictation => 0,
        HotkeyKind::Edit => 1,
        HotkeyKind::PasteLast => 2,
    }
}

const fn hotkey_side_index(side: ModifierSide) -> usize {
    match side {
        ModifierSide::Left => 0,
        ModifierSide::Either => 1,
        ModifierSide::Right => 2,
    }
}

fn standalone_modifier_side(binding: &HotkeyBinding) -> Option<ModifierSide> {
    if binding.key.is_some() || binding.modifiers.count() != 1 {
        return None;
    }
    binding
        .modifiers
        .control
        .or(binding.modifiers.option)
        .or(binding.modifiers.shift)
        .or(binding.modifiers.command)
}

fn set_standalone_modifier_side(binding: &mut HotkeyBinding, side: ModifierSide) {
    if standalone_modifier_side(binding).is_none() {
        return;
    }
    if binding.modifiers.control.is_some() {
        binding.modifiers.control = Some(side);
    } else if binding.modifiers.option.is_some() {
        binding.modifiers.option = Some(side);
    } else if binding.modifiers.shift.is_some() {
        binding.modifiers.shift = Some(side);
    } else if binding.modifiers.command.is_some() {
        binding.modifiers.command = Some(side);
    }
}

fn hotkey_binding_conflicts(
    settings: &AppSettings,
    kind: HotkeyKind,
    binding: &HotkeyBinding,
) -> bool {
    let mut others = match kind {
        HotkeyKind::Dictation => Vec::new(),
        HotkeyKind::Edit | HotkeyKind::PasteLast => vec![settings.dictation_hotkey.clone()],
    };
    if kind != HotkeyKind::Edit && settings.voice_action.enabled {
        others.push(settings.edit_hotkey.clone());
    }
    if kind != HotkeyKind::PasteLast
        && let Some(paste) = &settings.paste_last_hotkey
    {
        others.push(paste.clone());
    }
    if crate::DEVELOPER_FEATURES_ENABLED {
        others.push(HotkeyBinding::paste_meeting_default());
    }
    crate::app_settings::hotkey_conflicts(binding, others)
}

fn hotkey_side_binding(
    settings: &AppSettings,
    kind: HotkeyKind,
    side: ModifierSide,
) -> Option<HotkeyBinding> {
    let mut binding = match kind {
        HotkeyKind::Dictation => &settings.dictation_hotkey,
        HotkeyKind::Edit => &settings.edit_hotkey,
        HotkeyKind::PasteLast => settings.paste_last_hotkey.as_ref()?,
    }
    .clone();
    standalone_modifier_side(&binding)?;
    set_standalone_modifier_side(&mut binding, side);
    (!hotkey_binding_conflicts(settings, kind, &binding)).then_some(binding)
}

fn hotkey_control_width(active: bool, idle_width: f32, animated_width: f32) -> f32 {
    if active { animated_width } else { idle_width }
}

fn hotkey_control_intensity(active: bool, intensity: f32) -> f32 {
    if active { intensity } else { 0.0 }
}

fn hotkey_capture_width(keycap_count: usize) -> f32 {
    if keycap_count == 0 {
        180.0
    } else {
        236.0 + hotkey_keycaps_width(keycap_count)
    }
}

fn hotkey_saved_width(keycap_count: usize) -> f32 {
    (53.0 + hotkey_keycaps_width(keycap_count)).max(HOTKEY_MIN_WIDTH)
}

fn hotkey_keycaps_width(keycap_count: usize) -> f32 {
    if keycap_count == 0 {
        0.0
    } else {
        34.0 * keycap_count as f32 + 3.0 * keycap_count.saturating_sub(1) as f32
    }
}

fn recording_audio_index(behavior: RecordingAudioBehavior) -> usize {
    match behavior {
        RecordingAudioBehavior::Mute => 0,
        RecordingAudioBehavior::PauseMedia => 1,
        RecordingAudioBehavior::DoNothing => 2,
    }
}

fn segmented_geometry(position: f32, widths: [f32; 3]) -> (f32, f32) {
    let position = position.clamp(0.0, 2.0);
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(2);
    let progress = position - lower as f32;
    let lefts = [2.0, 2.0 + widths[0], 2.0 + widths[0] + widths[1]];
    (
        lefts[lower] + (lefts[upper] - lefts[lower]) * progress,
        widths[lower] + (widths[upper] - widths[lower]) * progress,
    )
}

fn sound_volume_index(settings: &AppSettings) -> usize {
    if !settings.sound_effects {
        return 0;
    }
    [0.25_f32, 0.5, 0.75, 1.0]
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            (settings.sound_effect_volume - **left)
                .abs()
                .total_cmp(&(settings.sound_effect_volume - **right).abs())
        })
        .map_or(0, |(index, _)| index + 1)
}

fn open_opencode_beta_docs() {
    if let Err(error) = crate::commands::execute(crate::commands::Action::OpenUrl(
        OPENCODE_BETA_DOCS_URL.into(),
    )) {
        tracing::error!(%error, "could not open the OpenCode beta documentation");
    }
}

fn advance_highlight(current: usize, direction: i32, count: usize) -> usize {
    if direction < 0 {
        current.saturating_sub(1)
    } else {
        (current + 1).min(count - 1)
    }
}

fn mode_row(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    selected: bool,
    applications: &[String],
    browser_hosts: &[String],
    catalog: &ApplicationCatalogState,
) -> Div {
    let title = title.into();
    let subtitle = subtitle.into();
    let has_activations = !applications.is_empty() || !browser_hosts.is_empty();
    div()
        .w_full()
        .min_h(px(52.0))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .items_start()
        .justify_center()
        .gap_1()
        .rounded(px(6.0))
        .when(selected, |row| {
            row.bg(rgb(0x292929)).hover(|row| row.bg(rgb(0x303030)))
        })
        .when(!selected, |row| row.hover(|row| row.bg(rgb(SURFACE_HOVER))))
        .child(
            div()
                .w_full()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .truncate()
                .child(title),
        )
        .when(has_activations, |row| {
            row.child(mode_activation_icons(applications, browser_hosts, catalog))
        })
        .when(!has_activations, |row| {
            row.child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(if selected { TEXT_SOFT } else { FAINT }))
                    .truncate()
                    .child(subtitle),
            )
        })
}

fn mode_activation_icons(
    applications: &[String],
    browser_hosts: &[String],
    catalog: &ApplicationCatalogState,
) -> AnyElement {
    let total = applications.len() + browser_hosts.len();
    let visible = total.min(5);
    let mut icons = Vec::with_capacity(visible + usize::from(total > visible));
    for name in applications.iter().take(visible) {
        let application = match catalog {
            ApplicationCatalogState::Loaded(applications) => applications
                .iter()
                .find(|application| &application.name == name),
            _ => None,
        };
        icons.push(application_icon(application, Some(name), 18.0));
    }
    for host in browser_hosts
        .iter()
        .take(visible.saturating_sub(icons.len()))
    {
        let initial = host
            .chars()
            .find(|character| character.is_alphanumeric())
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into());
        icons.push(
            div()
                .size(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(rgb(0x444444))
                .bg(rgb(0xeeeeee))
                .text_size(px(8.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x202020))
                .child(initial)
                .into_any_element(),
        );
    }
    if total > visible {
        icons.push(
            div()
                .h(px(18.0))
                .px_1()
                .flex()
                .items_center()
                .rounded(px(4.0))
                .bg(rgb(0x303030))
                .text_size(px(8.0))
                .text_color(rgb(MUTED))
                .child(format!("+{}", total - visible))
                .into_any_element(),
        );
    }
    div()
        .h(px(18.0))
        .flex()
        .items_center()
        .gap_1()
        .children(icons)
        .into_any_element()
}

fn application_icon(
    application: Option<&InstalledApplication>,
    fallback_name: Option<&str>,
    size: f32,
) -> AnyElement {
    if let Some(icon) = application.and_then(|application| application.icon.as_ref()) {
        return img(icon.clone())
            .size(px(size))
            .rounded(px(size * 0.22))
            .into_any_element();
    }
    let initial = fallback_name
        .and_then(|name| name.chars().find(|character| character.is_alphanumeric()))
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.22))
        .bg(rgb(0x343434))
        .text_size(px(size * 0.42))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT_SOFT))
        .child(initial)
        .into_any_element()
}

fn filtered_model_keys(catalog: &ModelCatalog, query: &str) -> Vec<Option<String>> {
    let mut choices = Vec::new();
    if query.is_empty() && catalog.default_name.is_some() {
        choices.push(None);
    }
    choices.extend(
        catalog
            .models
            .iter()
            .filter(|choice| {
                query.is_empty()
                    || choice.key.to_lowercase().contains(query)
                    || choice.name.to_lowercase().contains(query)
                    || choice.provider.to_lowercase().contains(query)
            })
            .take(8)
            .map(|choice| Some(choice.key.clone())),
    );
    choices
}

fn model_presentation(
    catalog: &ModelCatalogState,
    selected_key: Option<&str>,
) -> Option<ModelPresentation> {
    let ModelCatalogState::Loaded(catalog) = catalog else {
        return selected_key.map(fallback_model_presentation);
    };
    let is_default = selected_key.is_none();
    let key = selected_key.or(catalog.default_key.as_deref())?;
    let choice = catalog.models.iter().find(|choice| choice.key == key);
    let name = choice
        .map(|choice| choice.name.clone())
        .or_else(|| is_default.then(|| catalog.default_name.clone()).flatten())
        .unwrap_or_else(|| model_id(key).to_owned());
    let provider = choice
        .map(|choice| provider_label(&choice.provider))
        .unwrap_or_else(|| {
            provider_label(key.split_once('/').map_or(key, |(provider, _)| provider))
        });
    Some(ModelPresentation {
        name,
        provider,
        key: key.to_owned(),
        is_default,
    })
}

fn apply_model_variant(
    settings: &mut AppSettings,
    target: ModelPickerTarget,
    variant: Option<String>,
    catalog: &ModelCatalogState,
) {
    let (model, selected_variant) = match target {
        ModelPickerTarget::Mode(selection) => {
            let processing = match selection {
                ModeSelection::Default => {
                    &mut settings.dictation_processing.default_mode.post_processing
                }
                ModeSelection::Custom(index) => {
                    &mut settings.dictation_processing.modes[index].post_processing
                }
            };
            (&mut processing.model, &mut processing.variant)
        }
        ModelPickerTarget::VoiceAction => (
            &mut settings.voice_action.model,
            &mut settings.voice_action.variant,
        ),
    };
    if variant.is_some()
        && model.is_none()
        && let ModelCatalogState::Loaded(catalog) = catalog
    {
        // V2 variants belong to an explicit catalog model, not the floating default.
        *model = catalog.default_key.clone();
    }
    *selected_variant = variant;
}

fn model_variants(catalog: &ModelCatalog, selected_key: Option<&str>) -> Vec<String> {
    selected_key
        .or(catalog.default_key.as_deref())
        .and_then(|key| catalog.models.iter().find(|choice| choice.key == key))
        .map_or_else(Vec::new, |choice| choice.variants.clone())
}

fn fallback_model_presentation(key: &str) -> ModelPresentation {
    let provider = key.split_once('/').map_or(key, |(provider, _)| provider);
    ModelPresentation {
        name: model_id(key).to_owned(),
        provider: provider_label(provider),
        key: key.to_owned(),
        is_default: false,
    }
}

fn model_id(key: &str) -> &str {
    key.split_once('/').map_or(key, |(_, model)| model)
}

fn provider_label(provider: &str) -> String {
    if provider.eq_ignore_ascii_case("opencode") {
        return "OpenCode".into();
    }
    provider
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn model_choice_row(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    highlighted: bool,
) -> Div {
    div()
        .w_full()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .border_b_1()
        .border_color(rgb(LINE))
        .when(highlighted, |row| row.bg(rgb(SURFACE_SELECTED)))
        .hover(|row| row.bg(rgb(SURFACE_HOVER)))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(TEXT_SOFT))
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(FAINT))
                .child(subtitle.into()),
        )
}

fn settings_input(
    label: &'static str,
    description: impl Into<String>,
    input: Entity<TextInput>,
) -> AnyElement {
    settings_control(label, description, input)
}

fn compact_mode_field(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(9.0)).text_color(rgb(FAINT)).child(label))
        .child(control)
}

fn settings_control(
    label: &'static str,
    description: impl Into<String>,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_SOFT))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(FAINT))
                        .child(description.into()),
                ),
        )
        .child(control)
        .into_any_element()
}

fn input_text(input: &ProcessingInput, cx: &App) -> String {
    input.entity.read(cx).text().trim().to_string()
}

fn browser_hosts(value: &str) -> Vec<String> {
    let mut hosts: Vec<_> = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| {
            let candidate = if value.contains("://") {
                value.to_owned()
            } else {
                format!("https://{value}")
            };
            url::Url::parse(&candidate)
                .ok()?
                .host_str()
                .map(str::to_lowercase)
        })
        .collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

fn reorder_transformation(values: &mut Vec<String>, id: &str, target_index: usize) -> bool {
    let Some(from) = values.iter().position(|candidate| candidate == id) else {
        return false;
    };
    if from == target_index {
        return false;
    }
    let value = values.remove(from);
    let target = target_index.min(values.len());
    values.insert(target, value);
    true
}

fn apply_mode_inputs(inputs: &ModeInputs, mode: &mut DictationMode, is_default: bool, cx: &App) {
    mode.name = if is_default {
        "Global".into()
    } else {
        input_text(&inputs.name, cx)
    };
    mode.browser_hosts = if is_default {
        Vec::new()
    } else {
        browser_hosts(&input_text(&inputs.browser_hosts, cx))
    };
    for (inputs, replacement) in inputs.replacements.iter().zip(&mut mode.replacements) {
        replacement.matched_phrase = inputs.matched_phrase.entity.read(cx).text().to_string();
        replacement.output = inputs.output.entity.read(cx).text().to_string();
    }
    mode.post_processing.enabled = inputs.processing_toggle.enabled();
    mode.post_processing.prompt = input_text(&inputs.prompt, cx);
    mode.post_processing.deadline_seconds = input_text(&inputs.deadline, cx)
        .parse::<u64>()
        .unwrap_or(mode.post_processing.deadline_seconds)
        .max(1);
}

fn dictation_model_notice(
    status: SetupStatus,
    preparing: bool,
) -> Option<(&'static str, &'static str)> {
    if status.transcription_model {
        return None;
    }
    Some(if preparing {
        (
            "Preparing speech model",
            "Dictation will be available once your local speech model is ready.",
        )
    } else {
        (
            "Download a speech model to start dictating",
            "The dictation shortcut needs a local speech model. Choose one to download first.",
        )
    })
}

fn command_model_notice(status: &CommandModelStatus) -> Option<(&'static str, &'static str)> {
    match status {
        CommandModelStatus::Disabled | CommandModelStatus::Ready => None,
        CommandModelStatus::Loading => Some((
            "Preparing voice commands",
            "The command model is loading. Hotkey dictation does not require it.",
        )),
        CommandModelStatus::Failed => Some((
            "Voice commands are unavailable",
            "The command model could not load. Retry or turn off commands; hotkey dictation is unaffected.",
        )),
    }
}

fn setup_row(title: &'static str, description: &'static str, control: AnyElement) -> Div {
    div()
        .w_full()
        .min_h(px(70.0))
        .py_3()
        .flex()
        .items_center()
        .gap_4()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(settings_copy(title, description)),
        )
        .child(div().flex_shrink_0().child(control))
}

fn setup_group_label(label: &'static str) -> Div {
    div()
        .pb_2()
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(label)
}

fn setup_ready_badge() -> AnyElement {
    div()
        .h(px(28.0))
        .px_3()
        .flex()
        .items_center()
        .rounded_sm()
        .bg(rgb(0x17231a))
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(0x91bd99))
        .child("Ready")
        .into_any_element()
}

const fn permission_warning_copy(kind: PermissionKind) -> (&'static str, &'static str) {
    match kind {
        PermissionKind::Microphone => (
            "Microphone access is off",
            "HEX cannot record dictation until microphone access is restored.",
        ),
        PermissionKind::InputMonitoring => (
            "Input Monitoring is off",
            "HEX cannot recognize the dictation shortcut in other apps.",
        ),
        PermissionKind::Accessibility => (
            "Accessibility is off",
            "HEX cannot paste completed dictation into the foreground app.",
        ),
    }
}

const fn permission_action_label(action: PermissionAction) -> &'static str {
    match action {
        PermissionAction::RequestMicrophone => "Allow",
        PermissionAction::OpenMicrophoneSettings => "Open Settings",
        PermissionAction::OpenInputMonitoringSettings
        | PermissionAction::OpenAccessibilitySettings => "Grant Access",
    }
}

const fn permission_warning_id(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "permission-warning-microphone",
        PermissionKind::InputMonitoring => "permission-warning-input-monitoring",
        PermissionKind::Accessibility => "permission-warning-accessibility",
    }
}

fn hotkey_modifiers(modifiers: GpuiModifiers) -> HotkeyModifiers {
    hotkey_modifiers_with_flags(modifiers, crate::suppression::physical_modifier_flags())
}

fn hotkey_modifiers_with_flags(modifiers: GpuiModifiers, flags: u64) -> HotkeyModifiers {
    let physical = crate::app_settings::modifiers_from_flags(flags);
    if !physical.is_empty() {
        return physical;
    }
    HotkeyModifiers {
        control: modifiers.control.then_some(Default::default()),
        option: modifiers.alt.then_some(Default::default()),
        shift: modifiers.shift.then_some(Default::default()),
        command: modifiers.platform.then_some(Default::default()),
        function: modifiers.function,
    }
}

fn hotkey_key(key: &str) -> std::result::Result<HotkeyKey, &'static str> {
    let special = match key {
        "space" => Some((49, "Space")),
        "tab" => Some((48, "Tab")),
        "enter" => Some((36, "Return")),
        "backspace" => Some((51, "Delete")),
        "up" => Some((126, "Up")),
        "down" => Some((125, "Down")),
        "left" => Some((123, "Left")),
        "right" => Some((124, "Right")),
        "pageup" => Some((116, "Page Up")),
        "pagedown" => Some((121, "Page Down")),
        "home" => Some((115, "Home")),
        "end" => Some((119, "End")),
        "delete" => Some((117, "Forward Delete")),
        "insert" => Some((114, "Help")),
        "f1" => Some((122, "F1")),
        "f2" => Some((120, "F2")),
        "f3" => Some((99, "F3")),
        "f4" => Some((118, "F4")),
        "f5" => Some((96, "F5")),
        "f6" => Some((97, "F6")),
        "f7" => Some((98, "F7")),
        "f8" => Some((100, "F8")),
        "f9" => Some((101, "F9")),
        "f10" => Some((109, "F10")),
        "f11" => Some((103, "F11")),
        "f12" => Some((111, "F12")),
        "f13" => Some((105, "F13")),
        "f14" => Some((107, "F14")),
        "f15" => Some((113, "F15")),
        "f16" => Some((106, "F16")),
        "f17" => Some((64, "F17")),
        "f18" => Some((79, "F18")),
        "f19" => Some((80, "F19")),
        "f20" => Some((90, "F20")),
        _ => None,
    };
    if let Some((code, label)) = special {
        return Ok(HotkeyKey {
            code,
            label: label.into(),
        });
    }
    let mut characters = key.chars();
    let Some(character) = characters.next() else {
        return Err("Unsupported key");
    };
    if characters.next().is_some() {
        return Err("Unsupported key");
    }
    let code = crate::keyboard::key_code_for(character).map_err(|_| "Unsupported key")?;
    Ok(HotkeyKey {
        code,
        label: character.to_uppercase().collect(),
    })
}

fn is_function_key(label: &str) -> bool {
    label
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
}

fn detail_placeholder(message: &'static str) -> AnyElement {
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

fn detail_row(label: &'static str, value: impl Into<String>) -> AnyElement {
    div()
        .py_3()
        .flex()
        .items_start()
        .gap_4()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .w(px(92.0))
                .flex_none()
                .text_size(px(11.0))
                .text_color(rgb(FAINT))
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .line_height(px(18.0))
                .text_color(rgb(TEXT_SOFT))
                .child(value.into()),
        )
        .into_any_element()
}

fn command_task(command: &CommandInfo) -> String {
    if let Some(group) = &command.group {
        return group.clone();
    }
    if command.id.starts_with("mode.") {
        "Voice control".into()
    } else if command.id.starts_with("dictation.") {
        "Dictation".into()
    } else if command.id.starts_with("meeting.") {
        "Meetings".into()
    } else {
        match command.description.as_str() {
            "Open application" => "Open applications",
            "Open website" => "Open websites",
            "Navigate active tab" => "Browser navigation",
            "Use keyboard shortcut" => "Keyboard",
            "Use application shortcut" | "Open application destination" => "Application navigation",
            _ => "Other",
        }
        .into()
    }
}

fn merged_catalog(compiled: &[CatalogCommand], personal: &StatusSnapshot) -> Vec<CatalogCommand> {
    let mut commands = compiled.to_vec();
    commands.extend(personal.catalog.iter().filter_map(|command| {
        let (phrase, aliases) = command.phrases.split_first()?;
        Some(CatalogCommand {
            task: command.group.clone(),
            scope: match &command.context {
                StatusContext::Global => CommandScope::Global,
                StatusContext::Application { application } => {
                    CommandScope::Application(application.clone())
                }
                StatusContext::BrowserHost { browser_host } => {
                    CommandScope::Browser(browser_host.clone())
                }
            },
            phrase: phrase.clone(),
            aliases: aliases.to_vec(),
            description: command.description.clone(),
            id: command.id.clone(),
        })
    }));
    commands
}

fn open_path(path: &std::path::Path) {
    if let Err(error) = ProcessCommand::new("/usr/bin/open").arg(path).spawn() {
        tracing::error!(%error, path = %path.display(), "could not open personal command path");
    }
}

fn command_scope(scope: &CommandScope) -> String {
    match scope {
        CommandScope::Sleeping | CommandScope::Global => "Global".into(),
        CommandScope::Application(application) => application.clone(),
        CommandScope::Browser(host) => host.clone(),
    }
}

fn meeting_status(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Starting => "Starting",
        MeetingStatus::Recording => "Recording",
        MeetingStatus::Transcribing => "Finalizing",
        MeetingStatus::Complete => "Complete",
        MeetingStatus::Interrupted => "Interrupted",
        MeetingStatus::Failed => "Failed",
    }
}

fn meeting_progress_action(label: &'static str) -> AnyElement {
    div()
        .h(px(30.0))
        .px_3()
        .flex()
        .items_center()
        .rounded_sm()
        .bg(rgb(SURFACE))
        .text_size(px(12.0))
        .text_color(rgb(MUTED))
        .child(label)
        .into_any_element()
}

fn log_meeting_request_error(error: TrySendError<MeetingRequest>) {
    match error {
        TrySendError::Full(_) => tracing::warn!("meeting controller is busy"),
        TrySendError::Disconnected(_) => tracing::error!("meeting controller is unavailable"),
    }
}

fn meeting_duration(meeting: &MeetingManifest) -> u64 {
    meeting.duration_ms.unwrap_or_else(|| {
        if matches!(
            meeting.status,
            MeetingStatus::Starting | MeetingStatus::Recording
        ) {
            now_ms().saturating_sub(meeting.created_at_ms)
        } else {
            meeting
                .ended_at_ms
                .map_or(0, |ended| ended.saturating_sub(meeting.created_at_ms))
        }
    })
}

fn activity_context(reader: &EventReader) -> (Option<String>, Option<String>) {
    reader
        .current_context()
        .map(|(application, browser_url)| {
            (
                application.clone(),
                browser_url
                    .as_deref()
                    .and_then(|url| url::Url::parse(url).ok())
                    .and_then(|url| url.host_str().map(str::to_owned)),
            )
        })
        .unwrap_or_default()
}

fn event_age(timestamp_ms: u64) -> String {
    let seconds = now_ms().saturating_sub(timestamp_ms) / 1_000;
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn event_summary(event: &VoiceEvent) -> String {
    match event {
        VoiceEvent::SessionStarted { .. } => "HEX session started".into(),
        VoiceEvent::State { state, device, .. } => match state {
            VoiceState::Sleeping => "Voice commands went to sleep".into(),
            VoiceState::Listening => format!("Voice commands resumed on {device}"),
            VoiceState::Dictating => format!("Dictation started on {device}"),
            VoiceState::Transcribing => "Dictation is being transcribed".into(),
            VoiceState::Stopping => "HEX is stopping".into(),
        },
        VoiceEvent::Transcript {
            phase,
            latency_ms,
            text,
            ..
        } => match phase {
            TranscriptPhase::Started => "Speech recognition started".into(),
            TranscriptPhase::Updated => format!("Heard \"{text}\" so far"),
            TranscriptPhase::Completed => format!("Heard \"{text}\" in {latency_ms} ms"),
        },
        VoiceEvent::Command {
            heard,
            command,
            outcome,
            ..
        } => {
            let command = command.as_deref().unwrap_or("an unmatched command");
            match outcome {
                CommandOutcome::Ignored => format!("Ignored \"{heard}\""),
                CommandOutcome::Woke => format!("\"{heard}\" woke HEX"),
                CommandOutcome::Slept => format!("\"{heard}\" put voice commands to sleep"),
                CommandOutcome::Submitted => format!("Sent {command} for \"{heard}\""),
                CommandOutcome::Executed => format!("Ran {command} for \"{heard}\""),
                CommandOutcome::Failed(error) => format!("Could not run {command}: {error}"),
            }
        }
        VoiceEvent::Dictation {
            phase,
            text,
            processing,
            ..
        } => {
            let description = match phase {
                DictationPhase::Started => "Dictation started".into(),
                DictationPhase::Discarded => "A brief dictation was discarded".into(),
                DictationPhase::Cancelled => "Dictation was cancelled".into(),
                DictationPhase::Transcribing => "Dictation is being transcribed".into(),
                DictationPhase::Pasted => text_or("Dictation was pasted", text),
                DictationPhase::Edited => text_or("Selected text was edited", text),
                DictationPhase::VoiceAction => text_or("Voice Action completed", text),
                DictationPhase::Logged => text_or("Journal entry was saved", text),
                DictationPhase::Repasted => text_or("The last dictation was pasted again", text),
                DictationPhase::MeetingPasted => text_or("New meeting transcript was pasted", text),
                DictationPhase::Failed(error) => format!("Dictation failed: {error}"),
            };
            processing
                .as_ref()
                .map_or(description.clone(), |processing| {
                    processing.fallback.as_ref().map_or_else(
                        || {
                            format!(
                                "{description} · {} profile · {} ms",
                                processing.profile, processing.latency_ms
                            )
                        },
                        |error| {
                            format!(
                                "{description} · {} profile fell back to raw after {} ms: {}",
                                processing.profile, processing.latency_ms, error
                            )
                        },
                    )
                })
        }
        VoiceEvent::Context {
            application,
            browser_url,
            ..
        } => match (application, browser_url) {
            (Some(application), Some(url)) => format!("Context changed to {application} / {url}"),
            (Some(application), None) => format!("Context changed to {application}"),
            _ => "Foreground context became unavailable".into(),
        },
        VoiceEvent::ApiServerStarted { port, .. } => {
            format!("Local API started on port {port}")
        }
        VoiceEvent::ApiServerStopped { .. } => "Local API stopped".into(),
        VoiceEvent::ApiAuthFailed { method, path, .. } => {
            format!("Rejected unauthenticated {method} {path}")
        }
    }
}

fn text_or(prefix: &str, text: &str) -> String {
    if text.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}: \"{text}\"")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::personal_commands::StatusExecution;

    #[test]
    fn command_model_failure_has_recovery_without_blocking_dictation() {
        assert!(command_model_notice(&CommandModelStatus::Ready).is_none());
        assert!(command_model_notice(&CommandModelStatus::Disabled).is_none());
        assert_eq!(
            command_model_notice(&CommandModelStatus::Loading)
                .unwrap()
                .0,
            "Preparing voice commands"
        );
        let (_, description) = command_model_notice(&CommandModelStatus::Failed).unwrap();
        assert!(description.contains("Retry"));
        assert!(description.contains("dictation is unaffected"));
    }

    #[test]
    fn opening_the_app_prioritizes_settings_and_missing_permissions() {
        assert_eq!(Pane::default(), Pane::Settings);
        let ready = SetupStatus {
            microphone: PermissionState::Ready,
            input_monitoring: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: true,
        };
        assert_eq!(Pane::Modes.on_reopen(ready), Pane::Modes);
        for status in [
            SetupStatus {
                microphone: PermissionState::NeedsRequest,
                ..ready
            },
            SetupStatus {
                input_monitoring: PermissionState::NeedsSettings,
                ..ready
            },
            SetupStatus {
                accessibility: PermissionState::NeedsSettings,
                ..ready
            },
        ] {
            assert_eq!(Pane::Modes.on_reopen(status), Pane::Settings);
            assert_eq!(Pane::VoiceAction.on_reopen(status), Pane::Settings);
            assert_eq!(Pane::Settings.on_reopen(status), Pane::Settings);
        }
        assert_eq!(Pane::from_preview(PreviewPane::Modes), Pane::Modes);
    }

    #[test]
    fn missing_dictation_model_has_a_notice_even_with_all_permissions_granted() {
        let mut status = SetupStatus {
            microphone: PermissionState::Ready,
            input_monitoring: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: false,
        };
        let (title, description) = dictation_model_notice(status, false).unwrap();
        assert_eq!(title, "Download a speech model to start dictating");
        assert!(description.contains("dictation shortcut"));
        assert!(!status.ready());

        assert_eq!(
            dictation_model_notice(status, true).unwrap().0,
            "Preparing speech model"
        );
        // A failed or cancelled preparation must expose the download action again.
        assert_eq!(dictation_model_notice(status, false).unwrap().0, title);

        status.transcription_model = true;
        assert!(dictation_model_notice(status, false).is_none());
        assert!(dictation_model_notice(status, true).is_none());

        // Permission failures have their own setup UI.
        status.microphone = PermissionState::NeedsRequest;
        assert!(dictation_model_notice(status, false).is_none());
    }

    #[test]
    fn inactive_hotkey_controls_keep_their_idle_width() {
        assert_eq!(hotkey_control_width(false, 146.0, 270.0), 146.0);
        assert_eq!(hotkey_control_width(true, 146.0, 270.0), 270.0);
        assert_eq!(hotkey_control_intensity(false, 0.8), 0.0);
        assert_eq!(hotkey_control_intensity(true, 0.8), 0.8);
    }

    #[test]
    fn voice_action_defaults_off_and_explains_its_inactive_shortcut() {
        let settings = AppSettings::default();
        assert!(!settings.voice_action.enabled);
        let (status, description) = voice_action_status_copy(settings.voice_action.enabled);
        assert_eq!(status, "Off");
        assert!(description.contains("Off by default"));
        assert!(description.contains("not reserved"));

        let (status, description) = voice_action_status_copy(true);
        assert_eq!(status, "Enabled");
        assert!(description.contains("OpenCode"));
        assert!(!description.contains("not reserved"));
    }

    #[test]
    fn disabled_voice_action_does_not_reserve_its_shortcut() {
        let mut settings = AppSettings::default();
        for enabled in [false, true] {
            settings.voice_action.enabled = enabled;
            for kind in [HotkeyKind::Dictation, HotkeyKind::PasteLast] {
                assert_eq!(
                    hotkey_binding_conflicts(&settings, kind, &settings.edit_hotkey),
                    enabled,
                );
            }
        }
        settings.voice_action.enabled = false;
        assert!(hotkey_binding_conflicts(
            &settings,
            HotkeyKind::Edit,
            &settings.dictation_hotkey,
        ));
    }

    #[test]
    fn voice_action_opt_in_rejects_active_conflicts_without_rebinding() {
        let defaults = AppSettings::default();
        let mut active = vec![
            defaults.dictation_hotkey,
            defaults.paste_last_hotkey.unwrap(),
        ];
        if crate::DEVELOPER_FEATURES_ENABLED {
            active.push(HotkeyBinding::paste_meeting_default());
        }
        for binding in active {
            let mut settings = AppSettings {
                edit_hotkey: binding,
                ..AppSettings::default()
            };
            let before = serde_json::to_value(&settings).unwrap();
            let error = set_voice_action_enabled(&mut settings, true).unwrap_err();

            assert!(error.contains("already in use"));
            assert!(error.contains("Change it below"));
            assert_eq!(serde_json::to_value(&settings).unwrap(), before);
        }
    }

    #[test]
    fn voice_action_toggle_preserves_configuration_and_can_always_disable() {
        let mut settings: AppSettings = serde_json::from_value(serde_json::json!({
            "voice_action": {
                "model": "example/saved-model",
                "variant": "high",
                "deadline_seconds": 45
            },
            "edit_hotkey": {
                "modifiers": { "control": "left" }
            }
        }))
        .unwrap();
        assert!(!settings.voice_action.enabled);
        let mut expected = serde_json::to_value(&settings).unwrap();
        for enabled in [true, false] {
            set_voice_action_enabled(&mut settings, enabled).unwrap();
            expected["voice_action"]["enabled"] = serde_json::json!(enabled);
            assert_eq!(serde_json::to_value(&settings).unwrap(), expected);
        }

        set_voice_action_enabled(&mut settings, true).unwrap();
        settings.dictation_hotkey = settings.edit_hotkey.clone();
        set_voice_action_enabled(&mut settings, false).unwrap();
        assert!(!settings.voice_action.enabled);
    }

    #[test]
    fn shortcut_side_changes_reject_overlap_without_changing_the_saved_binding() {
        let mut settings = AppSettings::default();
        settings.voice_action.enabled = true;
        settings.dictation_hotkey.modifiers.option = Some(ModifierSide::Left);
        settings.edit_hotkey = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            },
            key: None,
        };
        let original = settings.dictation_hotkey.clone();

        assert!(
            hotkey_side_binding(&settings, HotkeyKind::Dictation, ModifierSide::Right).is_none()
        );
        assert!(
            hotkey_side_binding(&settings, HotkeyKind::Dictation, ModifierSide::Either).is_none()
        );
        assert_eq!(settings.dictation_hotkey, original);
        assert_eq!(
            hotkey_side_binding(&settings, HotkeyKind::Dictation, ModifierSide::Left),
            Some(original)
        );

        settings.edit_hotkey = HotkeyBinding::edit_default();
        let candidate =
            hotkey_side_binding(&settings, HotkeyKind::Dictation, ModifierSide::Right).unwrap();
        assert_eq!(
            standalone_modifier_side(&candidate),
            Some(ModifierSide::Right)
        );
    }

    #[test]
    fn side_selection_only_changes_a_standalone_modifier() {
        let mut standalone = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: None,
        };
        set_standalone_modifier_side(&mut standalone, ModifierSide::Either);
        assert_eq!(
            standalone_modifier_side(&standalone),
            Some(ModifierSide::Either)
        );

        let mut chord = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 0,
                label: "A".into(),
            }),
        };
        set_standalone_modifier_side(&mut chord, ModifierSide::Right);
        assert_eq!(chord.modifiers.option, Some(ModifierSide::Left));
    }

    #[test]
    fn production_navigation_keeps_commands_available_as_an_opt_in() {
        assert_eq!(
            Pane::all(DesktopCapabilities::macos(false)),
            vec![
                Pane::Settings,
                Pane::Modes,
                Pane::Commands,
                Pane::VoiceAction,
                Pane::History,
            ]
        );
    }

    #[test]
    fn application_catalog_loads_only_when_explicitly_requested() {
        let loading = ApplicationCatalogState::Loading;
        let loaded = ApplicationCatalogState::Loaded(Vec::new());

        assert!(application_catalog_load_needed(false, &loading, false));
        assert!(!application_catalog_load_needed(false, &loading, true));
        assert!(!application_catalog_load_needed(false, &loaded, false));
        assert!(!application_catalog_load_needed(true, &loading, false));
    }

    #[test]
    fn hotkey_capture_preserves_the_function_modifier() {
        let modifiers = hotkey_modifiers_with_flags(
            GpuiModifiers {
                function: true,
                ..Default::default()
            },
            0,
        );

        assert!(modifiers.function);
        assert_eq!(modifiers.count(), 1);
    }

    #[test]
    fn toggle_spring_is_frame_rate_independent_and_settles() {
        let simulate = |frame_rate: u32| {
            let mut spring = ToggleSpring::new(false);
            spring.set_enabled(true);
            for _ in 0..frame_rate {
                spring.advance(Duration::from_secs_f32(1.0 / frame_rate as f32));
            }
            spring
        };

        let at_60_hz = simulate(60);
        let at_120_hz = simulate(120);
        assert!(at_60_hz.is_settled());
        assert!(at_120_hz.is_settled());
        assert!((at_60_hz.position - at_120_hz.position).abs() < 0.001);
        assert_eq!(at_60_hz.position, 1.0);
    }

    #[test]
    fn recent_activity_keeps_context_older_than_the_visible_window() {
        let path = std::env::temp_dir().join(format!(
            "voice-control-app-window-{}-{}.ndjson",
            std::process::id(),
            crate::events::now_ms()
        ));
        let mut lines = vec![
            serde_json::to_string(&VoiceEvent::Context {
                timestamp_ms: 1,
                application: Some("Brave Browser".into()),
                browser_url: Some("https://meet.google.com/example".into()),
            })
            .unwrap(),
        ];
        lines.extend((0..=ACTIVITY_LIMIT).map(|timestamp_ms| {
            serde_json::to_string(&VoiceEvent::State {
                timestamp_ms: timestamp_ms as u64 + 2,
                state: VoiceState::Listening,
                device: "Test microphone".into(),
            })
            .unwrap()
        }));
        fs::write(&path, lines.join("\n")).unwrap();

        let mut reader = EventReader::open(&path);
        reader.refresh().unwrap();

        assert_eq!(reader.recent(ACTIVITY_LIMIT).len(), ACTIVITY_LIMIT);
        assert_eq!(
            activity_context(&reader),
            (Some("Brave Browser".into()), Some("meet.google.com".into()))
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn browser_hosts_are_canonicalized_and_deduplicated() {
        assert_eq!(
            browser_hosts(" https://www.Reddit.com/r/rust, , reddit.com, github.com/path"),
            ["github.com", "reddit.com", "www.reddit.com"]
        );
    }

    #[test]
    fn transformations_reorder_in_both_directions() {
        let mut transformations = vec!["one".into(), "two".into(), "three".into()];

        assert!(reorder_transformation(&mut transformations, "one", 2));
        assert_eq!(transformations, ["two", "three", "one"]);
        assert!(reorder_transformation(&mut transformations, "one", 0));
        assert_eq!(transformations, ["one", "two", "three"]);
    }

    #[test]
    fn transformation_reorder_ignores_same_position_and_missing_ids() {
        let mut transformations = vec!["one".into(), "two".into()];

        assert!(!reorder_transformation(&mut transformations, "one", 0));
        assert!(!reorder_transformation(&mut transformations, "missing", 1));
        assert_eq!(transformations, ["one", "two"]);
    }

    #[test]
    fn picker_arrows_stop_at_the_ends() {
        assert_eq!(advance_highlight(0, -1, 4), 0);
        assert_eq!(advance_highlight(2, 1, 4), 3);
        assert_eq!(advance_highlight(3, 1, 4), 3);
    }

    #[test]
    fn selected_transcription_model_is_not_active_until_installed() {
        let selection = TranscriptionSelection::default();
        let model = definition(selection.model);

        assert!(!transcription_selection_is_active(
            &selection,
            model,
            &selection.language,
            false
        ));
        assert!(transcription_selection_is_active(
            &selection,
            model,
            &selection.language,
            true
        ));
    }

    #[test]
    fn selected_model_presents_provider_name_and_resolved_key() {
        let catalog = ModelCatalogState::Loaded(ModelCatalog {
            models: vec![crate::dictation_processor::ModelChoice {
                key: "opencode/example-rewrite".into(),
                name: "Example Rewrite".into(),
                provider: "opencode".into(),
                variants: Vec::new(),
            }],
            default_key: Some("opencode/example-rewrite".into()),
            default_name: Some("Example Rewrite".into()),
        });

        let selected = model_presentation(&catalog, Some("opencode/example-rewrite")).unwrap();
        assert_eq!(selected.name, "Example Rewrite");
        assert_eq!(selected.provider, "OpenCode");
        assert_eq!(selected.key, "opencode/example-rewrite");
        assert!(!selected.is_default);

        let default = model_presentation(&catalog, None).unwrap();
        assert!(default.is_default);
    }

    #[test]
    fn model_search_finds_provider_models_with_nested_ids() {
        let catalog = ModelCatalog {
            models: vec![crate::dictation_processor::ModelChoice {
                key: "console-groq/openai/gpt-oss-120b".into(),
                name: "GPT OSS 120B".into(),
                provider: "console-groq".into(),
                variants: Vec::new(),
            }],
            default_key: None,
            default_name: None,
        };

        assert_eq!(
            filtered_model_keys(&catalog, "gpt oss"),
            [Some("console-groq/openai/gpt-oss-120b".into())]
        );
    }

    #[test]
    fn opencode_failure_copy_preserves_the_actionable_error() {
        let state = ModelCatalogState::Failed(
            "OpenCode /api/model failed: config.providers was invalid".into(),
        );

        let copy = opencode_unavailable_copy(&state).unwrap();

        assert_eq!(copy.title, "OpenCode is unavailable");
        assert_eq!(
            copy.description,
            "HEX could not load the OpenCode model catalog."
        );
        assert_eq!(
            copy.error,
            Some("OpenCode /api/model failed: config.providers was invalid")
        );
        assert!(copy.can_retry);
        assert!(!copy.can_open_setup);
    }

    #[test]
    fn opencode_missing_copy_offers_setup_without_an_error() {
        let copy = opencode_unavailable_copy(&ModelCatalogState::Missing).unwrap();

        assert_eq!(copy.title, "Voice Action requires OpenCode");
        assert_eq!(copy.error, None);
        assert!(copy.can_retry);
        assert!(copy.can_open_setup);
    }

    #[test]
    fn opencode_errors_are_bounded_without_splitting_utf8() {
        let error = "é".repeat(MAX_OPENCODE_ERROR_BYTES);
        let bounded = bounded_opencode_error(&error);

        assert!(bounded.ends_with('…'));
        assert!(bounded.is_char_boundary(bounded.len()));
        assert!(bounded.len() <= MAX_OPENCODE_ERROR_BYTES + '…'.len_utf8());
    }

    #[test]
    fn default_model_exposes_its_thinking_variants() {
        let catalog = ModelCatalog {
            models: vec![crate::dictation_processor::ModelChoice {
                key: "opencode/example-rewrite".into(),
                name: "Example Rewrite".into(),
                provider: "opencode".into(),
                variants: vec!["low".into(), "high".into()],
            }],
            default_key: Some("opencode/example-rewrite".into()),
            default_name: Some("Example Rewrite".into()),
        };

        assert_eq!(model_variants(&catalog, None), ["low", "high"]);
    }

    #[test]
    fn model_variant_selection_resolves_defaults_without_replacing_explicit_models() {
        let catalog = ModelCatalogState::Loaded(ModelCatalog {
            models: Vec::new(),
            default_key: Some("example/rewrite-default".into()),
            default_name: Some("Example Rewrite".into()),
        });
        for (target, path) in [
            (
                ModelPickerTarget::Mode(ModeSelection::Default),
                "/dictation_processing/default_mode/post_processing",
            ),
            (
                ModelPickerTarget::Mode(ModeSelection::Custom(0)),
                "/dictation_processing/modes/0/post_processing",
            ),
            (ModelPickerTarget::VoiceAction, "/voice_action"),
        ] {
            for model in [None, Some("example/explicit-selection")] {
                for variant in [None, Some("high")] {
                    let mut settings: AppSettings = serde_json::from_value(serde_json::json!({
                        "dictation_processing": {
                            "default_mode": { "post_processing": { "model": model, "variant": "low" } },
                            "modes": [{ "post_processing": { "model": model, "variant": "low" } }]
                        },
                        "voice_action": { "model": model, "variant": "low" }
                    }))
                    .unwrap();
                    let mut expected = serde_json::to_value(&settings).unwrap();
                    let selection = expected.pointer_mut(path).unwrap();
                    selection["model"] =
                        serde_json::json!(model.or(variant.map(|_| "example/rewrite-default")));
                    selection["variant"] = serde_json::json!(variant);

                    apply_model_variant(
                        &mut settings,
                        target,
                        variant.map(str::to_owned),
                        &catalog,
                    );

                    assert_eq!(serde_json::to_value(&settings).unwrap(), expected);
                }
            }
        }
    }

    #[test]
    fn custom_catalog_preserves_groups() {
        let compiled = vec![CatalogCommand {
            task: "Built in".into(),
            scope: CommandScope::Global,
            phrase: "built in".into(),
            aliases: Vec::new(),
            description: "Compiled command".into(),
            id: "compiled".into(),
        }];
        let status = StatusSnapshot {
            catalog: vec![
                crate::personal_commands::StatusCommand {
                    id: "default".into(),
                    phrases: vec!["default custom".into()],
                    group: "Other".into(),
                    description: "Custom command".into(),
                    context: StatusContext::Global,
                    execution: StatusExecution::Handler,
                },
                crate::personal_commands::StatusCommand {
                    id: "work".into(),
                    phrases: vec!["do work".into()],
                    group: "Work".into(),
                    description: "Do work".into(),
                    context: StatusContext::Global,
                    execution: StatusExecution::Handler,
                },
            ],
            ..StatusSnapshot::default()
        };

        let merged = merged_catalog(&compiled, &status);
        assert_eq!(merged[0].task, "Built in");
        assert_eq!(merged[1].task, "Other");
        assert_eq!(merged[2].task, "Work");
    }

    #[test]
    fn personal_workspace_states_cover_setup_and_runtime_failures() {
        let mut status = StatusSnapshot::default();
        assert_eq!(
            personal_workspace_state(false, false, &status),
            PersonalWorkspaceState::Missing
        );
        assert_eq!(
            personal_workspace_state(false, true, &status),
            PersonalWorkspaceState::Creating
        );
        assert_eq!(
            personal_workspace_state(true, false, &status),
            PersonalWorkspaceState::Ready
        );
        status.host_state = HostState::Unavailable;
        assert_eq!(
            personal_workspace_state(true, false, &status),
            PersonalWorkspaceState::Error
        );
        status.active_generation = Some(3);
        assert_eq!(
            personal_workspace_state(true, false, &status),
            PersonalWorkspaceState::Ready
        );
    }
}
