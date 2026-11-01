use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, Modifiers as GpuiModifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, PathPromptOptions, Pixels, Point, Render, SharedString, StyleRefinement,
    Subscription, Timer, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, deferred, div, img, prelude::*, px, rgb, size,
};

use crate::app_settings::{
    AppSettings, DictationMode, HotkeyBinding, HotkeyKey, HotkeyModifiers, IndicatorPosition,
    ModifierSide, RecordingAudioBehavior, TextReplacement,
};
use crate::application_catalog::InstalledApplication;
use crate::design_tokens::*;
use crate::desktop_activity::DesktopActivity;
use crate::desktop_i18n::{event_age, tr, tr_fill, tr_named};
use crate::desktop_transcription_picker::{
    ModelDeletion, TranscriptionPickerDelegate, TranscriptionPickerModel,
    TranscriptionPickerProgress, TranscriptionPickerStatus, TranscriptionPickerView,
    render_transcription_picker as render_shared_transcription_picker,
    transcription_selection_is_active,
};
use crate::desktop_ui::{
    ButtonKind, Icon, NavigationIcon, Tone, busy_label, button, danger_menu_item, detail_body,
    detail_header, detail_placeholder, detail_row, detail_title, disclosure_button,
    disclosure_frame, dropdown_menu_with_progress, error_message, error_panel, hotkey_keycaps,
    icon, icon_button, inline_error, list_row, list_row_meta, list_row_title, listener_status,
    menu_item, menu_with_progress, mix_color, navigation_item, pane_body, pane_content,
    pane_detail, pane_header, pane_header_with_action, pane_list, pane_scroll_body, panel,
    panel_header, progress_track, section_label, settings_copy, settings_row,
    settings_section_label, sidebar_frame, sliding_segmented_control, sliding_segmented_for,
    sliding_segmented_item, small_button, status_dot, tag, toggle, truncated_line, window_frame,
};
use crate::developer_control::DeveloperPane;
use crate::dictation_processor::{ModelCatalog, ModelChoice};
#[cfg(target_os = "macos")]
use crate::events::now_ms;
use crate::events::{
    CommandOutcome, DictationPhase, EventReader, TranscriptPhase, VoiceEvent, VoiceState,
};
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention};
use crate::login_item::{LoginItemRequest, LoginItemResponse, LoginItemStatus, LoginItemWorker};
use crate::motion::{ToggleSpring, WindowMotion};
use crate::onboarding::{PermissionState, SetupStatus};
use crate::personal_commands::{HostState, StatusSnapshot, StatusTransformation};
use crate::text_input::{
    Changed as TextChanged, Dismissed as TextDismissed, Navigate as TextNavigate,
    Submitted as TextSubmitted, TextInput,
};
use crate::transcription_models::{
    ModelDefinition, ModelRuntime, TranscriptionModelId, TranscriptionSelection, definition,
    language_name, validate,
};
use crate::transcription_preparation::{PreparationStatus, TranscriptionPreparation};

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;
const HOTKEY_MIN_WIDTH: f32 = 148.0;
/// Wide enough for "100%" with the segment's own padding.
const SOUND_VOLUME_ITEM_WIDTH: f32 = 40.0;
/// Left / Either / Right segment widths.
const HOTKEY_SIDE_WIDTHS: [f32; 3] = [40.0, 50.0, 40.0];
/// The side segments plus the control's 2 px padding and 1 px border per side.
const HOTKEY_SIDE_SELECTOR_WIDTH: f32 =
    HOTKEY_SIDE_WIDTHS[0] + HOTKEY_SIDE_WIDTHS[1] + HOTKEY_SIDE_WIDTHS[2] + 6.0;
const OPENCODE_INSTALL_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
/// The setup screen flips each row as macOS grants it.
const TOUR_PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_millis(400);
const MAX_OPENCODE_ERROR_BYTES: usize = 4 * 1024;
const ACTIVITY_LIMIT: usize = 100;
const OPENCODE_BETA_DOCS_URL: &str = "https://v2.opencode.ai/";
const NEW_HEX_URL: &str = "https://anoma.ly/hex";

// Each OS supplies the same set of `platform` items: its window state, the
// capabilities below, window chrome, and the actions only its runtime can
// perform. Everything else in this file is the one shared window.
#[cfg_attr(target_os = "macos", path = "app_window/macos.rs")]
#[cfg_attr(target_os = "windows", path = "app_window/windows.rs")]
#[cfg_attr(target_os = "linux", path = "app_window/linux.rs")]
mod platform;
mod tour;

#[cfg(not(target_os = "macos"))]
pub use platform::open;
#[cfg(target_os = "macos")]
pub use platform::{
    commit_transcription_selection, menu_transcription_selection, open_or_focus, open_preview,
    refresh_transcription_menu,
};

/// What the current OS's runtime can do. A missing capability omits its
/// pane or settings row; the window never shows an imitation of a feature
/// the runtime cannot perform.
#[derive(Clone, Copy, Debug)]
struct Capabilities {
    meetings: bool,
    commands: bool,
    hud_lab: bool,
    fun: bool,
    modes: bool,
    voice_action: bool,
    history: bool,
    /// Privacy permissions gate dictation (macOS).
    permissions: bool,
    dock_icon: bool,
    launch_at_login: bool,
    /// The listener can be off: an Enabled/Disabled card and Enable on launch.
    listener_control: bool,
    paste_last: bool,
    /// Other applications' audio can be muted or paused while dictating.
    recording_audio: bool,
    /// The microphone can close between dictations.
    release_microphone: bool,
    /// Recording can wait for a second tap of the shortcut.
    double_tap_only: bool,
    /// Shortcuts can require the left or right modifier key.
    modifier_sides: bool,
    indicator_position: bool,
    interface_language: bool,
    software_updates: bool,
    /// Paste with Ctrl-Shift-V for terminals.
    terminal_paste: bool,
    /// The window sees dictation events, so onboarding can end with a
    /// practice dictation into its own field.
    practice_dictation: bool,
}

/// The microphone menu is deferred above the settings panel and occludes
/// everything beneath it, so hovering or choosing a device never reaches the
/// controls under the menu. A mouse-down anywhere else dismisses it.
fn microphone_picker_menu(
    choices: Vec<Option<String>>,
    selected: Option<String>,
    error: Option<String>,
    progress: f32,
    choose: impl Fn(&Option<String>, &mut Window, &mut App) + 'static,
    dismiss: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    let choose = Rc::new(choose);
    dropdown_menu_with_progress(progress)
        .id("microphone-picker")
        .debug_selector(|| "microphone-picker".into())
        .right_0()
        .w(px(DISCLOSURE_WIDTH))
        .max_h(px(240.0))
        .overflow_y_scroll()
        .on_mouse_down_out(dismiss)
        .children(choices.into_iter().enumerate().map(|(index, device)| {
            let is_selected = selected == device;
            let label = device.clone().unwrap_or_else(|| tr("Automatic").into());
            let choose = choose.clone();
            menu_item(is_selected)
                .id(("microphone-choice", index))
                .debug_selector(move || format!("microphone-choice-{index}"))
                .child(truncated_line(FOOTNOTE, label))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    choose(&device, window, cx);
                })
        }))
        .when_some(error, |picker, error| picker.child(inline_error(error)))
}

/// The sidebar's Update button, shown while an update is available.
fn sidebar_update_button() -> gpui::Stateful<Div> {
    button(ButtonKind::Secondary, status_dot(ACCENT_TEXT))
        .child(tr("Update"))
        .id("check-for-updates-sidebar")
}

fn settings_pane(title: &'static str, scroll_id: &'static str, content: Div) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(pane_header(title))
        .child(pane_scroll_body(
            scroll_id,
            content
                .relative()
                .debug_selector(|| "settings-content".into()),
        ))
        .into_any_element()
}

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

/// Opens a new app window with this OS's window options and chrome.
fn open_new(
    event_path: PathBuf,
    history: Option<History>,
    preview: Option<AppWindowPreview>,
    args: platform::Args,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    let preview_mode = preview.is_some();
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let handle = cx.open_window(
        platform::window_options(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Hex".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            is_resizable: !preview_mode,
            is_minimizable: true,
            window_min_size: Some(size(px(MINIMUM_WIDTH), px(MINIMUM_HEIGHT))),
            tabbing_identifier: preview_mode.then(|| "hex-preview".into()),
            ..Default::default()
        }),
        |window, cx| cx.new(|cx| AppWindow::new(event_path, history, preview, args, window, cx)),
    )?;
    cx.activate(true);
    Ok(handle)
}

#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum PreviewModelState {
    Actual,
    Installed,
    Missing,
    Downloading,
    Error,
}

/// A deterministic onboarding state for previews and screenshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum OnboardingPreview {
    Welcome,
    HowItWorks,
    Setup,
    SetupProgress,
    SetupReady,
    TryIt,
    TryItListening,
    TryItShort,
    TryItSilent,
    TryItLanded,
    TryItReopen,
    Done,
}

impl OnboardingPreview {
    fn status(self) -> SetupStatus {
        let ready = SetupStatus {
            microphone: PermissionState::Ready,
            input_monitoring: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: true,
        };
        match self {
            Self::Welcome | Self::HowItWorks | Self::Setup => SetupStatus {
                microphone: PermissionState::NeedsRequest,
                input_monitoring: PermissionState::NeedsSettings,
                accessibility: PermissionState::NeedsSettings,
                transcription_model: false,
            },
            Self::SetupProgress => SetupStatus {
                accessibility: PermissionState::NeedsSettings,
                transcription_model: false,
                ..ready
            },
            _ => ready,
        }
    }

    fn flow(self, permissions: bool, practice: bool) -> crate::onboarding::Flow {
        use crate::onboarding::{Flow, Input, ModelProgress, Observation, SetupItem, Step};
        let fresh = Self::Setup.status();
        let start = match self {
            Self::Welcome => Step::Welcome,
            Self::HowItWorks => Step::HowItWorks,
            _ => Step::Setup,
        };
        let initial = if self == Self::SetupProgress {
            self.status()
        } else {
            fresh
        };
        // Later steps walk through setup, as a first run does, so the page
        // dots match what a person sees.
        let mut flow = Flow::new(start, initial, permissions, practice);
        let status = self.status();
        if self == Self::SetupProgress {
            flow.on(Input::Grant(SetupItem::Accessibility));
            flow.on(Input::Status(
                status,
                ModelProgress {
                    downloading: Some(0.42),
                    ..ModelProgress::default()
                },
            ));
        } else if start == Step::Setup && self != Self::Setup {
            flow.on(Input::Status(
                status,
                ModelProgress {
                    installed: true,
                    ..ModelProgress::default()
                },
            ));
            if self != Self::SetupReady {
                flow.on(Input::Continue);
            }
            if self == Self::Done {
                flow.on(Input::Skip);
            }
        }
        let observations: &[Observation] = match self {
            Self::TryItListening => &[Observation::Started],
            Self::TryItShort => &[Observation::Started, Observation::Discarded],
            Self::TryItSilent => &[
                Observation::Started,
                Observation::Transcribing,
                Observation::Discarded,
            ],
            Self::TryItLanded => &[
                Observation::Started,
                Observation::Transcribing,
                Observation::Pasted,
            ],
            _ => &[],
        };
        for observation in observations {
            flow.on(Input::Dictation(*observation));
        }
        if self == Self::TryItReopen {
            flow.on(Input::ListenerFailed {
                relaunch_allowed: false,
            });
        }
        flow
    }
}

#[derive(Clone)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct AppWindowPreview {
    pub pane: DeveloperPane,
    pub transcription_picker: Option<(String, PreviewModelState)>,
    pub onboarding: Option<OnboardingPreview>,
    pub confirm_model_deletion: bool,
    pub collapse_mode_processing: bool,
    pub open_transformation_picker: bool,
    pub select_global_mode: bool,
    pub voice_action_enabled: bool,
    pub opencode_unavailable: bool,
    pub permissions_missing: bool,
    pub model_missing: bool,
    pub command_model_missing: bool,
    pub open_history_retention: bool,
    pub confirm_release_microphone: bool,
    pub update_available: bool,
    pub meetings_enabled: bool,
    pub screen_recording_missing: bool,
    pub meeting_recording: bool,
    pub confirm_meeting_delete: bool,
    pub open_meeting_actions: bool,
    pub meetings_empty: bool,
    pub experimental_mic: bool,
    pub fun_mode: Option<crate::voice_filter::VoiceFilterMode>,
    pub fun_driver_missing: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Pane {
    HudLab,
    Fun,
    Meetings,
    Commands,
    Activity,
    History,
    Modes,
    VoiceAction,
    #[default]
    Settings,
}

impl Pane {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn on_reopen(self, status: SetupStatus) -> Self {
        if crate::onboarding::permission_warnings(status).is_empty() {
            self
        } else {
            Self::Settings
        }
    }

    const ALL: [Self; 9] = [
        Self::Settings,
        Self::Modes,
        Self::Commands,
        Self::VoiceAction,
        Self::History,
        Self::Fun,
        Self::HudLab,
        Self::Meetings,
        Self::Activity,
    ];

    #[cfg(all(test, target_os = "macos"))]
    fn all(capabilities: Capabilities, developer_features: bool, meetings: bool) -> Vec<Self> {
        Self::all_with_fun(capabilities, developer_features, meetings, false)
    }

    fn all_with_fun(
        capabilities: Capabilities,
        developer_features: bool,
        meetings: bool,
        fun: bool,
    ) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|pane| match pane {
                Self::Settings => true,
                Self::Modes => capabilities.modes,
                Self::Commands => capabilities.commands,
                Self::VoiceAction => capabilities.voice_action,
                Self::History => capabilities.history,
                Self::Fun => capabilities.fun && fun,
                Self::Meetings => capabilities.meetings && (developer_features || meetings),
                Self::HudLab => capabilities.hud_lab && developer_features,
                Self::Activity => developer_features,
            })
            .collect()
    }

    fn label(self) -> &'static str {
        match self {
            Self::HudLab => "HUD Lab",
            Self::Fun => tr("Fun"),
            Self::Meetings => tr("Meetings"),
            Self::Commands => tr("Commands"),
            Self::Activity => tr("Activity"),
            Self::History => tr("History"),
            Self::Modes => tr("Modes"),
            Self::VoiceAction => tr("Voice Action"),
            Self::Settings => tr("Settings"),
        }
    }

    fn icon(self) -> NavigationIcon {
        match self {
            Self::Settings => NavigationIcon::Settings,
            Self::Modes => NavigationIcon::Modes,
            Self::VoiceAction => NavigationIcon::VoiceAction,
            Self::History => NavigationIcon::History,
            Self::Commands => NavigationIcon::Commands,
            Self::Fun => NavigationIcon::Fun,
            Self::Meetings => NavigationIcon::Meetings,
            Self::Activity => NavigationIcon::Activity,
            Self::HudLab => NavigationIcon::HudLab,
        }
    }

    fn from_developer(pane: DeveloperPane) -> Self {
        match pane {
            DeveloperPane::Settings => Self::Settings,
            DeveloperPane::Modes => Self::Modes,
            DeveloperPane::VoiceAction => Self::VoiceAction,
            DeveloperPane::History => Self::History,
            DeveloperPane::HudLab => Self::HudLab,
            DeveloperPane::Fun => Self::Fun,
            DeveloperPane::Commands => Self::Commands,
            DeveloperPane::Meetings => Self::Meetings,
            DeveloperPane::Activity => Self::Activity,
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn developer(self) -> DeveloperPane {
        match self {
            Self::Settings => DeveloperPane::Settings,
            Self::Modes => DeveloperPane::Modes,
            Self::VoiceAction => DeveloperPane::VoiceAction,
            Self::History => DeveloperPane::History,
            Self::HudLab => DeveloperPane::HudLab,
            Self::Fun => DeveloperPane::Fun,
            Self::Commands => DeveloperPane::Commands,
            Self::Meetings => DeveloperPane::Meetings,
            Self::Activity => DeveloperPane::Activity,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum PersonalWorkspaceState {
    Missing,
    Creating,
    Ready,
    Error,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

struct ModeEditor {
    // Authoritative editor draft; AppSettings receives a projection only when
    // constructing a save candidate, never by keeping parallel indices in sync.
    settings: DictationMode,
    inputs: ModeInputs,
}

struct ModeEditors {
    default_mode: ModeEditor,
    modes: Vec<ModeEditor>,
}

impl ModeEditors {
    fn project(&self, settings: &mut AppSettings) {
        settings.dictation_processing.default_mode = self.default_mode.settings.clone();
        settings.dictation_processing.modes =
            self.modes.iter().map(|row| row.settings.clone()).collect();
    }
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
                    .h(px(CONTROL_HEIGHT))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded(RADIUS_CONTROL)
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .bg(rgb(SURFACE_SELECTED))
                    .shadow_lg()
                    .text_body()
                    .text_color(rgb(TEXT_PRIMARY))
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

struct OpenCodeUnavailableCopy {
    title: &'static str,
    description: &'static str,
    error: Option<String>,
    can_retry: bool,
    retry_label: &'static str,
    can_open_setup: bool,
}

#[derive(Clone, Copy)]
enum OpenCodeFeature {
    VoiceAction,
    Transformation,
}

fn opencode_unavailable_copy(
    state: &ModelCatalogState,
    feature: OpenCodeFeature,
) -> Option<OpenCodeUnavailableCopy> {
    match state {
        ModelCatalogState::Loading => Some(OpenCodeUnavailableCopy {
            title: tr("Checking for OpenCode"),
            description: match feature {
                OpenCodeFeature::VoiceAction => {
                    tr("Voice Action will be available after Hex connects to OpenCode.")
                }
                OpenCodeFeature::Transformation => {
                    tr("OpenCode transformation will be available after Hex connects to OpenCode.")
                }
            },
            error: None,
            can_retry: false,
            retry_label: tr("Retry"),
            can_open_setup: false,
        }),
        ModelCatalogState::Missing => Some(OpenCodeUnavailableCopy {
            title: match feature {
                OpenCodeFeature::VoiceAction => tr("Voice Action requires OpenCode"),
                OpenCodeFeature::Transformation => tr("Transformation requires OpenCode"),
            },
            description: match feature {
                OpenCodeFeature::VoiceAction => tr(
                    "Install and configure OpenCode to turn spoken instructions into paste-ready text.",
                ),
                OpenCodeFeature::Transformation => {
                    tr("Install and configure OpenCode to transform dictated text.")
                }
            },
            error: None,
            can_retry: true,
            retry_label: tr("Check again"),
            can_open_setup: true,
        }),
        ModelCatalogState::Failed(error) => Some(OpenCodeUnavailableCopy {
            title: tr("OpenCode is unavailable"),
            description: tr("Hex could not load the OpenCode model catalog."),
            error: Some(error.clone()),
            can_retry: true,
            retry_label: tr("Retry"),
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
    #[cfg(all(test, target_os = "macos"))]
    key: String,
    is_default: bool,
}

enum ApplicationCatalogState {
    Loading,
    Loaded(Vec<InstalledApplication>),
}

impl ApplicationCatalogState {
    fn application_named(&self, name: &str) -> Option<&InstalledApplication> {
        let Self::Loaded(applications) = self else {
            return None;
        };
        applications
            .iter()
            .find(|application| application.name == name)
    }
}

/// The listener's state on an OS where it can be off.
struct ListenerView {
    running: bool,
    status: String,
    last_failure: Option<String>,
    error: Option<String>,
}

/// The background update check's state, for the Software updates row.
/// The update state the Software updates row shows. `version` is `None`
/// where the updater does not report it.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
enum SoftwareUpdate {
    Checking,
    Current,
    Failed,
    Available { version: Option<String> },
    ReadyToRestart { version: Option<String> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HotkeyKind {
    Dictation,
    Edit,
    PasteLast,
}

fn set_hotkey_binding(settings: &mut AppSettings, kind: HotkeyKind, binding: HotkeyBinding) {
    match kind {
        HotkeyKind::Dictation => {
            if binding.key.is_none() {
                settings.double_tap_only = false;
            }
            settings.dictation_hotkey = binding;
        }
        HotkeyKind::Edit => settings.edit_hotkey = binding,
        HotkeyKind::PasteLast => settings.paste_last_hotkey = Some(binding),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum MicrophonePolicyChange {
    SetCommands(bool),
    SetReleaseWhileIdle(bool),
    KeepReadyAndEnableCommands,
    DisableCommandsAndRelease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunSlider {
    Strength,
    MuffledWall,
    MuffledRoom,
    MuffledLevel,
    QuietLevel,
    MutedRoomTone,
    ChipmunkPitch,
    RobotCarrier,
    WalkieCrunch,
    UnderwaterDepth,
    FarRoom,
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

struct NavigationSidebar {
    window_entity: WeakEntity<AppWindow>,
    pane: Pane,
    meetings_available: bool,
    fun_available: bool,
    update_available: bool,
    preview: bool,
}

impl Render for NavigationSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let items = Pane::all_with_fun(
            platform::CAPABILITIES,
            crate::DEVELOPER_FEATURES_ENABLED,
            self.meetings_available,
            self.fun_available,
        )
        .into_iter()
        .enumerate()
        .map(|(index, pane)| {
            let selected = self.pane == pane;
            let window_entity = self.window_entity.clone();
            navigation_item(pane.icon(), selected)
                .id(("app-nav", index))
                .child(pane.label())
                .on_click(move |_, _, cx| {
                    let _ = window_entity.update(cx, |this, cx| {
                        this.select_pane(pane, cx);
                    });
                })
        });

        let update_button = self.update_available.then(|| {
            let window_entity = self.window_entity.clone();
            let preview = self.preview;
            sidebar_update_button().on_click(move |_, window, cx| {
                #[cfg(target_os = "macos")]
                if !preview {
                    window.dispatch_action(Box::new(CheckForUpdates), cx);
                    window.refresh();
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (window, preview);
                let _ = window_entity.update(cx, |this, cx| {
                    this.install_update(cx);
                });
            })
        });

        platform::sidebar(sidebar_frame())
            .child(div().flex().flex_col().gap_0p5().children(items))
            .child(platform::window_drag_region(div().flex_1()))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .h(px(CONTROL_HEIGHT))
                    .mt_3()
                    .child(
                        div()
                            .flex_none()
                            .pl_3()
                            .text_caption()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(tr_fill("Version {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .children(update_button),
            )
    }
}

struct FunWaveformView {
    last_progress: f32,
    was_active: bool,
}

impl FunWaveformView {
    fn new() -> Self {
        Self {
            last_progress: 0.0,
            was_active: false,
        }
    }
}

impl Render for FunWaveformView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use crate::hex_microphone::DemoSourceKind;

        let _frame_guard = crate::motion::begin_frame("FunWaveform");
        let demo = crate::hex_microphone::demo_snapshot();
        let recomputing = crate::hex_microphone::demo_recompute_is_running();

        let bars_you: Vec<f32> = demo
            .peaks_you
            .iter()
            .enumerate()
            .map(|(idx, &p)| window.spring(("fun-bar-you", idx), p.clamp(0.06, 1.0), SPRING_SETTLE))
            .collect();
        let bars_call: Vec<f32> = demo
            .peaks_call
            .iter()
            .enumerate()
            .map(|(idx, &p)| {
                window.spring(("fun-bar-call", idx), p.clamp(0.06, 1.0), SPRING_SETTLE)
            })
            .collect();

        let active = demo.is_recording || demo.playing.is_some();
        let raw_progress = if demo.is_recording {
            demo.recording_progress
        } else {
            demo.playback_progress
        }
        .clamp(0.0, 1.0);

        if active {
            self.last_progress = raw_progress;
        }
        if active != self.was_active {
            if !active {
                self.last_progress = raw_progress.max(self.last_progress);
                crate::motion::log_pane_summary("FunWaveform", "demo_stopped");
                crate::motion::log_pane_summary("Fun", "demo_stopped");
            }
            self.was_active = active;
            window.refresh();
        }

        let vis = window
            .spring_when("fun-progress-vis", active, SPRING_SNAP)
            .clamp(0.0, 1.0);

        if active || recomputing {
            window.request_animation_frame();
        }

        let duration_s = (demo.duration_ms as f64) / 1000.0;
        let span_start_s = format!("{:.1}s", demo.span.start_ms / 1000.0);
        let span_end_s = format!("{:.1}s", demo.span.end_ms / 1000.0);
        let span_badge = tag(Tone::Accent, format!("{} – {}", span_start_s, span_end_s));
        let clip_quote = match demo.source {
            DemoSourceKind::Example => {
                "“Yeah, I’m looking at the new layout now — Reply to Olive: pick up dinner…”"
            }
            DemoSourceKind::Recording => tr("Your 5s sample"),
        };

        let total_bars = bars_you.len().max(1);
        let duration_f = (demo.duration_ms.max(1)) as f64;
        let render_waveform_bars = |bars: &[f32], call_track: bool| -> Div {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .h(px(28.0))
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .gap(px(2.0))
                .rounded(RADIUS_SMALL)
                .bg(rgb(SURFACE))
                .children(bars.iter().enumerate().map(|(idx, &bar_val)| {
                    let bar_ms = ((idx as f64) + 0.5) / (total_bars as f64) * duration_f;
                    let in_span = bar_ms >= demo.span.start_ms && bar_ms <= demo.span.end_ms;
                    let height = ((bar_val.clamp(0.06, 1.0) * 20.0) * 64.0).round() / 64.0;
                    let color = if call_track {
                        if in_span { ACCENT_TEXT } else { TEXT_SECONDARY }
                    } else if in_span {
                        TEXT_PRIMARY
                    } else {
                        TEXT_TERTIARY
                    };
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(height))
                        .rounded(RADIUS_KNOB)
                        .bg(rgb(color))
                }))
        };

        let progress = self.last_progress.clamp(0.0, 1.0);

        div()
            .id("fun-waveform-card")
            .p_3()
            .flex()
            .flex_col()
            .gap_2p5()
            .rounded(RADIUS_CONTROL)
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(CANVAS))
            .child(
                div()
                    .h(px(TAG_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .text_footnote()
                                    .text_color(rgb(TEXT_SECONDARY))
                                    .child(clip_quote),
                            )
                            .child(span_badge),
                    )
                    .child(
                        div()
                            .w(px(44.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_caption()
                            .text_monospace()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(format!("{duration_s:.1}s")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .text_caption()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(tr("You")),
                    )
                    .child(render_waveform_bars(&bars_you, false)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(80.0))
                            .flex_none()
                            .text_caption()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(ACCENT_TEXT))
                            .child(tr("Call hears")),
                    )
                    .child(render_waveform_bars(&bars_call, true)),
            )
            .child(
                div()
                    .h(px(PROGRESS_HEIGHT))
                    .opacity(vis)
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .child(progress_track(div().w(gpui::relative(progress)))),
            )
    }
}

/// Root GPUI entity for the production app shell.
pub struct AppWindow {
    preview: bool,
    pane: Pane,
    navigation_sidebar: Entity<NavigationSidebar>,
    navigation_view: AnyView,
    fun_waveform: Entity<FunWaveformView>,
    event_reader: EventReader,
    activity: DesktopActivity,
    platform: platform::Platform,
    setup_status: SetupStatus,
    /// The first-run tour is showing.
    setup_visible: bool,
    tour: crate::onboarding::Flow,
    tour_forward: bool,
    tour_relaunch_failed: bool,
    tour_quit_requested: bool,
    practice_input: Entity<TextInput>,
    practice_since: Option<u64>,
    practice_seen: usize,
    onboarding_completed: bool,
    permission_refresh_at: Instant,
    settings: AppSettings,
    settings_load_error: Option<String>,
    #[cfg(test)]
    settings_save_override: Option<fn(&AppSettings) -> color_eyre::Result<()>>,
    microphone_devices: Vec<String>,
    microphone_picker_open: bool,
    microphone_picker_error: Option<String>,
    launch_at_login_status: Option<LoginItemStatus>,
    login_item_worker: Option<LoginItemWorker>,
    launch_at_login_error: Option<String>,
    launch_at_login_toggle: ToggleSpring,
    commands_toggle: ToggleSpring,
    release_microphone_toggle: ToggleSpring,
    shortcut_mode_toggle: ToggleSpring,
    pending_microphone_policy: Option<MicrophonePolicyChange>,
    double_tap_toggle: ToggleSpring,
    double_tap_only_visibility: ToggleSpring,
    dock_icon_toggle: ToggleSpring,
    enable_on_launch_toggle: ToggleSpring,
    terminal_paste_toggle: ToggleSpring,
    indicator_position_spring: ToggleSpring,
    interface_language_open: bool,
    sound_volume_spring: ToggleSpring,
    recording_audio_spring: ToggleSpring,
    variant_picker_open: Option<ModelPickerTarget>,
    transcription_hints: ProcessingInput,
    transcription_picker_language: Option<String>,
    transcription_status: PreparationStatus,
    transcription_preparation: TranscriptionPreparation,
    transcription_preview_installed: Option<bool>,
    transcription_deletion: Option<TranscriptionModelId>,
    transcription_deletion_error: Option<String>,
    mode_editors: ModeEditors,
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
    personal_commands_status: StatusSnapshot,
    personal_workspace: PathBuf,
    personal_config_exists: bool,
    personal_workspace_receiver: Option<Receiver<Result<PathBuf, String>>>,
    personal_workspace_error: Option<String>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    personal_commands_prompt_copied: bool,
    events: Vec<VoiceEvent>,
    current_context: (Option<String>, Option<String>),
    selected_event: Option<usize>,
    history: Option<History>,
    history_search: ProcessingInput,
    history_entries: Vec<HistoryEntry>,
    selected_history: Option<u64>,
    history_error: Option<String>,
    history_clear_armed: bool,
    history_retention_open: bool,
    history_copied: Option<u64>,
    fun_comfort_noise_toggle: ToggleSpring,
    fun_driver_action_seq: u64,
    fun_driver_missing_preview: bool,
    fun_mic_status: crate::hex_microphone::HexMicrophoneStatus,
    fun_mic_status_receiver: Option<Receiver<crate::hex_microphone::HexMicrophoneStatus>>,
    fun_demo_button_state: (
        crate::hex_microphone::DemoSourceKind,
        bool,
        bool,
        Option<crate::hex_microphone::DemoPlaybackTrack>,
    ),
    fun_active_slider: Option<FunSlider>,
    fun_last_non_normal_mode: crate::voice_filter::VoiceFilterMode,
}

impl AppWindow {
    fn new(
        event_path: PathBuf,
        history: Option<History>,
        preview: Option<AppWindowPreview>,
        args: platform::Args,
        native_window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |window, cx| {
            let mut interval = Duration::from_millis(500);
            loop {
                Timer::after(interval).await;
                if window
                    .update(cx, |window, cx| {
                        // The tour and the Fun demo recorder/player react at 100 ms.
                        let fun_busy = (window.pane == Pane::Fun
                            && crate::hex_microphone::demo_is_busy())
                            || crate::hex_microphone::driver_action_in_progress();
                        interval = if (window.setup_visible && !window.preview) || fun_busy {
                            Duration::from_millis(100)
                        } else {
                            Duration::from_millis(500)
                        };
                        let model_catalog_changed = window.poll_model_catalog();
                        window.retry_model_catalog();
                        let application_catalog_changed = window.poll_application_catalog();
                        let transcription_changed = window.poll_transcription_download();
                        let setup_changed = window.poll_setup(false);
                        let login_item_changed = window.poll_login_item();
                        let personal_commands_changed = window.poll_personal_commands();
                        let personal_workspace_changed = window.poll_personal_workspace();
                        let history_changed = window.poll_history(cx);
                        let platform_changed = window.poll_platform(cx);
                        let hex_mic_changed = window.poll_hex_microphone(cx);
                        let tour_changed = window.poll_tour();
                        if window.tour_quit_requested {
                            cx.quit();
                            return;
                        }
                        if tour_changed
                            || model_catalog_changed
                            || application_catalog_changed
                            || transcription_changed
                            || setup_changed
                            || login_item_changed
                            || personal_commands_changed
                            || personal_workspace_changed
                            || history_changed
                            || platform_changed
                            || hex_mic_changed
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
                commands_enabled: preview.command_model_missing
                    || preview.confirm_release_microphone,
                ..AppSettings::default()
            };
            settings.voice_action.enabled = preview.voice_action_enabled;
            settings.meetings.enabled =
                preview.meetings_enabled || matches!(preview.pane, DeveloperPane::Meetings);
            settings.experimental.hex_microphone =
                preview.experimental_mic || matches!(preview.pane, DeveloperPane::Fun);
            if let Some(mode) = preview.fun_mode {
                settings.experimental.hex_microphone_mode = mode;
            }
            crate::hex_microphone::sync_from_settings(
                settings.experimental.hex_microphone_mode,
                settings.experimental.hex_microphone_comfort_noise,
                settings.experimental.hex_microphone_strength,
            );
            if let Some((language, _)) = &preview.transcription_picker
                && let Some(choice) = preview_choices(language).first()
            {
                settings.transcription.model = choice.model.id;
                settings.transcription.language = language.clone();
            }
            if matches!(preview.pane, DeveloperPane::Modes) {
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
                        Some(tr_fill(
                            "Could not load app settings: {}",
                            &format!("{error:#}"),
                        )),
                    )
                }
            }
        };
        let transcription_hints =
            Self::transcription_hints_input(&settings.transcription.recognition_hints, cx);
        let mode_editors = Self::mode_editors(&settings, cx);
        let voice_action_inputs = Self::voice_action_inputs(&settings, cx);
        let (model_catalog, model_catalog_receiver) = if preview
            .as_ref()
            .is_some_and(|preview| preview.opencode_unavailable)
        {
            (ModelCatalogState::Missing, None)
        } else if preview_mode {
            (
                ModelCatalogState::Loaded(ModelCatalog {
                    models: vec![
                        ModelChoice {
                            key: "google/gemini-3.1-flash-lite".into(),
                            name: "Gemini 3.1 Flash Lite".into(),
                            provider: "Google".into(),
                            variants: vec![
                                "minimal".into(),
                                "low".into(),
                                "medium".into(),
                                "high".into(),
                            ],
                        },
                        ModelChoice {
                            key: "anthropic/claude-haiku-4-5".into(),
                            name: "Claude Haiku 4.5".into(),
                            provider: "Anthropic".into(),
                            variants: Vec::new(),
                        },
                        ModelChoice {
                            key: "openai/gpt-5.4-nano".into(),
                            name: "GPT-5.4 Nano".into(),
                            provider: "OpenAI".into(),
                            variants: vec!["none".into(), "low".into(), "medium".into()],
                        },
                    ],
                    default_key: Some("google/gemini-3.1-flash-lite".into()),
                    default_name: Some("Gemini 3.1 Flash Lite".into()),
                    default_variant: Some("minimal".into()),
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
        let platform = platform::Platform::new(
            args,
            preview.as_ref(),
            &settings,
            &personal_commands_status,
            cx,
        );
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
        let onboarding_preview = preview.as_ref().and_then(|preview| preview.onboarding);
        let setup_status = if let Some(onboarding) = onboarding_preview {
            onboarding.status()
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
        let resume = (!preview_mode)
            .then(crate::onboarding::take_resume)
            .flatten();
        let setup_visible = onboarding_preview.is_some()
            || resume.is_some()
            || !onboarding_completed && !setup_status.ready();
        let practice = platform::CAPABILITIES.practice_dictation;
        let tour = match onboarding_preview {
            Some(onboarding) => onboarding.flow(platform::CAPABILITIES.permissions, practice),
            None => crate::onboarding::Flow::new(
                resume.unwrap_or(crate::onboarding::Step::Welcome),
                setup_status,
                platform::CAPABILITIES.permissions,
                practice,
            ),
        };
        let practice_input = cx.new(|cx| {
            TextInput::multiline(
                cx,
                tr("Your words will appear here"),
                if onboarding_preview == Some(OnboardingPreview::TryItLanded) {
                    tr("Hex turns what I say into text, right where I'm typing.")
                } else {
                    ""
                },
            )
        });
        let microphone_devices = if preview_mode {
            vec!["Built-in Microphone".into()]
        } else {
            crate::audio::input_device_names().unwrap_or_else(|error| {
                tracing::warn!(%error, "could not list microphones for settings");
                Vec::new()
            })
        };
        let launch_at_login_status = preview_mode.then_some(LoginItemStatus::Disabled);
        let (login_item_worker, launch_at_login_error) =
            if preview_mode || !platform::CAPABILITIES.launch_at_login {
                (None, None)
            } else {
                match LoginItemWorker::new() {
                    Ok(worker) => (Some(worker), None),
                    Err(error) => (None, Some(error)),
                }
            };
        let history = if preview_mode {
            preview
                .as_ref()
                .is_some_and(|preview| matches!(preview.pane, DeveloperPane::History))
                .then(preview_history)
                .flatten()
        } else {
            history
        };
        let history_search = Self::history_search_input(cx);
        let hex_mic_comfort_noise = settings.experimental.hex_microphone_comfort_noise;
        let initial_pane = preview.as_ref().map_or(Pane::default(), |preview| {
            Pane::from_developer(preview.pane)
        });
        let fun_driver_missing = preview
            .as_ref()
            .is_some_and(|preview| preview.fun_driver_missing);
        let fun_mic_status = Self::preview_hex_microphone_status(fun_driver_missing);
        let personal_config_exists =
            !preview_mode && personal_workspace.join("hex.config.ts").is_file();
        let weak_self = cx.weak_entity();
        let navigation_sidebar = cx.new(|_| NavigationSidebar {
            window_entity: weak_self,
            pane: initial_pane,
            meetings_available: settings.meetings.available(),
            fun_available: settings.experimental.hex_microphone_available()
                || initial_pane == Pane::Fun,
            update_available: false,
            preview: preview_mode,
        });
        let navigation_view = AnyView::from(navigation_sidebar.clone()).cached(
            StyleRefinement::default()
                .w(px(SIDEBAR_WIDTH))
                .h_full()
                .flex_none(),
        );
        let fun_waveform = cx.new(|_| FunWaveformView::new());
        if std::env::var_os("HEX_PREVIEW_BEFORE").is_none()
            && (settings.experimental.hex_microphone_available() || initial_pane == Pane::Fun)
        {
            crate::hex_microphone::prewarm_demo_async();
        }
        let mut window = Self {
            preview: preview_mode,
            pane: initial_pane,
            navigation_sidebar,
            navigation_view,
            fun_waveform,
            event_reader: EventReader::open(event_path),
            activity: DesktopActivity::default(),
            platform,
            setup_status,
            setup_visible,
            tour,
            tour_forward: true,
            tour_relaunch_failed: false,
            tour_quit_requested: false,
            practice_input,
            practice_since: None,
            practice_seen: 0,
            onboarding_completed,
            permission_refresh_at: Instant::now() + PERMISSION_REFRESH_INTERVAL,
            microphone_devices,
            microphone_picker_open: false,
            microphone_picker_error: None,
            launch_at_login_status,
            login_item_worker,
            launch_at_login_error,
            launch_at_login_toggle: ToggleSpring::new(
                launch_at_login_status == Some(LoginItemStatus::Enabled),
            ),
            commands_toggle: ToggleSpring::new(settings.commands_enabled),
            release_microphone_toggle: ToggleSpring::new(settings.release_microphone_while_idle),
            shortcut_mode_toggle: ToggleSpring::new(
                settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
            ),
            pending_microphone_policy: preview
                .as_ref()
                .filter(|preview| preview.confirm_release_microphone)
                .map(|_| MicrophonePolicyChange::DisableCommandsAndRelease),
            double_tap_toggle: ToggleSpring::new(settings.double_tap_lock),
            double_tap_only_visibility: ToggleSpring::new(
                settings.double_tap_lock && settings.dictation_hotkey.key.is_some(),
            ),
            dock_icon_toggle: ToggleSpring::new(settings.show_dock_icon),
            enable_on_launch_toggle: ToggleSpring::new(settings.enable_on_launch),
            terminal_paste_toggle: ToggleSpring::new(settings.paste_with_shift),
            indicator_position_spring: ToggleSpring::at(indicator_position_index(
                settings.indicator_position,
            ) as f32),
            interface_language_open: false,
            sound_volume_spring: ToggleSpring::at(sound_volume_index(&settings) as f32),
            recording_audio_spring: ToggleSpring::at(recording_audio_index(
                settings.recording_audio_behavior,
            ) as f32),
            variant_picker_open: None,
            transcription_hints,
            transcription_picker_language: preview_picker.map(|(language, _)| language.clone()),
            transcription_status: PreparationStatus {
                model: preview_downloading,
                downloaded_bytes: preview_downloading
                    .and_then(|model| definition(model).download_bytes())
                    .map_or(0, |bytes| bytes * 37 / 100),
                error: matches!(preview_model_state, Some(PreviewModelState::Error))
                    .then(|| "Preview download failed while verifying the model checksum.".into()),
                ..Default::default()
            },
            transcription_preparation: if preview.is_some() {
                TranscriptionPreparation::default()
            } else {
                cx.default_global::<TranscriptionPreparation>().clone()
            },
            transcription_preview_installed: match preview_model_state {
                Some(PreviewModelState::Installed) => Some(true),
                Some(
                    PreviewModelState::Missing
                    | PreviewModelState::Downloading
                    | PreviewModelState::Error,
                ) => Some(false),
                Some(PreviewModelState::Actual) | None => preview_model_missing.then_some(false),
            },
            transcription_deletion: preview_picker
                .filter(|_| {
                    preview
                        .as_ref()
                        .is_some_and(|preview| preview.confirm_model_deletion)
                })
                .and_then(|(language, _)| {
                    crate::transcription_models::choices_for_runtime(language)
                        .into_iter()
                        .map(|choice| choice.model)
                        .find(|model| {
                            matches!(model.runtime, ModelRuntime::Gguf(_))
                                && model.id != settings.transcription.model
                        })
                        .map(|model| model.id)
                }),
            transcription_deletion_error: None,
            mode_editors,
            voice_action_inputs,
            selected_mode: if preview
                .as_ref()
                .is_some_and(|preview| preview.select_global_mode)
            {
                ModeSelection::Default
            } else if preview
                .as_ref()
                .is_some_and(|preview| matches!(preview.pane, DeveloperPane::Modes))
            {
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
                HotkeyKind::Dictation,
                HotkeyKind::Edit,
                HotkeyKind::PasteLast,
            ]
            .map(|kind| {
                ToggleSpring::new(
                    hotkey_binding(&settings, kind)
                        .and_then(standalone_modifier_side)
                        .is_some(),
                )
            }),
            hotkey_side_selection_springs: [
                HotkeyKind::Dictation,
                HotkeyKind::Edit,
                HotkeyKind::PasteLast,
            ]
            .map(|kind| {
                ToggleSpring::at(hotkey_side_index(
                    hotkey_binding(&settings, kind)
                        .and_then(standalone_modifier_side)
                        .unwrap_or(ModifierSide::Either),
                ) as f32)
            }),
            window_focus,
            hotkey_focus,
            _hotkey_blur_subscription: hotkey_blur_subscription,
            _activation_subscription: activation_subscription,
            settings,
            settings_load_error,
            #[cfg(test)]
            settings_save_override: None,
            personal_commands_status,
            personal_workspace,
            personal_config_exists,
            personal_workspace_receiver: None,
            personal_workspace_error: None,
            personal_commands_prompt_copied: false,
            events: Vec::new(),
            current_context: (None, None),
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
            fun_comfort_noise_toggle: ToggleSpring::new(hex_mic_comfort_noise),
            fun_driver_action_seq: crate::hex_microphone::driver_action_seq(),
            fun_driver_missing_preview: fun_driver_missing,
            fun_mic_status,
            fun_mic_status_receiver: None,
            fun_demo_button_state: (
                crate::hex_microphone::DemoSourceKind::Example,
                false,
                false,
                None,
            ),
            fun_active_slider: None,
            fun_last_non_normal_mode: crate::voice_filter::VoiceFilterMode::Muffled,
        };
        window.platform_opened(preview.as_ref());
        if !window.preview {
            window.reload_events();
            window.request_hex_microphone_status();
        }
        window.reload_history(cx);
        if window.preview {
            window.selected_history = window.history_entries.first().map(|entry| entry.id);
            window.spawn_preview_choreography(cx);
        }
        if window.pane == Pane::Modes {
            window.ensure_application_catalog_load();
        }
        window
    }

    fn spawn_preview_choreography(&self, cx: &mut Context<Self>) {
        let Ok(kind) = std::env::var("HEX_PREVIEW_CHOREOGRAPHY") else {
            return;
        };
        cx.spawn(async move |window, cx| {
            use crate::voice_filter::VoiceFilterMode;
            match kind.as_str() {
                "fun" => {
                    Timer::after(Duration::from_millis(650)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.set_hex_microphone_mode(VoiceFilterMode::Muted, cx);
                    });
                    Timer::after(Duration::from_millis(850)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.set_hex_microphone_mode(VoiceFilterMode::Robot, cx);
                    });
                    Timer::after(Duration::from_millis(850)).await;
                    let _ = window.update(cx, |this, cx| {
                        crate::hex_microphone::simulate_silent_demo_playback(
                            Duration::from_millis(1100),
                        );
                        this.fun_waveform.update(cx, |_, cx| cx.notify());
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(1250)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.set_hex_microphone_mode(VoiceFilterMode::Chipmunk, cx);
                    });
                    Timer::after(Duration::from_millis(750)).await;
                    crate::motion::log_pane_summary("Fun", "choreography_done");
                    crate::motion::log_pane_summary("FunWaveform", "choreography_done");
                }
                "panes" => {
                    Timer::after(Duration::from_millis(600)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.microphone_picker_open = true;
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(650)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.microphone_picker_open = false;
                        this.set_shortcut_mode(crate::dictation::ShortcutMode::Toggle, cx);
                    });
                    Timer::after(Duration::from_millis(650)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.select_pane(Pane::Modes, cx);
                    });
                    Timer::after(Duration::from_millis(650)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.select_pane(Pane::Fun, cx);
                    });
                    Timer::after(Duration::from_millis(650)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.select_pane(Pane::Settings, cx);
                    });
                    Timer::after(Duration::from_millis(550)).await;
                    crate::motion::log_pane_summary("Settings", "choreography_done");
                    crate::motion::log_pane_summary("Modes", "choreography_done");
                    crate::motion::log_pane_summary("Fun", "choreography_done");
                }
                #[cfg(target_os = "macos")]
                "hud" => {
                    Timer::after(Duration::from_millis(550)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.platform.hud_demo = platform::HudDemoState::Recording;
                        this.apply_hud_lab();
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(900)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.platform.hud_demo = platform::HudDemoState::Transcribing;
                        this.platform.hud_queued = 2;
                        this.apply_hud_lab();
                        cx.notify();
                    });
                    Timer::after(Duration::from_millis(900)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.platform.hud_demo = platform::HudDemoState::Processing;
                        this.platform.hud_queued = 0;
                        this.select_hud_style(1, cx);
                    });
                    Timer::after(Duration::from_millis(900)).await;
                    let _ = window.update(cx, |this, cx| {
                        this.platform.hud_demo = platform::HudDemoState::Recording;
                        this.select_hud_style(0, cx);
                    });
                    Timer::after(Duration::from_millis(650)).await;
                    crate::motion::log_pane_summary("HUD Lab", "choreography_done");
                }
                _ => {}
            }
        })
        .detach();
    }

    /// Refreshes disk-backed meeting and activity data while retaining valid
    /// selections. This is also called whenever an existing shell is reopened.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.preview {
            self.platform_refresh();
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
        if self.pane != pane {
            crate::motion::log_pane_summary(self.pane.label(), "pane_switched");
        }
        if self.pane == Pane::Fun && pane != Pane::Fun {
            crate::hex_microphone::stop_demo_playback();
            crate::hex_microphone::stop_demo_recording();
        }
        self.pane = pane;
        self.mode_context_menu = None;
        self.history_retention_open = false;
        self.pending_microphone_policy = None;
        self.fun_active_slider = None;
        match pane {
            Pane::HudLab | Pane::Meetings => self.platform_select_pane(pane),
            Pane::Commands | Pane::Activity => self.reload_events(),
            Pane::History => self.reload_history(cx),
            Pane::Modes => self.ensure_application_catalog_load(),
            Pane::Fun => {
                if std::env::var_os("HEX_PREVIEW_BEFORE").is_none() {
                    crate::hex_microphone::prewarm_demo_async();
                }
                if !self.preview {
                    self.request_hex_microphone_status();
                }
            }
            Pane::VoiceAction => {}
            Pane::Settings => {
                self.permission_refresh_at = Instant::now();
                if !self.preview && self.settings.experimental.hex_microphone_available() {
                    self.request_hex_microphone_status();
                }
            }
        }
        cx.notify();
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn show_settings(&mut self, cx: &mut Context<Self>) {
        self.select_pane(Pane::Settings, cx);
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn show_transcription_models(&mut self, cx: &mut Context<Self>) {
        self.show_settings(cx);
        self.transcription_picker_language = Some(self.settings.transcription.language.clone());
        self.poll_transcription_download();
        cx.notify();
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn developer_select_pane(&mut self, pane: DeveloperPane, cx: &mut Context<Self>) {
        self.select_pane(Pane::from_developer(pane), cx);
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn developer_pane(&self) -> DeveloperPane {
        self.pane.developer()
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
        let mut candidate = self.settings_candidate();
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
        let result = self.commit_settings(candidate);
        cx.notify();
        result?;
        self.commands_toggle
            .set_enabled(self.settings.commands_enabled);
        self.release_microphone_toggle
            .set_enabled(self.settings.release_microphone_while_idle);
        self.pending_microphone_policy = None;
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
        if self.preview {
            return false;
        }
        let status = self.preparation_status();
        let changed = self.transcription_status != status;
        self.transcription_status = status;
        changed || self.transcription_status.model.is_some()
    }

    /// The model preparation the window shows: this OS's service when it
    /// prepares models, otherwise the window's own.
    fn preparation_status(&self) -> PreparationStatus {
        self.platform_preparation_status()
            .unwrap_or_else(|| self.transcription_preparation.status())
    }

    fn poll_setup(&mut self, force: bool) -> bool {
        if self.preview
            || !force
                && (Instant::now() < self.permission_refresh_at
                    || !self.setup_visible
                        && !self.platform_awaits_setup()
                        && self.pane != Pane::Settings
                        && self.setup_status.transcription_model)
        {
            return false;
        }
        self.permission_refresh_at = Instant::now()
            + if self.setup_visible {
                TOUR_PERMISSION_REFRESH_INTERVAL
            } else {
                PERMISSION_REFRESH_INTERVAL
            };
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
            self.platform_setup_ready();
        }
        self.platform_poll_setup() || changed
    }

    fn poll_login_item(&mut self) -> bool {
        let Some(worker) = &mut self.login_item_worker else {
            return false;
        };
        let response = worker.poll();
        let changed = match response {
            Ok(Some(response)) => self.apply_login_item_response(response),
            Ok(None) => false,
            Err(error) => {
                self.login_item_worker_failed(error);
                return true;
            }
        };
        let request_changed = self.request_login_item(LoginItemRequest::Status);
        changed || request_changed
    }

    fn apply_login_item_response(&mut self, response: LoginItemResponse) -> bool {
        let before = (
            self.launch_at_login_status,
            self.launch_at_login_toggle.enabled(),
        );
        let error_changed = match response.result {
            Ok(status) => {
                let clear_error = self.launch_at_login_status.is_none()
                    || matches!(response.request, LoginItemRequest::SetEnabled(_))
                    || self.launch_at_login_status != Some(status)
                        && status == LoginItemStatus::Enabled;
                self.launch_at_login_status = Some(status);
                clear_error && self.launch_at_login_error.take().is_some()
            }
            Err(error) => {
                if let Some(status) = error.status {
                    self.launch_at_login_status = Some(status);
                }
                let changed = self.launch_at_login_error.as_deref() != Some(error.message.as_str());
                self.launch_at_login_error = Some(error.message);
                changed
            }
        };
        let enabled = self
            .login_item_worker
            .as_ref()
            .and_then(LoginItemWorker::desired_enabled)
            .unwrap_or(self.launch_at_login_status == Some(LoginItemStatus::Enabled));
        self.launch_at_login_toggle.set_enabled(enabled);
        error_changed
            || before
                != (
                    self.launch_at_login_status,
                    self.launch_at_login_toggle.enabled(),
                )
    }

    fn request_login_item(&mut self, request: LoginItemRequest) -> bool {
        let Some(worker) = &mut self.login_item_worker else {
            return false;
        };
        if let Err(error) = worker.request(request) {
            self.login_item_worker_failed(error);
            return true;
        }
        false
    }

    fn login_item_worker_failed(&mut self, error: String) {
        self.login_item_worker = None;
        self.launch_at_login_error = Some(error);
        self.launch_at_login_toggle
            .set_enabled(self.launch_at_login_status == Some(LoginItemStatus::Enabled));
    }

    fn poll_personal_commands(&mut self) -> bool {
        if self.preview {
            return false;
        }
        let config_exists = self.personal_workspace.join("hex.config.ts").is_file();
        let config_changed = config_exists != self.personal_config_exists;
        self.personal_config_exists = config_exists;
        let Ok(mut status) = crate::personal_commands::load_status() else {
            return config_changed;
        };
        crate::personal_commands::include_builtin_transformations(&mut status);
        if status == self.personal_commands_status {
            return config_changed;
        }
        self.personal_commands_status = status;
        self.personal_commands_changed();
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
                self.personal_config_exists = workspace.join("hex.config.ts").is_file();
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

    fn apply_transcription_selection(
        &mut self,
        selection: TranscriptionSelection,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        validate(&selection).map_err(|error| error.to_string())?;
        let mut candidate = self.settings_candidate();
        candidate.remember_transcription(selection);
        self.commit_settings(candidate)
            .map_err(|error| error.to_string())?;
        if self.preview {
            self.transcription_preview_installed = Some(true);
            self.setup_status.transcription_model = true;
        }
        self.transcription_hints.entity.update(cx, |input, cx| {
            input.set_text(&self.settings.transcription.recognition_hints, cx);
        });
        self.permission_refresh_at = Instant::now();
        self.transcription_picker_language = None;
        self.clear_transcription_error();
        Ok(())
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
        self.clear_transcription_error();
        if self.preview {
            self.apply_transcription_selection(selection, cx)
                .expect("preview selections do not write settings");
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
        if !self.platform_prepare_transcription(&selection) {
            self.transcription_preparation.start(selection, false);
        }
        self.poll_transcription_download();
    }

    fn cancel_transcription_download(&mut self) {
        if !self.platform_cancel_transcription() {
            self.transcription_preparation.cancel();
        }
        self.transcription_status = self.preparation_status();
    }

    fn clear_transcription_error(&mut self) {
        self.transcription_preparation.set_error(None);
        self.transcription_status = self.preparation_status();
        self.transcription_deletion = None;
        self.transcription_deletion_error = None;
    }

    /// Downloaded local models other than the dictation model can be deleted,
    /// whatever language the picker shows, unless they are being prepared.
    fn transcription_model_deletion(
        &self,
        model: &ModelDefinition,
        installed: bool,
    ) -> ModelDeletion {
        if !installed
            || !matches!(model.runtime, ModelRuntime::Gguf(_))
            || self.settings.transcription.model == model.id
            || self.transcription_status.model == Some(model.id)
        {
            ModelDeletion::Unavailable
        } else if self.transcription_deletion == Some(model.id) {
            ModelDeletion::Confirming
        } else {
            ModelDeletion::Offered
        }
    }

    fn delete_transcription_model(&mut self, model: &'static ModelDefinition) {
        self.transcription_deletion_error = None;
        let language = self
            .transcription_picker_language
            .clone()
            .unwrap_or_default();
        let installed = self.transcription_model_installed(model, &language);
        match self.transcription_model_deletion(model, installed) {
            ModelDeletion::Unavailable => self.transcription_deletion = None,
            ModelDeletion::Offered => self.transcription_deletion = Some(model.id),
            ModelDeletion::Confirming => {
                self.transcription_deletion = None;
                if self.preview {
                    return;
                }
                match crate::transcription_models::remove_installed(model) {
                    Ok(()) => tracing::info!(model = ?model.id, "deleted transcription model"),
                    Err(error) => {
                        tracing::warn!(model = ?model.id, %error, "could not delete transcription model");
                        self.transcription_deletion_error = Some(error.to_string());
                    }
                }
            }
        }
    }

    fn transcription_model_installed(&self, model: &ModelDefinition, language: &str) -> bool {
        self.transcription_preview_installed
            .unwrap_or_else(|| crate::transcription_models::is_installed(model, language))
    }

    fn reload_events(&mut self) {
        if !matches!(self.pane, Pane::Commands | Pane::Activity) {
            return;
        }
        let selected = self
            .selected_event
            .and_then(|index| self.events.get(index))
            .cloned();
        self.activity.refresh(&mut self.event_reader);
        if self.activity.error.is_some() {
            return;
        }
        self.events = self.event_reader.recent(ACTIVITY_LIMIT);
        self.current_context = activity_context(&self.event_reader);
        self.selected_event =
            selected.and_then(|selected| self.events.iter().position(|event| event == &selected));
    }

    fn history_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, tr("Search history"), ""));
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
        if !self.update_settings(cx, |settings| settings.history_retention = retention) {
            return;
        }
        if let Some(history) = &self.history
            && let Err(error) = history.set_retention(retention)
        {
            self.history_error = Some(error.to_string());
        }
        self.reload_history(cx);
        cx.notify();
    }

    fn copy_history_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.history_entries.iter().find(|entry| entry.id == id) else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(entry.final_text.clone()));
        self.history_copied = Some(id);
        self.history_error = None;
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

    fn render_history(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let retention = self.settings.history_retention;
        let search = div()
            .w(px(SEARCH_FIELD_WIDTH))
            .child(self.history_search.entity.clone());
        let retention_control = button(
            ButtonKind::Secondary,
            tr_fill("Keep: {}", tr(retention.label())),
        )
        .id("history-retention")
        .child(icon(Icon::ChevronDown, TEXT_TERTIARY))
        .on_click(cx.listener(|this, _, _, cx| {
            this.history_retention_open = true;
            cx.notify();
        }));
        let retention_menu_progress = window.spring_when(
            "history-retention-menu-progress",
            self.history_retention_open,
            SPRING_SNAP,
        );
        let retention_control = div().relative().child(retention_control).when(
            self.history_retention_open || (!cfg!(test) && retention_menu_progress > 0.01),
            |control| {
                control.child(deferred(
                    dropdown_menu_with_progress(retention_menu_progress)
                        .id("history-retention-menu")
                        .right_0()
                        .w(px(MENU_WIDTH))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.history_retention_open = false;
                            cx.notify();
                        }))
                        .children(HistoryRetention::ALL.into_iter().enumerate().map(
                            |(index, choice)| {
                                menu_item(choice == retention)
                                    .id(("history-retention-choice", index))
                                    .child(tr(choice.label()))
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
        let clear = if self.history_clear_armed {
            button(ButtonKind::Danger, tr("Really clear all?"))
        } else {
            button(ButtonKind::Secondary, tr("Clear all"))
        }
        .id("history-clear")
        .on_click(cx.listener(|this, _, _, cx| this.clear_history(cx)))
        .into_any_element();
        let header_action = div()
            .flex()
            .items_center()
            .gap_2()
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
                let mut meta = tr(entry.kind.label()).to_string();
                if let Some(application) = &entry.application {
                    meta.push_str(" · ");
                    meta.push_str(application);
                }
                timestamped_list_row(
                    ("history-entry", index),
                    selected,
                    age,
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(list_row_title(preview, selected))
                        .child(list_row_meta(meta, selected)),
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
            .child(pane_header_with_action(tr("History"), Some(header_action)))
            .child(
                pane_body().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(pane_list(
                            "history-list",
                            PANE_LIST_WIDTH,
                            if retention_off {
                                Some(tr("History is off. New dictations are not retained."))
                            } else if self.history_entries.is_empty()
                                && self.history_error.is_none()
                            {
                                Some(tr("No dictations retained yet."))
                            } else {
                                None
                            },
                            tr("History could not be loaded."),
                            self.history_error.clone(),
                            rows,
                        ))
                        .child(pane_detail(self.render_history_detail(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_history_detail(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self
            .selected_history
            .and_then(|id| self.history_entries.iter().find(|entry| entry.id == id))
        else {
            return detail_placeholder(tr("Select a history entry."));
        };
        let id = entry.id;
        let copied = self.history_copied == Some(id);
        let show_raw = entry.raw_text.trim() != entry.final_text.trim();
        let action_button = |label: &'static str, id_suffix: &'static str| {
            button(ButtonKind::Secondary, label)
                .id(SharedString::from(format!("history-action-{id_suffix}")))
        };
        let mut latency = tr_named(
            "{audio} ms audio · {inference} ms inference",
            &[
                ("audio", &entry.audio_ms.to_string()),
                ("inference", &entry.inference_ms.to_string()),
            ],
        );
        if entry.total_ms > 0 {
            latency.push_str(&format!(
                " · {}",
                tr_fill("{} ms total", &entry.total_ms.to_string())
            ));
        }
        let section = |label: &'static str, first: bool| {
            div()
                .w_full()
                .when(!first, |section| {
                    section.mt_5().pt_5().border_t_1().border_color(rgb(LINE))
                })
                .child(section_label(label))
        };
        div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .flex_col()
            .child(detail_header(
                detail_title(tr(entry.kind.label())),
                vec![event_age(entry.timestamp_ms).into_any_element()],
                vec![
                    action_button(if copied { tr("Copied") } else { tr("Copy") }, "copy")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.copy_history_entry(id, cx)),
                        )
                        .into_any_element(),
                    action_button(tr("Delete"), "delete")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.delete_history_entry(id, cx)),
                        )
                        .into_any_element(),
                ],
            ))
            .child(
                detail_body("history-detail")
                    .child(
                        section(tr("Final text"), true).child(
                            div()
                                .pt_2()
                                .text_body()
                                .text_color(rgb(TEXT_PRIMARY))
                                .child(entry.final_text.clone()),
                        ),
                    )
                    .when(show_raw, |detail| {
                        detail.child(
                            section(tr("Raw transcript"), false).child(
                                div()
                                    .pt_2()
                                    .text_body()
                                    .text_color(rgb(TEXT_TERTIARY))
                                    .child(entry.raw_text.clone()),
                            ),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .mt_5()
                            .flex()
                            .flex_col()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .when_some(entry.application.clone(), |rows, application| {
                                rows.child(detail_row(tr("Application"), application))
                            })
                            .when_some(entry.processing.clone(), |rows, processing| {
                                let mut summary = format!(
                                    "{} · {} ms",
                                    processing.profile, processing.latency_ms
                                );
                                if let Some(fallback) = &processing.fallback {
                                    summary.push_str(&format!(
                                        " · {}",
                                        tr_fill("fell back: {}", fallback)
                                    ));
                                }
                                rows.child(detail_row(tr("Processing"), summary))
                            })
                            .child(detail_row(tr("Latency"), latency)),
                    ),
            )
            .into_any_element()
    }

    fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let meetings_available = self.settings.meetings.available();
        let fun_available =
            self.settings.experimental.hex_microphone_available() || self.pane == Pane::Fun;
        let update_available = self.platform_update_button(cx).is_some();
        let pane = self.pane;
        let preview = self.preview;
        self.navigation_sidebar.update(cx, |sidebar, cx| {
            if sidebar.pane != pane
                || sidebar.meetings_available != meetings_available
                || sidebar.fun_available != fun_available
                || sidebar.update_available != update_available
                || sidebar.preview != preview
            {
                sidebar.pane = pane;
                sidebar.meetings_available = meetings_available;
                sidebar.fun_available = fun_available;
                sidebar.update_available = update_available;
                sidebar.preview = preview;
                cx.notify();
            }
        });
        self.navigation_view.clone().into_any_element()
    }

    fn render_hotkey_control(
        &mut self,
        kind: HotkeyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let binding_keycaps = hotkey_binding(&self.settings, kind)
            .map_or_else(|| vec![tr("Off").into()], HotkeyBinding::keycaps);
        let idle_width =
            hotkey_idle_width(binding_keycaps.len()) + hotkey_label_overflow(&binding_keycaps);
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
            rgb(SUCCESS_SURFACE)
        } else {
            rgb(DANGER_SURFACE)
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
                    tr("Press shortcut")
                } else {
                    tr("Release to save or add key")
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
                            .bg(rgb(DANGER))
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
                            .text_footnote()
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(label),
                    )
                    .child(
                        small_button(ButtonKind::Secondary, tr("Cancel"))
                            .id(match kind {
                                HotkeyKind::Dictation => "cancel-dictation-hotkey-capture",
                                HotkeyKind::Edit => "cancel-edit-hotkey-capture",
                                HotkeyKind::PasteLast => "cancel-paste-last-hotkey-capture",
                            })
                            .ml_1()
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
                .child(tag(Tone::Success, tr("Saved")))
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
                        .text_footnote()
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(tr("Change shortcut")),
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
            .h(px(CONTROL_HEIGHT))
            .px(px(2.0))
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(RADIUS_CONTROL)
            .border_1()
            .border_color(mix_color(
                rgb(LINE),
                rgb(TEXT_SECONDARY),
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
        let side = hotkey_binding(&self.settings, kind)
            .filter(|_| platform::CAPABILITIES.modifier_sides)
            .and_then(standalone_modifier_side);
        let side_animation = &mut self.hotkey_side_animations[hotkey_kind_index(kind)];
        side_animation.set_enabled(side.is_some());
        let side_position = side_animation.render_position(window).clamp(0.0, 1.0);
        let selected = side.unwrap_or(ModifierSide::Either);
        let side_widths = HOTKEY_SIDE_WIDTHS;
        let side_selection_spring =
            &mut self.hotkey_side_selection_springs[hotkey_kind_index(kind)];
        side_selection_spring.set_target(hotkey_side_index(selected) as f32);
        let selection_position = side_selection_spring.render_position(window);
        let side_selector = div()
            .w(px(HOTKEY_SIDE_SELECTOR_WIDTH * side_position))
            .mr(px(8.0 * side_position))
            .flex_none()
            .overflow_hidden()
            .opacity(side_position)
            .child(
                sliding_segmented_control(selection_position, &side_widths)
                    .w(px(HOTKEY_SIDE_SELECTOR_WIDTH))
                    .children(
                        [
                            (tr("Left"), ModifierSide::Left),
                            (tr("Either"), ModifierSide::Either),
                            (tr("Right"), ModifierSide::Right),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, side))| {
                            let candidate = hotkey_side_binding(&self.settings, kind, side);
                            sliding_segmented_item(side_widths[index], selected == side)
                                .id(("hotkey-side", hotkey_kind_index(kind) * 3 + index))
                                .when(candidate.is_none(), |item| item.opacity(DISABLED_OPACITY))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let Some(candidate) =
                                        hotkey_side_binding(&this.settings, kind, side)
                                    else {
                                        return;
                                    };
                                    if !this.update_settings(cx, |settings| {
                                        set_hotkey_binding(settings, kind, candidate);
                                    }) {
                                        return;
                                    }
                                    if kind == HotkeyKind::Edit {
                                        this.voice_action_inputs.error = None;
                                    }
                                    this.hotkey_side_selection_springs[hotkey_kind_index(kind)]
                                        .set_target(index as f32);
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

    fn begin_hotkey_capture(
        &mut self,
        kind: HotkeyKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.preview {
            platform::set_hotkey_capture_active(true);
        }
        self.hotkey_width_spring.set_target(hotkey_capture_width(0));
        self.hotkey_capture = HotkeyCaptureState::Listening {
            kind,
            modifiers: HotkeyModifiers::default(),
            message: None,
            started_at: Instant::now(),
        };
        self.hotkey_capture_animation.set_enabled(true);
        self.hotkey_focus.focus(window);
        self.platform_capture_started(cx);
        cx.notify();
    }

    fn cancel_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.hotkey_capture, HotkeyCaptureState::Idle) {
            if !self.preview {
                platform::set_hotkey_capture_active(false);
                self.platform_capture_cancelled();
            }
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
        if event.is_held || !platform::records_key_events() {
            return;
        }
        if event.keystroke.key == "escape" {
            self.cancel_hotkey_capture(cx);
            return;
        }
        let modifiers =
            platform::hotkey_modifiers(event.keystroke.modifiers).without_side_constraints();
        let key = match platform::hotkey_key(&event.keystroke.key) {
            Ok(key) => key,
            Err(message) => {
                self.set_hotkey_capture_message(message, cx);
                return;
            }
        };
        if modifiers.is_empty() && !is_function_key(&key.label) {
            self.set_hotkey_capture_message(tr("Add a modifier"), cx);
            return;
        }
        let binding = HotkeyBinding {
            modifiers,
            key: Some(key),
        };
        let keycaps = binding.keycaps();
        self.hotkey_width_spring
            .set_target(hotkey_capture_width(keycaps.len()) + hotkey_label_overflow(&keycaps));
        self.save_hotkey_binding(binding, cx);
    }

    fn capture_hotkey_modifiers(&mut self, event: &ModifiersChangedEvent, cx: &mut Context<Self>) {
        if !platform::records_key_events() {
            return;
        }
        let current = normalize_captured_modifiers(platform::hotkey_modifiers(event.modifiers));
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
                let keycaps = HotkeyBinding {
                    modifiers: *modifiers,
                    key: None,
                }
                .keycaps();
                self.hotkey_width_spring.set_target(
                    hotkey_capture_width(keycaps.len()) + hotkey_label_overflow(&keycaps),
                );
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
            self.set_hotkey_capture_message(tr("Press a shortcut"), cx);
            return;
        }
        let kind = match self.hotkey_capture {
            HotkeyCaptureState::Listening { kind, .. } => kind,
            HotkeyCaptureState::Idle | HotkeyCaptureState::Saved { .. } => return,
        };
        if let Err(message) = platform::validate_hotkey(kind, &binding) {
            self.set_hotkey_capture_message(message, cx);
            return;
        }
        if hotkey_binding_conflicts(&self.settings, kind, &binding) {
            self.set_hotkey_capture_message(tr("Already in use"), cx);
            return;
        }
        if !self.preview && self.platform_save_hotkey(kind, &binding) {
            self.set_hotkey_capture_message(tr("Saving…"), cx);
            return;
        }
        let keycaps = binding.keycaps();
        let keycap_count = keycaps.len();
        let label_overflow = hotkey_label_overflow(&keycaps);
        if !self.update_settings(cx, |settings| set_hotkey_binding(settings, kind, binding)) {
            self.set_hotkey_capture_message(tr("Could not save shortcut. Try again."), cx);
            return;
        }
        self.hotkey_width_spring
            .set_target(hotkey_saved_width(keycap_count) + label_overflow);
        if kind == HotkeyKind::Edit {
            self.voice_action_inputs.error = None;
        }
        if !self.preview {
            platform::set_hotkey_capture_active(false);
        }
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
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(index + 1 < replacement_count, |row| {
                        row.border_b_1().border_color(rgb(LINE))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(inputs.matched_phrase.entity.clone()),
                    )
                    .child(icon(Icon::ArrowRight, TEXT_TERTIARY))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(inputs.output.entity.clone()),
                    )
                    .child(
                        icon_button(ButtonKind::Ghost, Icon::Close)
                            .id(("remove-text-replacement", index))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.update_mode_settings(selection, cx, |mode| {
                                    mode.replacements.remove(index);
                                }) {
                                    this.mode_inputs_mut(selection).replacements.remove(index);
                                }
                            })),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let add = icon_button(ButtonKind::Ghost, Icon::Plus)
            .mr(px(-SPACE_2))
            .id("add-mode-replacement")
            .on_click(cx.listener(move |this, _, window, cx| {
                let replacement = TextReplacement::default();
                if this.update_mode_settings(selection, cx, |mode| {
                    mode.replacements.push(replacement.clone());
                }) {
                    let inputs = Self::replacement_inputs(&replacement, cx);
                    let focus = inputs.matched_phrase.entity.focus_handle(cx);
                    this.mode_inputs_mut(selection).replacements.push(inputs);
                    focus.focus(window);
                }
            }))
            .into_any_element();

        panel()
            .child(panel_header(tr("Corrections"), Some(add)))
            .when(replacement_count == 0, |panel| {
                panel.child(
                    div()
                        .px_4()
                        .py_3()
                        .text_footnote()
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(tr("No corrections in this mode.")),
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
                let downloading = self.transcription_status.model == Some(model.id);
                let active = transcription_selection_is_active(
                    &self.settings.transcription,
                    model,
                    selected_language,
                    installed,
                );
                let progress = if downloading {
                    model.download_bytes().map_or(0.0, |bytes| {
                        (self.transcription_status.downloaded_bytes as f32 / bytes as f32)
                            .clamp(0.0, 1.0)
                    })
                } else {
                    0.0
                };
                let status = if downloading {
                    if installed {
                        let elapsed = self
                            .transcription_status
                            .started
                            .map_or(0, |started| started.elapsed().as_secs());
                        let progress = self
                            .transcription_status
                            .started
                            .map_or(0.0, |started| (started.elapsed().as_secs_f32() % 1.6) / 1.6);
                        TranscriptionPickerStatus::Preparing {
                            label: tr_fill("Loading model · {}s", &elapsed.to_string()),
                            progress: Some(TranscriptionPickerProgress::Loading(progress)),
                        }
                    } else {
                        TranscriptionPickerStatus::Preparing {
                            label: tr_fill("Downloading {}%", &format!("{:.0}", progress * 100.0)),
                            progress: matches!(model.runtime, ModelRuntime::Gguf(_))
                                .then_some(TranscriptionPickerProgress::Downloading(progress)),
                        }
                    }
                } else if active {
                    TranscriptionPickerStatus::Active
                } else {
                    TranscriptionPickerStatus::Available {
                        installed,
                        deletion: self.transcription_model_deletion(model, installed),
                    }
                };
                TranscriptionPickerModel { choice, status }
            })
            .collect();
        TranscriptionPickerView {
            error: self.transcription_status.error.clone(),
            deletion_error: self.transcription_deletion_error.clone(),
            language: selected_language.to_string(),
            models,
            open_progress: 1.0,
            language_progress: 1.0,
        }
    }

    fn render_transcription_picker(
        &mut self,
        selected_language: &str,
        open_progress: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut view = self.transcription_picker_view(selected_language);
        view.open_progress = open_progress;
        view.language_progress = 1.0;
        render_shared_transcription_picker(view, cx)
    }

    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>) {
        self.transcription_picker_language = None;
        self.clear_transcription_error();
        cx.notify();
    }

    fn render_microphone_picker(&self, progress: f32, cx: &mut Context<Self>) -> AnyElement {
        let choices = std::iter::once(None)
            .chain(self.microphone_devices.iter().cloned().map(Some))
            .collect::<Vec<_>>();
        microphone_picker_menu(
            choices,
            self.settings.microphone.clone(),
            self.microphone_picker_error.clone(),
            progress,
            cx.listener(|this, device: &Option<String>, _, cx| {
                if this.update_settings(cx, |settings| settings.microphone = device.clone()) {
                    this.microphone_picker_open = false;
                    this.microphone_picker_error = None;
                }
            }),
            cx.listener(|this, _, _, cx| {
                this.microphone_picker_open = false;
                this.microphone_picker_error = None;
                cx.notify();
            }),
        )
        .into_any_element()
    }

    fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preview {
            self.launch_at_login_status = Some(if enabled {
                LoginItemStatus::Enabled
            } else {
                LoginItemStatus::Disabled
            });
        } else if self.launch_at_login_status.is_none() || self.login_item_worker.is_none() {
            return;
        }
        self.launch_at_login_toggle.set_enabled(enabled);
        self.request_login_item(LoginItemRequest::SetEnabled(enabled));
        cx.notify();
    }

    fn render_model_notice(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.setup_visible {
            return None;
        }
        let dictation_notice =
            dictation_model_notice(self.setup_status, self.transcription_status.model.is_some());
        let (title, description, action) = if let Some((title, description)) = dictation_notice {
            let action = button(ButtonKind::Primary, tr("Choose model"))
                .id("choose-required-model")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.transcription_picker_language =
                        Some(this.settings.transcription.language.clone());
                    cx.notify();
                }))
                .into_any_element();
            (title, description, action)
        } else {
            self.platform_model_notice(cx)?
        };
        Some(
            div()
                .id("dictation-model-notice")
                .w_full()
                .flex()
                .justify_center()
                .flex_shrink_0()
                .px(px(PANE_GUTTER))
                .pt_6()
                .child(
                    panel()
                        .max_w(px(PANE_CONTENT_WIDTH))
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(settings_copy(title, description)),
                        )
                        .child(div().flex_shrink_0().child(action)),
                )
                .into_any_element(),
        )
    }

    /// 2.3.2's only change: the top of every pane points macOS users at the
    /// new Hex, which this build can't install itself.
    fn render_new_hex_notice(&self) -> Option<AnyElement> {
        if !cfg!(target_os = "macos") || self.setup_visible {
            return None;
        }
        let action = button(ButtonKind::Primary, "Get the new Hex")
            .id("get-new-hex")
            .on_click(|_, _, _| open_new_hex());
        Some(
            div()
                .id("new-hex-notice")
                .w_full()
                .flex()
                .justify_center()
                .flex_shrink_0()
                .px(px(PANE_GUTTER))
                .pt_6()
                .child(
                    panel()
                        .max_w(px(PANE_CONTENT_WIDTH))
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(div().flex_1().min_w_0().child(settings_copy(
                            "The new Hex is here",
                            "Rebuilt from the ground up, lighter, and it updates itself. \
                             This version gets no more updates.",
                        )))
                        .child(div().flex_shrink_0().child(action)),
                )
                .into_any_element(),
        )
    }

    fn render_listener_card(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let listener = self.platform_listener()?;
        let running = listener.running;
        let shortcut = self.settings.dictation_hotkey.keycaps().join("+");
        let hint = match (running, self.settings.shortcut_mode) {
            (true, crate::dictation::ShortcutMode::Toggle) => tr_fill(
                "Ready anywhere · Press {} to start and stop dictating",
                &shortcut,
            ),
            (true, crate::dictation::ShortcutMode::Hold) => {
                tr_fill("Ready anywhere · Hold {} to dictate", &shortcut)
            }
            (false, _) => tr_fill("Enable Hex to dictate into any app with {}", &shortcut),
        };
        let action = button(
            ButtonKind::Secondary,
            if running { tr("Disable") } else { tr("Enable") },
        )
        .id("toggle-listening")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_listening(!running);
            cx.notify();
        }));
        Some(
            div()
                .pt_6()
                .child(
                    panel().child(
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
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(status_dot(if running {
                                        SUCCESS
                                    } else {
                                        TEXT_TERTIARY
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .child(
                                                div()
                                                    .text_body()
                                                    .text_color(rgb(TEXT_PRIMARY))
                                                    .child(tr(&listener.status).to_string()),
                                            )
                                            .child(truncated_line(FOOTNOTE, hint))
                                            .when_some(listener.last_failure, |column, failure| {
                                                column.child(
                                                    truncated_line(
                                                        FOOTNOTE,
                                                        tr_fill(
                                                            "Last dictation failed: {}",
                                                            &failure,
                                                        ),
                                                    )
                                                    .text_color(rgb(DANGER)),
                                                )
                                            }),
                                    ),
                            )
                            .child(action),
                    ),
                )
                .when_some(listener.error, |card, error| {
                    card.child(error_message(tr("Hex could not apply this change."), error))
                })
                .into_any_element(),
        )
    }

    fn render_interface_language_control(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.ui_language.clone();
        let label = tr(crate::desktop_i18n::choice_name(selected.as_deref()));
        let menu_progress = window.spring_when(
            "interface-language-picker-progress",
            self.interface_language_open,
            SPRING_SNAP,
        );
        let menu =
            (self.interface_language_open || (!cfg!(test) && menu_progress > 0.01)).then(|| {
                dropdown_menu_with_progress(menu_progress)
                    .id("interface-language-picker")
                    .right_0()
                    .w(px(DISCLOSURE_WIDTH))
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.interface_language_open = false;
                        cx.notify();
                    }))
                    .children(
                        crate::desktop_i18n::LANGUAGE_CHOICES
                            .iter()
                            .enumerate()
                            .map(|(index, (code, name))| {
                                let code = code.map(str::to_string);
                                menu_item(selected == code)
                                    .id(("interface-language-choice", index))
                                    .child(truncated_line(FOOTNOTE, tr(name)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.interface_language_open = false;
                                        let code = code.clone();
                                        if this.update_settings(cx, |settings| {
                                            settings.ui_language = code.clone();
                                        }) {
                                            crate::desktop_i18n::apply(code.as_deref());
                                        }
                                    }))
                            }),
                    )
            });
        div()
            .relative()
            .flex_none()
            .child(
                disclosure_button(label)
                    .id("interface-language")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.interface_language_open = !this.interface_language_open;
                        cx.notify();
                    })),
            )
            .children(menu.map(deferred))
            .into_any_element()
    }

    fn render_software_update_control(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let control = match self.platform_software_update()? {
            SoftwareUpdate::Checking => busy_label(tr("Checking")).into_any_element(),
            SoftwareUpdate::Available { version } => self.render_update_button(
                version.map_or_else(|| tr("Update").into(), |v| tr_fill("Update to {}", &v)),
                cx,
            ),
            SoftwareUpdate::ReadyToRestart { version } => self.render_update_button(
                version.map_or_else(
                    || tr("Restart to update").into(),
                    |v| tr_fill("Restart into {}", &v),
                ),
                cx,
            ),
            SoftwareUpdate::Current if !platform::MANUAL_UPDATE_CHECK => div()
                .text_footnote()
                .text_color(rgb(TEXT_TERTIARY))
                .child(tr("Up to date"))
                .into_any_element(),
            SoftwareUpdate::Failed if !platform::MANUAL_UPDATE_CHECK => div()
                .text_footnote()
                .text_color(rgb(TEXT_TERTIARY))
                .child(tr("Check failed; retrying later"))
                .into_any_element(),
            SoftwareUpdate::Current | SoftwareUpdate::Failed => {
                button(ButtonKind::Secondary, tr("Check now"))
                    .id("software-update")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.check_for_updates();
                        cx.notify();
                    }))
                    .into_any_element()
            }
        };
        Some(control)
    }

    fn render_update_button(&self, label: String, cx: &mut Context<Self>) -> AnyElement {
        button(ButtonKind::Secondary, status_dot(ACCENT_TEXT))
            .child(label)
            .id("software-update")
            .debug_selector(|| "software-update".into())
            .on_click(cx.listener(|this, _, _, cx| {
                this.install_update(cx);
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let capabilities = platform::CAPABILITIES;
        let platform_notice = self.platform_settings_notice(cx);
        let listener_card = capabilities
            .listener_control
            .then(|| self.render_listener_card(cx))
            .flatten();
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
                this.clear_transcription_error();
                cx.notify();
            }));
        let double_tap_position = self.double_tap_toggle.render_position(window);
        let release_microphone_position = self.release_microphone_toggle.render_position(window);
        let toggle_mode = self.settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
        self.double_tap_only_visibility.set_enabled(
            !toggle_mode
                && self.settings.double_tap_lock
                && self.settings.dictation_hotkey.key.is_some(),
        );
        let double_tap_only_visibility = self
            .double_tap_only_visibility
            .render_position(window)
            .clamp(0.0, 1.0);
        let dock_icon_position = self.dock_icon_toggle.render_position(window);
        let platform_section = self.platform_settings_section(window, cx);
        let (top_platform_section, bottom_platform_section) =
            if self.preview && self.settings.meetings.enabled {
                (platform_section, Vec::new())
            } else {
                (Vec::new(), platform_section)
            };
        let enable_on_launch_position = self.enable_on_launch_toggle.render_position(window);
        let terminal_paste_position = self.terminal_paste_toggle.render_position(window);
        let indicator_position = self.indicator_position_spring.render_position(window);
        let indicator_control = sliding_segmented_control(indicator_position, &[64.0; 3])
            .id("indicator-position")
            .debug_selector(|| "indicator-position".into())
            .children(
                [
                    (tr("Top"), IndicatorPosition::Top),
                    (tr("Bottom"), IndicatorPosition::Bottom),
                    (tr("Off"), IndicatorPosition::Hidden),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (label, position))| {
                    sliding_segmented_item(64.0, self.settings.indicator_position == position)
                        .id(("indicator-position", index))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.settings.indicator_position != position
                                && this.update_settings(cx, |settings| {
                                    settings.indicator_position = position;
                                })
                            {
                                this.indicator_position_spring.set_target(index as f32);
                            }
                        }))
                }),
            );
        let interface_language_control = capabilities
            .interface_language
            .then(|| self.render_interface_language_control(window, cx));
        let software_update_control = capabilities
            .software_updates
            .then(|| self.render_software_update_control(cx))
            .flatten();
        let launch_at_login_position = self.launch_at_login_toggle.render_position(window);
        let microphone_label = self
            .settings
            .microphone
            .clone()
            .unwrap_or_else(|| tr("Automatic").into());
        let microphone_picker_progress = window.spring_when(
            "microphone-picker-progress",
            self.microphone_picker_open,
            SPRING_SNAP,
        );
        let microphone_picker = (self.microphone_picker_open
            || (!cfg!(test) && microphone_picker_progress > 0.01))
            .then(|| self.render_microphone_picker(microphone_picker_progress, cx));
        let sound_volume_position = self.sound_volume_spring.render_position(window);
        let selected_volume = sound_volume_index(&self.settings);
        let sound_volume =
            sliding_segmented_control(sound_volume_position, &[SOUND_VOLUME_ITEM_WIDTH; 5])
                .children(
                    crate::desktop_ui::SOUND_VOLUME_STEPS
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, volume))| {
                            sliding_segmented_item(
                                SOUND_VOLUME_ITEM_WIDTH,
                                selected_volume == index,
                            )
                            .id(("sound-volume", index))
                            .child(tr(label))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.update_settings(cx, |settings| {
                                        settings.sound_effects = volume > 0.0;
                                        if volume > 0.0 {
                                            settings.sound_effect_volume = volume;
                                        }
                                    }) {
                                        return;
                                    }
                                    this.sound_volume_spring.set_target(index as f32);
                                    if volume > 0.0 && !this.preview {
                                        crate::feedback::play(
                                            crate::feedback::Tone::DictationStart,
                                        );
                                    }
                                },
                            ))
                        }),
                );
        let launch_at_login_control = if self.launch_at_login_status.is_none() {
            div()
                .text_footnote()
                .text_color(rgb(TEXT_TERTIARY))
                .child(if self.login_item_worker.is_some() {
                    tr("Checking…")
                } else {
                    tr("Unavailable")
                })
                .into_any_element()
        } else if self.launch_at_login_status == Some(LoginItemStatus::RequiresApproval) {
            button(ButtonKind::Secondary, tr("Open settings"))
                .id("launch-at-login-approval")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.request_login_item(LoginItemRequest::OpenSettings);
                    cx.notify();
                }))
                .into_any_element()
        } else {
            toggle(launch_at_login_position)
        };
        let recording_audio_position = self.recording_audio_spring.render_position(window);
        let audio_widths = [50.0, 90.0, 80.0];
        let audio_behavior = sliding_segmented_control(recording_audio_position, &audio_widths)
            .children(RecordingAudioBehavior::ALL.into_iter().enumerate().map(
                |(index, behavior)| {
                    let selected = self.settings.recording_audio_behavior == behavior;
                    sliding_segmented_item(audio_widths[index], selected)
                        .id(("recording-audio-behavior", index))
                        .child(tr(behavior.label()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.settings.recording_audio_behavior == behavior {
                                return;
                            }
                            if this.update_settings(cx, |settings| {
                                settings.recording_audio_behavior = behavior
                            }) {
                                this.recording_audio_spring.set_target(index as f32);
                            }
                        }))
                },
            ));
        let shortcut_mode_position = self.shortcut_mode_toggle.render_position(window);
        let shortcut_mode = sliding_segmented_control(shortcut_mode_position, &[72.0; 2])
            .id("shortcut-mode")
            .children(
                [
                    (tr("Hold"), crate::dictation::ShortcutMode::Hold),
                    (tr("Toggle"), crate::dictation::ShortcutMode::Toggle),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (label, mode))| {
                    sliding_segmented_item(72.0, self.settings.shortcut_mode == mode)
                        .id(("shortcut-mode", index))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_shortcut_mode(mode, cx);
                        }))
                }),
            );
        let microphone_mode = sliding_segmented_control(release_microphone_position, &[114.0; 2])
            .id("microphone-mode")
            .children(
                [
                    (tr("Keep ready (fast)"), false),
                    (tr("Release when idle"), true),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (label, release))| {
                    sliding_segmented_item(
                        114.0,
                        self.settings.release_microphone_while_idle == release,
                    )
                    .id(("microphone-mode", index))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if release && !this.settings.release_microphone_while_idle {
                            this.pending_microphone_policy =
                                Some(MicrophonePolicyChange::DisableCommandsAndRelease);
                            cx.notify();
                            return;
                        }
                        this.pending_microphone_policy = None;
                        if this.settings.release_microphone_while_idle != release
                            && let Err(error) = this.update_microphone_policy(
                                MicrophonePolicyChange::SetReleaseWhileIdle(release),
                                cx,
                            )
                        {
                            tracing::error!(%error, "could not update microphone policy");
                        }
                        cx.notify();
                    }))
                }),
            );
        let microphone_confirmation = (self.pending_microphone_policy
            == Some(MicrophonePolicyChange::DisableCommandsAndRelease))
        .then(|| {
            div()
                .px_4()
                .py_3()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .border_b_1()
                .border_color(rgb(LINE))
                .bg(rgb(CANVAS))
                .child(
                    div().flex_1().min_w_0().child(settings_copy(
                        tr("Release the microphone when idle?"),
                        if self.settings.commands_enabled {
                            tr("Opens the microphone on shortcut press without pre-roll and turns Commands off.")
                        } else {
                            tr("Opens the microphone on shortcut press without pre-roll. Commands remain off.")
                        },
                    )),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            button(ButtonKind::Secondary, tr("Cancel"))
                                .id("cancel-release-microphone")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pending_microphone_policy = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            button(ButtonKind::Primary, tr("Release microphone"))
                                .id("confirm-release-microphone")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Err(error) = this.update_microphone_policy(
                                        MicrophonePolicyChange::DisableCommandsAndRelease,
                                        cx,
                                    ) {
                                        tracing::error!(%error, "could not update microphone policy");
                                    }
                                })),
                        ),
                )
        });
        settings_pane(
            tr("Settings"),
            "settings-scroll",
            div()
                .children(platform_notice)
                .children(listener_card)
                .children(top_platform_section)
                .child(settings_section_label(tr("Dictation")))
                .child(
                    panel()
                        .child(settings_row(
                            tr("Local transcription"),
                            tr("Language and on-device speech model"),
                            transcription_control,
                        ))
                        .child(settings_row(
                            tr("Microphone"),
                            tr("Input device for dictation and commands"),
                            div()
                                .relative()
                                .flex_none()
                                .child(
                                    disclosure_button(microphone_label)
                                        .id("microphone-setting")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_microphone_picker(cx)
                                        })),
                                )
                                .children(microphone_picker.map(deferred)),
                        ))
                        .when(
                            transcription_model.supports_recognition_hints,
                            |settings| {
                                settings.child(settings_row(
                                    tr("Recognition hints"),
                                    tr("Names and terms to prime the speech model"),
                                    div()
                                        .w(px(320.0))
                                        .flex_none()
                                        .child(
                                            self.transcription_hints.entity.clone(),
                                        ),
                                ))
                            },
                        )
                        .child(settings_row(
                            tr("Shortcut mode"),
                            if toggle_mode {
                                tr("Press to start dictating, press again to transcribe")
                            } else {
                                tr("Hold to dictate, release to transcribe")
                            },
                            shortcut_mode,
                        ))
                        .child(
                            settings_row(
                                tr("Dictation shortcut"),
                                tr("Click to record a new shortcut"),
                                hotkey_control,
                            )
                            .border_b_0()
                            .id("dictation-hotkey-setting"),
                        ),
                )
                .child(settings_section_label(tr("Behavior")))
                .child(
                    panel()
                        .when(capabilities.recording_audio, |panel| {
                            panel.child(
                                settings_row(
                                    tr("While dictating"),
                                    tr("Mute or pause other audio during dictation"),
                                    audio_behavior,
                                )
                                .id("recording-audio-setting"),
                            )
                        })
                        .when(capabilities.release_microphone, |panel| {
                            panel
                                .child(settings_row(
                                    tr("Microphone mode"),
                                    if self.settings.release_microphone_while_idle {
                                        tr("Opens only while holding the shortcut. Disables pre-roll and commands.")
                                    } else {
                                        tr("Keeps the microphone open for instant start and pre-roll")
                                    },
                                    microphone_mode,
                                ))
                                .children(microphone_confirmation)
                        })
                        .when(!toggle_mode, |panel| {
                            panel.child(
                                settings_row(
                                    tr("Double-tap to lock"),
                                    tr("Double-tap the shortcut for hands-free dictation"),
                                    toggle(double_tap_position),
                                )
                                .id("double-tap-setting")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let enabled = !this.settings.double_tap_lock;
                                    this.set_double_tap_lock(enabled, cx);
                                })),
                            )
                        })
                        .when(capabilities.double_tap_only, |panel| panel.child(
                            div()
                                .h(px(SETTINGS_ROW_MIN_HEIGHT * double_tap_only_visibility))
                                .overflow_hidden()
                                .opacity(double_tap_only_visibility)
                                .child(
                                    settings_row(
                                        tr("Double-tap only"),
                                        tr("Wait for two complete shortcut taps before recording"),
                                        toggle(if self.settings.double_tap_only { 1.0 } else { 0.0 }),
                                    )
                                    .id("double-tap-only-setting")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let enabled = !this.settings.double_tap_only;
                                        this.set_double_tap_only(enabled, cx);
                                    })),
                                ),
                        ))
                        .when(capabilities.indicator_position, |panel| {
                            panel.child(
                                settings_row(
                                    tr("Recording indicator"),
                                    tr("Show the dictation pill at the top or bottom of the screen"),
                                    indicator_control,
                                )
                                .when(!capabilities.terminal_paste && !capabilities.paste_last, |row| {
                                    row.border_b_0()
                                })
                                .id("indicator-position-setting"),
                            )
                        })
                        .when(capabilities.terminal_paste, |panel| {
                            panel.child(
                                settings_row(
                                    tr("Terminal paste"),
                                    tr("Paste with Ctrl+Shift+V so terminals accept it"),
                                    toggle(terminal_paste_position),
                                )
                                .when(!capabilities.paste_last, |row| row.border_b_0())
                                .id("terminal-paste-setting")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let enabled = !this.settings.paste_with_shift;
                                    if this.update_settings(cx, |settings| {
                                        settings.paste_with_shift = enabled
                                    }) {
                                        this.terminal_paste_toggle.set_enabled(enabled);
                                    }
                                })),
                            )
                        })
                        .when(capabilities.paste_last, |panel| panel.child(
                            settings_row(
                                tr("Paste last dictation"),
                                tr("Re-paste the most recent dictation"),
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(paste_last_control)
                                    .when(self.settings.paste_last_hotkey.is_some(), |row| {
                                        row.child(
                                            button(ButtonKind::Secondary, tr("Clear"))
                                                .id("disable-paste-last-hotkey")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.update_settings(cx, |settings| settings.paste_last_hotkey = None);
                                                })),
                                        )
                                    }),
                            )
                            .border_b_0()
                            .id("paste-last-hotkey-setting"),
                        ))
                )
                .children(bottom_platform_section)
                .child(settings_section_label(tr("Application")))
                .child(
                    panel()
                        .when(capabilities.launch_at_login, |panel| panel.child(
                            settings_row(
                                tr("Launch at login"),
                                tr("Start Hex when you log in"),
                                launch_at_login_control,
                            )
                            .id("launch-at-login-setting")
                            .when(
                                matches!(self.launch_at_login_status, Some(LoginItemStatus::Enabled | LoginItemStatus::Disabled))
                                    && (self.preview || self.login_item_worker.is_some()),
                                |row| {
                                    row.on_click(cx.listener(
                                        |this, _, _, cx| {
                                            let enabled = !this.launch_at_login_toggle.enabled();
                                            this.set_launch_at_login(enabled, cx);
                                        },
                                    ))
                                },
                            ),
                        ))
                        .when_some(
                            self.launch_at_login_error.clone(),
                            |settings, error| {
                                settings.child(inline_error(error))
                            },
                        )
                        .when(capabilities.listener_control, |panel| panel.child(
                            settings_row(
                                tr("Enable on launch"),
                                tr("Turn Hex on in every app whenever it opens"),
                                toggle(enable_on_launch_position),
                            )
                            .id("enable-on-launch-setting")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let enabled = !this.settings.enable_on_launch;
                                if this.update_settings(cx, |settings| {
                                    settings.enable_on_launch = enabled
                                }) {
                                    this.enable_on_launch_toggle.set_enabled(enabled);
                                }
                            })),
                        ))
                        .when(capabilities.dock_icon, |panel| panel.child(
                            settings_row(
                                tr("Show Dock icon"),
                                tr("Keep Hex in the menu bar when Settings is closed"),
                                toggle(dock_icon_position),
                            )
                            .id("dock-icon-setting")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.update_settings(cx, |settings| settings.show_dock_icon = !settings.show_dock_icon) {
                                    return;
                                }
                                this.dock_icon_toggle
                                    .set_enabled(this.settings.show_dock_icon);
                            })),
                        ))
                        .when_some(interface_language_control, |panel, control| {
                            panel.child(
                                settings_row(
                                    tr("Interface language"),
                                    tr("Language of the Hex interface"),
                                    control,
                                )
                                .id("interface-language-setting"),
                            )
                        })
                        .child(
                            settings_row(
                                tr("Sound volume"),
                                tr("Recording, cancellation, and error tones"),
                                sound_volume,
                            )
                            .when(software_update_control.is_none(), |row| row.border_b_0())
                            .id("sound-effects-setting"),
                        )
                        .when_some(software_update_control, |panel, control| {
                            panel.child(
                                settings_row(
                                    tr("Software updates"),
                                    tr("Checks for new Hex releases in the background"),
                                    control,
                                )
                                .border_b_0()
                                .id("software-updates-setting"),
                            )
                        }),
                )
                .when(
                    platform::CAPABILITIES.fun
                        && self.settings.experimental.hex_microphone_available(),
                    |column| {
                        let status = self.hex_microphone_status();
                        let driver_row = self.render_hex_microphone_driver_row(
                            "settings-hex-mic-driver-row",
                            "settings-hex-mic-driver-action",
                            &status,
                            cx,
                        );
                        let mode = self.settings.experimental.hex_microphone_mode;
                        let comfort_position =
                            self.fun_comfort_noise_toggle.render_position(window);
                        const SETTINGS_FUN_MODE_WIDTHS: [f32; 4] = [66.0, 56.0, 52.0, 62.0];
                        let settings_choices = [
                            crate::voice_filter::VoiceFilterMode::Muffled,
                            crate::voice_filter::VoiceFilterMode::Muted,
                            crate::voice_filter::VoiceFilterMode::Quiet,
                            crate::voice_filter::VoiceFilterMode::Normal,
                        ];
                        let settings_mode_idx = settings_choices
                            .iter()
                            .position(|&c| c == mode)
                            .unwrap_or(0);
                        let mode_pills = sliding_segmented_for(
                            window,
                            "settings-hex-mic-mode-picker",
                            settings_mode_idx,
                            &SETTINGS_FUN_MODE_WIDTHS,
                        )
                        .id("settings-hex-mic-mode")
                        .children(
                            settings_choices
                                .into_iter()
                                .enumerate()
                                .map(|(index, choice)| {
                                    sliding_segmented_item(
                                        SETTINGS_FUN_MODE_WIDTHS[index],
                                        mode == choice,
                                    )
                                    .id(("settings-hex-mic-mode-item", index))
                                    .child(Self::voice_filter_mode_label(choice))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_hex_microphone_mode(choice, cx);
                                    }))
                                }),
                        );
                        let comfort_control = div()
                            .id("settings-hex-mic-comfort-noise")
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(toggle(comfort_position))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let next =
                                    !this.settings.experimental.hex_microphone_comfort_noise;
                                this.set_hex_microphone_comfort_noise(next, cx);
                            }));
                        let open_fun = button(ButtonKind::Secondary, tr("Open Fun"))
                            .id("open-fun-pane")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_pane(Pane::Fun, cx);
                            }));
                        column
                            .child(settings_section_label(tr("Experimental: Hex microphone")))
                            .child(
                                panel()
                                    .id("experimental-hex-microphone-panel")
                                    .child(driver_row)
                                    .child(settings_row(
                                        tr("Mode while dictating"),
                                        Self::voice_filter_mode_description(mode),
                                        mode_pills,
                                    ))
                                    .child(settings_row(
                                        tr("Comfort noise"),
                                        tr("Keeps a quiet room tone while muted so callers know the line is live"),
                                        comfort_control,
                                    ))
                                    .child(
                                        settings_row(
                                            tr("Try it"),
                                            tr("Try Hex Microphone and preview how callers hear you while you dictate"),
                                            open_fun,
                                        )
                                        .border_b_0(),
                                    ),
                            )
                    },
                ),
        )
    }

    fn voice_filter_mode_label(mode: crate::voice_filter::VoiceFilterMode) -> &'static str {
        use crate::voice_filter::VoiceFilterMode;
        match mode {
            VoiceFilterMode::Muffled => tr("Muffled"),
            VoiceFilterMode::Muted => tr("Muted"),
            VoiceFilterMode::Quiet => tr("Quiet"),
            VoiceFilterMode::Normal => tr("Normal"),
            VoiceFilterMode::Chipmunk => tr("Chipmunk"),
            VoiceFilterMode::Robot => tr("Robot"),
            VoiceFilterMode::Walkie => tr("Walkie"),
            VoiceFilterMode::Underwater => tr("Underwater"),
            VoiceFilterMode::Far => tr("Far"),
        }
    }

    fn voice_filter_mode_description(mode: crate::voice_filter::VoiceFilterMode) -> &'static str {
        use crate::voice_filter::VoiceFilterMode;
        match mode {
            VoiceFilterMode::Muffled => {
                tr("Sounds like a hand over the microphone while you dictate")
            }
            VoiceFilterMode::Muted => {
                tr("Silences your voice with soft room tone while you dictate")
            }
            VoiceFilterMode::Quiet => tr("Turns your voice down to a whisper while you dictate"),
            VoiceFilterMode::Normal => tr("Passes your voice through unchanged while you dictate"),
            VoiceFilterMode::Chipmunk => tr("Sounds pitched up while you dictate"),
            VoiceFilterMode::Robot => tr("Sounds robotic while you dictate"),
            VoiceFilterMode::Walkie => tr("Sounds like a handheld radio while you dictate"),
            VoiceFilterMode::Underwater => tr("Sounds submerged while you dictate"),
            VoiceFilterMode::Far => tr("Sounds like you are across the room while you dictate"),
        }
    }

    fn set_hex_microphone_mode(
        &mut self,
        mode: crate::voice_filter::VoiceFilterMode,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(cx, |settings| {
            settings.experimental.hex_microphone_mode = mode;
        });
        crate::hex_microphone::update_tuned_config(|cfg| {
            cfg.mode = mode;
        });
        self.fun_waveform.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn set_hex_microphone_comfort_noise(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.update_settings(cx, |settings| {
            settings.experimental.hex_microphone_comfort_noise = enabled;
        }) {
            self.fun_comfort_noise_toggle.set_enabled(enabled);
        }
        crate::hex_microphone::update_tuned_config(|cfg| {
            cfg.comfort_noise = enabled;
        });
        cx.notify();
    }

    fn fun_slider_normalized(
        slider: FunSlider,
        cfg: &crate::hex_microphone::TunedFilterConfig,
    ) -> (f32, String) {
        let norm = |val: f64, min: f64, max: f64| -> f32 {
            ((val - min) / (max - min)).clamp(0.0, 1.0) as f32
        };
        match slider {
            FunSlider::Strength => (
                cfg.strength.clamp(0.0, 1.0),
                format!("{:.0}%", cfg.strength * 100.0),
            ),
            FunSlider::MuffledWall => (
                norm(cfg.muffled_wall_hz, 150.0, 3000.0),
                format!("{:.0} Hz", cfg.muffled_wall_hz),
            ),
            FunSlider::MuffledRoom => (
                norm(cfg.muffled_room_mix, 0.0, 0.8),
                format!("{:.0}%", cfg.muffled_room_mix * 100.0),
            ),
            FunSlider::MuffledLevel => (
                norm(cfg.muffled_level_db, -24.0, 0.0),
                format!("{:.0} dB", cfg.muffled_level_db),
            ),
            FunSlider::QuietLevel => (
                norm(cfg.quiet_level_db, -36.0, -6.0),
                format!("{:.0} dB", cfg.quiet_level_db),
            ),
            FunSlider::MutedRoomTone => (
                norm(cfg.muted_room_tone_db, -80.0, -42.0),
                format!("{:.0} dB", cfg.muted_room_tone_db),
            ),
            FunSlider::ChipmunkPitch => (
                norm(cfg.chipmunk_semitones, 2.0, 12.0),
                format!("+{:.1} st", cfg.chipmunk_semitones),
            ),
            FunSlider::RobotCarrier => (
                norm(cfg.robot_carrier_hz, 30.0, 180.0),
                format!("{:.0} Hz", cfg.robot_carrier_hz),
            ),
            FunSlider::WalkieCrunch => (
                norm(cfg.walkie_crunch, 1.0, 8.0),
                format!("{:.1}x", cfg.walkie_crunch),
            ),
            FunSlider::UnderwaterDepth => (
                norm(cfg.underwater_depth_hz, 160.0, 900.0),
                format!("{:.0} Hz", cfg.underwater_depth_hz),
            ),
            FunSlider::FarRoom => (
                norm(cfg.far_distance_mix, 0.2, 0.95),
                format!("{:.0}%", cfg.far_distance_mix * 100.0),
            ),
        }
    }

    fn apply_fun_slider(&mut self, slider: FunSlider, normalized: f32, cx: &mut Context<Self>) {
        let t = f64::from(normalized.clamp(0.0, 1.0));
        let lerp = |min: f64, max: f64| min + (max - min) * t;
        crate::hex_microphone::update_tuned_config(|cfg| match slider {
            FunSlider::Strength => cfg.strength = t as f32,
            FunSlider::MuffledWall => cfg.muffled_wall_hz = lerp(150.0, 3000.0).round(),
            FunSlider::MuffledRoom => {
                cfg.muffled_room_mix = (lerp(0.0, 0.8) * 100.0).round() / 100.0
            }
            FunSlider::MuffledLevel => cfg.muffled_level_db = lerp(-24.0, 0.0).round(),
            FunSlider::QuietLevel => cfg.quiet_level_db = lerp(-36.0, -6.0).round(),
            FunSlider::MutedRoomTone => cfg.muted_room_tone_db = lerp(-80.0, -42.0).round(),
            FunSlider::ChipmunkPitch => {
                cfg.chipmunk_semitones = (lerp(2.0, 12.0) * 2.0).round() / 2.0
            }
            FunSlider::RobotCarrier => cfg.robot_carrier_hz = lerp(30.0, 180.0).round(),
            FunSlider::WalkieCrunch => cfg.walkie_crunch = (lerp(1.0, 8.0) * 10.0).round() / 10.0,
            FunSlider::UnderwaterDepth => cfg.underwater_depth_hz = lerp(160.0, 900.0).round(),
            FunSlider::FarRoom => cfg.far_distance_mix = (lerp(0.2, 0.95) * 100.0).round() / 100.0,
        });
        if slider == FunSlider::Strength {
            let strength = normalized.clamp(0.0, 1.0);
            self.settings.experimental.hex_microphone_strength = strength;
            self.schedule_settings_save(cx);
        }
        cx.notify();
    }

    fn render_fun_slider(
        &self,
        window: &mut Window,
        slot_key: &'static str,
        slider: FunSlider,
        label: &'static str,
        cfg: &crate::hex_microphone::TunedFilterConfig,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const TRACK_WIDTH: f32 = 310.0;
        let (target_value, display) = Self::fun_slider_normalized(slider, cfg);
        let value = window
            .spring_or_jump(
                slot_key,
                target_value,
                self.fun_active_slider.is_some(),
                SPRING_SNAP,
            )
            .clamp(0.0, 1.0);
        let id = match slider {
            FunSlider::Strength => "fun-slider-strength",
            FunSlider::MuffledWall => "fun-slider-wall",
            FunSlider::MuffledRoom => "fun-slider-room",
            FunSlider::MuffledLevel => "fun-slider-level",
            FunSlider::QuietLevel => "fun-slider-quiet",
            FunSlider::MutedRoomTone => "fun-slider-room-tone",
            FunSlider::ChipmunkPitch => "fun-slider-pitch",
            FunSlider::RobotCarrier => "fun-slider-carrier",
            FunSlider::WalkieCrunch => "fun-slider-crunch",
            FunSlider::UnderwaterDepth => "fun-slider-depth",
            FunSlider::FarRoom => "fun-slider-far-room",
        };
        div()
            .w(px(TRACK_WIDTH))
            .h(px(46.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .h(px(FOOTNOTE.line_height))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_footnote()
                    .child(div().text_color(rgb(TEXT_SECONDARY)).child(label))
                    .child(
                        div()
                            .min_w(px(56.0))
                            .flex()
                            .justify_end()
                            .text_monospace()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(display),
                    ),
            )
            .child(
                div()
                    .id(id)
                    .w(px(TRACK_WIDTH))
                    .h(px(22.0))
                    .px(px(6.0))
                    .relative()
                    .flex()
                    .items_center()
                    .rounded(RADIUS_CONTROL)
                    .bg(rgb(CANVAS))
                    .border_1()
                    .border_color(rgb(LINE))
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .relative()
                            .rounded(RADIUS_KNOB)
                            .bg(rgb(LINE_STRONG))
                            .child(
                                div()
                                    .w(gpui::relative(value))
                                    .h_full()
                                    .rounded(RADIUS_KNOB)
                                    .bg(rgb(ACCENT)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-4.0))
                                    .left(px(((TRACK_WIDTH - 12.0) * value - 6.0)
                                        .clamp(0.0, TRACK_WIDTH - 24.0)))
                                    .size(px(12.0))
                                    .rounded_full()
                                    .bg(rgb(TEXT_PRIMARY)),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .px(px(6.0))
                            .flex()
                            .children((0..=40).map(|step| {
                                let norm = (step as f32) / 40.0;
                                div()
                                    .flex_1()
                                    .h_full()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.fun_active_slider = Some(slider);
                                            this.apply_fun_slider(slider, norm, cx);
                                        }),
                                    )
                                    .on_mouse_move(cx.listener(
                                        move |this, event: &gpui::MouseMoveEvent, _, cx| {
                                            if event.pressed_button == Some(MouseButton::Left)
                                                && this.fun_active_slider == Some(slider)
                                            {
                                                this.apply_fun_slider(slider, norm, cx);
                                            }
                                        },
                                    ))
                            })),
                    ),
            )
            .into_any_element()
    }

    fn poll_hex_microphone(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        if let Some(receiver) = &self.fun_mic_status_receiver
            && let Ok(next_status) = receiver.try_recv()
        {
            self.fun_mic_status_receiver = None;
            if next_status != self.fun_mic_status {
                self.fun_mic_status = next_status;
                changed = true;
            }
        }
        let next = crate::hex_microphone::driver_action_seq();
        if next != self.fun_driver_action_seq {
            self.fun_driver_action_seq = next;
            self.fun_waveform.update(cx, |_, cx| cx.notify());
            if !self.preview {
                self.request_hex_microphone_status();
            }
            changed = true;
        }
        if !self.preview
            && self.fun_mic_status_receiver.is_none()
            && (self.pane == Pane::Fun
                || (self.pane == Pane::Settings
                    && self.settings.experimental.hex_microphone_available())
                || crate::hex_microphone::driver_action_in_progress())
        {
            self.request_hex_microphone_status();
        }
        if self.pane == Pane::Fun && crate::hex_microphone::demo_is_busy() {
            self.fun_waveform.update(cx, |_, cx| cx.notify());
            let snap = crate::hex_microphone::demo_snapshot();
            let button_state = (
                snap.source,
                snap.has_recording,
                snap.is_recording,
                snap.playing,
            );
            if button_state != self.fun_demo_button_state
                || std::env::var_os("HEX_PREVIEW_BEFORE").is_some()
            {
                self.fun_demo_button_state = button_state;
                changed = true;
            }
        }
        changed
    }

    fn request_hex_microphone_status(&mut self) {
        if self.preview || self.fun_mic_status_receiver.is_some() {
            return;
        }
        let (sender, receiver) = sync_channel(1);
        thread::spawn(move || {
            let _ = sender.send(crate::hex_microphone::status());
        });
        self.fun_mic_status_receiver = Some(receiver);
    }

    fn preview_hex_microphone_status(missing: bool) -> crate::hex_microphone::HexMicrophoneStatus {
        let bundled_version = env!("CARGO_PKG_VERSION").to_string();
        crate::hex_microphone::HexMicrophoneStatus {
            driver_bundle_installed: !missing,
            driver_loaded: !missing,
            installed_version: (!missing).then(|| bundled_version.clone()),
            bundled_version,
            sink_streaming: !missing,
            mic_in_use: !missing,
            active_apps: if missing {
                Vec::new()
            } else {
                vec!["Slack".into()]
            },
            action_state: crate::hex_microphone::DriverActionState::Idle,
        }
    }

    fn hex_microphone_status(&self) -> crate::hex_microphone::HexMicrophoneStatus {
        if std::env::var_os("HEX_PREVIEW_BEFORE").is_some() {
            let _ = crate::hex_microphone::status();
        }
        if self.preview {
            Self::preview_hex_microphone_status(self.fun_driver_missing_preview)
        } else {
            let mut status = self.fun_mic_status.clone();
            status.action_state = crate::hex_microphone::current_driver_action_state();
            status
        }
    }

    fn render_hex_microphone_driver_row(
        &self,
        row_id: &'static str,
        action_prefix: &'static str,
        status: &crate::hex_microphone::HexMicrophoneStatus,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        use crate::hex_microphone::DriverActionState;

        match &status.action_state {
            DriverActionState::Installing => {
                return settings_row(
                    tr("Virtual microphone driver"),
                    tr("Installing driver and restarting Core Audio…"),
                    tag(Tone::Neutral, tr("Installing…")),
                )
                .id(row_id);
            }
            DriverActionState::Uninstalling => {
                return settings_row(
                    tr("Virtual microphone driver"),
                    tr("Removing driver and restarting Core Audio…"),
                    tag(Tone::Neutral, tr("Uninstalling…")),
                )
                .id(row_id);
            }
            _ => {}
        }

        if status.needs_update() {
            let installed = status.installed_version.as_deref().unwrap_or("1.0.0");
            let desc = match &status.action_state {
                DriverActionState::Cancelled => tr("Update cancelled").into(),
                DriverActionState::Failed(reason) => tr_fill("Could not update driver: {}", reason),
                _ => tr_fill("Update driver to v{}", &status.bundled_version),
            };
            let controls = div()
                .flex()
                .items_center()
                .gap_2()
                .child(tag(Tone::Accent, tr_fill("v{} installed", installed)))
                .child(
                    small_button(ButtonKind::Secondary, tr("Update"))
                        .id((action_prefix, 1usize))
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::hex_microphone::install_driver_async();
                            cx.notify();
                        })),
                )
                .child(
                    small_button(ButtonKind::Secondary, tr("Uninstall"))
                        .id((action_prefix, 2usize))
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::hex_microphone::uninstall_driver_async();
                            cx.notify();
                        })),
                );
            return settings_row(tr("Virtual microphone driver"), desc, controls).id(row_id);
        }

        if status.driver_loaded {
            let version = status
                .installed_version
                .as_deref()
                .unwrap_or(&status.bundled_version);
            let desc = match &status.action_state {
                DriverActionState::Cancelled => tr("Uninstall cancelled").into(),
                DriverActionState::Failed(reason) => tr_fill("Could not update driver: {}", reason),
                _ if !status.active_apps.is_empty() => tr_fill(
                    "Streaming 48 kHz audio to {}",
                    &status.active_apps.join(", "),
                ),
                _ => tr("Choose Hex Microphone as your input in Slack, Zoom, or Discord").into(),
            };
            let controls = div()
                .flex()
                .items_center()
                .gap_2()
                .child(tag(Tone::Success, tr_fill("Installed v{}", version)))
                .child(
                    small_button(ButtonKind::Secondary, tr("Uninstall"))
                        .id((action_prefix, 2usize))
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::hex_microphone::uninstall_driver_async();
                            cx.notify();
                        })),
                );
            return settings_row(tr("Virtual microphone driver"), desc, controls).id(row_id);
        }

        let (desc, status_badge) = match &status.action_state {
            DriverActionState::DoneUninstalled => (
                tr("Driver removed").into(),
                Some(tag(Tone::Neutral, tr("Removed"))),
            ),
            DriverActionState::Cancelled => (
                tr("Installation cancelled").into(),
                Some(tag(Tone::Neutral, tr("Cancelled"))),
            ),
            DriverActionState::Failed(reason) => (
                tr_fill("Could not install driver: {}", reason),
                Some(tag(Tone::Danger, tr("Failed"))),
            ),
            _ => (
                tr("Install the CoreAudio driver to choose Hex Microphone in Slack, Zoom, or Discord")
                    .into(),
                None,
            ),
        };
        let controls = div()
            .flex()
            .items_center()
            .gap_2()
            .children(status_badge)
            .child(
                small_button(ButtonKind::Secondary, tr("Install"))
                    .id((action_prefix, 0usize))
                    .on_click(cx.listener(|_, _, _, cx| {
                        crate::hex_microphone::install_driver_async();
                        cx.notify();
                    })),
            );
        settings_row(tr("Virtual microphone driver"), desc, controls).id(row_id)
    }

    fn render_fun(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::hex_microphone::{
            DemoPlaybackTrack, DemoSourceKind, DriverActionState, TunedFilterConfig,
        };
        use crate::voice_filter::VoiceFilterMode;

        let status = self.hex_microphone_status();
        let cfg = crate::hex_microphone::current_tuned_config();
        let current_mode = cfg.mode;
        let demo = crate::hex_microphone::demo_snapshot();
        self.fun_demo_button_state = (
            demo.source,
            demo.has_recording,
            demo.is_recording,
            demo.playing,
        );

        let header_status = match &status.action_state {
            DriverActionState::Installing => {
                tag(Tone::Neutral, tr("Installing…")).into_any_element()
            }
            DriverActionState::Uninstalling => {
                tag(Tone::Neutral, tr("Uninstalling…")).into_any_element()
            }
            _ if !status.driver_loaded => {
                tag(Tone::Neutral, tr("Driver not installed")).into_any_element()
            }
            _ if status.needs_update() => {
                tag(Tone::Accent, tr_fill("Update v{}", &status.bundled_version)).into_any_element()
            }
            _ if !status.active_apps.is_empty() => tag(
                Tone::Success,
                tr_fill("In use by {}", &status.active_apps.join(", ")),
            )
            .into_any_element(),
            _ => tag(
                Tone::Success,
                tr_fill(
                    "Installed v{}",
                    status
                        .installed_version
                        .as_deref()
                        .unwrap_or(&status.bundled_version),
                ),
            )
            .into_any_element(),
        };

        let driver_row = self.render_hex_microphone_driver_row(
            "fun-driver-row",
            "fun-driver-action",
            &status,
            cx,
        );

        const FUN_MODE_WIDTHS: [f32; 9] = [66.0, 56.0, 52.0, 62.0, 76.0, 54.0, 60.0, 86.0, 44.0];
        let current_mode_index = VoiceFilterMode::ALL
            .iter()
            .position(|&m| m == current_mode)
            .unwrap_or(0);
        let mode_bar = sliding_segmented_for(
            window,
            "fun-mode-picker",
            current_mode_index,
            &FUN_MODE_WIDTHS,
        )
        .id("fun-mode-picker")
        .children(
            VoiceFilterMode::ALL
                .into_iter()
                .enumerate()
                .map(|(index, choice)| {
                    sliding_segmented_item(FUN_MODE_WIDTHS[index], current_mode == choice)
                        .id(("fun-mode-item", index))
                        .child(Self::voice_filter_mode_label(choice))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_hex_microphone_mode(choice, cx);
                        }))
                }),
        );

        if current_mode != VoiceFilterMode::Normal {
            self.fun_last_non_normal_mode = current_mode;
        }
        let active_slider_mode = if current_mode == VoiceFilterMode::Normal {
            self.fun_last_non_normal_mode
        } else {
            current_mode
        };
        let (primary_slider, primary_label) = match active_slider_mode {
            VoiceFilterMode::Muffled | VoiceFilterMode::Normal => {
                (FunSlider::MuffledWall, tr("Wall"))
            }
            VoiceFilterMode::Muted => (FunSlider::MutedRoomTone, tr("Room tone")),
            VoiceFilterMode::Quiet => (FunSlider::QuietLevel, tr("Level")),
            VoiceFilterMode::Chipmunk => (FunSlider::ChipmunkPitch, tr("Pitch")),
            VoiceFilterMode::Robot => (FunSlider::RobotCarrier, tr("Carrier")),
            VoiceFilterMode::Walkie => (FunSlider::WalkieCrunch, tr("Crunch")),
            VoiceFilterMode::Underwater => (FunSlider::UnderwaterDepth, tr("Depth")),
            VoiceFilterMode::Far => (FunSlider::FarRoom, tr("Room")),
        };

        let row1_vis = window
            .spring_when(
                "fun-sliders-row1-vis",
                current_mode != VoiceFilterMode::Normal,
                SPRING_REFLOW,
            )
            .clamp(0.0, 1.0);
        let row2_vis = window
            .spring_when(
                "fun-sliders-row2-vis",
                current_mode == VoiceFilterMode::Muffled,
                SPRING_REFLOW,
            )
            .clamp(0.0, 1.0);

        let sliders_row1 = (row1_vis > 0.001).then(|| {
            let h = ((58.0 * row1_vis) * 64.0).round() / 64.0;
            div()
                .h(px(h))
                .pt_3()
                .overflow_hidden()
                .opacity(row1_vis)
                .flex()
                .flex_wrap()
                .gap_4()
                .child(self.render_fun_slider(
                    window,
                    "fun-slider-slot-0",
                    primary_slider,
                    primary_label,
                    &cfg,
                    cx,
                ))
                .child(self.render_fun_slider(
                    window,
                    "fun-slider-slot-1",
                    FunSlider::Strength,
                    tr("Effect strength"),
                    &cfg,
                    cx,
                ))
        });

        let sliders_row2 = (row2_vis > 0.001).then(|| {
            let h = ((SETTINGS_ROW_MIN_HEIGHT * row2_vis) * 64.0).round() / 64.0;
            div()
                .h(px(h))
                .pt_1p5()
                .overflow_hidden()
                .opacity(row2_vis)
                .flex()
                .flex_wrap()
                .gap_4()
                .child(self.render_fun_slider(
                    window,
                    "fun-slider-slot-2",
                    FunSlider::MuffledRoom,
                    tr("Room"),
                    &cfg,
                    cx,
                ))
                .child(self.render_fun_slider(
                    window,
                    "fun-slider-slot-3",
                    FunSlider::MuffledLevel,
                    tr("Level"),
                    &cfg,
                    cx,
                ))
        });

        let comfort_position = self.fun_comfort_noise_toggle.render_position(window);
        let comfort_visibility = window
            .spring_when(
                "fun-comfort-noise-visibility",
                current_mode == VoiceFilterMode::Muted,
                SPRING_REFLOW,
            )
            .clamp(0.0, 1.0);
        let comfort_row = (comfort_visibility > 0.001).then(|| {
            let control = div()
                .id("fun-comfort-noise-toggle")
                .flex()
                .items_center()
                .cursor_pointer()
                .child(toggle(comfort_position))
                .on_click(cx.listener(|this, _, _, cx| {
                    let next = !this.settings.experimental.hex_microphone_comfort_noise;
                    this.set_hex_microphone_comfort_noise(next, cx);
                }));
            let h = ((SETTINGS_ROW_MIN_HEIGHT * comfort_visibility) * 64.0).round() / 64.0;
            div()
                .h(px(h))
                .overflow_hidden()
                .opacity(comfort_visibility)
                .child(settings_row(
                    tr("Comfort noise"),
                    tr("Keeps a quiet room tone while muted so callers know the line is live"),
                    control,
                ))
        });

        let source_picker = demo.has_recording.then(|| {
            const SOURCE_WIDTHS: [f32; 2] = [72.0, 82.0];
            let source_idx = if demo.source == DemoSourceKind::Recording {
                1.0
            } else {
                0.0
            };
            let source_pos = window.spring("fun-demo-source-index", source_idx, SPRING_SNAP);
            sliding_segmented_control(source_pos, &SOURCE_WIDTHS)
                .id("fun-demo-source")
                .child(
                    sliding_segmented_item(
                        SOURCE_WIDTHS[0],
                        demo.source == DemoSourceKind::Example,
                    )
                    .id("fun-demo-source-example")
                    .child(tr("Example"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        crate::hex_microphone::select_demo_source(DemoSourceKind::Example);
                        cx.notify();
                    })),
                )
                .child(
                    sliding_segmented_item(
                        SOURCE_WIDTHS[1],
                        demo.source == DemoSourceKind::Recording,
                    )
                    .id("fun-demo-source-recording")
                    .child(tr("Your voice"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        crate::hex_microphone::select_demo_source(DemoSourceKind::Recording);
                        cx.notify();
                    })),
                )
        });

        let record_btn = if demo.is_recording {
            button(ButtonKind::Danger, status_dot(DANGER))
                .id("fun-demo-record")
                .min_w(px(124.0))
                .justify_center()
                .child(tr("Stop recording"))
                .on_click(cx.listener(|this, _, _, cx| {
                    crate::hex_microphone::stop_demo_recording();
                    this.fun_waveform.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }))
        } else {
            button(ButtonKind::Secondary, status_dot(DANGER))
                .id("fun-demo-record")
                .min_w(px(124.0))
                .justify_center()
                .child(tr("Record 5s"))
                .on_click(cx.listener(|this, _, _, cx| {
                    crate::hex_microphone::start_demo_recording();
                    this.fun_waveform.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }))
        };

        let playing_call = demo.playing == Some(DemoPlaybackTrack::CallHears);
        let playing_you = demo.playing == Some(DemoPlaybackTrack::You);

        let play_call_btn = button(
            ButtonKind::Primary,
            if playing_call {
                tr("Stop")
            } else {
                tr("Play what call hears")
            },
        )
        .id("fun-play-call-hears")
        .w(px(154.0))
        .justify_center()
        .on_click(cx.listener(move |this, _, _, cx| {
            if playing_call {
                crate::hex_microphone::stop_demo_playback();
            } else {
                crate::hex_microphone::play_demo_track(DemoPlaybackTrack::CallHears);
            }
            this.fun_waveform.update(cx, |_, cx| cx.notify());
            cx.notify();
        }));

        let play_you_btn = button(
            ButtonKind::Secondary,
            if playing_you {
                tr("Stop")
            } else {
                tr("Play your voice")
            },
        )
        .id("fun-play-you")
        .w(px(124.0))
        .justify_center()
        .on_click(cx.listener(move |this, _, _, cx| {
            if playing_you {
                crate::hex_microphone::stop_demo_playback();
            } else {
                crate::hex_microphone::play_demo_track(DemoPlaybackTrack::You);
            }
            this.fun_waveform.update(cx, |_, cx| cx.notify());
            cx.notify();
        }));

        settings_pane(
            tr("Fun"),
            "fun-scroll",
            div()
                .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, _| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.fun_active_slider = None;
                    }
                }))
                .child(
                    panel()
                        .mt_6()
                        .id("fun-hex-microphone-card")
                        .child(panel_header(tr("Hex Microphone"), Some(header_status)))
                        .child(driver_row)
                        .child(
                            div()
                                .px_4()
                                .py_3()
                                .flex()
                                .flex_col()
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_4()
                                        .child(settings_copy(
                                            tr("Mode while dictating"),
                                            Self::voice_filter_mode_description(current_mode),
                                        ))
                                        .child(
                                            small_button(ButtonKind::Secondary, tr("Reset"))
                                                .id("fun-reset-tuning")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let defaults = TunedFilterConfig::default();
                                                    crate::hex_microphone::update_tuned_config(
                                                        |cfg| {
                                                            let keep_mode = cfg.mode;
                                                            *cfg = defaults;
                                                            cfg.mode = keep_mode;
                                                        },
                                                    );
                                                    this.set_hex_microphone_comfort_noise(true, cx);
                                                })),
                                        ),
                                )
                                .child(div().pt_3().child(mode_bar))
                                .children(sliders_row1)
                                .children(sliders_row2),
                        )
                        .children(comfort_row)
                        .child(
                            div()
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_center()
                                        .justify_between()
                                        .gap_4()
                                        .child(settings_copy(
                                            tr("Try it"),
                                            tr("Compare your raw microphone with what the call hears during dictation"),
                                        ))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .children(source_picker)
                                                .child(record_btn),
                                        ),
                                )
                                .child(self.fun_waveform.clone())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(play_call_btn)
                                                .child(play_you_btn),
                                        )
                                        .child(
                                            div()
                                                .text_caption()
                                                .text_color(rgb(TEXT_TERTIARY))
                                                .child(tr("Stays in memory")),
                                        ),
                                ),
                        ),
                ),
        )
    }

    fn toggle_microphone_picker(&mut self, cx: &mut Context<Self>) {
        self.microphone_picker_open = !self.microphone_picker_open;
        if self.microphone_picker_open {
            match crate::audio::input_device_names() {
                Ok(devices) => {
                    self.microphone_devices = devices;
                    self.microphone_picker_error = None;
                }
                Err(error) => self.microphone_picker_error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    fn render_voice_action(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.voice_action.enabled;
        let description = voice_action_description(enabled);
        let position = self
            .voice_action_inputs
            .enabled_toggle
            .render_position(window);
        let enabled_control = div()
            .id("voice-action-enabled")
            .flex()
            .items_center()
            .child(toggle(position))
            .on_click(cx.listener(|this, _, _, cx| {
                this.cancel_hotkey_capture(cx);
                let enabled = !this.settings.voice_action.enabled;
                let mut candidate = this.settings_candidate();
                let result = set_voice_action_enabled(&mut candidate, enabled)
                    .map_err(str::to_string)
                    .and_then(|()| {
                        this.commit_settings(candidate)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(()) => {
                        this.voice_action_inputs.enabled_toggle.set_enabled(enabled);
                        this.voice_action_inputs.error = None;
                    }
                    Err(error) => {
                        this.voice_action_inputs.error = Some(error);
                    }
                }
                cx.notify();
            }));
        let hotkey = self.render_hotkey_setting_control(HotkeyKind::Edit, window, cx);
        let processing = if !enabled {
            None
        } else if let Some(copy) =
            opencode_unavailable_copy(&self.model_catalog, OpenCodeFeature::VoiceAction)
        {
            let show_actions = copy.can_retry || copy.can_open_setup;
            let actions = Self::opencode_unavailable_actions(
                &copy,
                "retry-opencode",
                "open-opencode-setup",
                tr("Open OpenCode setup"),
                cx,
            )
            .mt_4();
            Some(
                panel()
                    .p_4()
                    .child(settings_copy(copy.title, copy.description))
                    .when_some(copy.error, |card, error| {
                        card.child(error_panel(tr("OpenCode reported:"), error).mt_3())
                    })
                    .when(show_actions, |card| card.child(actions))
                    .into_any_element(),
            )
        } else {
            Some(self.render_voice_action_processing(window, cx))
        };
        settings_pane(
            tr("Voice Action"),
            "voice-action-scroll",
            div()
                .child(
                    panel().mt_6().child(
                        settings_row(tr("Enable Voice Action"), description, enabled_control)
                            .border_b_0(),
                    ),
                )
                .when_some(self.voice_action_inputs.error.clone(), |column, error| {
                    column.child(error_message(tr("Could not update Voice Action:"), error))
                })
                .child(settings_section_label(tr("Capture")))
                .child(
                    panel().child(
                        settings_row(
                            tr("Shortcut"),
                            if enabled {
                                tr("Hold to speak; selected text is included automatically")
                            } else {
                                tr("Saved for when Voice Action is enabled")
                            },
                            hotkey,
                        )
                        .border_b_0()
                        .id("voice-action-hotkey-setting"),
                    ),
                )
                .when_some(processing, |column, processing| {
                    column
                        .child(settings_section_label(tr("Processing")))
                        .child(processing)
                }),
        )
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        // Modes and text fields are explicit drafts; failed writes remain dirty
        // for retry or the close-time flush.
        self.settings_dirty = true;
        if let Err(error) = self.persist_settings() {
            tracing::error!(%error, "could not save app settings");
        }
        cx.notify();
    }

    fn persist_settings(&mut self) -> color_eyre::Result<()> {
        self.commit_settings(self.settings_candidate())
    }

    fn settings_candidate(&self) -> AppSettings {
        let mut candidate = self.settings.clone();
        self.mode_editors.project(&mut candidate);
        candidate
    }

    /// Saves before assigning so a failed save leaves the live settings
    /// untouched; the generation bump supersedes any pending debounced save.
    fn commit_settings(&mut self, candidate: AppSettings) -> color_eyre::Result<()> {
        #[cfg(test)]
        if let Some(save) = self.settings_save_override {
            return self.commit_settings_with(candidate, save);
        }
        let preview = self.preview;
        self.commit_settings_with(
            candidate,
            |candidate| {
                if preview { Ok(()) } else { candidate.save() }
            },
        )
    }

    fn commit_settings_with(
        &mut self,
        candidate: AppSettings,
        save: impl FnOnce(&AppSettings) -> color_eyre::Result<()>,
    ) -> color_eyre::Result<()> {
        if let Err(error) = self.settings.commit_with(candidate, save) {
            self.settings_load_error = Some(error.to_string());
            return Err(error);
        }
        self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
        self.settings_dirty = false;
        self.settings_load_error = None;
        if !self.preview {
            self.platform_settings_saved();
        }
        Ok(())
    }

    fn update_settings(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut AppSettings),
    ) -> bool {
        let mut candidate = self.settings_candidate();
        update(&mut candidate);
        let saved = self.commit_settings(candidate).is_ok();
        cx.notify();
        saved
    }

    fn set_double_tap_lock(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.update_settings(cx, |settings| {
            settings.double_tap_lock = enabled;
            if !enabled {
                settings.double_tap_only = false;
            }
        }) {
            self.double_tap_toggle.set_enabled(enabled);
        }
    }

    fn set_shortcut_mode(&mut self, mode: crate::dictation::ShortcutMode, cx: &mut Context<Self>) {
        if self.settings.shortcut_mode != mode
            && self.update_settings(cx, |settings| settings.shortcut_mode = mode)
        {
            self.shortcut_mode_toggle
                .set_enabled(mode == crate::dictation::ShortcutMode::Toggle);
        }
    }

    fn set_double_tap_only(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.update_settings(cx, |settings| {
            settings.double_tap_only =
                enabled && settings.double_tap_lock && settings.dictation_hotkey.key.is_some();
        });
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
        ReplacementInputs {
            matched_phrase: Self::processing_input(
                tr("Spoken phrase (e.g. open code)"),
                &replacement.matched_phrase,
                cx,
            ),
            output: Self::processing_input(
                tr("Replacement (e.g. OpenCode)"),
                &replacement.output,
                cx,
            ),
        }
    }

    fn transcription_hints_input(initial: &str, cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| {
            TextInput::multiline(
                cx,
                tr("Names and terms Whisper should expect, e.g. OpenCode, Effect…"),
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
        let model = Self::model_input(
            settings.voice_action.model.as_deref().unwrap_or_default(),
            |_| ModelPickerTarget::VoiceAction,
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

    /// `target` is resolved at event time so the Mode picker follows the
    /// currently selected mode rather than the one selected at construction.
    fn model_input(
        initial: &str,
        target: fn(&Self) -> ModelPickerTarget,
        cx: &mut Context<Self>,
    ) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, tr("Search available models"), initial));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.model_picker_highlight = 0;
            cx.notify();
        });
        let navigate = cx.subscribe(&entity, move |this, _, event: &TextNavigate, cx| {
            let count = this.model_choice_keys(target(this), cx).len();
            if count > 0 {
                this.model_picker_highlight =
                    advance_highlight(this.model_picker_highlight, event.0, count);
                cx.notify();
            }
        });
        let submitted = cx.subscribe(&entity, move |this, _, _: &TextSubmitted, cx| {
            let target = target(this);
            if let Some(model) = this
                .model_choice_keys(target, cx)
                .get(this.model_picker_highlight)
                .cloned()
            {
                this.select_model(target, model, cx);
            }
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed, navigate, submitted],
        }
    }

    fn application_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, tr("Search applications"), ""));
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
            this.close_application_picker(cx);
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
        let entity = cx.new(|cx| TextInput::multiline(cx, placeholder, initial));
        Self::synchronized_processing_input(entity, cx)
    }

    fn mode_inputs(mode: &DictationMode, cx: &mut Context<Self>) -> ModeInputs {
        let processing = &mode.post_processing;
        ModeInputs {
            name: Self::processing_input(tr("Mode name"), &mode.name, cx),
            browser_hosts: Self::processing_input(
                "github.com, x.com",
                &mode.browser_hosts.join(", "),
                cx,
            ),
            prompt: Self::multiline_processing_input(
                tr("Describe how OpenCode should rewrite your dictation…"),
                &processing.prompt,
                cx,
            ),
            model: Self::model_input(
                processing.model.as_deref().unwrap_or_default(),
                |this| ModelPickerTarget::Mode(this.selected_mode),
                cx,
            ),
            deadline: Self::processing_input("30", &processing.deadline_seconds.to_string(), cx),
            processing_toggle: ToggleSpring::new(processing.enabled),
            replacements: mode
                .replacements
                .iter()
                .map(|replacement| Self::replacement_inputs(replacement, cx))
                .collect(),
        }
    }

    fn mode_editor(mode: &DictationMode, cx: &mut Context<Self>) -> ModeEditor {
        ModeEditor {
            settings: mode.clone(),
            inputs: Self::mode_inputs(mode, cx),
        }
    }

    fn mode_editors(settings: &AppSettings, cx: &mut Context<Self>) -> ModeEditors {
        let processing = &settings.dictation_processing;
        ModeEditors {
            default_mode: Self::mode_editor(&processing.default_mode, cx),
            modes: processing
                .modes
                .iter()
                .map(|mode| Self::mode_editor(mode, cx))
                .collect(),
        }
    }

    fn sync_processing_settings(&mut self, cx: &mut Context<Self>) {
        let row = &mut self.mode_editors.default_mode;
        apply_mode_inputs(&row.inputs, &mut row.settings, true, cx);
        for row in &mut self.mode_editors.modes {
            apply_mode_inputs(&row.inputs, &mut row.settings, false, cx);
        }
        self.schedule_settings_save(cx);
    }

    fn update_mode_settings(
        &mut self,
        selection: ModeSelection,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut DictationMode),
    ) -> bool {
        let mut candidate = self.settings_candidate();
        let mode = mode_settings_mut(&mut candidate, selection);
        update(mode);
        let mode = mode.clone();
        let saved = self.commit_settings(candidate).is_ok();
        if saved {
            self.mode_editor_mut(selection).settings = mode;
        }
        cx.notify();
        saved
    }

    fn add_mode(&mut self, cx: &mut Context<Self>) -> Option<ModeSelection> {
        let mut mode = self.mode_editors.default_mode.settings.clone();
        mode.name = tr_fill("Mode {}", &(self.mode_editors.modes.len() + 1).to_string());
        mode.applications.clear();
        mode.browser_hosts.clear();
        let mut candidate = self.settings_candidate();
        candidate.dictation_processing.modes.push(mode.clone());
        let saved = self.commit_settings(candidate).is_ok();
        cx.notify();
        if !saved {
            return None;
        }
        let selection = ModeSelection::Custom(self.mode_editors.modes.len());
        self.mode_editors.modes.push(Self::mode_editor(&mode, cx));
        Some(selection)
    }

    fn delete_mode(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.mode_editors.modes.len() {
            return;
        }
        let mut candidate = self.settings_candidate();
        candidate.dictation_processing.modes.remove(index);
        if self.commit_settings(candidate).is_ok() {
            self.mode_editors.modes.remove(index);
            self.selected_mode = ModeSelection::Default;
            self.mode_delete_armed = false;
            self.variant_picker_open = None;
            self.application_picker_open = false;
            self.transformation_picker_open = false;
        }
        cx.notify();
    }

    fn schedule_settings_save(&mut self, cx: &mut Context<Self>) {
        self.settings_dirty = true;
        self.settings_save_generation = self.settings_save_generation.wrapping_add(1);
        let generation = self.settings_save_generation;
        let delay = cx.background_executor().timer(Duration::from_millis(250));
        cx.spawn(async move |window, cx| {
            delay.await;
            let _ = window.update(cx, |window, cx| {
                if window.settings_save_generation == generation {
                    window.save_settings(cx);
                }
            });
        })
        .detach();
    }

    fn render_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let add = icon_button(ButtonKind::Secondary, Icon::Plus)
            .id("add-dictation-mode")
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(selection) = this.add_mode(cx) {
                    this.select_mode(selection, window, cx);
                }
            }));
        let default_selected = self.selected_mode == ModeSelection::Default;
        let mut rows = vec![
            mode_row(
                tr("Global"),
                tr("Fallback for everything"),
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
            self.mode_editors
                .modes
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let name = input_text(&row.inputs.name, cx);
                    let mode = &row.settings;
                    let selected = self.selected_mode == ModeSelection::Custom(index);
                    mode_row(
                        name,
                        tr("No activation rules"),
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
                tr("Modes"),
                Some(add.into_any_element()),
            ))
            .child(
                pane_body().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(pane_list(
                            "modes-list",
                            PANE_LIST_WIDTH_COMPACT,
                            None,
                            "",
                            None,
                            rows,
                        ))
                        .child(detail),
                ),
            )
            .into_any_element()
    }

    fn render_mode_context_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu_progress = window.spring_when(
            "mode-context-menu",
            self.mode_context_menu.is_some(),
            SPRING_SETTLE,
        );
        let position = self.mode_context_menu?;
        Some(
            menu_with_progress(menu_progress)
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(MENU_WIDTH))
                .child(
                    danger_menu_item()
                        .id("context-delete-mode")
                        .child(tr("Delete mode…"))
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
            opencode_unavailable_copy(&self.model_catalog, OpenCodeFeature::Transformation);
        let corrections = self.render_mode_replacements(selection, cx);
        let transformations = self.render_mode_transformations(selection, window, cx);
        let application_picker =
            (!is_default).then(|| self.render_application_picker(selection, window, cx));
        let basics = if is_default {
            panel()
                .child(
                    div()
                        .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
                        .px_4()
                        .py_2()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(settings_copy(
                            tr("Global"),
                            tr("Used unless a more specific mode matches."),
                        )),
                )
                .into_any_element()
        } else {
            panel()
                .child(
                    div()
                        .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .text_body()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr("Name")),
                        )
                        .child(div().max_w(px(MODE_FIELD_WIDTH)).flex_1().child(name)),
                )
                .when_some(application_picker, |panel, picker| {
                    panel.child(
                        div()
                            .when(platform::WEB_PAGES_DESCRIPTION.is_some(), |row| {
                                row.border_b_1().border_color(rgb(LINE))
                            })
                            .child(picker),
                    )
                })
                .when_some(platform::WEB_PAGES_DESCRIPTION, |panel, description| {
                    panel.child(
                        div()
                            .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
                            .px_4()
                            .py_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(settings_copy(tr("Web pages"), tr(description)))
                            .child(
                                div()
                                    .max_w(px(MODE_FIELD_WIDTH))
                                    .flex_1()
                                    .child(browser_hosts),
                            ),
                    )
                })
                .into_any_element()
        };

        let processing_toggle = div()
            .id("mode-processing-toggle")
            .h(px(CONTROL_HEIGHT))
            .flex()
            .items_center()
            .child(toggle(processing_position))
            .when(!processing_can_toggle, |control| {
                control.opacity(DISABLED_OPACITY)
            })
            .when(processing_can_toggle, |control| {
                control.on_click(cx.listener(move |this, _, _, cx| {
                    let enabled = !this.selected_mode_settings().post_processing.enabled;
                    if this.update_mode_settings(this.selected_mode, cx, |mode| {
                        mode.post_processing.enabled = enabled;
                    }) {
                        this.selected_mode_inputs_mut()
                            .processing_toggle
                            .set_enabled(enabled);
                    }
                }))
            })
            .into_any_element();
        let processing_settings = processing_enabled.then(|| {
            self.render_processing_settings(
                ModelPickerTarget::Mode(selection),
                prompt,
                model,
                deadline,
                window,
                cx,
            )
        });
        let processing_unavailable = processing_unavailable.map(|copy| {
            let show_actions = copy.can_retry || copy.can_open_setup;
            let actions = Self::opencode_unavailable_actions(
                &copy,
                "retry-mode-opencode",
                "open-mode-opencode-setup",
                tr("Open setup"),
                cx,
            )
            .items_center();
            div()
                .px_4()
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
                        .gap_0p5()
                        .child(
                            div()
                                .text_body()
                                .font_weight(FontWeight::MEDIUM)
                                .child(copy.title),
                        )
                        .child(
                            div()
                                .text_footnote()
                                .text_color(rgb(if copy.error.is_some() {
                                    DANGER
                                } else {
                                    TEXT_TERTIARY
                                }))
                                .child(copy.error.unwrap_or_else(|| copy.description.into())),
                        ),
                )
                .when(show_actions, |notice| notice.child(actions))
        });
        let processing = panel()
            .child(
                panel_header(tr("OpenCode transformation"), Some(processing_toggle)).when(
                    !processing_enabled && processing_unavailable.is_none(),
                    |header| header.border_b_0(),
                ),
            )
            .when_some(processing_settings, |panel, settings| {
                panel.child(div().px_4().pb_4().child(settings))
            })
            .when_some(processing_unavailable, |panel, unavailable| {
                panel.child(unavailable)
            })
            .into_any_element();

        let remove = (!is_default).then(|| {
            let action = if self.mode_delete_armed {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        button(ButtonKind::Secondary, tr("Cancel"))
                            .id("cancel-remove-mode")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mode_delete_armed = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button(ButtonKind::Danger, tr("Confirm delete"))
                            .id("confirm-remove-mode")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let ModeSelection::Custom(index) = this.selected_mode else {
                                    return;
                                };
                                this.delete_mode(index, cx);
                            })),
                    )
                    .into_any_element()
            } else {
                button(ButtonKind::Danger, tr("Delete mode"))
                    .id("remove-selected-mode")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mode_delete_armed = true;
                        cx.notify();
                    }))
                    .into_any_element()
            };
            div()
                .pt_2()
                .flex()
                .items_center()
                .justify_end()
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
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(basics)
                    .child(section_label(tr("Text processing")).pt_2().px_4())
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.mode_settings(selection).transformations.clone();
        let catalog = &self.personal_commands_status.transformations;
        let picker_progress = window.spring_when(
            "transformation-picker-open",
            self.transformation_picker_open && !catalog.is_empty(),
            SPRING_SETTLE,
        );
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
                    .h(px(CONTROL_HEIGHT + SPACE_2))
                    .pl_2()
                    .pr_1()
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
                        this.update_mode_settings(selection, cx, |mode| {
                            reorder_transformation(
                                &mut mode.transformations,
                                &drag.id,
                                target_index,
                            );
                        });
                    }))
                    .child(
                        div()
                            .id(("drag-mode-transformation", target_index))
                            .w_5()
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_grab()
                            .child(icon(Icon::DragHandle, TEXT_DISABLED))
                            .on_drag(drag, |drag, position, _, cx| {
                                cx.new(|_| drag.clone().at(position))
                            }),
                    )
                    .child(
                        truncated_line(BODY, name)
                            .flex_1()
                            .text_color(rgb(TEXT_SECONDARY)),
                    )
                    .child(
                        icon_button(ButtonKind::Ghost, Icon::Close)
                            .id(("remove-mode-transformation", target_index))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_mode_settings(selection, cx, |mode| {
                                    mode.transformations
                                        .retain(|candidate| candidate != &dragged_id);
                                });
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
                        .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
                        .px_4()
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
                                        .text_body()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT_SECONDARY))
                                        .child(name),
                                )
                                .when_some(description, |copy, description| {
                                    copy.child(
                                        div()
                                            .pt_0p5()
                                            .text_footnote()
                                            .text_color(rgb(TEXT_TERTIARY))
                                            .child(description),
                                    )
                                }),
                        )
                        .child(small_button(ButtonKind::Secondary, tr("Add")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_mode_settings(selection, cx, |mode| {
                                mode.transformations.push(id.clone());
                            });
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
            let control = if self.transformation_picker_open {
                small_button(ButtonKind::Secondary, tr("Done"))
            } else {
                icon_button(ButtonKind::Ghost, Icon::Plus).mr(px(-SPACE_2))
            };
            control
                .id("toggle-transformation-picker")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.transformation_picker_open = !this.transformation_picker_open;
                    cx.notify();
                }))
                .into_any_element()
        });

        let workspace = (!platform::CAPABILITIES.commands)
            .then(|| self.render_transformation_workspace(cx))
            .flatten();
        panel()
            .child(panel_header(tr("Transformations"), add).when(
                selected.is_empty() && !self.transformation_picker_open && workspace.is_none(),
                |header| header.border_b_0(),
            ))
            .children(selected_rows)
            .when(self.transformation_picker_open && !catalog_empty, |panel| {
                panel.child(
                    div()
                        .opacity(picker_progress.clamp(0.0, 1.0))
                        .mt(px(
                            -(1.0 - picker_progress.clamp(0.0, 1.0)) * MENU_ENTER_OFFSET
                        ))
                        .when(available_empty, |list| {
                            list.child(
                                div()
                                    .border_t_1()
                                    .border_color(rgb(LINE))
                                    .px_4()
                                    .py_3()
                                    .text_footnote()
                                    .text_color(rgb(TEXT_TERTIARY))
                                    .child(tr(
                                        "Every registered transformation is already included.",
                                    )),
                            )
                        })
                        .children(available_rows),
                )
            })
            .children(workspace)
            .into_any_element()
    }

    /// The personal TypeScript workspace that defines custom
    /// transformations: its setup, loading, and failures, with the action
    /// that recovers. Hidden once its transformations are loaded.
    fn render_transformation_workspace(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.preview {
            return None;
        }
        let status = &self.personal_commands_status;
        let (description, error, action) = if self.personal_workspace_receiver.is_some() {
            (
                tr("Preparing the TypeScript workspace and installing its dependencies…"),
                None,
                None,
            )
        } else if let Some(error) = self.personal_workspace_error.clone() {
            (
                tr("The TypeScript workspace could not be prepared."),
                Some(error),
                Some(tr("Retry setup")),
            )
        } else if !self.personal_config_exists {
            (
                tr("Create a TypeScript workspace to define reusable transformations."),
                None,
                Some(tr("Set up")),
            )
        } else {
            match status.host_state {
                HostState::Active if status.last_reload_error.is_none() => return None,
                HostState::Starting => (
                    tr("Loading registered TypeScript transformations…"),
                    None,
                    None,
                ),
                _ => (
                    tr("Registered TypeScript transformations are unavailable."),
                    status.last_reload_error.clone(),
                    Some(tr("Retry")),
                ),
            }
        };
        let is_error = error.is_some();
        Some(
            div()
                .px_4()
                .py_3()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .border_t_1()
                .border_color(rgb(LINE))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(settings_copy(tr("Custom transformations"), description))
                        .when_some(error, |copy, error| {
                            copy.child(inline_error(bounded_opencode_error(&error)))
                        }),
                )
                .when_some(action, |row, label| {
                    row.child(
                        small_button(ButtonKind::Secondary, label)
                            .id("transformation-workspace-action")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.provision_personal_workspace();
                                cx.notify();
                            })),
                    )
                })
                .when(is_error, |row| row.items_start())
                .into_any_element(),
        )
    }

    fn render_application_picker(
        &self,
        selection: ModeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tray_progress = window.spring_when(
            "application-picker-open",
            self.application_picker_open,
            SPRING_SETTLE,
        );
        let selected = self.mode_settings(selection).applications.clone();
        let selected_rows = selected.iter().enumerate().map(|(index, name)| {
            let application = self.application_named(name);
            let name = name.clone();
            div()
                .id(("selected-application", index))
                .h(px(CONTROL_HEIGHT_SMALL))
                .pl_1()
                .flex()
                .items_center()
                .gap_1p5()
                .rounded(RADIUS_SMALL)
                .bg(rgb(SURFACE_SELECTED))
                .text_caption()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_SECONDARY))
                .child(application_icon(
                    application,
                    Some(name.as_str()),
                    ICON_SIZE + 2.0,
                ))
                .child(name.clone())
                .child(
                    icon_button(ButtonKind::Ghost, Icon::Close)
                        .id(("remove-selected-application", index))
                        .size(px(CONTROL_HEIGHT_SMALL))
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
                    list_row(index == self.application_picker_highlight)
                        .id(("available-application", index))
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(application_icon(
                            Some(&application),
                            Some(&name),
                            CONTROL_HEIGHT,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(list_row_title(name.clone(), false))
                                .child(list_row_meta(location, false)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.add_mode_application(selection, name.clone(), cx);
                            }),
                        )
                });
        let picker_status = match &self.application_catalog {
            ApplicationCatalogState::Loading => Some(tr("Finding applications…").into()),
            ApplicationCatalogState::Loaded(applications) if applications.is_empty() => {
                Some(tr("No applications found.").into())
            }
            ApplicationCatalogState::Loaded(_) if no_matches && !query.is_empty() => {
                Some(tr_fill("No applications match \"{}\".", &query))
            }
            _ => None,
        };
        let tray = self.application_picker_open.then(|| {
            let clamped = tray_progress.clamp(0.0, 1.0);
            div()
                .mx_4()
                .mb_4()
                .mt(px(-(1.0 - clamped) * MENU_ENTER_OFFSET))
                .opacity(clamped)
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .rounded(RADIUS_CONTROL)
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CANVAS))
                .child(self.application_search.entity.clone())
                .when_some(picker_status, |tray, status| {
                    tray.child(
                        div()
                            .px_2()
                            .py_3()
                            .text_footnote()
                            .text_color(rgb(TEXT_TERTIARY))
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
                            small_button(
                                ButtonKind::Secondary,
                                tr(platform::CHOOSE_APPLICATION_FILE),
                            )
                            .id("choose-application-bundle")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.choose_application(selection, cx);
                                },
                            )),
                        )
                        .child(
                            small_button(ButtonKind::Secondary, tr("Done"))
                                .id("close-application-picker")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_application_picker(cx);
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
                    .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(settings_copy(
                        tr("Applications"),
                        if platform::focused_application_known() {
                            tr("Switch when an app is frontmost")
                        } else {
                            tr("Wayland doesn't report the frontmost app; these apply under X11")
                        },
                    ))
                    .child(
                        div()
                            .max_w(px(MODE_FIELD_WIDTH))
                            .min_h(px(CONTROL_HEIGHT))
                            .flex_1()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .children(selected_rows)
                            .child(
                                icon_button(ButtonKind::Secondary, Icon::Plus)
                                    .id("open-application-picker")
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
                picker.child(inline_error(error))
            })
            .when_some(tray, |picker, tray| picker.child(tray))
            .into_any_element()
    }

    fn application_named(&self, name: &str) -> Option<&InstalledApplication> {
        self.application_catalog.application_named(name)
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
            .mode_editors
            .modes
            .iter()
            .enumerate()
            .find(|(index, row)| {
                selection != ModeSelection::Custom(*index)
                    && row.settings.applications.contains(&name)
            })
            .map(|(_, row)| row.settings.name.clone())
        {
            self.application_picker_error = Some(tr_named(
                "{name} already activates {conflict}. Remove it there before reassigning it.",
                &[("name", &name), ("conflict", &conflict)],
            ));
            cx.notify();
            return;
        }
        if !self.mode_settings(selection).applications.contains(&name)
            && self.update_mode_settings(selection, cx, |mode| {
                mode.applications.push(name);
                mode.applications.sort_by_key(|name| name.to_lowercase());
            })
        {
            self.application_picker_highlight = 0;
            self.application_search
                .entity
                .update(cx, |input, cx| input.set_text("", cx));
            self.application_picker_error = None;
        }
    }

    fn remove_mode_application(
        &mut self,
        selection: ModeSelection,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        self.update_mode_settings(selection, cx, |mode| {
            mode.applications.retain(|application| application != name);
        });
    }

    fn choose_application(&mut self, selection: ModeSelection, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Add Application").into()),
        });
        cx.spawn(async move |window, cx| {
            let path = match receiver
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
            {
                Ok(paths) => paths.and_then(|paths| paths.into_iter().next()),
                Err(error) => {
                    let _ = window.update(cx, |window, cx| {
                        window.application_picker_error = Some(error);
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
                (tr("Loading OpenCode…").into(), None, Vec::new(), Vec::new())
            }
            ModelCatalogState::Missing => (
                tr("OpenCode beta required").into(),
                None,
                Vec::new(),
                Vec::new(),
            ),
            ModelCatalogState::Failed(error) => (
                tr_fill("OpenCode models unavailable: {}", &error.to_string()),
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
                                Some(key) => {
                                    tr_fill("OpenCode default: {}", &format!("{name} ({key})"))
                                }
                                None => tr_fill("OpenCode default: {}", name),
                            })
                    })
                    .flatten();
                (
                    tr_fill("{} available models", &catalog.models.len().to_string()),
                    default_label,
                    matches,
                    variants,
                )
            }
        };
        let suggestions_open = focused && (!matches.is_empty() || default_label.is_some());
        let suggestions_progress =
            window.spring_when("processing-suggestions", suggestions_open, SPRING_SETTLE);
        let suggestions = suggestions_open.then(|| {
            menu_with_progress(suggestions_progress)
                .mt_1()
                .when_some(default_label.clone(), |list, label| {
                    list.child(
                        model_choice_row(
                            tr("Use OpenCode default"),
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
        let effective_variant = selected_variant.or_else(|| {
            selected_model.is_none().then(|| {
                let ModelCatalogState::Loaded(catalog) = &self.model_catalog else {
                    return None;
                };
                catalog.default_variant.as_deref()
            })?
        });
        let has_variants = !variants.is_empty();
        let variant_open = self.variant_picker_open == Some(target);
        let variant_progress = window.spring_when(
            "processing-variants",
            has_variants && variant_open,
            SPRING_SETTLE,
        );
        let variant_list = (has_variants && variant_open).then(|| {
            let choices = std::iter::once(None)
                .chain(variants.iter().cloned().map(Some))
                .collect::<Vec<_>>();
            menu_with_progress(variant_progress).mt_1().children(
                choices.into_iter().enumerate().map(|(index, variant)| {
                    let selected = effective_variant == variant.as_deref();
                    let label = variant.clone().unwrap_or_else(|| tr("Default").into());
                    menu_item(selected)
                        .id(("model-variant", index))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.variant_picker_open = None;
                            this.set_model_variant(target, variant.clone(), cx);
                        }))
                }),
            )
        });
        let variant_control = div()
            .w(px(160.0))
            .flex_none()
            .child(
                disclosure_button(effective_variant.unwrap_or(tr("Default")).to_string())
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
            disclosure_frame()
                .id("selected-model")
                .w_full()
                .flex_shrink()
                .child(
                    truncated_line(FOOTNOTE, presentation.name)
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(TEXT_PRIMARY)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_caption()
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(presentation.provider),
                )
                .when(presentation.is_default, |control| {
                    control.child(tag(Tone::Neutral, tr("Default")))
                })
                .child(icon(Icon::ChevronDown, TEXT_TERTIARY))
                .on_click(cx.listener(move |_, _, window, cx| {
                    model_input.focus_handle(cx).focus(window);
                    cx.notify();
                }))
                .into_any_element()
        } else {
            model.clone().into_any_element()
        };
        let refresh_control =
            matches!(&self.model_catalog, ModelCatalogState::Loaded(_)).then(|| {
                button(ButtonKind::Secondary, tr("Refresh"))
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
    fn opencode_unavailable_actions(
        copy: &OpenCodeUnavailableCopy,
        retry_id: &'static str,
        setup_id: &'static str,
        setup_label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .gap_2()
            .when(copy.can_retry, |actions| {
                actions.child(
                    button(ButtonKind::Secondary, copy.retry_label)
                        .id(retry_id)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.reload_model_catalog();
                            cx.notify();
                        })),
                )
            })
            .when(copy.can_open_setup, |actions| {
                actions.child(
                    button(ButtonKind::Secondary, setup_label)
                        .id(setup_id)
                        .on_click(|_, _, _| open_opencode_beta_docs()),
                )
            })
    }

    fn render_voice_action_processing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .w(px(380.0))
            .flex_none()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().min_w(px(0.0)).flex_1().child(model_control))
                    .when_some(refresh_control, |row, refresh| row.child(refresh)),
            )
            .when_some(suggestions, |field, suggestions| field.child(suggestions));
        panel()
            .child(
                settings_row(tr("OpenCode model"), catalog_status, model_field)
                    .when(!has_variants, |row| row.border_b_0()),
            )
            .when(has_variants, |panel| {
                panel.child(
                    settings_row(
                        tr("Thinking"),
                        tr("Choose how much reasoning the model should use"),
                        div().w(px(160.0)).flex_none().child(variant_control),
                    )
                    .border_b_0(),
                )
            })
            .into_any_element()
    }

    fn render_processing_settings(
        &mut self,
        target: ModelPickerTarget,
        prompt: Entity<TextInput>,
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
                                tr("Model"),
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
                                        button(ButtonKind::Secondary, tr("Install OpenCode"))
                                            .mt_2()
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
                            .child(settings_control(tr("Deadline"), tr("Seconds"), deadline)),
                    ),
            )
            .when(has_variants, |processing| {
                processing.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(field_label(tr("Thinking")))
                        .child(div().flex().child(variant_control)),
                )
            })
            .child(settings_control(
                tr("Instructions"),
                tr("System instructions for rewriting dictation in this mode"),
                prompt,
            ))
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
        let recommended_variant = model.as_deref().and_then(|key| {
            let ModelCatalogState::Loaded(catalog) = &self.model_catalog else {
                return None;
            };
            catalog.recommended_variant_for(key).map(str::to_owned)
        });
        let text = model.clone().unwrap_or_default();
        let saved = match target {
            ModelPickerTarget::Mode(selection) => {
                self.update_mode_settings(selection, cx, |mode| {
                    mode.post_processing.model = model;
                    mode.post_processing.variant = recommended_variant;
                })
            }
            ModelPickerTarget::VoiceAction => self.update_settings(cx, |settings| {
                settings.voice_action.model = model;
                settings.voice_action.variant = recommended_variant;
            }),
        };
        if saved {
            self.model_input_for(target)
                .update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    fn set_model_variant(
        &mut self,
        target: ModelPickerTarget,
        variant: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.settings_candidate();
        apply_model_variant(&mut candidate, target, variant, &self.model_catalog);
        if self.commit_settings(candidate).is_ok()
            && let ModelPickerTarget::Mode(selection) = target
        {
            let mode = mode_settings_mut(&mut self.settings, selection).clone();
            self.mode_editor_mut(selection).settings = mode;
        }
        cx.notify();
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
        self.close_application_picker(cx);
        self.application_picker_error = None;
        self.transformation_picker_open = false;
        self.variant_picker_open = None;
        self.model_picker_highlight = 0;
        self.mode_delete_armed = false;
        self.mode_context_menu = None;
        window.blur();
        cx.notify();
    }

    fn close_application_picker(&mut self, cx: &mut Context<Self>) {
        self.application_picker_open = false;
        self.application_picker_highlight = 0;
        self.application_search
            .entity
            .update(cx, |input, cx| input.set_text("", cx));
    }

    fn mode_inputs_mut(&mut self, selection: ModeSelection) -> &mut ModeInputs {
        &mut self.mode_editor_mut(selection).inputs
    }

    fn mode_inputs_for(&self, selection: ModeSelection) -> &ModeInputs {
        &self.mode_editor_for(selection).inputs
    }

    fn selected_mode_settings(&self) -> &DictationMode {
        self.mode_settings(self.selected_mode)
    }

    fn mode_settings(&self, selection: ModeSelection) -> &DictationMode {
        &self.mode_editor_for(selection).settings
    }

    fn mode_editor_for(&self, selection: ModeSelection) -> &ModeEditor {
        match selection {
            ModeSelection::Default => &self.mode_editors.default_mode,
            ModeSelection::Custom(index) => &self.mode_editors.modes[index],
        }
    }

    fn mode_editor_mut(&mut self, selection: ModeSelection) -> &mut ModeEditor {
        match selection {
            ModeSelection::Default => &mut self.mode_editors.default_mode,
            ModeSelection::Custom(index) => &mut self.mode_editors.modes[index],
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn current_context(&self) -> (Option<String>, Option<String>) {
        self.current_context.clone()
    }

    fn render_activity(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let refresh = button(ButtonKind::Secondary, tr("Refresh"))
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
            .when_some(self.activity.state_label(), |actions, status| {
                let active = self.activity.state != Some(VoiceState::Stopping);
                actions.child(listener_status(
                    status,
                    self.activity.device.clone().unwrap_or_default(),
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
                timestamped_list_row(
                    ("activity-event", index),
                    selected,
                    age,
                    div()
                        .flex_1()
                        .map(|text| {
                            if boundary {
                                text.text_caption()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_TERTIARY))
                            } else {
                                text.text_body().text_color(rgb(TEXT_SECONDARY))
                            }
                        })
                        .child(summary),
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
            .child(pane_header_with_action(tr("Activity"), Some(header_action)))
            .child(
                pane_body().child(
                    pane_content()
                        .flex_row()
                        .gap_5()
                        .child(pane_list(
                            "activity-list",
                            PANE_LIST_WIDTH,
                            (self.events.is_empty() && self.activity.error.is_none())
                                .then_some("No activity yet."),
                            "Activity could not be loaded.",
                            self.activity.error.clone(),
                            rows,
                        ))
                        .child(pane_detail(self.render_event_detail())),
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
            .child(div().text_headline().child(event_summary(event)))
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
                            .pt_2()
                            .text_footnote()
                            .text_monospace()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(raw),
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
            && let Err(error) = self.settings_candidate().save()
        {
            tracing::error!(%error, "could not flush app settings while closing window");
        }
        if !self.preview {
            platform::set_hotkey_capture_active(false);
            self.platform_closed();
        }
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

    fn delete_transcription_model(&mut self, model: TranscriptionModelId, _cx: &mut Context<Self>) {
        AppWindow::delete_transcription_model(self, definition(model));
    }

    fn dismiss_transcription_picker(&mut self, cx: &mut Context<Self>) {
        AppWindow::dismiss_transcription_picker(self, cx);
    }

    fn select_transcription_language(&mut self, language: String, cx: &mut Context<Self>) {
        if self.transcription_picker_language.as_deref() != Some(&language) {
            self.cancel_transcription_download();
            self.transcription_picker_language = Some(language);
            self.clear_transcription_error();
            cx.notify();
        }
    }
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _frame_guard = crate::motion::begin_frame(self.pane.label());
        let new_hex_notice = self.render_new_hex_notice();
        let model_notice = self.render_model_notice(cx);
        let content = match self.pane {
            Pane::HudLab | Pane::Meetings | Pane::Commands => {
                match self.render_platform_pane(self.pane, window, cx) {
                    Some(content) => content,
                    None => self.render_settings(window, cx),
                }
            }
            Pane::Activity => self.render_activity(cx),
            Pane::History => self.render_history(window, cx),
            Pane::Modes => self.render_modes(window, cx),
            Pane::VoiceAction => self.render_voice_action(window, cx),
            Pane::Fun => self.render_fun(window, cx),
            Pane::Settings => self.render_settings(window, cx),
        };
        let transcription_picker_progress = window.spring_when(
            "transcription-picker-open",
            self.transcription_picker_language.is_some(),
            SPRING_SETTLE,
        );
        let transcription_picker = self.transcription_picker_language.clone().map(|language| {
            self.render_transcription_picker(&language, transcription_picker_progress, cx)
        });
        let setup = self.setup_visible.then(|| self.render_tour(window, cx));
        let mode_context_menu = (self.pane == Pane::Modes)
            .then(|| self.render_mode_context_menu(window, cx))
            .flatten();
        platform::frame(window_frame())
            .track_focus(&self.window_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key != "escape" {
                    return;
                }
                if this.history_retention_open {
                    this.history_retention_open = false;
                    cx.stop_propagation();
                    cx.notify();
                } else if this.mode_context_menu.take().is_some()
                    || this.pending_microphone_policy.take().is_some()
                {
                    cx.stop_propagation();
                    cx.notify();
                } else if this.transcription_picker_language.is_some() {
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
                    .children(platform::caption(window))
                    .children(new_hex_notice)
                    .children(model_notice)
                    .when_some(self.settings_load_error.clone(), |column, error| {
                        column.child(
                            div()
                                .px(px(PANE_GUTTER))
                                .py_2()
                                .debug_selector(|| "settings-error".into())
                                .child(error_message(tr("Settings error:"), error)),
                        )
                    })
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

fn voice_action_description(enabled: bool) -> &'static str {
    if enabled {
        tr("Use the shortcut below with OpenCode to run spoken instructions")
    } else {
        tr("Off by default. Its saved shortcut is not reserved until you enable Voice Action.")
    }
}

fn set_voice_action_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled && hotkey_binding_conflicts(settings, HotkeyKind::Edit, &settings.edit_hotkey) {
        return Err(tr(
            "The Voice Action shortcut is already in use. Change it below or change the other shortcut before enabling Voice Action.",
        ));
    }
    settings.voice_action.enabled = enabled;
    Ok(())
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

fn normalize_captured_modifiers(modifiers: HotkeyModifiers) -> HotkeyModifiers {
    let mut sides = [
        modifiers.control,
        modifiers.option,
        modifiers.shift,
        modifiers.command,
    ]
    .into_iter()
    .flatten();
    let Some(first) = sides.next() else {
        return modifiers;
    };
    if sides.all(|side| side == first) {
        modifiers
    } else {
        modifiers.without_side_constraints()
    }
}

fn standalone_modifier_side(binding: &HotkeyBinding) -> Option<ModifierSide> {
    if binding.key.is_some() {
        return None;
    }
    let mut sides = [
        binding.modifiers.control,
        binding.modifiers.option,
        binding.modifiers.shift,
        binding.modifiers.command,
    ]
    .into_iter()
    .flatten();
    let first = sides.next()?;
    sides.all(|side| side == first).then_some(first)
}

fn set_standalone_modifier_side(binding: &mut HotkeyBinding, side: ModifierSide) {
    if standalone_modifier_side(binding).is_none() {
        return;
    }
    for current in [
        &mut binding.modifiers.control,
        &mut binding.modifiers.option,
        &mut binding.modifiers.shift,
        &mut binding.modifiers.command,
    ] {
        if current.is_some() {
            *current = Some(side);
        }
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
    if settings.meetings.available() {
        others.push(HotkeyBinding::paste_meeting_default());
    }
    crate::app_settings::hotkey_conflicts(binding, others)
}

fn hotkey_binding(settings: &AppSettings, kind: HotkeyKind) -> Option<&HotkeyBinding> {
    match kind {
        HotkeyKind::Dictation => Some(&settings.dictation_hotkey),
        HotkeyKind::Edit => Some(&settings.edit_hotkey),
        HotkeyKind::PasteLast => settings.paste_last_hotkey.as_ref(),
    }
}

fn hotkey_side_binding(
    settings: &AppSettings,
    kind: HotkeyKind,
    side: ModifierSide,
) -> Option<HotkeyBinding> {
    let mut binding = hotkey_binding(settings, kind)?.clone();
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

/// Extra width for keycaps whose word labels ("Space", "Ctrl") outgrow the
/// square keycap that symbol labels fit.
fn hotkey_label_overflow(keycaps: &[String]) -> f32 {
    keycaps
        .iter()
        .map(|label| (label.chars().count() as f32 * 7.0 + 16.0 - KEYCAP_WIDTH).max(0.0))
        .sum()
}

fn hotkey_keycaps_width(keycap_count: usize) -> f32 {
    if keycap_count == 0 {
        0.0
    } else {
        KEYCAP_WIDTH * keycap_count as f32 + 3.0 * keycap_count.saturating_sub(1) as f32
    }
}

fn indicator_position_index(position: IndicatorPosition) -> usize {
    match position {
        IndicatorPosition::Top => 0,
        IndicatorPosition::Bottom => 1,
        IndicatorPosition::Hidden => 2,
    }
}

fn recording_audio_index(behavior: RecordingAudioBehavior) -> usize {
    match behavior {
        RecordingAudioBehavior::Mute => 0,
        RecordingAudioBehavior::PauseMedia => 1,
        RecordingAudioBehavior::DoNothing => 2,
    }
}

fn sound_volume_index(settings: &AppSettings) -> usize {
    if !settings.sound_effects {
        return 0;
    }
    crate::desktop_ui::SOUND_VOLUME_STEPS[1..]
        .iter()
        .enumerate()
        .min_by(|(_, (_, left)), (_, (_, right))| {
            (settings.sound_effect_volume - *left)
                .abs()
                .total_cmp(&(settings.sound_effect_volume - *right).abs())
        })
        .map_or(0, |(index, _)| index + 1)
}

fn open_new_hex() {
    if let Err(error) =
        crate::commands::execute(crate::commands::Action::OpenUrl(NEW_HEX_URL.into()))
    {
        tracing::error!(%error, "could not open the new Hex's download page");
    }
}

/// True once per install: the first launch of 2.3.2 opens the window so
/// the new-Hex notice is seen even by people who never open Settings.
pub(crate) fn take_new_hex_notice_launch() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let Ok(marker) = crate::app_paths::support_dir().map(|dir| dir.join("new-hex-notice-shown"))
    else {
        return false;
    };
    if marker.exists() {
        return false;
    }
    if let Err(error) = std::fs::write(&marker, b"") {
        tracing::warn!(%error, "could not record the new-Hex notice");
        return false;
    }
    true
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
    list_row(selected)
        .gap_1()
        .child(list_row_title(title, selected))
        .when(has_activations, |row| {
            row.child(mode_activation_icons(applications, browser_hosts, catalog))
        })
        .when(!has_activations, |row| {
            row.child(list_row_meta(subtitle, selected))
        })
}

const ACTIVATION_ICON_SIZE: f32 = 18.0;

fn mode_activation_icons(
    applications: &[String],
    browser_hosts: &[String],
    catalog: &ApplicationCatalogState,
) -> AnyElement {
    let total = applications.len() + browser_hosts.len();
    let visible = total.min(5);
    let mut icons = Vec::with_capacity(visible + usize::from(total > visible));
    for name in applications.iter().take(visible) {
        let application = catalog.application_named(name);
        icons.push(application_icon(
            application,
            Some(name),
            ACTIVATION_ICON_SIZE,
        ));
    }
    for _ in browser_hosts
        .iter()
        .take(visible.saturating_sub(icons.len()))
    {
        icons.push(
            div()
                .size(px(ACTIVATION_ICON_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(icon_radius(ACTIVATION_ICON_SIZE))
                .bg(rgb(SURFACE_SELECTED))
                .child(icon(Icon::Globe, TEXT_SECONDARY))
                .into_any_element(),
        );
    }
    if total > visible {
        icons.push(tag(Tone::Neutral, format!("+{}", total - visible)).into_any_element());
    }
    div()
        .h(px(ACTIVATION_ICON_SIZE))
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
            .rounded(icon_radius(size))
            .into_any_element();
    }
    let initial = fallback_name
        .and_then(|name| name.chars().find(|character| character.is_alphanumeric()))
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(icon_radius(size))
        .bg(rgb(SURFACE_SELECTED))
        .map(|monogram| {
            if size >= CONTROL_HEIGHT {
                monogram.text_body()
            } else {
                monogram.text_caption()
            }
        })
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT_SECONDARY))
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
        #[cfg(all(test, target_os = "macos"))]
        key: key.to_owned(),
        is_default,
    })
}

fn mode_settings_mut(settings: &mut AppSettings, selection: ModeSelection) -> &mut DictationMode {
    match selection {
        ModeSelection::Default => &mut settings.dictation_processing.default_mode,
        ModeSelection::Custom(index) => &mut settings.dictation_processing.modes[index],
    }
}

fn apply_model_variant(
    settings: &mut AppSettings,
    target: ModelPickerTarget,
    variant: Option<String>,
    catalog: &ModelCatalogState,
) {
    let (model, selected_variant) = match target {
        ModelPickerTarget::Mode(selection) => {
            let processing = &mut mode_settings_mut(settings, selection).post_processing;
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
        #[cfg(all(test, target_os = "macos"))]
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
    menu_item(highlighted)
        .child(truncated_line(FOOTNOTE, title.into()).flex_1())
        .child(
            div()
                .flex_none()
                .text_caption()
                .text_color(rgb(TEXT_TERTIARY))
                .child(subtitle.into()),
        )
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
                .child(field_label(label))
                .child(
                    div()
                        .text_caption()
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(description.into()),
                ),
        )
        .child(control)
        .into_any_element()
}

/// The label above a stacked form field.
fn field_label(label: &'static str) -> Div {
    div()
        .text_footnote()
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(TEXT_SECONDARY))
        .child(label)
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
        tr("Global").into()
    } else {
        input_text(&inputs.name, cx)
    };
    mode.browser_hosts = if is_default {
        Vec::new()
    } else {
        browser_hosts(&input_text(&inputs.browser_hosts, cx))
    };
    mode.replacements = inputs
        .replacements
        .iter()
        .map(|inputs| TextReplacement {
            matched_phrase: inputs.matched_phrase.entity.read(cx).text().to_string(),
            output: inputs.output.entity.read(cx).text().to_string(),
        })
        .collect();
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
            tr("Preparing speech model"),
            tr("Dictation will be available once your local speech model is ready."),
        )
    } else {
        (
            tr("Download a speech model to start dictating"),
            tr("The dictation shortcut needs a local speech model. Choose one to download first."),
        )
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

fn is_function_key(label: &str) -> bool {
    label
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
}

fn timestamped_list_row(
    id: (&'static str, usize),
    selected: bool,
    age: String,
    body: Div,
) -> gpui::Stateful<Div> {
    list_row(selected)
        .id(id)
        .flex_row()
        .items_start()
        .justify_between()
        .gap_3()
        .child(body.min_w(px(0.0)))
        .child(
            div()
                .flex_none()
                .pt(px(baseline_offset(BODY, CAPTION)))
                .text_caption()
                .text_color(rgb(if selected {
                    TEXT_SECONDARY
                } else {
                    TEXT_TERTIARY
                }))
                .child(age),
        )
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

fn event_summary(event: &VoiceEvent) -> String {
    match event {
        VoiceEvent::SessionStarted { .. } => "Hex session started".into(),
        VoiceEvent::State { state, device, .. } => match state {
            VoiceState::Sleeping => "Voice commands went to sleep".into(),
            VoiceState::Listening => format!("Voice commands resumed on {device}"),
            VoiceState::Dictating => format!("Dictation started on {device}"),
            VoiceState::Transcribing => "Dictation is being transcribed".into(),
            VoiceState::Stopping => "Hex is stopping".into(),
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
                CommandOutcome::Woke => format!("\"{heard}\" woke Hex"),
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

// The window tests drive GPUI's test platform, a macOS dev-dependency.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::fs;

    use std::cell::RefCell;

    use super::platform::{
        CatalogCommand, command_model_notice, merged_catalog, sidebar_update_button,
    };
    use super::*;
    use crate::commands::CommandScope;
    use crate::meeting;
    use crate::personal_commands::{StatusContext, StatusExecution};
    use crate::recognition::CommandModelStatus;

    fn preview_fixture(
        preview: AppWindowPreview,
        window: &mut Window,
        cx: &mut Context<AppWindow>,
    ) -> AppWindow {
        AppWindow::new(
            PathBuf::from("unused-preview-events.ndjson"),
            None,
            Some(preview),
            platform::Args {
                commands: crate::config::voice_control(),
                meeting_requests: sync_channel(1).0,
                indicator: crate::dictation_indicator::channel().0,
                recognition_start: None,
            },
            window,
            cx,
        )
    }

    fn pane_preview(pane: DeveloperPane) -> AppWindowPreview {
        AppWindowPreview {
            pane,
            transcription_picker: None,
            onboarding: None,
            confirm_model_deletion: false,
            collapse_mode_processing: false,
            open_transformation_picker: false,
            select_global_mode: false,
            voice_action_enabled: false,
            opencode_unavailable: false,
            permissions_missing: false,
            model_missing: false,
            command_model_missing: false,
            open_history_retention: false,
            confirm_release_microphone: false,
            update_available: false,
            meetings_enabled: true,
            screen_recording_missing: false,
            meeting_recording: false,
            confirm_meeting_delete: false,
            open_meeting_actions: false,
            meetings_empty: false,
            experimental_mic: false,
            fun_mode: None,
            fun_driver_missing: false,
        }
    }

    fn editor_fixture(window: &mut Window, cx: &mut Context<AppWindow>) -> AppWindow {
        preview_fixture(
            AppWindowPreview {
                collapse_mode_processing: true,
                select_global_mode: true,
                opencode_unavailable: true,
                meetings_enabled: false,
                ..pane_preview(DeveloperPane::Modes)
            },
            window,
            cx,
        )
    }

    fn meetings_fixture(window: &mut Window, cx: &mut Context<AppWindow>) -> AppWindow {
        preview_fixture(pane_preview(DeveloperPane::Meetings), window, cx)
    }

    fn history_fixture(window: &mut Window, cx: &mut Context<AppWindow>) -> AppWindow {
        preview_fixture(pane_preview(DeveloperPane::History), window, cx)
    }

    #[gpui::test]
    fn settings_offer_the_shared_update_and_indicator_rows(cx: &mut gpui::TestAppContext) {
        let (_, cx) = cx.add_window_view(|window, cx| {
            preview_fixture(
                AppWindowPreview {
                    update_available: true,
                    ..pane_preview(DeveloperPane::Settings)
                },
                window,
                cx,
            )
        });
        cx.simulate_resize(size(px(WINDOW_WIDTH), px(4000.0)));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("software-update").is_some());
        assert!(cx.debug_bounds("indicator-position").is_some());
    }

    #[gpui::test]
    fn history_detail_rows_keep_their_values_on_one_line(cx: &mut gpui::TestAppContext) {
        let (_, cx) = cx.add_window_view(history_fixture);
        cx.simulate_resize(size(px(WINDOW_WIDTH), px(MINIMUM_HEIGHT)));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let value = cx.debug_bounds("detail-row-value").expect("detail row");
        assert!(value.size.width > px(200.0), "{value:?}");
    }

    #[gpui::test]
    fn meeting_turns_alternate_sides_inside_the_detail(cx: &mut gpui::TestAppContext) {
        let (_, cx) = cx.add_window_view(meetings_fixture);
        for width in [MINIMUM_WIDTH, WINDOW_WIDTH, 1400.0] {
            cx.simulate_resize(size(px(width), px(MINIMUM_HEIGHT)));
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            let detail = cx.debug_bounds("meeting-detail").expect("detail rendered");
            let them = cx.debug_bounds("meeting-bubble-0").expect("first turn");
            let you = cx.debug_bounds("meeting-bubble-1").expect("second turn");
            let actions = cx.debug_bounds("detail-actions").expect("actions");
            for bounds in [them, you, actions] {
                assert!(
                    bounds.left() >= detail.left() && bounds.right() <= detail.right(),
                    "{width}: {bounds:?} outside {detail:?}"
                );
            }
            assert!(them.left() < you.left(), "{width}: Them sits left of You");
            assert!(you.right() > them.right(), "{width}: You sits right");
            assert!(
                detail.right() - you.right() <= px(25.0),
                "{width}: You is right-aligned"
            );
        }
    }

    #[gpui::test]
    fn meeting_overflow_menu_opens_beside_the_visible_actions(cx: &mut gpui::TestAppContext) {
        let (view, cx) = cx.add_window_view(meetings_fixture);
        cx.update(|window, cx| {
            view.update(cx, |view, _| view.platform.meeting_actions_open = true);
            window.refresh();
        });
        cx.run_until_parked();
        let actions = cx.debug_bounds("detail-actions").expect("actions");
        let menu = cx.debug_bounds("meeting-actions-menu").expect("menu");
        assert!(menu.top() >= actions.bottom() - px(CONTROL_HEIGHT));
        assert_eq!(menu.size.width, px(MENU_WIDTH));
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.select_meeting("preview-design".into());
                assert!(
                    !view.platform.meeting_actions_open,
                    "a new selection closes the menu"
                );
            })
        });
    }

    #[gpui::test]
    fn meeting_search_delete_and_rename_work_on_the_selected_meeting(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(meetings_fixture);
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert_eq!(
                    view.platform.selected_meeting.as_deref(),
                    Some("preview-notes")
                );
                view.platform
                    .meeting_search
                    .entity
                    .update(cx, |input, cx| input.set_text("gravity", cx));
                view.refresh_meeting_search(cx);
                let visible = view
                    .visible_meetings(cx)
                    .into_iter()
                    .map(|meeting| meeting.id.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(visible, ["preview-live", "preview-notes"]);
                view.platform
                    .meeting_search
                    .entity
                    .update(cx, |input, cx| input.set_text("", cx));

                view.start_meeting_rename(
                    "preview-design".into(),
                    "Design review".into(),
                    window,
                    cx,
                );
                let (_, input) = view.platform.meeting_rename.as_ref().unwrap();
                input
                    .entity
                    .update(cx, |input, cx| input.set_text("  Motion   review ", cx));
                view.finish_meeting_rename(cx);
                assert!(view.platform.meeting_rename.is_none());
                let design = view
                    .platform
                    .meetings
                    .iter()
                    .find(|m| m.id == "preview-design")
                    .unwrap();
                assert_eq!(meeting::display_title(design), "Motion review");

                view.trash_meeting("preview-notes".into(), cx);
                assert_eq!(
                    view.platform.meeting_delete_armed.as_deref(),
                    Some("preview-notes")
                );
                assert_eq!(view.platform.meetings.len(), 4, "the first click only asks");
                view.trash_meeting("preview-notes".into(), cx);
                assert_eq!(view.platform.meetings.len(), 3);
                assert!(
                    view.platform
                        .meetings
                        .iter()
                        .all(|m| m.id != "preview-notes")
                );
                assert_eq!(
                    view.platform.selected_meeting.as_deref(),
                    Some("preview-live")
                );
            });
        });
    }

    #[gpui::test]
    fn login_item_response_reconciles_optimistic_toggle_and_unknown_state(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.launch_at_login_status = None;
                assert!(view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::Status,
                    result: Ok(LoginItemStatus::Disabled),
                }));
                assert_eq!(view.launch_at_login_status, Some(LoginItemStatus::Disabled));
                view.launch_at_login_toggle.set_enabled(true);
                view.launch_at_login_error = Some("Registration failed".into());
                // Even with unchanged confirmed status and error, rollback needs a redraw.
                assert!(view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::SetEnabled(true),
                    result: Err(crate::login_item::LoginItemFailure {
                        status: Some(LoginItemStatus::Disabled),
                        message: "Registration failed".into(),
                    }),
                }));
                assert!(!view.launch_at_login_toggle.enabled());
                assert!(!view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::SetEnabled(true),
                    result: Err(crate::login_item::LoginItemFailure {
                        status: Some(LoginItemStatus::Disabled),
                        message: "Registration failed".into(),
                    }),
                }));
                // Clearing only the error still needs a redraw.
                assert!(view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::SetEnabled(false),
                    result: Ok(LoginItemStatus::Disabled),
                }));
                assert!(view.launch_at_login_error.is_none());
                assert!(view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::SetEnabled(true),
                    result: Ok(LoginItemStatus::RequiresApproval),
                }));
                assert_eq!(
                    view.launch_at_login_status,
                    Some(LoginItemStatus::RequiresApproval)
                );
                assert!(view.launch_at_login_error.is_none());
                assert!(view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::Status,
                    result: Ok(LoginItemStatus::Enabled),
                }));
                assert!(view.launch_at_login_toggle.enabled());
                assert!(!view.apply_login_item_response(LoginItemResponse {
                    request: LoginItemRequest::Status,
                    result: Ok(LoginItemStatus::Enabled),
                }));
            });
        });
    }

    fn reject_settings_save(_: &AppSettings) -> color_eyre::Result<()> {
        Err(color_eyre::eyre::eyre!("fixture persistence failure"))
    }

    #[gpui::test]
    fn model_deletion_confirms_first_and_spares_the_dictation_model(cx: &mut gpui::TestAppContext) {
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.transcription_preview_installed = Some(true);
                view.transcription_picker_language = Some("en".into());
                let current = definition(view.settings.transcription.model);
                let other = crate::transcription_models::choices_for_runtime("en")
                    .into_iter()
                    .map(|choice| choice.model)
                    .find(|model| {
                        matches!(model.runtime, ModelRuntime::Gguf(_)) && model.id != current.id
                    })
                    .unwrap();

                assert_eq!(
                    view.transcription_model_deletion(current, true),
                    ModelDeletion::Unavailable
                );
                assert_eq!(
                    view.transcription_model_deletion(other, false),
                    ModelDeletion::Unavailable
                );
                assert_eq!(
                    view.transcription_model_deletion(other, true),
                    ModelDeletion::Offered
                );

                view.delete_transcription_model(other);
                assert_eq!(
                    view.transcription_model_deletion(other, true),
                    ModelDeletion::Confirming
                );
                view.clear_transcription_error();
                assert_eq!(
                    view.transcription_model_deletion(other, true),
                    ModelDeletion::Offered
                );

                view.delete_transcription_model(other);
                view.delete_transcription_model(other);
                assert_eq!(view.transcription_deletion, None);
                assert_eq!(view.transcription_deletion_error, None);

                view.transcription_status.model = Some(other.id);
                assert_eq!(
                    view.transcription_model_deletion(other, true),
                    ModelDeletion::Unavailable
                );
            });
        });
    }

    #[gpui::test]
    fn activity_log_is_only_read_when_an_observing_pane_needs_it(cx: &mut gpui::TestAppContext) {
        let path = std::env::temp_dir().join(format!(
            "hex-lazy-activity-{}-{}.ndjson",
            std::process::id(),
            now_ms()
        ));
        std::fs::write(&path, b"{\"kind\":\"context\",\"timestamp_ms\":1,\"application\":\"Fixture\",\"browser_url\":null}\n").unwrap();
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.event_reader = EventReader::open(&path);
                view.pane = Pane::Settings;
                view.reload_events();
                assert!(view.event_reader.events().is_empty());
                view.select_pane(Pane::Commands, cx);
                assert_eq!(view.current_context.0.as_deref(), Some("Fixture"));

                std::fs::write(
                    &path,
                    b"{\"kind\":\"session_started\",\"timestamp_ms\":2}\n",
                )
                .unwrap();
                view.select_pane(Pane::Settings, cx);
                view.reload_events();
                assert_eq!(view.current_context.0.as_deref(), Some("Fixture"));
                view.select_pane(Pane::Activity, cx);
                assert_eq!(view.current_context, (None, None));
                assert_eq!(view.activity.session_started_at, Some(2));
            });
        });
        std::fs::remove_file(path).unwrap();
    }

    #[gpui::test]
    fn failed_shortcut_save_stays_in_capture_and_preserves_pending_edits(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.settings.transcription.recognition_hints = "Unsaved vocabulary".into();
                view.schedule_settings_save(cx);
                let generation = view.settings_save_generation;
                let before = serde_json::to_value(&view.settings).unwrap();
                view.settings_save_override = Some(reject_settings_save);
                view.hotkey_capture = HotkeyCaptureState::Listening {
                    kind: HotkeyKind::Dictation,
                    modifiers: HotkeyModifiers::default(),
                    message: None,
                    started_at: Instant::now(),
                };
                let binding = HotkeyBinding {
                    modifiers: HotkeyModifiers {
                        control: Some(ModifierSide::Either),
                        ..Default::default()
                    },
                    key: None,
                };
                view.save_hotkey_binding(binding.clone(), cx);
                assert!(view.hotkey_capture.is_listening());
                assert_eq!(serde_json::to_value(&view.settings).unwrap(), before);
                assert!(view.settings_dirty);
                assert_eq!(view.settings_save_generation, generation);
                assert_eq!(
                    view.settings_load_error.as_deref(),
                    Some("fixture persistence failure")
                );

                // An immediate retry commits the unrelated draft as well, and only
                // then ends capture and invalidates the earlier debounce.
                view.settings_save_override = None;
                view.save_hotkey_binding(binding.clone(), cx);
                assert!(matches!(
                    view.hotkey_capture,
                    HotkeyCaptureState::Saved { .. }
                ));
                assert_eq!(view.settings.dictation_hotkey, binding);
                assert_eq!(
                    view.settings.transcription.recognition_hints,
                    "Unsaved vocabulary"
                );
                assert!(!view.settings_dirty);
                assert_ne!(view.settings_save_generation, generation);
                assert!(view.settings_load_error.is_none());
            })
        });
    }

    #[gpui::test]
    fn failed_retention_save_keeps_previous_choice_and_reports_the_error(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.settings_save_override = Some(reject_settings_save);
                view.set_history_retention(HistoryRetention::Off, cx);
                assert_eq!(view.settings.history_retention, HistoryRetention::Week);
                assert_eq!(
                    view.settings_load_error.as_deref(),
                    Some("fixture persistence failure")
                );
            })
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-error").is_some());
    }

    #[gpui::test]
    fn mode_rows_preserve_edits_across_add_delete_and_supersede_old_debounces(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(editor_fixture);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let name = view
                    .mode_inputs_for(ModeSelection::Custom(0))
                    .name
                    .entity
                    .clone();
                name.update(cx, |input, cx| input.set_text("Pending mode name", cx));
                view.sync_processing_settings(cx);
                assert!(view.settings_dirty);
                let pending_generation = view.settings_save_generation;

                view.settings_save_override = Some(reject_settings_save);
                assert!(view.add_mode(cx).is_none());
                view.delete_mode(0, cx);
                assert_eq!(view.mode_editors.modes.len(), 1);
                assert_eq!(
                    view.settings_candidate().dictation_processing.modes[0].name,
                    "Pending mode name"
                );
                assert_eq!(
                    view.settings.dictation_processing.modes[0].name,
                    "Work notes"
                );
                assert_eq!(view.settings_save_generation, pending_generation);

                view.settings_save_override = None;
                assert!(view.add_mode(cx) == Some(ModeSelection::Custom(1)));
                assert_eq!(view.mode_editors.modes.len(), 2);
                assert_eq!(
                    view.settings.dictation_processing.modes[0].name,
                    "Pending mode name"
                );
                let second_name = view
                    .mode_inputs_for(ModeSelection::Custom(1))
                    .name
                    .entity
                    .clone();
                second_name.update(cx, |input, cx| input.set_text("Survivor", cx));
                view.sync_processing_settings(cx);
                view.delete_mode(0, cx);
                assert_eq!(view.mode_editors.modes.len(), 1);
                assert_eq!(
                    view.mode_settings(ModeSelection::Custom(0)).name,
                    "Survivor"
                );
                assert_eq!(view.settings.dictation_processing.modes[0].name, "Survivor");
                assert!(!view.settings_dirty);
                view.select_model(
                    ModelPickerTarget::Mode(ModeSelection::Custom(0)),
                    Some("example/writer".into()),
                    cx,
                );

                // If either obsolete debounce writes, the injected failure becomes
                // visible and makes the assertions below fail.
                view.settings_save_override = Some(reject_settings_save);
            })
        });
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                assert!(view.settings_load_error.is_none());
                assert!(!view.settings_dirty);
                let name = view
                    .mode_inputs_for(ModeSelection::Custom(0))
                    .name
                    .entity
                    .clone();
                name.update(cx, |input, cx| input.set_text("Later draft", cx));
                view.sync_processing_settings(cx);
            })
        });
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                assert!(view.settings_dirty);
                assert_eq!(
                    view.settings_load_error.as_deref(),
                    Some("fixture persistence failure")
                );
                assert_eq!(
                    view.settings_candidate().dictation_processing.modes[0].name,
                    "Later draft"
                );
                assert_eq!(view.settings.dictation_processing.modes[0].name, "Survivor");
                assert_eq!(
                    view.settings.dictation_processing.modes[0]
                        .post_processing
                        .model
                        .as_deref(),
                    Some("example/writer")
                );
            })
        });
    }

    #[gpui::test]
    fn settings_content_stays_inside_the_pane_at_supported_widths(cx: &mut gpui::TestAppContext) {
        struct SettingsLayout;

        impl Render for SettingsLayout {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                window_frame()
                    .child(sidebar_frame())
                    .child(
                        div().flex_1().min_w_0().h_full().child(settings_pane(
                            "Settings",
                            "settings-scroll",
                            div()
                                .child(panel().child(settings_row(
                                    "Microphone",
                                    "Automatically chooses the preferred available input",
                                    disclosure_button("Automatic"),
                                )))
                                .child(settings_copy(
                                    "Release the microphone when idle?",
                                    "Hex will open the microphone when you press the shortcut. This adds a start-up delay and removes pre-roll, so the beginning of speech may be missed if you speak right away.",
                                )),
                        )),
                    )
            }
        }

        let (_, cx) = cx.add_window_view(|_, _| SettingsLayout);
        for width in [MINIMUM_WIDTH, WINDOW_WIDTH, 1400.0] {
            cx.simulate_resize(size(px(width), px(MINIMUM_HEIGHT)));
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            let column = cx
                .debug_bounds("settings-content")
                .expect("content was rendered");
            let available = width - SIDEBAR_WIDTH - 64.0;
            let expected_width = available.min(PANE_CONTENT_WIDTH);
            assert_eq!(column.size.width, px(expected_width));
            assert_eq!(
                column.origin.x,
                px(SIDEBAR_WIDTH + 32.0 + (available - expected_width) / 2.0)
            );
        }
    }

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
        assert_eq!(
            Pane::from_developer(
                serde_json::from_str::<DeveloperPane>(r#""replacements""#).unwrap()
            ),
            Pane::Modes
        );
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
    fn sidebar_update_button_only_appears_for_an_available_update() {
        use crate::sparkle::UpdateStatus;

        for (status, visible) in [
            (UpdateStatus::Unavailable, false),
            (UpdateStatus::Idle, false),
            (UpdateStatus::Checking, false),
            (UpdateStatus::UpToDate, false),
            (UpdateStatus::UpdateAvailable, true),
        ] {
            assert_eq!(sidebar_update_button(status, false).is_some(), visible);
        }
    }

    #[gpui::test]
    fn sidebar_update_click_dispatches_only_outside_preview(cx: &mut gpui::TestAppContext) {
        struct UpdateButton {
            preview: bool,
        }

        impl Render for UpdateButton {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                sidebar_update_button(crate::sparkle::UpdateStatus::UpdateAvailable, self.preview)
                    .unwrap()
                    .debug_selector(|| "sidebar-update".into())
            }
        }

        let calls = Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let calls = calls.clone();
            cx.on_action(move |_: &CheckForUpdates, _| calls.set(calls.get() + 1));
        });

        for preview in [true, false] {
            let (_, cx) = cx.add_window_view(|_, _| UpdateButton { preview });
            cx.update(|window, cx| {
                // Test windows do not activate automatically; an inactive window masks the bug.
                window.activate_window();
                assert!(cx.active_window() == Some(window.window_handle()));
            });
            cx.run_until_parked();
            let button = cx.debug_bounds("sidebar-update").unwrap();
            cx.simulate_click(button.center(), GpuiModifiers::default());
            assert_eq!(calls.get(), usize::from(!preview), "preview={preview}");
        }
    }

    #[gpui::test]
    fn microphone_picker_occludes_the_controls_beneath_it(cx: &mut gpui::TestAppContext) {
        #[derive(Default)]
        struct Observed {
            beneath_clicks: usize,
            beneath_hovered: bool,
            chosen: Option<Option<String>>,
            dismissed: usize,
        }

        struct PickerOverControl {
            observed: Rc<RefCell<Observed>>,
        }

        impl Render for PickerOverControl {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let observed = self.observed.clone();
                let hovered = self.observed.clone();
                let chosen = self.observed.clone();
                let dismissed = self.observed.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .relative()
                            .w(px(220.0))
                            .child(disclosure_button("Automatic").id("anchor"))
                            .child(deferred(microphone_picker_menu(
                                vec![None, Some("Studio Microphone".into())],
                                None,
                                None,
                                1.0,
                                move |device, _, _| {
                                    chosen.borrow_mut().chosen = Some(device.clone());
                                },
                                move |_, _, _| dismissed.borrow_mut().dismissed += 1,
                            )))
                            .child(
                                div()
                                    .id("beneath")
                                    .debug_selector(|| "beneath".into())
                                    .w(px(220.0))
                                    .h(px(120.0))
                                    .on_hover(move |is_hovered, _, _| {
                                        if *is_hovered {
                                            hovered.borrow_mut().beneath_hovered = true;
                                        }
                                    })
                                    .on_click(move |_, _, _| {
                                        observed.borrow_mut().beneath_clicks += 1;
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("outside")
                            .debug_selector(|| "outside".into())
                            .w(px(220.0))
                            .h(px(200.0)),
                    )
            }
        }

        let observed = Rc::new(RefCell::new(Observed::default()));
        let (_, cx) = cx.add_window_view(|_, _| PickerOverControl {
            observed: observed.clone(),
        });
        cx.update(|window, cx| {
            window.activate_window();
            assert!(cx.active_window() == Some(window.window_handle()));
        });
        cx.run_until_parked();
        let beneath = cx.debug_bounds("beneath").unwrap();
        let menu = cx.debug_bounds("microphone-picker").unwrap();
        let second_choice = cx.debug_bounds("microphone-choice-1").unwrap();
        assert!(
            menu.contains(&second_choice.center()) && beneath.contains(&second_choice.center()),
            "fixture must place a menu choice over the control beneath"
        );

        cx.simulate_mouse_move(second_choice.center(), None, GpuiModifiers::default());
        cx.run_until_parked();
        assert!(
            !observed.borrow().beneath_hovered,
            "hover leaked beneath the menu"
        );

        cx.simulate_click(second_choice.center(), GpuiModifiers::default());
        cx.run_until_parked();
        {
            let observed = observed.borrow();
            assert_eq!(observed.beneath_clicks, 0, "click leaked beneath the menu");
            assert_eq!(observed.chosen, Some(Some("Studio Microphone".into())));
            assert_eq!(
                observed.dismissed, 0,
                "choosing must not also count as dismissal"
            );
        }

        let outside = cx.debug_bounds("outside").unwrap();
        cx.simulate_click(outside.center(), GpuiModifiers::default());
        cx.run_until_parked();
        assert_eq!(observed.borrow().dismissed, 1);
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
        let description = voice_action_description(settings.voice_action.enabled);
        assert!(description.contains("Off by default"));
        assert!(description.contains("not reserved"));

        let description = voice_action_description(true);
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
    fn side_selection_changes_modifier_only_bindings_and_ignores_key_chords() {
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

        let mut modifier_chord = HotkeyBinding {
            modifiers: normalize_captured_modifiers(HotkeyModifiers {
                option: Some(ModifierSide::Right),
                command: Some(ModifierSide::Right),
                ..Default::default()
            }),
            key: None,
        };
        assert_eq!(
            standalone_modifier_side(&modifier_chord),
            Some(ModifierSide::Right)
        );
        assert_eq!(modifier_chord.keycaps(), vec!["R⌥", "R⌘"]);
        set_standalone_modifier_side(&mut modifier_chord, ModifierSide::Left);
        assert_eq!(
            modifier_chord.modifiers,
            HotkeyModifiers {
                option: Some(ModifierSide::Left),
                command: Some(ModifierSide::Left),
                ..Default::default()
            }
        );

        let mixed = normalize_captured_modifiers(HotkeyModifiers {
            option: Some(ModifierSide::Left),
            command: Some(ModifierSide::Right),
            ..Default::default()
        });
        assert_eq!(
            mixed,
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                command: Some(ModifierSide::Either),
                ..Default::default()
            }
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
            Pane::all(platform::CAPABILITIES, false, false),
            vec![
                Pane::Settings,
                Pane::Modes,
                Pane::Commands,
                Pane::VoiceAction,
                Pane::History,
            ]
        );
        assert_eq!(
            Pane::all(platform::CAPABILITIES, false, true),
            vec![
                Pane::Settings,
                Pane::Modes,
                Pane::Commands,
                Pane::VoiceAction,
                Pane::History,
                Pane::Meetings,
            ],
            "the meetings opt-in adds only the Meetings pane"
        );
    }

    #[test]
    fn developer_navigation_appends_developer_panes_in_order() {
        assert_eq!(
            Pane::all(platform::CAPABILITIES, true, false),
            vec![
                Pane::Settings,
                Pane::Modes,
                Pane::Commands,
                Pane::VoiceAction,
                Pane::History,
                Pane::HudLab,
                Pane::Meetings,
                Pane::Activity,
            ]
        );
    }

    #[test]
    fn experimental_fun_pane_appears_only_when_enabled() {
        assert!(
            !Pane::all_with_fun(platform::CAPABILITIES, false, false, false).contains(&Pane::Fun)
        );
        assert_eq!(
            Pane::all_with_fun(platform::CAPABILITIES, false, false, true),
            vec![
                Pane::Settings,
                Pane::Modes,
                Pane::Commands,
                Pane::VoiceAction,
                Pane::History,
                Pane::Fun,
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
        assert!((at_60_hz.position() - at_120_hz.position()).abs() < 0.001);
        assert_eq!(at_60_hz.position(), 1.0);
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
            default_variant: None,
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
            default_variant: None,
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

        let copy = opencode_unavailable_copy(&state, OpenCodeFeature::VoiceAction).unwrap();

        assert_eq!(copy.title, "OpenCode is unavailable");
        assert_eq!(
            copy.description,
            "Hex could not load the OpenCode model catalog."
        );
        assert_eq!(
            copy.error.as_deref(),
            Some("OpenCode /api/model failed: config.providers was invalid")
        );
        assert!(copy.can_retry);
        assert!(!copy.can_open_setup);
    }

    #[test]
    fn opencode_missing_copy_offers_setup_without_an_error() {
        let copy =
            opencode_unavailable_copy(&ModelCatalogState::Missing, OpenCodeFeature::VoiceAction)
                .unwrap();

        assert_eq!(copy.title, "Voice Action requires OpenCode");
        assert_eq!(copy.error, None);
        assert!(copy.can_retry);
        assert!(copy.can_open_setup);
    }

    #[test]
    fn opencode_transformation_copy_describes_modes_without_changing_actions() {
        let loading =
            opencode_unavailable_copy(&ModelCatalogState::Loading, OpenCodeFeature::Transformation)
                .unwrap();
        assert_eq!(loading.title, "Checking for OpenCode");
        assert_eq!(
            loading.description,
            "OpenCode transformation will be available after Hex connects to OpenCode."
        );
        assert!(!loading.can_retry);
        assert!(!loading.can_open_setup);

        let missing =
            opencode_unavailable_copy(&ModelCatalogState::Missing, OpenCodeFeature::Transformation)
                .unwrap();
        assert_eq!(missing.title, "Transformation requires OpenCode");
        assert_eq!(
            missing.description,
            "Install and configure OpenCode to transform dictated text."
        );
        assert_eq!(missing.error, None);
        assert!(missing.can_retry);
        assert_eq!(missing.retry_label, "Check again");
        assert!(missing.can_open_setup);

        let failed = ModelCatalogState::Failed("catalog failure".into());
        let error = opencode_unavailable_copy(&failed, OpenCodeFeature::Transformation).unwrap();
        assert_eq!(error.title, "OpenCode is unavailable");
        assert_eq!(error.error.as_deref(), Some("catalog failure"));
        assert!(error.can_retry);
        assert!(!error.can_open_setup);
        let loaded = ModelCatalogState::Loaded(ModelCatalog {
            models: Vec::new(),
            default_key: None,
            default_name: None,
            default_variant: None,
        });
        for feature in [
            OpenCodeFeature::VoiceAction,
            OpenCodeFeature::Transformation,
        ] {
            assert!(opencode_unavailable_copy(&loaded, feature).is_none());
        }
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
            default_variant: None,
        };

        assert_eq!(model_variants(&catalog, None), ["low", "high"]);
    }

    #[test]
    fn model_variant_selection_resolves_defaults_without_replacing_explicit_models() {
        let catalog = ModelCatalogState::Loaded(ModelCatalog {
            models: Vec::new(),
            default_key: Some("example/rewrite-default".into()),
            default_name: Some("Example Rewrite".into()),
            default_variant: None,
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
