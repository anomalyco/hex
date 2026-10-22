//! macOS parts of the app window: Meetings, Commands, the HUD Lab, the
//! privacy permission prompts, Sparkle updates, and the Dock icon, plus the
//! state only they use.

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{SyncSender, TrySendError};

use gpui::{MouseMoveEvent, ScrollHandle, relative};

use super::tour::{PermissionControl, permission_control};
use super::*;
use crate::commands::{CommandConfig, CommandInfo, CommandScope};
use crate::desktop_ui::{disabled_menu_item, empty_state, list_section_label, menu_separator};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender, HudTuning};
use crate::meeting::{
    self, MeetingManifest, MeetingRequest, MeetingSource, MeetingStatus, TranscriptEntry,
};
use crate::onboarding::{PermissionAction, PermissionKind, PermissionWarning};
use crate::personal_commands::StatusContext;
use crate::recognition::CommandModelStatus;

pub(super) const CAPABILITIES: Capabilities = Capabilities {
    meetings: true,
    commands: true,
    hud_lab: true,
    fun: true,
    modes: true,
    voice_action: true,
    history: true,
    permissions: true,
    dock_icon: true,
    launch_at_login: true,
    listener_control: false,
    paste_last: true,
    recording_audio: true,
    release_microphone: true,
    double_tap_only: true,
    modifier_sides: true,
    indicator_position: true,
    interface_language: true,
    software_updates: true,
    terminal_paste: false,
    practice_dictation: true,
};

/// macOS records shortcuts from window key events, resolving keys through
/// the active keyboard layout.
pub(super) fn records_key_events() -> bool {
    true
}

/// Sparkle has its own Check for Updates command.
/// How the Modes pane describes website rules; `None` hides them where
/// the listener can't read the browser's address.
pub(super) const WEB_PAGES_DESCRIPTION: Option<&str> =
    Some("Switch when a domain is active in Brave");

/// Whether the listener knows which application is frontmost.
pub(super) fn focused_application_known() -> bool {
    true
}

/// The Modes application picker's button for an app it doesn't list.
pub(super) const CHOOSE_APPLICATION_FILE: &str = "Choose from Finder…";

pub(super) const MANUAL_UPDATE_CHECK: bool = true;

/// What the macOS application coordinator hands a new window.
pub(super) struct Args {
    pub(super) commands: CommandConfig,
    pub(super) meeting_requests: SyncSender<MeetingRequest>,
    pub(super) indicator: DictationIndicatorSender,
    pub(super) recognition_start: Option<SyncSender<()>>,
}

pub(super) struct Platform {
    pub(super) meeting_requests: SyncSender<MeetingRequest>,
    pub(super) indicator: DictationIndicatorSender,
    pub(super) hud_tuning: HudTuning,
    pub(super) hud_demo: HudDemoState,
    pub(super) hud_queued: usize,
    pub(super) hud_dragging: Option<HudControl>,
    pub(super) hud_copied: bool,
    pub(super) recognition_start: Option<SyncSender<()>>,
    pub(super) update_status: crate::sparkle::UpdateStatus,
    pub(super) update_status_changed_at: Instant,
    pub(super) command_model_status: CommandModelStatus,
    pub(super) meetings_toggle: ToggleSpring,
    pub(super) meeting_indicator_toggle: ToggleSpring,
    pub(super) meeting_notes_toggle: ToggleSpring,
    pub(super) screen_recording_permitted: bool,
    /// Permissions whose Allow was pressed since they were last granted.
    pub(super) permissions_asked: Vec<PermissionKind>,
    pub(super) meeting_folder_error: Option<String>,
    pub(super) meetings: Vec<MeetingManifest>,
    pub(super) meetings_error: Option<String>,
    pub(super) meeting_runtime_error: Option<String>,
    pub(super) meeting_refresh_started_ms: Option<u64>,
    pub(super) meeting_stop_pending: bool,
    pub(super) selected_meeting: Option<String>,
    pub(super) meeting_transcript: MeetingTranscript,
    pub(super) meeting_transcript_id: Option<String>,
    pub(super) meeting_transcript_reader: meeting::TranscriptReader,
    pub(super) meeting_transcript_scroll: ScrollHandle,
    /// Coalesced turns of the selected meeting, for Copy Transcript.
    pub(super) meeting_turns: Vec<TranscriptEntry>,
    pub(super) meeting_search: ProcessingInput,
    /// Lowercased searchable text per meeting, loaded when a search starts.
    pub(super) meeting_search_text: HashMap<String, String>,
    pub(super) meeting_rename: Option<(String, ProcessingInput)>,
    pub(super) meeting_delete_armed: Option<String>,
    /// The detail's overflow menu of less common meeting actions is open.
    pub(super) meeting_actions_open: bool,
    pub(super) meeting_action_error: Option<String>,
    pub(super) meeting_copied: Option<(String, &'static str)>,
    /// Deterministic transcripts for the Meetings preview.
    pub(super) meeting_preview_transcripts: Option<HashMap<String, Vec<TranscriptEntry>>>,
    pub(super) compiled_commands: Vec<CatalogCommand>,
    pub(super) commands: Vec<CatalogCommand>,
    pub(super) command_search: ProcessingInput,
    pub(super) selected_command: Option<String>,
}

impl Platform {
    pub(super) fn new(
        args: Args,
        preview: Option<&AppWindowPreview>,
        settings: &AppSettings,
        personal_commands: &StatusSnapshot,
        cx: &mut Context<AppWindow>,
    ) -> Self {
        let preview_mode = preview.is_some();
        if !preview_mode {
            crate::app_settings::set_dock_icon_visible(true);
        }
        let compiled_commands = args
            .commands
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
        let commands = merged_catalog(&compiled_commands, personal_commands);
        Self {
            meeting_requests: args.meeting_requests,
            indicator: args.indicator,
            hud_tuning: HudTuning::default(),
            hud_demo: HudDemoState::Transcribing,
            hud_queued: 1,
            hud_dragging: None,
            hud_copied: false,
            recognition_start: args.recognition_start,
            update_status: if let Some(preview) = preview {
                if preview.update_available {
                    crate::sparkle::UpdateStatus::UpdateAvailable
                } else {
                    crate::sparkle::UpdateStatus::Idle
                }
            } else {
                crate::sparkle::status()
            },
            update_status_changed_at: Instant::now(),
            command_model_status: if preview.is_some_and(|preview| preview.command_model_missing) {
                CommandModelStatus::Failed
            } else if preview_mode {
                CommandModelStatus::Disabled
            } else {
                crate::recognition::command_model_status()
            },
            meetings_toggle: ToggleSpring::new(settings.meetings.enabled),
            meeting_indicator_toggle: ToggleSpring::new(settings.meetings.show_recording_indicator),
            meeting_notes_toggle: ToggleSpring::new(
                settings.meetings.summarize_with_apple_intelligence,
            ),
            screen_recording_permitted: match preview {
                Some(preview) => !preview.screen_recording_missing,
                None => crate::meeting::system_audio_ready(),
            },
            permissions_asked: Vec::new(),
            meeting_folder_error: None,
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
            meeting_turns: Vec::new(),
            meeting_search: AppWindow::meeting_search_input(cx),
            meeting_search_text: HashMap::new(),
            meeting_rename: None,
            meeting_delete_armed: None,
            meeting_actions_open: false,
            meeting_action_error: None,
            meeting_copied: None,
            meeting_preview_transcripts: None,
            compiled_commands,
            commands,
            command_search: AppWindow::command_search_input(cx),
            selected_command: None,
        }
    }
}

pub(super) fn window_options(options: WindowOptions) -> WindowOptions {
    options
}

/// macOS draws the traffic lights over the transparent titlebar itself.
pub(super) fn frame(frame: Div) -> Div {
    frame
}

pub(super) fn sidebar(sidebar: Div) -> Div {
    sidebar
}

/// Empty sidebar space moves the window, as the titlebar does.
pub(super) fn window_drag_region(region: Div) -> Div {
    region.on_mouse_down(MouseButton::Left, |_, _, _| {
        let Some(main_thread) = objc2::MainThreadMarker::new() else {
            return;
        };
        let application = objc2_app_kit::NSApplication::sharedApplication(main_thread);
        if let Some(event) = application.currentEvent()
            && let Some(ns_window) = event.window(main_thread)
        {
            ns_window.performWindowDragWithEvent(&event);
        }
    })
}

pub(super) fn caption(_: &mut Window) -> Option<AnyElement> {
    None
}

/// Suspends the global shortcut monitor while Settings records a shortcut.
pub(super) fn set_hotkey_capture_active(active: bool) {
    crate::app_settings::set_hotkey_capture_active(active);
}

/// macOS accepts every binding the capture can produce.
pub(super) fn validate_hotkey(
    _: HotkeyKind,
    _: &HotkeyBinding,
) -> std::result::Result<(), &'static str> {
    Ok(())
}

impl AppWindow {
    /// Models are prepared in this window.
    pub(super) fn platform_prepare_transcription(&mut self, _: &TranscriptionSelection) -> bool {
        false
    }

    pub(super) fn platform_cancel_transcription(&mut self) -> bool {
        false
    }

    pub(super) fn platform_preparation_status(&self) -> Option<PreparationStatus> {
        None
    }

    /// Shortcuts are saved with the window's settings.
    pub(super) fn platform_save_hotkey(&mut self, _: HotkeyKind, _: &HotkeyBinding) -> bool {
        false
    }

    pub(super) fn platform_capture_cancelled(&mut self) {}

    pub(super) fn poll_platform(&mut self, cx: &mut Context<Self>) -> bool {
        let command_model_status = if self.preview {
            self.platform.command_model_status
        } else {
            crate::recognition::command_model_status()
        };
        let command_model_changed = self.platform.command_model_status != command_model_status;
        self.platform.command_model_status = command_model_status;
        let update_status = if self.preview {
            self.platform.update_status
        } else {
            crate::sparkle::status()
        };
        let update_status_changed = self.platform.update_status != update_status;
        if update_status_changed {
            self.platform.update_status = update_status;
            self.platform.update_status_changed_at = Instant::now();
        }
        let update_confirmation_expired = self.platform.update_status
            == crate::sparkle::UpdateStatus::UpToDate
            && self.platform.update_status_changed_at.elapsed() >= Duration::from_secs(4);
        if update_confirmation_expired {
            self.platform.update_status = crate::sparkle::UpdateStatus::Idle;
            crate::sparkle::clear_confirmation();
        }
        if self.settings.meetings.available()
            && (self.platform.meeting_refresh_started_ms.is_some()
                || meeting::active(&self.platform.meetings).is_some())
        {
            self.reload_meetings();
            cx.notify();
        }
        command_model_changed || update_status_changed || update_confirmation_expired
    }

    /// Opens on an active meeting, and loads preview fixtures.
    pub(super) fn platform_opened(&mut self, preview: Option<&AppWindowPreview>) {
        if !self.preview && self.settings.meetings.available() {
            self.reload_meetings();
            if meeting::active(&self.platform.meetings).is_some() {
                self.pane = Pane::Meetings;
            }
        }
        if let Some(preview) =
            preview.filter(|preview| matches!(preview.pane, DeveloperPane::Meetings))
            && self.pane == Pane::Meetings
        {
            self.load_meeting_preview(&MeetingPreview {
                recording: preview.meeting_recording,
                confirm_delete: preview.confirm_meeting_delete,
                open_actions: preview.open_meeting_actions,
                empty: preview.meetings_empty,
            });
        }
        if self.pane == Pane::HudLab {
            self.apply_hud_lab();
        }
    }

    pub(super) fn platform_refresh(&mut self) {
        if self.settings.meetings.available() {
            self.reload_meetings();
        }
    }

    pub(super) fn platform_select_pane(&mut self, pane: Pane) {
        match pane {
            Pane::HudLab => self.apply_hud_lab(),
            Pane::Meetings => self.reload_meetings(),
            _ => {}
        }
    }

    pub(super) fn render_platform_pane(
        &mut self,
        pane: Pane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match pane {
            Pane::HudLab => Some(self.render_hud_lab(window, cx)),
            Pane::Meetings => Some(self.render_meetings(window, cx)),
            Pane::Commands => Some(self.render_commands(window, cx)),
            _ => None,
        }
    }

    /// `AppSettings::save` already applied the runtime atomics.
    pub(super) fn platform_settings_saved(&mut self) {}

    pub(super) fn platform_closed(&mut self) {
        crate::app_settings::set_dock_icon_visible(crate::app_settings::dock_icon_visible(
            self.settings.show_dock_icon,
            crate::status_item::installed(),
        ));
    }

    pub(super) fn platform_settings_notice(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.render_permission_warnings(cx)
    }

    /// Settings records every shortcut this platform registers.
    pub(super) fn platform_shortcut_rows(&self) -> bool {
        true
    }

    pub(super) fn platform_settings_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            settings_section_label(tr("Meetings")).into_any_element(),
            self.render_meeting_settings(window, cx).into_any_element(),
        ]
    }

    /// The recognition coordinator waits for setup to finish.
    pub(super) fn platform_awaits_setup(&self) -> bool {
        self.platform.recognition_start.is_some()
    }

    pub(super) fn platform_setup_ready(&mut self) {
        if let Some(start) = self.platform.recognition_start.take() {
            let _ = start.try_send(());
        }
    }

    pub(super) fn platform_poll_setup(&mut self) -> bool {
        let meetings = self.settings.meetings.available();
        let permitted = meetings && crate::meeting::system_audio_ready();
        let status = self.setup_status;
        let before = self.platform.permissions_asked.len();
        self.platform.permissions_asked.retain(|kind| match kind {
            PermissionKind::Microphone => status.microphone != PermissionState::Ready,
            PermissionKind::InputMonitoring => status.input_monitoring != PermissionState::Ready,
            PermissionKind::Accessibility => status.accessibility != PermissionState::Ready,
            PermissionKind::SystemAudio => !permitted,
        });
        let mut changed = before != self.platform.permissions_asked.len();
        if !meetings {
            return changed;
        }
        changed |= permitted != self.platform.screen_recording_permitted;
        self.platform.screen_recording_permitted = permitted;
        changed
    }

    pub(super) fn personal_commands_changed(&mut self) {
        self.platform.commands = merged_catalog(
            &self.platform.compiled_commands,
            &self.personal_commands_status,
        );
        if self.platform.selected_command.as_ref().is_some_and(|id| {
            !self
                .platform
                .commands
                .iter()
                .any(|command| &command.id == id)
        }) {
            self.platform.selected_command = None;
        }
    }

    pub(super) fn platform_update_button(
        &mut self,
        _: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        sidebar_update_button(self.platform.update_status, self.preview)
    }

    pub(super) fn platform_capture_started(&mut self, _: &mut Context<Self>) {}

    /// The macOS listener runs whenever Hex does.
    pub(super) fn platform_listener(&self) -> Option<ListenerView> {
        None
    }

    pub(super) fn set_listening(&mut self, _: bool) {}

    /// Sparkle's state. Sparkle presents the update itself, so installing
    /// starts its check, which offers the found update.
    pub(super) fn platform_software_update(&self) -> Option<SoftwareUpdate> {
        use crate::sparkle::UpdateStatus;
        Some(match self.platform.update_status {
            UpdateStatus::Unavailable => return None,
            UpdateStatus::Checking => SoftwareUpdate::Checking,
            UpdateStatus::Idle | UpdateStatus::UpToDate => SoftwareUpdate::Current,
            UpdateStatus::UpdateAvailable => SoftwareUpdate::Available { version: None },
        })
    }

    pub(super) fn check_for_updates(&mut self) {
        if !self.preview {
            crate::sparkle::check_for_updates();
        }
    }

    pub(super) fn install_update(&mut self, _: &mut Context<Self>) {
        self.check_for_updates();
    }
}

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
                this.platform.meeting_requests = meeting_requests.clone();
                this.platform.indicator = indicator.clone();
                this.platform.recognition_start = recognition_start.clone();
                this.history = history.clone();
                this.refresh(cx);
                window.activate_window();
            })
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }

    let handle = open_new(
        event_path,
        history,
        None,
        Args {
            commands,
            meeting_requests,
            indicator,
            recognition_start,
        },
        cx,
    )?;
    *app_window.borrow_mut() = Some(handle);
    Ok(handle)
}

/// Commit through the live editor when present, preserving its unsaved settings.
/// Quiet startup does not require creating or focusing a Settings window.
pub fn commit_transcription_selection(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    selection: TranscriptionSelection,
    cx: &mut App,
) -> Result<AppSettings, String> {
    let handle = app_window.borrow().as_ref().copied();
    if let Some(handle) = handle
        && let Ok(result) = handle.update(cx, |this, _, cx| {
            this.apply_transcription_selection(selection.clone(), cx)?;
            cx.notify();
            Ok(this.settings.clone())
        })
    {
        return result;
    }
    let mut settings = AppSettings::load().map_err(|error| error.to_string())?;
    settings
        .save_transcription(selection)
        .map_err(|error| error.to_string())?;
    Ok(settings)
}

pub fn menu_transcription_selection(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    model: TranscriptionModelId,
    fallback: &AppSettings,
    cx: &mut App,
) -> TranscriptionSelection {
    let handle = app_window.borrow().as_ref().copied();
    handle
        .and_then(|handle| {
            handle
                .update(cx, |this, _, _| {
                    this.settings.transcription_for_model(model)
                })
                .ok()
        })
        .unwrap_or_else(|| fallback.transcription_for_model(model))
}

pub fn refresh_transcription_menu(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    fallback: &AppSettings,
    status: &PreparationStatus,
    cx: &mut App,
) {
    let handle = app_window.borrow().as_ref().copied();
    if handle.is_some_and(|handle| {
        handle
            .update(cx, |this, _, _| {
                crate::status_item::update_transcription(&this.settings, status);
            })
            .is_ok()
    }) {
        return;
    }
    crate::status_item::update_transcription(fallback, status);
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
    let handle = open_new(
        event_path,
        None,
        Some(preview),
        Args {
            commands,
            meeting_requests,
            indicator,
            recognition_start: None,
        },
        cx,
    )?;
    *app_window.borrow_mut() = Some(handle);
    Ok(handle)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HudDemoState {
    Recording,
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HudControl {
    LineCount,
    Curvature,
    Speed,
    Blur,
    Glow,
    Depth,
    LightAngle,
    Outline,
}

pub(super) const HUD_MAX_SPEED: f32 = 24.0;

#[derive(Clone)]
pub(super) struct CatalogCommand {
    pub(super) task: String,
    pub(super) scope: CommandScope,
    pub(super) phrase: String,
    pub(super) aliases: Vec<String>,
    pub(super) description: String,
    pub(super) id: String,
}

#[derive(Clone, PartialEq)]
pub(super) struct TranscriptLine {
    source: MeetingSource,
    clock: String,
    text: String,
    pause_before_ms: Option<u64>,
}

pub(super) struct MeetingPreview {
    recording: bool,
    confirm_delete: bool,
    open_actions: bool,
    empty: bool,
}

#[derive(Clone)]
pub(super) enum MeetingTranscript {
    Empty,
    Loaded(Vec<TranscriptLine>),
    Unavailable(String),
}

pub(super) fn sidebar_update_button(
    status: crate::sparkle::UpdateStatus,
    preview: bool,
) -> Option<gpui::Stateful<Div>> {
    (status == crate::sparkle::UpdateStatus::UpdateAvailable).then(|| {
        super::sidebar_update_button().on_click(move |_, window, cx| {
            if !preview {
                window.dispatch_action(Box::new(CheckForUpdates), cx);
                window.refresh();
            }
        })
    })
}

pub(super) fn command_task(command: &CommandInfo) -> String {
    if let Some(group) = &command.group {
        return group.clone();
    }
    if command.id.starts_with("mode.") {
        tr("Voice control").into()
    } else if command.id.starts_with("dictation.") {
        tr("Dictation").into()
    } else if command.id.starts_with("meeting.") {
        tr("Meetings").into()
    } else {
        match command.description.as_str() {
            "Open application" => tr("Open applications"),
            "Open website" => tr("Open websites"),
            "Navigate active tab" => tr("Browser navigation"),
            "Use keyboard shortcut" => tr("Keyboard"),
            "Use application shortcut" | "Open application destination" => {
                tr("Application navigation")
            }
            _ => tr("Other"),
        }
        .into()
    }
}

pub(super) fn merged_catalog(
    compiled: &[CatalogCommand],
    personal: &StatusSnapshot,
) -> Vec<CatalogCommand> {
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

pub(super) fn open_path(path: &std::path::Path) {
    if let Err(error) = ProcessCommand::new("/usr/bin/open").arg(path).spawn() {
        tracing::error!(%error, path = %path.display(), "could not open personal command path");
    }
}

pub(super) fn command_scope(scope: &CommandScope) -> String {
    match scope {
        CommandScope::Sleeping | CommandScope::Global => tr("Global").into(),
        CommandScope::Application(application) => application.clone(),
        CommandScope::Browser(host) => host.clone(),
    }
}

pub(super) fn render_meeting_notes(notes: meeting::MeetingNotes) -> Div {
    let summary = notes.summary.trim().to_string();
    div()
        .mb_5()
        .pb_5()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .border_b_1()
        .border_color(rgb(LINE))
        .child(section_label(tr("Summary")))
        .child(
            div()
                .text_body()
                .text_color(rgb(if summary.is_empty() {
                    TEXT_TERTIARY
                } else {
                    TEXT_SECONDARY
                }))
                .child(if summary.is_empty() {
                    tr("Nothing to summarize.").to_string()
                } else {
                    summary
                }),
        )
}

/// A compact status tag; `Ready` appears only in the detail header, so the
/// list stays quiet for finished meetings.
pub(super) fn meeting_status_chip(
    status: MeetingStatus,
    stop_pending: bool,
    show_ready: bool,
) -> Option<Div> {
    let (label, tone) = match status {
        MeetingStatus::Recording if stop_pending => (tr("Transcribing"), Tone::Accent),
        MeetingStatus::Starting | MeetingStatus::Recording => (tr("Recording"), Tone::Danger),
        MeetingStatus::Transcribing => (tr("Transcribing"), Tone::Accent),
        MeetingStatus::Complete if show_ready => (tr("Ready"), Tone::Success),
        MeetingStatus::Complete => return None,
        MeetingStatus::Interrupted => (tr("Interrupted"), Tone::Neutral),
        MeetingStatus::Failed => (tr("Failed"), Tone::Danger),
    };
    let live =
        matches!(status, MeetingStatus::Starting | MeetingStatus::Recording) && !stop_pending;
    Some(tag(tone, label).when(live, |chip| {
        chip.child(status_dot(DANGER).size(px(5.0)))
            .flex_row_reverse()
    }))
}

/// `42 min`, `1 h 05 min`, or `< 1 min`.
pub(super) fn meeting_length(milliseconds: u64) -> String {
    let minutes = milliseconds / 60_000;
    match minutes {
        0 => tr("< 1 min").into(),
        1..=59 => tr_fill("{} min", &minutes.to_string()),
        _ => tr_named(
            "{hours} h {minutes} min",
            &[
                ("hours", &(minutes / 60).to_string()),
                ("minutes", &format!("{:02}", minutes % 60)),
            ],
        ),
    }
}

pub(super) fn transcript_lines(turns: &[TranscriptEntry]) -> Vec<TranscriptLine> {
    turns
        .iter()
        .scan(None, |previous_end: &mut Option<u64>, turn| {
            let pause_before_ms = previous_end
                .map(|end| turn.start_ms.saturating_sub(end))
                .filter(|pause| *pause >= 30_000);
            *previous_end = Some(previous_end.map_or(turn.end_ms, |end| end.max(turn.end_ms)));
            Some(TranscriptLine {
                source: turn.source,
                clock: meeting::format_duration(turn.start_ms),
                text: turn.text.clone(),
                pause_before_ms,
            })
        })
        .collect()
}

/// One conversational turn: the other side on the left, the user on the
/// right in the accent color.
pub(super) fn transcript_turn(index: usize, line: &TranscriptLine) -> AnyElement {
    let mine = line.source == MeetingSource::Microphone;
    div()
        .id(("transcript-line", index))
        .flex()
        .flex_col()
        .when_some(line.pause_before_ms, |row, pause| {
            row.child(
                div()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_caption()
                    .text_color(rgb(TEXT_TERTIARY))
                    .child(div().h(px(1.0)).flex_1().bg(rgb(LINE)))
                    .child(tr_fill("{} pause", &meeting_length(pause)))
                    .child(div().h(px(1.0)).flex_1().bg(rgb(LINE))),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .when(mine, |turn| turn.items_end())
                .when(!mine, |turn| turn.items_start())
                .child(
                    // Inset to the bubble's text, not its rounded edge.
                    div()
                        .px_3()
                        .flex()
                        .gap_2()
                        .text_caption()
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(if mine { ACCENT_TEXT } else { TEXT_TERTIARY }))
                                .child(line.source.label()),
                        )
                        .child(line.clock.clone()),
                )
                .child(
                    div()
                        .debug_selector(move || format!("meeting-bubble-{index}"))
                        .max_w(relative(0.82))
                        .px_3()
                        .py_2()
                        .rounded(RADIUS_PANEL)
                        .border_1()
                        .when(mine, |bubble| {
                            bubble
                                .border_color(rgb(ACCENT_BORDER))
                                .bg(rgb(ACCENT_SURFACE))
                                .text_color(rgb(TEXT_PRIMARY))
                        })
                        .when(!mine, |bubble| {
                            bubble
                                .border_color(rgb(LINE))
                                .bg(rgb(CANVAS))
                                .text_color(rgb(TEXT_SECONDARY))
                        })
                        .text_body()
                        .child(line.text.clone()),
                ),
        )
        .into_any_element()
}

pub(super) fn reveal_meeting(id: &str, export_path: Option<&std::path::Path>) {
    if let Some(path) = export_path {
        meeting::reveal(path);
        return;
    }
    match meeting::root() {
        Ok(root) => match ProcessCommand::new("/usr/bin/open")
            .arg(root.join(id))
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::error!(%status, "could not reveal meeting files"),
            Err(error) => tracing::error!(%error, "could not reveal meeting files"),
        },
        Err(error) => tracing::error!(%error, "could not locate meeting files"),
    }
}

/// Deterministic meetings for the Meetings preview: one recording, one with
/// notes, one without, and one that failed.
pub(super) fn preview_meetings(
    empty: bool,
) -> (Vec<MeetingManifest>, HashMap<String, Vec<TranscriptEntry>>) {
    if empty {
        return (Vec::new(), HashMap::new());
    }
    let now = now_ms();
    let minute = 60_000;
    let manifest =
        |id: &str, title: &str, status, started: u64, duration: u64, app: Option<&str>| {
            MeetingManifest {
                schema_version: 1,
                id: id.into(),
                title: title.into(),
                status,
                created_at_ms: started,
                ended_at_ms: (status != MeetingStatus::Recording).then_some(started + duration),
                duration_ms: (status != MeetingStatus::Recording).then_some(duration),
                system: Default::default(),
                microphone: Default::default(),
                live_dropped_packets: 0,
                live_transcription_error: None,
                error: None,
                app: app.map(Into::into),
                export_path: None,
                export_error: None,
                echo_suppressed_utterances: 0,
                provisional_title: None,
                notes: None,
                notes_error: None,
            }
        };
    let recording_start = now - 7 * minute - 12_000;
    let mut live = manifest(
        "preview-live",
        &meeting::default_title(Some("Zoom"), recording_start),
        MeetingStatus::Recording,
        recording_start,
        0,
        Some("Zoom"),
    );
    live.created_at_ms = recording_start;
    let synced_start = now - 3 * 60 * minute;
    let mut synced = manifest(
        "preview-notes",
        &meeting::default_title(Some("Zoom"), synced_start),
        MeetingStatus::Complete,
        synced_start,
        38 * minute + 20_000,
        Some("Zoom"),
    );
    synced.notes = Some(meeting::MeetingNotes {
        title: "Arcade launch plan".into(),
        summary: "One login for every Anomaly Arcade game, aiming for twenty to thirty games \
over the next couple of months. The build screen needs a proposal before more polish."
            .into(),
        generated_at_ms: synced_start + 39 * minute,
        elapsed_ms: 4_000,
        ..Default::default()
    });
    let design_start = now - 27 * 60 * minute;
    let design = manifest(
        "preview-design",
        "Design review",
        MeetingStatus::Complete,
        design_start,
        24 * minute,
        Some("FaceTime"),
    );
    let failed_start = now - 29 * 60 * minute;
    let mut failed = manifest(
        "preview-failed",
        &meeting::default_title(None, failed_start),
        MeetingStatus::Failed,
        failed_start,
        2 * minute,
        None,
    );
    failed.error = Some("ScreenCaptureKit stopped the capture.".into());

    let turn = |source, seconds: u64, text: &str| TranscriptEntry {
        source,
        start_ms: seconds * 1_000,
        end_ms: seconds * 1_000 + 2_500,
        text: text.into(),
    };
    use MeetingSource::{Microphone as You, System as Them};
    let conversation = vec![
        turn(
            Them,
            1,
            "Basically, I want Anomaly Arcade to be one login where you add friends and jump into any game.",
        ),
        turn(Them, 4, "I need to finish my StarCraft game, too."),
        turn(You, 9, "Oh nice. How far along is it?"),
        turn(
            Them,
            13,
            "Close. I feel like over a couple of months we could make twenty to thirty games.",
        ),
        turn(You, 20, "That's basically a new version of Steam."),
        turn(
            Them,
            24,
            "If we're seeing Roblox, the idea should be to become Roblox.",
        ),
        turn(
            You,
            31,
            "Apple has a new thing where you hold two phones together and there's this pulse and gravity well.",
        ),
        turn(
            You,
            34,
            "Could be a nice mini gravity well for the transitions.",
        ),
        turn(Them, 40, "Prototype it in WebGPU, whatever's faster."),
        turn(
            You,
            95,
            "The build screen looks rough, by the way. We need a proposal for it.",
        ),
        turn(
            Them,
            101,
            "Agreed. Write it up and I'll review it tomorrow.",
        ),
    ];
    let transcripts = HashMap::from([
        ("preview-live".to_string(), conversation[..7].to_vec()),
        ("preview-notes".to_string(), conversation.clone()),
        ("preview-design".to_string(), conversation[2..6].to_vec()),
    ]);
    (vec![live, synced, design, failed], transcripts)
}

pub(super) fn log_meeting_request_error(error: TrySendError<MeetingRequest>) {
    match error {
        TrySendError::Full(_) => tracing::warn!("meeting controller is busy"),
        TrySendError::Disconnected(_) => tracing::error!("meeting controller is unavailable"),
    }
}

pub(super) fn meeting_duration(meeting: &MeetingManifest) -> u64 {
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

pub(super) fn command_model_notice(
    status: &CommandModelStatus,
) -> Option<(&'static str, &'static str)> {
    match status {
        CommandModelStatus::Disabled | CommandModelStatus::Ready => None,
        CommandModelStatus::Loading => Some((
            tr("Preparing voice commands"),
            tr("The command model is loading. Hotkey dictation does not require it."),
        )),
        CommandModelStatus::Failed => Some((
            tr("Voice commands are unavailable"),
            tr(
                "The command model could not load. Retry or turn off commands; hotkey dictation is unaffected.",
            ),
        )),
    }
}

/// A permission's name as System Settings lists it, and what Hex cannot do
/// without it.
pub(super) fn permission_warning_copy(kind: PermissionKind) -> (&'static str, &'static str) {
    match kind {
        PermissionKind::Microphone => (
            tr("Microphone"),
            tr("Hex cannot record dictation until microphone access is restored."),
        ),
        PermissionKind::InputMonitoring => (
            tr("Input Monitoring"),
            tr("Hex cannot recognize the dictation shortcut in other apps."),
        ),
        PermissionKind::Accessibility => (
            accessibility_title(),
            tr("Hex cannot paste completed dictation into the foreground app."),
        ),
        PermissionKind::SystemAudio => (
            tr("Screen & System Audio Recording"),
            tr("Required to hear the other participants."),
        ),
    }
}

/// What System Settings calls the Accessibility list; macOS 27 renamed it.
pub(super) fn accessibility_title() -> &'static str {
    if crate::permission_guide::list_name(PermissionKind::Accessibility) == "Accessibility" {
        tr("Accessibility")
    } else {
        tr("Device Control and Data Access")
    }
}

pub(super) const fn permission_warning_id(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "permission-warning-microphone",
        PermissionKind::InputMonitoring => "permission-warning-input-monitoring",
        PermissionKind::Accessibility => "permission-warning-accessibility",
        PermissionKind::SystemAudio => "permission-warning-system-audio",
    }
}

pub(super) fn hotkey_modifiers(modifiers: GpuiModifiers) -> HotkeyModifiers {
    hotkey_modifiers_with_flags(modifiers, crate::suppression::physical_modifier_flags())
}

pub(super) fn hotkey_key(key: &str) -> std::result::Result<HotkeyKey, &'static str> {
    let special = match key {
        "space" => Some((49, "Space")),
        "tab" => Some((48, "Tab")),
        "enter" => Some((36, "Return")),
        "backspace" => Some((51, tr("Delete"))),
        "up" => Some((126, "Up")),
        "down" => Some((125, "Down")),
        "left" => Some((123, tr("Left"))),
        "right" => Some((124, tr("Right"))),
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
        return Err(tr("Unsupported key"));
    };
    if characters.next().is_some() {
        return Err(tr("Unsupported key"));
    }
    let code = crate::keyboard::key_code_for(character).map_err(|_| tr("Unsupported key"))?;
    Ok(HotkeyKey {
        code,
        label: character.to_uppercase().collect(),
    })
}

impl AppWindow {
    pub(super) fn platform_tour_permission(&mut self, effect: crate::onboarding::Effect) {
        use crate::onboarding::Effect;
        match effect {
            Effect::RequestMicrophone => crate::onboarding::request_microphone(),
            Effect::ShowGuide(kind) => crate::permission_guide::show(kind),
            _ => {}
        }
    }

    /// Reopens the installed app after this process quits.
    pub(super) fn platform_relaunch(&mut self) -> bool {
        crate::onboarding::spawn_relaunch()
    }

    /// The voice command model's notice when commands are on but the model
    /// is unavailable.
    pub(super) fn platform_model_notice(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(&'static str, &'static str, AnyElement)> {
        let (title, description) = self
            .settings
            .commands_enabled
            .then(|| command_model_notice(&self.platform.command_model_status))
            .flatten()?;
        let action = div()
            .flex()
            .items_center()
            .gap_2()
            .when(
                matches!(
                    self.platform.command_model_status,
                    CommandModelStatus::Failed
                ),
                |controls| {
                    controls.child(
                        button(ButtonKind::Secondary, tr("Retry"))
                            .id("retry-command-model")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.preview {
                                    cx.dispatch_action(&RetryCommands);
                                }
                            })),
                    )
                },
            )
            .child(
                button(ButtonKind::Secondary, tr("Turn off commands"))
                    .id("disable-unavailable-commands")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Err(error) = this.developer_set_commands_enabled(false, cx) {
                            tracing::warn!(%error, "could not disable voice commands");
                        }
                    })),
            )
            .into_any_element();
        Some((title, description, action))
    }

    pub(crate) fn show_meetings(&mut self, cx: &mut Context<Self>) {
        if self.settings.meetings.available() {
            self.select_pane(Pane::Meetings, cx);
        } else {
            self.show_settings(cx);
        }
    }

    pub(crate) fn open_meeting(&mut self, id: String, cx: &mut Context<Self>) {
        if self.settings.meetings.available() {
            self.select_pane(Pane::Meetings, cx);
            self.select_meeting(id);
            cx.notify();
        } else {
            self.show_settings(cx);
        }
    }

    /// Records a "Never offer" choice from the offer capsule.
    pub fn never_offer_meeting(
        &mut self,
        entry: crate::app_settings::NeverOfferMeeting,
        cx: &mut Context<Self>,
    ) {
        if !self
            .settings
            .meetings
            .never_offer
            .iter()
            .any(|known| known.key == entry.key)
        {
            self.update_settings(cx, |settings| {
                crate::app_settings::insert_never_offer(&mut settings.meetings, entry);
            });
        }
    }

    pub fn meeting_starting(&mut self, cx: &mut Context<Self>) {
        self.platform.meeting_runtime_error = None;
        self.platform.meeting_refresh_started_ms = Some(now_ms());
        self.platform.meeting_stop_pending = false;
        self.pane = Pane::Meetings;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_started(&mut self, cx: &mut Context<Self>) {
        self.platform.meeting_refresh_started_ms = None;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_updated(&mut self, cx: &mut Context<Self>) {
        self.platform.meeting_stop_pending = true;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_updated_notes(&mut self, cx: &mut Context<Self>) {
        self.reload_meetings();
        cx.notify();
    }

    pub(super) fn apple_intelligence_availability(
        &self,
    ) -> crate::apple_intelligence::Availability {
        if self.preview {
            crate::apple_intelligence::Availability::Available
        } else {
            crate::apple_intelligence::availability()
        }
    }

    pub fn meeting_finished(&mut self, cx: &mut Context<Self>) {
        self.platform.meeting_refresh_started_ms = None;
        self.platform.meeting_stop_pending = false;
        self.reload_meetings();
        cx.notify();
    }

    pub fn meeting_failed(&mut self, title: String, error: String, cx: &mut Context<Self>) {
        self.platform.meeting_runtime_error = Some(format!("{title}: {error}"));
        self.platform.meeting_refresh_started_ms = None;
        self.platform.meeting_stop_pending = false;
        self.pane = Pane::Meetings;
        self.reload_meetings();
        if let Some(failed) = self
            .platform
            .meetings
            .iter()
            .find(|meeting| meeting.status == MeetingStatus::Failed && meeting.title == title)
        {
            self.platform.selected_meeting = Some(failed.id.clone());
            self.load_selected_meeting();
        }
        cx.notify();
    }

    pub(super) fn reload_meetings(&mut self) {
        if self.platform.meeting_preview_transcripts.is_some() {
            return;
        }
        match meeting::list() {
            Ok(meetings) => {
                let previous_active =
                    meeting::active(&self.platform.meetings).map(|meeting| &meeting.id);
                let next_active = meeting::active(&meetings).map(|meeting| meeting.id.clone());
                if next_active.as_deref() != previous_active.map(String::as_str)
                    && next_active.is_some()
                {
                    self.platform.selected_meeting = next_active;
                }
                if meeting::active(&meetings).is_none()
                    && (previous_active.is_some()
                        || self
                            .platform
                            .meeting_refresh_started_ms
                            .is_some_and(|started| {
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
                    self.platform.meeting_refresh_started_ms = None;
                }
                self.platform.meetings = meetings;
                let indexed = &self.platform.meetings;
                self.platform.meeting_search_text.retain(|id, _| {
                    indexed
                        .iter()
                        .any(|manifest| &manifest.id == id && !manifest.status.is_active())
                });
                if meeting::active(&self.platform.meetings)
                    .is_none_or(|meeting| meeting.status == MeetingStatus::Transcribing)
                {
                    self.platform.meeting_stop_pending = false;
                }
                self.platform.meetings_error = None;
                if self
                    .platform
                    .selected_meeting
                    .as_ref()
                    .is_some_and(|selected| {
                        !self
                            .platform
                            .meetings
                            .iter()
                            .any(|item| &item.id == selected)
                    })
                {
                    self.platform.selected_meeting = None;
                    self.platform.meeting_transcript = MeetingTranscript::Empty;
                } else if self.platform.selected_meeting.is_some() {
                    self.load_selected_meeting();
                }
            }
            Err(error) => {
                tracing::error!(%error, "could not list meetings");
                self.platform.meetings_error = Some(error.to_string());
            }
        }
    }

    pub(super) fn load_selected_meeting(&mut self) {
        let Some(id) = self.platform.selected_meeting.clone() else {
            self.platform.meeting_transcript = MeetingTranscript::Empty;
            self.platform.meeting_transcript_id = None;
            return;
        };
        let selection_changed = self.platform.meeting_transcript_id.as_deref() != Some(&id);
        if selection_changed {
            self.platform.meeting_transcript_id = Some(id.clone());
            self.platform.meeting_transcript_scroll = ScrollHandle::new();
        }
        let previous_lines = match &self.platform.meeting_transcript {
            MeetingTranscript::Loaded(lines) if !selection_changed => Some(lines.clone()),
            MeetingTranscript::Empty
            | MeetingTranscript::Loaded(_)
            | MeetingTranscript::Unavailable(_) => None,
        };
        let was_following = previous_lines.as_ref().is_none_or(|lines| {
            lines.is_empty()
                || self.platform.meeting_transcript_scroll.max_offset().height
                    + self.platform.meeting_transcript_scroll.offset().y
                    <= px(48.0)
        });
        let entries = match &self.platform.meeting_preview_transcripts {
            Some(fixtures) => Ok(fixtures.get(&id).cloned().unwrap_or_default()),
            None => self.platform.meeting_transcript_reader.read(&id),
        };
        self.platform.meeting_transcript = match entries {
            Ok(entries) => {
                self.platform.meeting_turns = meeting::coalesce_transcript(entries);
                MeetingTranscript::Loaded(transcript_lines(&self.platform.meeting_turns))
            }
            Err(error) => {
                self.platform.meeting_turns.clear();
                MeetingTranscript::Unavailable(error.to_string())
            }
        };
        if !selection_changed
            && matches!(
                &self.platform.meeting_transcript,
                MeetingTranscript::Loaded(lines) if was_following && previous_lines.as_ref() != Some(lines)
            )
        {
            self.platform.meeting_transcript_scroll.scroll_to_bottom();
        }
    }

    pub(super) fn meeting_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, tr("Search meetings"), ""));
        let changed = cx.subscribe(&entity, |this, _, _: &TextChanged, cx| {
            this.refresh_meeting_search(cx);
            cx.notify();
        });
        ProcessingInput {
            entity,
            _subscriptions: vec![changed],
        }
    }

    pub(super) fn meeting_query(&self, cx: &App) -> String {
        self.platform
            .meeting_search
            .entity
            .read(cx)
            .text()
            .trim()
            .to_lowercase()
    }

    /// Loads searchable text for meetings not yet indexed. Runs only while
    /// a query is entered, so idle panes never read transcripts.
    pub(super) fn refresh_meeting_search(&mut self, cx: &App) {
        if self.meeting_query(cx).is_empty() {
            return;
        }
        for manifest in &self.platform.meetings {
            if !self.platform.meeting_search_text.contains_key(&manifest.id) {
                let text = match &self.platform.meeting_preview_transcripts {
                    Some(fixtures) => format!(
                        "{} {}",
                        meeting::display_title(manifest),
                        fixtures
                            .get(&manifest.id)
                            .into_iter()
                            .flatten()
                            .map(|entry| entry.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                    .to_lowercase(),
                    None => meeting::search_text(manifest),
                };
                self.platform
                    .meeting_search_text
                    .insert(manifest.id.clone(), text);
            }
        }
    }

    pub(super) fn visible_meetings(&self, cx: &App) -> Vec<&MeetingManifest> {
        let query = self.meeting_query(cx);
        self.platform
            .meetings
            .iter()
            .filter(|manifest| {
                query.is_empty()
                    || manifest.status.is_active()
                    || self
                        .platform
                        .meeting_search_text
                        .get(&manifest.id)
                        .is_none_or(|text| query.split_whitespace().all(|word| text.contains(word)))
            })
            .collect()
    }

    pub(super) fn select_meeting(&mut self, id: String) {
        if self.platform.selected_meeting.as_deref() != Some(id.as_str()) {
            self.platform.meeting_rename = None;
            self.platform.meeting_delete_armed = None;
            self.platform.meeting_actions_open = false;
            self.platform.meeting_action_error = None;
            self.platform.meeting_copied = None;
        }
        self.platform.selected_meeting = Some(id);
        self.load_selected_meeting();
    }

    pub(super) fn copy_meeting_text(
        &mut self,
        id: String,
        what: &'static str,
        text: String,
        cx: &mut Context<Self>,
    ) {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                self.platform.meeting_copied = Some((id, what));
                self.platform.meeting_action_error = None;
            }
            Err(error) => self.platform.meeting_action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn trash_meeting(&mut self, id: String, cx: &mut Context<Self>) {
        if self.platform.meeting_delete_armed.as_deref() != Some(id.as_str()) {
            self.platform.meeting_delete_armed = Some(id);
            self.platform.meeting_rename = None;
            cx.notify();
            return;
        }
        self.platform.meeting_delete_armed = None;
        let result = if self.preview {
            Ok(())
        } else {
            meeting::trash(&id)
        };
        match result {
            Ok(()) => {
                self.platform.meeting_action_error = None;
                self.platform.meeting_search_text.remove(&id);
                if self.preview {
                    self.platform.meetings.retain(|manifest| manifest.id != id);
                } else {
                    self.reload_meetings();
                }
                let next = self
                    .platform
                    .meetings
                    .first()
                    .map(|manifest| manifest.id.clone());
                self.platform.selected_meeting = None;
                if let Some(next) = next {
                    self.select_meeting(next);
                } else {
                    self.load_selected_meeting();
                }
            }
            Err(error) => self.platform.meeting_action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn start_meeting_rename(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.new(|cx| TextInput::new(cx, tr("Meeting title"), &title));
        let submitted = cx.subscribe(&entity, |this, _, _: &TextSubmitted, cx| {
            this.finish_meeting_rename(cx);
        });
        let dismissed = cx.subscribe(&entity, |this, _, _: &TextDismissed, cx| {
            this.platform.meeting_rename = None;
            cx.notify();
        });
        entity.focus_handle(cx).focus(window);
        self.platform.meeting_delete_armed = None;
        self.platform.meeting_rename = Some((
            id,
            ProcessingInput {
                entity,
                _subscriptions: vec![submitted, dismissed],
            },
        ));
        cx.notify();
    }

    pub(super) fn finish_meeting_rename(&mut self, cx: &mut Context<Self>) {
        let Some((id, input)) = self.platform.meeting_rename.take() else {
            return;
        };
        let title = input.entity.read(cx).text().to_string();
        let result = if self.preview {
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            match self
                .platform
                .meetings
                .iter_mut()
                .find(|manifest| manifest.id == id)
            {
                Some(manifest) if !title.is_empty() => {
                    manifest.title = title;
                    Ok(())
                }
                _ => Err(color_eyre::eyre::eyre!("The title is empty")),
            }
        } else {
            meeting::rename(&id, &title).map(|_| ())
        };
        match result {
            Ok(()) => self.platform.meeting_action_error = None,
            Err(error) => self.platform.meeting_action_error = Some(format!("{error:#}")),
        }
        self.platform.meeting_search_text.remove(&id);
        if !self.preview {
            self.reload_meetings();
        }
        cx.notify();
    }

    pub(super) fn load_meeting_preview(&mut self, preview: &MeetingPreview) {
        let (meetings, transcripts) = preview_meetings(preview.empty);
        self.platform.meetings = meetings;
        self.platform.meeting_preview_transcripts = Some(transcripts);
        let selected = if preview.recording {
            meeting::active(&self.platform.meetings)
        } else {
            self.platform
                .meetings
                .iter()
                .find(|manifest| manifest.status == MeetingStatus::Complete)
        }
        .map(|manifest| manifest.id.clone());
        if let Some(id) = selected {
            self.select_meeting(id.clone());
            if preview.confirm_delete {
                self.platform.meeting_delete_armed = Some(id);
            }
            self.platform.meeting_actions_open = preview.open_actions;
        }
    }

    pub(super) fn render_meeting_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let enabled = self.settings.meetings.enabled;
        let position = self.platform.meetings_toggle.render_position(window);
        let indicator_position = self
            .platform
            .meeting_indicator_toggle
            .render_position(window);
        let notes_position = self.platform.meeting_notes_toggle.render_position(window);
        let folder = self.settings.meetings.resolved_output_folder();
        let folder_label = folder
            .strip_prefix(dirs::home_dir().unwrap_or_default())
            .map(|relative| format!("~/{}", relative.display()))
            .unwrap_or_else(|_| folder.display().to_string());
        let system_audio = permission_control(
            if self.platform.screen_recording_permitted {
                PermissionControl::Allowed
            } else if self
                .platform
                .permissions_asked
                .contains(&PermissionKind::SystemAudio)
            {
                PermissionControl::Waiting
            } else {
                PermissionControl::Allow
            },
            ButtonKind::Secondary,
            "meeting-system-audio",
            cx.listener(|this, _, _, cx| {
                // macOS prompts once; after that the switch is in
                // System Settings, with the guide beside it.
                if !this.preview && !crate::meeting::request_system_audio_access() {
                    crate::permission_guide::show(PermissionKind::SystemAudio);
                }
                if !this
                    .platform
                    .permissions_asked
                    .contains(&PermissionKind::SystemAudio)
                {
                    this.platform
                        .permissions_asked
                        .push(PermissionKind::SystemAudio);
                }
                this.permission_refresh_at = Instant::now();
                cx.notify();
            }),
        );
        panel()
            .child(
                settings_row(
                    tr("Meetings (beta)"),
                    tr("Offer to record calls and save local transcripts"),
                    toggle(position),
                )
                .when(!enabled, |row| row.border_b_0())
                .id("meetings-setting")
                .on_click(cx.listener(|this, _, _, cx| {
                    let enabled = !this.settings.meetings.enabled;
                    if this.update_settings(cx, |settings| settings.meetings.enabled = enabled) {
                        this.platform.meetings_toggle.set_enabled(enabled);
                        if enabled {
                            this.platform.screen_recording_permitted =
                                this.preview || crate::meeting::system_audio_ready();
                            if !this.preview {
                                this.reload_meetings();
                            }
                        } else if this.pane == Pane::Meetings {
                            this.pane = Pane::Settings;
                        }
                    }
                })),
            )
            .when(enabled, |panel| {
                panel
                    .child(settings_row(
                        tr("Screen & System Audio Recording"),
                        if self.platform.screen_recording_permitted {
                            tr("Captures the other participants' audio")
                        } else {
                            tr("Required to hear the other participants.")
                        },
                        system_audio,
                    ))
                    .child(
                        settings_row(
                            tr("Show recording indicator"),
                            tr("Floating pill to copy live transcript turns or stop recording"),
                            toggle(indicator_position),
                        )
                        .id("meeting-indicator-setting")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let show = !this.settings.meetings.show_recording_indicator;
                            if this.update_settings(cx, |settings| {
                                settings.meetings.show_recording_indicator = show
                            }) {
                                this.platform.meeting_indicator_toggle.set_enabled(show);
                            }
                        })),
                    )
                    .child(settings_row(
                        tr("Transcript folder"),
                        tr("Finished meetings are saved here as Markdown"),
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                button(ButtonKind::Secondary, tr("Reveal"))
                                    .id("reveal-meeting-folder")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let folder =
                                            this.settings.meetings.resolved_output_folder();
                                        match std::fs::create_dir_all(&folder) {
                                            Ok(()) => crate::meeting::reveal(&folder),
                                            Err(error) => {
                                                this.platform.meeting_folder_error =
                                                    Some(error.to_string());
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                disclosure_button(folder_label)
                                    .id("meeting-folder-setting")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.choose_meeting_folder(cx);
                                    })),
                            ),
                    ))
                    .when(!self.settings.meetings.never_offer.is_empty(), |panel| {
                        panel.child(settings_row(
                            tr("Never offer"),
                            tr("No recording offers for these sites and apps"),
                            div().flex().flex_col().items_end().gap_1().children(
                                self.settings.meetings.never_offer.iter().enumerate().map(
                                    |(index, entry)| {
                                        let key = entry.key.clone();
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_footnote()
                                                    .text_color(rgb(TEXT_SECONDARY))
                                                    .child(entry.label.clone()),
                                            )
                                            .child(
                                                small_button(ButtonKind::Secondary, tr("Remove"))
                                                    .id(("never-offer-remove", index))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            let key = key.clone();
                                                            this.update_settings(cx, |settings| {
                                                                settings
                                                                    .meetings
                                                                    .never_offer
                                                                    .retain(|entry| {
                                                                        entry.key != key
                                                                    })
                                                            });
                                                        },
                                                    )),
                                            )
                                    },
                                ),
                            ),
                        ))
                    })
                    .child(
                        settings_row(
                            tr("Summarize with Apple Intelligence"),
                            self.apple_intelligence_availability().description(),
                            toggle(notes_position),
                        )
                        .border_b_0()
                        .id("meeting-notes-setting")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let enabled = !this.settings.meetings.summarize_with_apple_intelligence;
                            if this.update_settings(cx, |settings| {
                                settings.meetings.summarize_with_apple_intelligence = enabled
                            }) {
                                this.platform.meeting_notes_toggle.set_enabled(enabled);
                            }
                        })),
                    )
                    .when_some(
                        self.platform.meeting_folder_error.clone(),
                        |panel, error| panel.child(inline_error(error)),
                    )
            })
    }

    pub(super) fn choose_meeting_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("Use Folder").into()),
        });
        cx.spawn(async move |window, cx| {
            let result = receiver
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = window.update(cx, |window, cx| match result {
                Ok(Some(paths)) => {
                    if let Some(folder) = paths.into_iter().next() {
                        window.platform.meeting_folder_error = None;
                        window.update_settings(cx, |settings| {
                            settings.meetings.output_folder = Some(folder);
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    window.platform.meeting_folder_error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn render_meetings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active_meeting = meeting::active(&self.platform.meetings);
        let action = match active_meeting.map(|meeting| meeting.status) {
            _ if self.platform.meeting_stop_pending => {
                busy_label(tr("Finalizing…")).into_any_element()
            }
            Some(MeetingStatus::Transcribing) => busy_label(tr("Finalizing…")).into_any_element(),
            Some(MeetingStatus::Starting) => busy_label(tr("Starting…")).into_any_element(),
            // Recording is a state, not the screen's main action: a neutral
            // button in the same red-dot language as the Recording tag.
            Some(MeetingStatus::Recording) => button(ButtonKind::Secondary, status_dot(DANGER))
                .child(tr("Stop & transcribe"))
                .children(active_meeting.map(|meeting| {
                    div()
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(meeting::format_duration(meeting_duration(meeting)))
                }))
                .id("stop-meeting")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.platform.meeting_stop_pending = true;
                    if let Err(error) = this
                        .platform
                        .meeting_requests
                        .try_send(MeetingRequest::Stop)
                    {
                        this.platform.meeting_stop_pending = false;
                        log_meeting_request_error(error);
                    }
                    cx.notify();
                }))
                .into_any_element(),
            _ if self.platform.meeting_refresh_started_ms.is_some() => {
                busy_label(tr("Starting…")).into_any_element()
            }
            _ => button(ButtonKind::Primary, tr("Start recording"))
                .id("start-meeting")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.platform.meeting_refresh_started_ms = Some(now_ms());
                    if let Err(error) = this
                        .platform
                        .meeting_requests
                        .try_send(MeetingRequest::Start)
                    {
                        this.platform.meeting_refresh_started_ms = None;
                        log_meeting_request_error(error);
                    }
                    cx.notify();
                }))
                .into_any_element(),
        };
        let header_action = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(SEARCH_FIELD_WIDTH))
                    .child(self.platform.meeting_search.entity.clone()),
            )
            .child(action)
            .into_any_element();

        let now = now_ms();
        let visible = self
            .visible_meetings(cx)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let searching = !self.meeting_query(cx).is_empty();
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut current_day = None;
        for (index, meeting) in visible.iter().enumerate() {
            let day = meeting::day_label(meeting.created_at_ms, now);
            if current_day.as_ref() != Some(&day) {
                rows.push(list_section_label(day.clone(), rows.is_empty()).into_any_element());
                current_day = Some(day);
            }
            let id = meeting.id.clone();
            let selected = self.platform.selected_meeting.as_deref() == Some(id.as_str());
            let row_copied = self
                .platform
                .meeting_copied
                .as_ref()
                .is_some_and(|(copied_id, what)| copied_id == &id && *what == "row-transcript");
            let can_copy_row = meeting.status == MeetingStatus::Complete;
            let row_manifest = meeting.clone();
            let row_copy_id = id.clone();
            let mut meta = vec![
                meeting::clock(meeting.created_at_ms),
                meeting_length(meeting_duration(meeting)),
            ];
            meta.extend(meeting.app.clone());
            rows.push(
                list_row(selected)
                    .id(("meeting", index))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                list_row_title(meeting::display_title(meeting), selected).flex_1(),
                            )
                            .when_some(
                                meeting_status_chip(
                                    meeting.status,
                                    self.platform.meeting_stop_pending,
                                    false,
                                ),
                                |row, chip| row.child(chip),
                            )
                            .when(can_copy_row, |row| {
                                row.child(
                                    small_button(
                                        ButtonKind::Secondary,
                                        if row_copied { tr("Copied") } else { tr("Copy") },
                                    )
                                    .id(("meeting-row-copy", index))
                                    .w(px(MEETING_ROW_COPY_WIDTH))
                                    .justify_center()
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let markdown = match &this
                                                .platform
                                                .meeting_preview_transcripts
                                            {
                                                Some(fixtures) => fixtures
                                                    .get(&row_copy_id)
                                                    .map(|entries| {
                                                        let turns = meeting::coalesce_transcript(
                                                            entries.clone(),
                                                        );
                                                        meeting::transcript_markdown(&turns)
                                                    })
                                                    .ok_or_else(|| {
                                                        color_eyre::eyre::eyre!(
                                                            "no meeting transcript is available"
                                                        )
                                                    }),
                                                None => {
                                                    meeting::speaker_turns_markdown_for_manifest(
                                                        &row_manifest,
                                                    )
                                                }
                                            };
                                            match markdown {
                                                Ok(text) => this.copy_meeting_text(
                                                    row_copy_id.clone(),
                                                    "row-transcript",
                                                    text,
                                                    cx,
                                                ),
                                                Err(error) => {
                                                    this.platform.meeting_action_error =
                                                        Some(error.to_string());
                                                    cx.notify();
                                                }
                                            }
                                        },
                                    )),
                                )
                            }),
                    )
                    .child(list_row_meta(meta.join(" · "), selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_meeting(id.clone());
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }

        let list = pane_list(
            "meetings-list",
            PANE_LIST_WIDTH,
            (visible.is_empty() && self.platform.meetings_error.is_none()).then_some(
                if searching {
                    tr("No meetings match.")
                } else {
                    tr("No meetings yet.")
                },
            ),
            tr("Meetings could not be loaded."),
            self.platform.meetings_error.clone(),
            rows,
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action(tr("Meetings"), Some(header_action)))
            .child(
                pane_body().child(
                    pane_content()
                        .id("meetings-workspace")
                        .gap_3()
                        .when_some(
                            self.platform.meeting_runtime_error.clone(),
                            |column, error| {
                                column.child(
                                    error_panel(tr("Meeting could not be recorded."), error)
                                        .flex_none(),
                                )
                            },
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(0.0))
                                .flex()
                                .gap_5()
                                .child(list)
                                .child(pane_detail(self.render_meeting_detail(window, cx))),
                        ),
                ),
            )
            .into_any_element()
    }

    pub(super) fn render_meeting_detail(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(meeting) = self
            .platform
            .selected_meeting
            .as_deref()
            .and_then(|id| {
                self.platform
                    .meetings
                    .iter()
                    .find(|meeting| meeting.id == id)
            })
            .cloned()
        else {
            return detail_placeholder(if self.platform.meetings.is_empty() {
                tr("Record a call to see its notes and transcript here.")
            } else {
                tr("Select a meeting.")
            });
        };
        let id = meeting.id.clone();
        let active = meeting.status.is_active();
        let recording =
            meeting.status == MeetingStatus::Recording && !self.platform.meeting_stop_pending;
        let title = meeting::display_title(&meeting);
        let copied = |what: &'static str| {
            self.platform
                .meeting_copied
                .as_ref()
                .is_some_and(|(copied_id, copied)| copied_id == &id && *copied == what)
        };
        let action_of = |kind: ButtonKind, key: &'static str, label: &'static str| {
            button(kind, label).id(SharedString::from(format!("meeting-action-{key}")))
        };
        let action =
            |key: &'static str, label: &'static str| action_of(ButtonKind::Secondary, key, label);
        let menu_action = |key: &'static str, item: Div| {
            item.id(SharedString::from(format!("meeting-action-{key}")))
        };

        // Copy transcript and Rename stay visible; the rest live in the
        // overflow menu, with Delete last.
        let mut actions: Vec<AnyElement> = Vec::new();
        let mut menu_items: Vec<AnyElement> = Vec::new();
        if let Some(notes) = meeting.notes.clone() {
            let id = id.clone();
            menu_items.push(
                menu_action(
                    "copy-notes",
                    menu_item(false).child(if copied("notes") {
                        tr("Copied")
                    } else {
                        tr("Copy summary")
                    }),
                )
                // Stays open so "Copied" confirms the copy.
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.copy_meeting_text(id.clone(), "notes", meeting::notes_markdown(&notes), cx)
                }))
                .into_any_element(),
            );
        }
        if !self.platform.meeting_turns.is_empty() {
            let id = id.clone();
            let turns = self.platform.meeting_turns.clone();
            actions.push(
                action(
                    "copy-transcript",
                    if copied("transcript") {
                        tr("Copied")
                    } else {
                        tr("Copy transcript")
                    },
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.copy_meeting_text(
                        id.clone(),
                        "transcript",
                        meeting::transcript_markdown(&turns),
                        cx,
                    )
                }))
                .into_any_element(),
            );
        }
        if !active && self.platform.meeting_rename.is_none() {
            let (id, title) = (id.clone(), title.clone());
            actions.push(
                action("rename", tr("Rename"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_meeting_rename(id.clone(), title.clone(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        if !active {
            let export_path = meeting.export_path.clone().filter(|path| path.exists());
            let reveal_id = id.clone();
            menu_items.push(
                menu_action(
                    "reveal",
                    menu_item(false).child(if export_path.is_some() {
                        tr("Reveal Markdown")
                    } else {
                        tr("Reveal files")
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.platform.meeting_actions_open = false;
                    reveal_meeting(&reveal_id, export_path.as_deref());
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        let notes_running = meeting::notes_in_progress(&meeting.id);
        if meeting.status == MeetingStatus::Complete
            && self.apple_intelligence_availability().is_available()
        {
            if notes_running {
                menu_items.push(
                    disabled_menu_item()
                        .child(tr("Summarizing…"))
                        .into_any_element(),
                );
            } else {
                let id = id.clone();
                menu_items.push(
                    menu_action(
                        "notes",
                        menu_item(false).child(if meeting.notes.is_some() {
                            tr("Regenerate summary")
                        } else {
                            tr("Summarize")
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.platform.meeting_actions_open = false;
                        if let Err(error) = this
                            .platform
                            .meeting_requests
                            .try_send(MeetingRequest::Summarize(id.clone()))
                        {
                            log_meeting_request_error(error);
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                );
            }
        }
        let armed = self.platform.meeting_delete_armed.as_deref() == Some(id.as_str());
        if !active && !notes_running && !armed {
            let delete_id = id.clone();
            if !menu_items.is_empty() {
                menu_items.push(menu_separator().into_any_element());
            }
            menu_items.push(
                menu_action("delete", danger_menu_item().child(tr("Delete…")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.platform.meeting_actions_open = false;
                        this.trash_meeting(delete_id.clone(), cx)
                    }))
                    .into_any_element(),
            );
        }
        if !menu_items.is_empty() {
            let trigger = icon_button(ButtonKind::Secondary, Icon::Ellipsis)
                .id("meeting-action-more")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.platform.meeting_actions_open = true;
                    cx.notify();
                }));
            let menu_progress = window.spring_when(
                "meeting-actions-menu-progress",
                self.platform.meeting_actions_open,
                SPRING_SNAP,
            );
            actions.push(
                div()
                    .relative()
                    .child(trigger)
                    .when(self.platform.meeting_actions_open, |more| {
                        more.child(dropdown(
                            DropdownAlign::Left,
                            menu_progress,
                            menu()
                                .id("meeting-actions-menu")
                                .debug_selector(|| "meeting-actions-menu".into())
                                .w(px(MENU_WIDTH))
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.platform.meeting_actions_open = false;
                                    cx.notify();
                                }))
                                .children(menu_items),
                        ))
                    })
                    .into_any_element(),
            );
        }
        if !active && !notes_running && armed {
            // The confirmation takes the whole row until it is answered.
            actions.clear();
            let delete_id = id.clone();
            actions.push(
                action_of(
                    ButtonKind::Danger,
                    "delete",
                    if meeting
                        .export_path
                        .as_ref()
                        .is_some_and(|path| path.exists())
                    {
                        tr("Trash recording & Markdown")
                    } else {
                        tr("Trash recording")
                    },
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.trash_meeting(delete_id.clone(), cx)),
                )
                .into_any_element(),
            );
            actions.push(
                action("cancel-delete", tr("Cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.platform.meeting_delete_armed = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }

        let title_row: AnyElement = match &self.platform.meeting_rename {
            Some((rename_id, input)) if rename_id == &id => div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w(px(0.0)).child(input.entity.clone()))
                .child(
                    action("save-title", tr("Save"))
                        .on_click(cx.listener(|this, _, _, cx| this.finish_meeting_rename(cx))),
                )
                .child(action("cancel-title", tr("Cancel")).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.platform.meeting_rename = None;
                        cx.notify();
                    },
                )))
                .into_any_element(),
            _ => detail_title(title).into_any_element(),
        };

        let mut meta = vec![
            meeting::day_label(meeting.created_at_ms, now_ms()),
            meeting::clock(meeting.created_at_ms),
            meeting::format_duration(meeting_duration(&meeting)),
        ];
        meta.extend(meeting.app.clone());

        let errors = [
            self.platform
                .meeting_action_error
                .clone()
                .map(|error| (tr("Meeting could not be updated."), error)),
            meeting
                .error
                .clone()
                .map(|error| (tr("Recording failed."), error)),
            meeting
                .export_error
                .clone()
                .map(|error| (tr("Markdown not saved."), error)),
            meeting
                .notes_error
                .clone()
                .filter(|_| !notes_running)
                .map(|error| (tr("Notes could not be written."), error)),
            meeting
                .live_transcription_error
                .clone()
                .filter(|_| active)
                .map(|error| (tr("Live draft unavailable."), error)),
        ]
        .into_iter()
        .flatten();

        let notes = meeting.notes.clone().map(render_meeting_notes).or_else(|| {
            notes_running.then(|| {
                div()
                    .pb_5()
                    .text_footnote()
                    .text_color(rgb(TEXT_TERTIARY))
                    .child(tr("Writing a title and summary with Apple Intelligence…"))
            })
        });

        let transcript = match &self.platform.meeting_transcript {
            MeetingTranscript::Empty => empty_state(tr("No transcript selected.")),
            MeetingTranscript::Loaded(lines) if lines.is_empty() => {
                empty_state(match meeting.status {
                    MeetingStatus::Starting => tr("Preparing local audio capture…"),
                    MeetingStatus::Recording if meeting.live_transcription_error.is_some() => {
                        tr("Live transcription is unavailable. Audio is still recording safely.")
                    }
                    MeetingStatus::Recording if self.platform.meeting_stop_pending => {
                        tr("Building the final transcript…")
                    }
                    MeetingStatus::Recording => tr("Listening for speech…"),
                    MeetingStatus::Transcribing => tr("Building the final transcript…"),
                    MeetingStatus::Complete => tr("No speech was transcribed."),
                    MeetingStatus::Interrupted | MeetingStatus::Failed => {
                        tr("No recoverable transcript is available.")
                    }
                })
            }
            MeetingTranscript::Loaded(lines) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .children(
                    lines
                        .iter()
                        .enumerate()
                        .map(|(index, line)| transcript_turn(index, line)),
                )
                .when(recording, |turns| {
                    turns.child(
                        div()
                            .pt_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_caption()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(status_dot(DANGER))
                            .child(tr("Listening…")),
                    )
                })
                .into_any_element(),
            MeetingTranscript::Unavailable(error) => {
                error_message(tr("Transcript unavailable."), error.clone())
            }
        };
        let transcript_label = match meeting.status {
            MeetingStatus::Starting | MeetingStatus::Recording
                if !self.platform.meeting_stop_pending =>
            {
                tr("Live transcript")
            }
            MeetingStatus::Starting | MeetingStatus::Recording | MeetingStatus::Transcribing => {
                tr("Draft transcript")
            }
            MeetingStatus::Complete => tr("Transcript"),
            MeetingStatus::Interrupted | MeetingStatus::Failed => tr("Recovered draft"),
        };

        div()
            .debug_selector(|| "meeting-detail".into())
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(detail_header(
                title_row,
                meeting_status_chip(meeting.status, self.platform.meeting_stop_pending, true)
                    .map(IntoElement::into_any_element)
                    .into_iter()
                    .chain([truncated_line(FOOTNOTE, meta.join(" · "))
                        .flex_1()
                        .into_any_element()])
                    .collect(),
                actions,
            ))
            .child(
                detail_body("meeting-transcript")
                    .track_scroll(&self.platform.meeting_transcript_scroll)
                    .children(errors.map(|(message, error)| error_panel(message, error).mb_4()))
                    .when_some(notes, |content, notes| content.child(notes))
                    .child(
                        div()
                            .pb_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(section_label(transcript_label))
                            .when(recording, |label| label.child(status_dot(DANGER))),
                    )
                    .child(transcript),
            )
            .into_any_element()
    }

    pub(super) fn render_commands(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let config_path = self.personal_workspace.join("hex.config.ts");
        let workspace_state = personal_workspace_state(
            !self.preview && self.personal_config_exists,
            self.personal_workspace_receiver.is_some(),
            &self.personal_commands_status,
        );
        let last_error = self
            .personal_workspace_error
            .clone()
            .or_else(|| self.personal_commands_status.last_reload_error.clone());
        let open_config = config_path.clone();
        let config_control = match workspace_state {
            PersonalWorkspaceState::Missing => button(ButtonKind::Secondary, tr("Create config"))
                .id("personal-commands-create")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.provision_personal_workspace();
                    cx.notify();
                }))
                .into_any_element(),
            PersonalWorkspaceState::Creating => {
                busy_label(tr("Creating config…")).into_any_element()
            }
            PersonalWorkspaceState::Ready | PersonalWorkspaceState::Error => {
                button(ButtonKind::Secondary, tr("Edit config"))
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
                "Help me configure my Hex custom voice commands.\n\n\
                 Workspace: {workspace}\n\
                 Config: {config}\n\n\
                 Read AGENTS.md and .agents/skills/personal-commands/SKILL.md in the workspace \
                 before editing. Ask me what commands or dictation phrases I want, update \
                 hex.config.ts, and run `bun run check` from the workspace when finished. You \
                 may add npm packages when a command needs them."
            );
            button(
                ButtonKind::Secondary,
                if copied {
                    tr("Copied")
                } else {
                    tr("Copy agent prompt")
                },
            )
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
            button(
                ButtonKind::Primary,
                tr("Keep microphone ready & enable commands"),
            )
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
                .gap_2()
                .child(
                    div()
                        .text_footnote()
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(tr("Voice commands")),
                )
                .child(toggle(commands_position))
                .into_any_element()
        };
        let commands_control = div()
            .flex()
            .items_center()
            .gap_2()
            .child(config_control)
            .when_some(copy_prompt, |controls, prompt| controls.child(prompt))
            .child(div().mx_1().w(px(1.0)).h_5().bg(rgb(LINE)))
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
            .platform
            .command_search
            .entity
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        let visible: Vec<usize> = self
            .platform
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| Self::command_matches(command, &query).then_some(index))
            .collect();
        let mut tasks = Vec::new();
        for index in &visible {
            let task = self.platform.commands[*index].task.clone();
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
        let effective_selected = self
            .platform
            .selected_command
            .clone()
            .filter(|id| {
                visible
                    .iter()
                    .any(|index| self.platform.commands[*index].id == *id)
            })
            .or_else(|| {
                visible
                    .first()
                    .map(|index| self.platform.commands[*index].id.clone())
            });
        let mut groups: Vec<AnyElement> = Vec::new();
        for task in tasks {
            groups.push(list_section_label(task.clone(), groups.is_empty()).into_any_element());
            groups.extend(
                visible
                    .iter()
                    .copied()
                    .filter(|index| self.platform.commands[*index].task == task)
                    .map(|index| {
                        let command = self.platform.commands[index].clone();
                        let command_id = command.id.clone();
                        let selected = effective_selected.as_deref() == Some(command_id.as_str());
                        list_row(selected)
                            .id(("command", index))
                            .gap_0p5()
                            .child(list_row_title(command.phrase, selected))
                            .child(list_row_meta(command.description, selected))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.platform.selected_command = Some(command_id.clone());
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            );
        }

        let no_commands = groups.is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header_with_action(
                tr("Commands"),
                Some(commands_control),
            ))
            .child(
                pane_body().child(
                    pane_content()
                        .gap_3()
                        .when_some(last_error, |column, error| {
                            column.child(
                                error_panel(tr("Personal commands are not loading."), error)
                                    .flex_none(),
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
                                        .w(px(PANE_LIST_WIDTH))
                                        .h_full()
                                        .flex_none()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_none()
                                                .child(self.platform.command_search.entity.clone()),
                                        )
                                        .child(
                                            pane_list(
                                                "commands-list",
                                                PANE_LIST_WIDTH,
                                                no_commands.then_some(if query.is_empty() {
                                                    tr("No commands yet.")
                                                } else {
                                                    tr("No matching commands.")
                                                }),
                                                "",
                                                None,
                                                groups,
                                            )
                                            .flex_1()
                                            .min_h(px(0.0)),
                                        ),
                                )
                                .child(
                                    pane_detail(self.render_command_detail(
                                        effective_selected.as_deref(),
                                        &context_label,
                                    ))
                                    .id("command-detail"),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    pub(super) fn render_command_detail(
        &self,
        selected: Option<&str>,
        context_label: &str,
    ) -> AnyElement {
        let Some(command) = selected.and_then(|id| {
            self.platform
                .commands
                .iter()
                .find(|command| command.id == id)
        }) else {
            return detail_placeholder(tr("Select a command."));
        };
        let aliases = if command.aliases.is_empty() {
            tr("None").into()
        } else {
            command.aliases.join("\n")
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(detail_header(
                detail_title(command.phrase.clone()),
                vec![command.description.clone().into_any_element()],
                Vec::new(),
            ))
            .child(
                detail_body("command-detail-body")
                    .pt_1()
                    .child(detail_row(tr("Task"), command.task.clone()))
                    .child(detail_row(tr("Context"), command_scope(&command.scope)))
                    .child(detail_row(tr("Aliases"), aliases))
                    .child(detail_row(tr("Current context"), context_label.to_string())),
            )
            .into_any_element()
    }

    pub(super) fn command_search_input(cx: &mut Context<Self>) -> ProcessingInput {
        let entity = cx.new(|cx| TextInput::picker(cx, tr("Search commands"), ""));
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
    pub(super) fn command_matches(command: &CatalogCommand, query: &str) -> bool {
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

    pub(super) fn current_context_label(&self) -> String {
        let (application, browser_host) = self.current_context();
        match (application, browser_host) {
            (Some(application), Some(host)) => format!("{application} / {host}"),
            (Some(application), None) => application,
            _ => tr("Unavailable").into(),
        }
    }

    pub(super) fn apply_hud_lab(&self) {
        self.platform
            .indicator
            .send(DictationIndicatorEvent::Configure(self.platform.hud_tuning));
        self.platform.indicator.send(DictationIndicatorEvent::Reset);
        match self.platform.hud_demo {
            HudDemoState::Recording => {
                for job_id in 0..self.platform.hud_queued as u64 {
                    self.platform
                        .indicator
                        .send(DictationIndicatorEvent::Submitted { job_id });
                    self.platform
                        .indicator
                        .send(DictationIndicatorEvent::Transcribing { job_id });
                }
                self.platform
                    .indicator
                    .send(DictationIndicatorEvent::Started);
                self.platform
                    .indicator
                    .send(DictationIndicatorEvent::Meter {
                        average: 0.08,
                        peak: 0.42,
                    });
            }
            HudDemoState::Transcribing | HudDemoState::Processing => {
                self.platform
                    .indicator
                    .send(DictationIndicatorEvent::Submitted { job_id: 0 });
                self.platform.indicator.send(match self.platform.hud_demo {
                    HudDemoState::Transcribing => {
                        DictationIndicatorEvent::Transcribing { job_id: 0 }
                    }
                    HudDemoState::Processing => DictationIndicatorEvent::Processing { job_id: 0 },
                    HudDemoState::Recording => unreachable!(),
                });
                for job_id in 1..=self.platform.hud_queued as u64 {
                    self.platform
                        .indicator
                        .send(DictationIndicatorEvent::Submitted { job_id });
                    self.platform
                        .indicator
                        .send(DictationIndicatorEvent::Transcribing { job_id });
                }
            }
        }
    }

    pub(super) fn hud_control_value(&self, control: HudControl) -> f32 {
        match control {
            HudControl::LineCount => (self.platform.hud_tuning.line_count - 1.0) / 5.0,
            HudControl::Curvature => self.platform.hud_tuning.curvature,
            HudControl::Speed => self.platform.hud_tuning.speed / HUD_MAX_SPEED,
            HudControl::Blur => 1.0 - self.platform.hud_tuning.sharpness,
            HudControl::Glow => self.platform.hud_tuning.glow,
            HudControl::Depth => self.platform.hud_tuning.depth,
            HudControl::LightAngle => self.platform.hud_tuning.light_angle,
            HudControl::Outline => self.platform.hud_tuning.outline,
        }
        .clamp(0.0, 1.0)
    }

    pub(super) fn set_hud_control(
        &mut self,
        control: HudControl,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        let value = value.clamp(0.0, 1.0);
        match control {
            HudControl::LineCount => {
                self.platform.hud_tuning.line_count = (1.0 + value * 5.0).round()
            }
            HudControl::Curvature => self.platform.hud_tuning.curvature = value,
            HudControl::Speed => self.platform.hud_tuning.speed = value * HUD_MAX_SPEED,
            HudControl::Blur => self.platform.hud_tuning.sharpness = 1.0 - value,
            HudControl::Glow => self.platform.hud_tuning.glow = value,
            HudControl::Depth => self.platform.hud_tuning.depth = value,
            HudControl::LightAngle => self.platform.hud_tuning.light_angle = value,
            HudControl::Outline => self.platform.hud_tuning.outline = value,
        }
        self.platform.hud_copied = false;
        self.platform
            .indicator
            .send(DictationIndicatorEvent::Configure(self.platform.hud_tuning));
        cx.notify();
    }

    pub(super) fn update_hud_from_pointer(
        &mut self,
        control: HudControl,
        pointer_x: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        const SLIDER_WIDTH: f32 = 360.0;
        let available = (f32::from(window.viewport_size().width) - SIDEBAR_WIDTH - 64.0).max(0.0);
        let content_width = available.min(PANE_CONTENT_WIDTH);
        let slider_left = SIDEBAR_WIDTH + 32.0 + (available - content_width) / 2.0;
        self.set_hud_control(control, (pointer_x - slider_left) / SLIDER_WIDTH, cx);
    }

    pub(super) fn select_hud_style(&mut self, style: usize, cx: &mut Context<Self>) {
        self.platform.hud_tuning = match style {
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
            1 => HudTuning::default(),
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
        self.platform.hud_copied = false;
        self.apply_hud_lab();
        cx.notify();
    }

    pub(super) fn hud_preset(&self) -> String {
        format!(
            "hud-v1 style={} lines={:.0} curvature={:.2} speed={:.2} blur={:.2} glow={:.2} depth={:.2} light={:.2} outline={:.2}",
            self.platform.hud_tuning.style as usize,
            self.platform.hud_tuning.line_count,
            self.platform.hud_tuning.curvature,
            self.platform.hud_tuning.speed,
            1.0 - self.platform.hud_tuning.sharpness,
            self.platform.hud_tuning.glow,
            self.platform.hud_tuning.depth,
            self.platform.hud_tuning.light_angle,
            self.platform.hud_tuning.outline,
        )
    }

    pub(super) fn render_hud_slider(
        &self,
        control: HudControl,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = self.hud_control_value(control);
        let display = match control {
            HudControl::LineCount => format!("{:.0}", self.platform.hud_tuning.line_count),
            HudControl::Speed => format!("{:.2}x", self.platform.hud_tuning.speed),
            HudControl::LightAngle => {
                format!("{:.0}°", self.platform.hud_tuning.light_angle * 360.0)
            }
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
                    .text_footnote()
                    .child(label)
                    .child(div().text_color(rgb(TEXT_TERTIARY)).child(display)),
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
                            .bg(rgb(SURFACE_SELECTED)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(7.0))
                            .w(relative(value))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgb(ACCENT)),
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
                            .border_color(rgb(TEXT_PRIMARY))
                            .bg(rgb(SURFACE_SELECTED)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.platform.hud_dragging = Some(control);
                            this.update_hud_from_pointer(
                                control,
                                f32::from(event.position.x),
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_mouse_move(
                        cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                            if event.dragging() && this.platform.hud_dragging == Some(control) {
                                this.update_hud_from_pointer(
                                    control,
                                    f32::from(event.position.x),
                                    window,
                                    cx,
                                );
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_hud_lab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    let selected = self.platform.hud_tuning.style as usize == index;
                    div()
                        .id(("hud-style", index))
                        .w(px(190.0))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded(RADIUS_PANEL)
                        .border_1()
                        .border_color(rgb(if selected { ACCENT } else { LINE }))
                        .bg(rgb(if selected { ACCENT_SURFACE } else { SURFACE }))
                        .hover(|card| card.bg(rgb(SURFACE_HOVER)))
                        .child(div().text_caption().text_color(rgb(ACCENT_TEXT)).child(key))
                        .child(
                            div()
                                .text_body()
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_caption()
                                .text_color(rgb(TEXT_TERTIARY))
                                .child(description),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select_hud_style(index, cx)),
                        )
                });
        const HUD_STATE_WIDTHS: [f32; 3] = [88.0, 102.0, 92.0];
        let state_idx = match self.platform.hud_demo {
            HudDemoState::Recording => 0.0,
            HudDemoState::Transcribing => 1.0,
            HudDemoState::Processing => 2.0,
        };
        let state_pos = window.spring("hud-lab-state-index", state_idx, SPRING_SNAP);
        let state_button = |state, label: &'static str, index: usize| {
            sliding_segmented_item(HUD_STATE_WIDTHS[index], self.platform.hud_demo == state)
                .id(("hud-state", index))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.platform.hud_demo = state;
                    this.apply_hud_lab();
                    cx.notify();
                }))
        };
        const HUD_QUEUE_WIDTHS: [f32; 4] = [34.0; 4];
        let queue_pos = window.spring(
            "hud-lab-queue-index",
            self.platform.hud_queued.min(3) as f32,
            SPRING_SNAP,
        );
        let queue_buttons = (0..=3).map(|count| {
            sliding_segmented_item(HUD_QUEUE_WIDTHS[count], self.platform.hud_queued == count)
                .id(("hud-queue", count))
                .child(count.to_string())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.platform.hud_queued = count;
                    this.apply_hud_lab();
                    cx.notify();
                }))
        });
        let preset = self.hud_preset();
        let copied = self.platform.hud_copied;

        div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.platform.hud_dragging = None;
                }),
            )
            .child(pane_header_with_action(
                "HUD Lab",
                Some(
                    button(
                        ButtonKind::Secondary,
                        if copied { tr("Copied") } else { "Copy preset" },
                    )
                    .id("copy-hud-preset")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(this.hud_preset());
                            this.platform.hud_copied = true;
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
                ),
            ))
            .child(pane_scroll_body(
                "hud-lab-scroll",
                pane_content()
                    .pt_6()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                sliding_segmented_control(state_pos, &HUD_STATE_WIDTHS).children([
                                    state_button(HudDemoState::Recording, tr("Recording"), 0),
                                    state_button(HudDemoState::Transcribing, tr("Transcribing"), 1),
                                    state_button(HudDemoState::Processing, tr("Processing"), 2),
                                ]),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_caption()
                                            .text_color(rgb(TEXT_TERTIARY))
                                            .child("Queued"),
                                    )
                                    .child(
                                        sliding_segmented_control(queue_pos, &HUD_QUEUE_WIDTHS)
                                            .children(queue_buttons),
                                    ),
                            ),
                    )
                    .child(div().flex().gap_3().children(style_cards))
                    .child(
                        div()
                            .flex()
                            .child(div().flex().flex_col().gap_4().children([
                                self.render_hud_slider(HudControl::LineCount, "Line count", cx),
                                self.render_hud_slider(HudControl::Curvature, "Curvature", cx),
                                self.render_hud_slider(HudControl::Speed, tr("Speed"), cx),
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
                            .rounded(RADIUS_PANEL)
                            .border_1()
                            .border_color(rgb(LINE))
                            .bg(rgb(SURFACE))
                            .text_monospace()
                            .text_caption()
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(preset),
                    ),
            ))
            .into_any_element()
    }

    pub(super) fn render_permission_warnings(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let warnings = crate::onboarding::permission_warnings(self.setup_status);
        if self.setup_visible || warnings.is_empty() {
            return None;
        }
        let warning_count = warnings.len();
        Some(
            div()
                .child(settings_section_label(tr("Permissions needed")))
                .child(panel().children(warnings.into_iter().enumerate().map(
                    |(index, warning)| {
                        let (name, description) = permission_warning_copy(warning.kind);
                        let action = permission_control(
                            if self.platform.permissions_asked.contains(&warning.kind) {
                                PermissionControl::Waiting
                            } else {
                                PermissionControl::Allow
                            },
                            ButtonKind::Secondary,
                            permission_warning_id(warning.kind),
                            cx.listener(move |this, _, _, cx| {
                                this.perform_permission_action(warning);
                                cx.notify();
                            }),
                        );
                        settings_row(name, description, action)
                            .when(index + 1 == warning_count, |row| row.border_b_0())
                    },
                )))
                .into_any_element(),
        )
    }

    pub(super) fn perform_permission_action(&mut self, warning: PermissionWarning) {
        match warning.action {
            PermissionAction::RequestMicrophone => crate::onboarding::request_microphone(),
            PermissionAction::ShowGuide(kind) => crate::permission_guide::show(kind),
        }
        if !self.platform.permissions_asked.contains(&warning.kind) {
            self.platform.permissions_asked.push(warning.kind);
        }
        self.permission_refresh_at = Instant::now() + PERMISSION_REFRESH_INTERVAL;
    }
}
