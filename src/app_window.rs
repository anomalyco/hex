use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, Bounds, Context, FontWeight, IntoElement, Render, Rgba, ScrollHandle, Timer,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb,
    size,
};

use crate::app_settings::AppSettings;
use crate::commands::{CommandConfig, CommandInfo, CommandScope};
use crate::events::{CommandOutcome, DictationPhase, TranscriptPhase, VoiceEvent, VoiceState};
use crate::meeting::{self, MeetingManifest, MeetingRequest, MeetingStatus};

const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MINIMUM_WIDTH: f32 = 860.0;
const MINIMUM_HEIGHT: f32 = 560.0;
const SIDEBAR_WIDTH: f32 = 170.0;
const ACTIVITY_LIMIT: usize = 100;

const CANVAS: u32 = 0x111111;
const SIDEBAR: u32 = 0x0e0e0e;
const SURFACE: u32 = 0x171717;
const SURFACE_HOVER: u32 = 0x1d1d1d;
const SURFACE_SELECTED: u32 = 0x222222;
const LINE: u32 = 0x292929;
const TEXT: u32 = 0xeeeeee;
const TEXT_SOFT: u32 = 0xb8b8b8;
const MUTED: u32 = 0x858585;
const FAINT: u32 = 0x626262;
const NEGATIVE: u32 = 0xc98f89;

/// Opens the app shell, or focuses and refreshes an existing shell window.
pub fn open_or_focus(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    commands: CommandConfig,
    meeting_requests: SyncSender<MeetingRequest>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    if let Some(handle) = app_window.borrow().as_ref().copied()
        && handle
            .update(cx, |this, window, cx| {
                this.event_path = event_path.clone();
                this.meeting_requests = meeting_requests.clone();
                this.refresh(cx);
                window.activate_window();
            })
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }

    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("HEX".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            is_minimizable: true,
            window_min_size: Some(size(px(MINIMUM_WIDTH), px(MINIMUM_HEIGHT))),
            ..Default::default()
        },
        |_, cx| cx.new(|cx| AppWindow::new(event_path, commands, meeting_requests, cx)),
    )?;
    *app_window.borrow_mut() = Some(handle);
    cx.activate(true);
    Ok(handle)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Pane {
    Meetings,
    Commands,
    Activity,
    Settings,
}

impl Pane {
    const ALL: [Self; 4] = [
        Self::Meetings,
        Self::Commands,
        Self::Activity,
        Self::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Meetings => "Meetings",
            Self::Commands => "Commands",
            Self::Activity => "Activity",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone)]
struct CatalogCommand {
    task: &'static str,
    scope: CommandScope,
    phrase: String,
    aliases: Vec<String>,
    description: &'static str,
    id: &'static str,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ContextFilter {
    All,
    Global,
    Application(&'static str),
    Browser(&'static str),
}

impl ContextFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Global => "Global",
            Self::Application(application) => application,
            Self::Browser(host) => host,
        }
    }
}

#[derive(Clone, PartialEq)]
struct TranscriptLine {
    source: String,
    text: String,
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

impl ToggleSpring {
    fn new(enabled: bool) -> Self {
        let position = if enabled { 1.0 } else { 0.0 };
        Self {
            position,
            velocity: 0.0,
            target: position,
            last_frame: Instant::now(),
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.target = if enabled { 1.0 } else { 0.0 };
        self.last_frame = Instant::now();
    }

    fn advance(&mut self, elapsed: Duration) {
        let mut remaining = elapsed.as_secs_f32().min(0.1);
        while remaining > 0.0 {
            let dt = remaining.min(1.0 / 240.0);
            let acceleration = -24.0_f32.powi(2) * (self.position - self.target)
                - 2.0 * 0.72 * 24.0 * self.velocity;
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
}

/// Root GPUI entity for the production app shell.
pub struct AppWindow {
    pane: Pane,
    event_path: PathBuf,
    meeting_requests: SyncSender<MeetingRequest>,
    settings: AppSettings,
    sound_toggle: ToggleSpring,
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
    commands: Vec<CatalogCommand>,
    command_filter: ContextFilter,
    selected_command: Option<&'static str>,
    events: Vec<VoiceEvent>,
    current_context: (Option<String>, Option<String>),
    events_error: Option<String>,
    selected_event: Option<usize>,
}

impl AppWindow {
    fn new(
        event_path: PathBuf,
        commands: CommandConfig,
        meeting_requests: SyncSender<MeetingRequest>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |window, cx| {
            loop {
                Timer::after(Duration::from_millis(500)).await;
                if window
                    .update(cx, |window, cx| {
                        if window.meeting_refresh_started_ms.is_some()
                            || active_meeting(&window.meetings).is_some()
                        {
                            window.reload_meetings();
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
        let settings = AppSettings::load().unwrap_or_else(|error| {
            tracing::error!(%error, "could not load app settings");
            AppSettings::default()
        });
        let commands = commands
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
            .collect();
        let mut window = Self {
            pane: Pane::Settings,
            event_path,
            meeting_requests,
            sound_toggle: ToggleSpring::new(settings.sound_effects),
            settings,
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
            commands,
            command_filter: ContextFilter::All,
            selected_command: None,
            events: Vec::new(),
            current_context: (None, None),
            events_error: None,
            selected_event: None,
        };
        window.reload_meetings();
        if active_meeting(&window.meetings).is_some() {
            window.pane = Pane::Meetings;
        }
        window.reload_events();
        window
    }

    /// Refreshes disk-backed meeting and activity data while retaining valid
    /// selections. This is also called whenever an existing shell is reopened.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload_meetings();
        self.reload_events();
        cx.notify();
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
                let previous_active = active_meeting(&self.meetings).map(|meeting| &meeting.id);
                let next_active = active_meeting(&meetings).map(|meeting| meeting.id.clone());
                if next_active.as_deref() != previous_active.map(String::as_str)
                    && next_active.is_some()
                {
                    self.selected_meeting = next_active;
                }
                if active_meeting(&meetings).is_none()
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
                if active_meeting(&self.meetings)
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
                entries
                    .into_iter()
                    .map(|entry| TranscriptLine {
                        source: format!(
                            "{} {}",
                            format_duration(entry.start_ms),
                            entry.source.label()
                        ),
                        text: entry.text,
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
        match read_recent_events(&self.event_path) {
            Ok(recent) => {
                self.events = recent.events;
                self.current_context = recent.current_context;
                self.events_error = None;
                self.selected_event = selected.and_then(|selected| {
                    self.events.iter().position(|event| {
                        serde_json::to_string(event).ok().as_deref() == Some(selected.as_str())
                    })
                });
            }
            Err(error) => {
                tracing::error!(%error, path = %self.event_path.display(), "could not read activity");
                self.events_error = Some(error);
            }
        }
    }

    fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let items = Pane::ALL.into_iter().enumerate().map(|(index, pane)| {
            let selected = self.pane == pane;
            div()
                .id(("app-nav", index))
                .h(px(36.0))
                .px_3()
                .flex()
                .items_center()
                .rounded_sm()
                .text_sm()
                .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
                .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
                .cursor_pointer()
                .hover(|item| item.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                .child(pane.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pane = pane;
                    match pane {
                        Pane::Meetings => this.reload_meetings(),
                        Pane::Commands | Pane::Activity => this.reload_events(),
                        Pane::Settings => {}
                    }
                    cx.notify();
                }))
        });

        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .px_3()
            .pt(px(68.0))
            .bg(rgb(SIDEBAR))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(div().flex().flex_col().gap_1().children(items))
            .into_any_element()
    }

    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let toggle_position = self.sound_toggle.render_position(window);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header("Settings", None))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_8()
                    .py_7()
                    .child(
                        div().max_w(px(680.0)).child(
                            div()
                                .id("sound-effects-setting")
                                .w_full()
                                .py_5()
                                .flex()
                                .items_center()
                                .justify_between()
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Sound effects"),
                                        )
                                        .child(
                                            div().text_size(px(12.0)).text_color(rgb(MUTED)).child(
                                                "Play tones for modes, cancellation, and errors.",
                                            ),
                                        ),
                                )
                                .child(toggle(toggle_position))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.sound_effects = !this.settings.sound_effects;
                                    this.sound_toggle.set_enabled(this.settings.sound_effects);
                                    crate::feedback::set_enabled(this.settings.sound_effects);
                                    if let Err(error) = this.settings.save() {
                                        tracing::error!(%error, "could not save app settings");
                                    }
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_meetings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let action = if self.meeting_stop_pending {
            meeting_progress_action("Finalizing...")
        } else {
            match active_meeting(&self.meetings).map(|meeting| meeting.status) {
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
                    .cursor_pointer()
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
                    .cursor_pointer()
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
                let duration = format_duration(meeting_duration(meeting));
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
                    .cursor_pointer()
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
            .w(px(300.0))
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
            .child(pane_header("Meetings", Some(action)))
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
                div()
                    .id("meetings-workspace")
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .child(list)
                    .child(self.render_meeting_detail(cx)),
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
            .cursor_pointer()
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
                        .py_3()
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
                                    .child(format_duration(meeting_duration(&meeting))),
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

    fn render_commands(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let context_label = self.current_context_label();
        let filters = self.context_filters();
        if !filters.contains(&self.command_filter) {
            self.command_filter = ContextFilter::All;
        }
        let filter_items = filters.into_iter().enumerate().map(|(index, filter)| {
            let selected = self.command_filter == filter;
            div()
                .id(("command-filter", index))
                .h(px(30.0))
                .px_3()
                .flex()
                .items_center()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
                .when(selected, |item| item.bg(rgb(SURFACE_SELECTED)))
                .cursor_pointer()
                .hover(|item| item.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT_SOFT)))
                .child(filter.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.command_filter = filter;
                    this.selected_command = None;
                    cx.notify();
                }))
        });

        let visible: Vec<usize> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| self.command_is_visible(command).then_some(index))
            .collect();
        let mut tasks = Vec::new();
        for index in &visible {
            let task = self.commands[*index].task;
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
        let groups: Vec<AnyElement> = tasks
            .into_iter()
            .map(|task| {
                let rows = visible
                    .iter()
                    .copied()
                    .filter(|index| self.commands[*index].task == task)
                    .map(|index| {
                        let command = self.commands[index].clone();
                        let selected = self.selected_command == Some(command.id);
                        div()
                            .id(("command", index))
                            .w_full()
                            .px_4()
                            .py_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .when(selected, |row| row.bg(rgb(SURFACE_SELECTED)))
                            .cursor_pointer()
                            .hover(|row| row.bg(rgb(SURFACE_HOVER)))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(command.phrase),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(command.description)
                                    .child(command_scope(command.scope)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_command = Some(command.id);
                                cx.notify();
                            }))
                    });
                div()
                    .pb_3()
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(FAINT))
                            .child(task),
                    )
                    .children(rows)
                    .into_any_element()
            })
            .collect();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(pane_header("Commands", None))
            .child(
                div()
                    .h(px(47.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(div().flex().items_center().gap_1().children(filter_items))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(FAINT))
                            .child(format!("Context: {context_label}")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .child(
                        div()
                            .id("commands-list")
                            .w(px(400.0))
                            .h_full()
                            .flex_none()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(rgb(LINE))
                            .when(groups.is_empty(), |list| {
                                list.child(empty_message("No commands in this context."))
                            })
                            .children(groups),
                    )
                    .child(self.render_command_detail()),
            )
            .into_any_element()
    }

    fn render_command_detail(&self) -> AnyElement {
        let Some(command) = self
            .selected_command
            .and_then(|id| self.commands.iter().find(|command| command.id == id))
        else {
            return detail_placeholder("Select a command.");
        };
        let aliases = if command.aliases.is_empty() {
            "None".into()
        } else {
            command.aliases.join("\n")
        };
        div()
            .id("command-detail")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .px_6()
            .py_6()
            .child(
                div()
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(command.phrase.clone()),
            )
            .child(
                div()
                    .pt_2()
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(command.description),
            )
            .child(
                div()
                    .pt_6()
                    .child(detail_row("Task", command.task))
                    .child(detail_row("Context", command_scope(command.scope)))
                    .child(detail_row("Command ID", command.id))
                    .child(detail_row("Aliases", aliases)),
            )
            .into_any_element()
    }

    fn command_is_visible(&self, command: &CatalogCommand) -> bool {
        match self.command_filter {
            ContextFilter::All => true,
            ContextFilter::Global => {
                matches!(command.scope, CommandScope::Sleeping | CommandScope::Global)
            }
            ContextFilter::Application(application) => {
                command.scope == CommandScope::Application(application)
            }
            ContextFilter::Browser(host) => command.scope == CommandScope::Browser(host),
        }
    }

    fn context_filters(&self) -> Vec<ContextFilter> {
        let mut filters = vec![ContextFilter::All, ContextFilter::Global];
        let (application, browser_host) = self.current_context();
        for command in &self.commands {
            let filter = match command.scope {
                CommandScope::Application(scope) if application.as_deref() == Some(scope) => {
                    Some(ContextFilter::Application(scope))
                }
                CommandScope::Browser(scope)
                    if browser_host.as_deref().is_some_and(|host| {
                        host == scope || host.strip_prefix("www.") == Some(scope)
                    }) =>
                {
                    Some(ContextFilter::Browser(scope))
                }
                _ => None,
            };
            if let Some(filter) = filter
                && !filters.contains(&filter)
            {
                filters.push(filter);
            }
        }
        filters
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
        let refresh = div()
            .id("refresh-activity")
            .h(px(30.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_sm()
            .bg(rgb(SURFACE))
            .text_size(px(12.0))
            .text_color(rgb(TEXT_SOFT))
            .cursor_pointer()
            .hover(|button| button.bg(rgb(SURFACE_HOVER)).text_color(rgb(TEXT)))
            .child("Refresh")
            .on_click(cx.listener(|this, _, _, cx| {
                this.reload_events();
                cx.notify();
            }))
            .into_any_element();
        let rows: Vec<AnyElement> = self
            .events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                let selected = self.selected_event == Some(index);
                let summary = event_summary(event);
                let age = event_age(event_timestamp(event));
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
                    .cursor_pointer()
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
            .child(pane_header("Activity", Some(refresh)))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .child(
                        div()
                            .id("activity-list")
                            .w(px(460.0))
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
                                list.child(error_message("Activity could not be loaded.", error))
                            })
                            .children(rows),
                    )
                    .child(self.render_event_detail()),
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
                    .child(detail_row("Event", event_kind(event)))
                    .child(detail_row(
                        "Timestamp",
                        format!("{} ms", event_timestamp(event)),
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
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.pane {
            Pane::Meetings => self.render_meetings(cx),
            Pane::Commands => self.render_commands(cx),
            Pane::Activity => self.render_activity(cx),
            Pane::Settings => self.render_settings(window, cx),
        };
        div()
            .size_full()
            .overflow_hidden()
            .flex()
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT))
            .child(self.render_navigation(cx))
            .child(div().flex_1().h_full().overflow_hidden().child(content))
    }
}

fn pane_header(title: &'static str, action: Option<AnyElement>) -> AnyElement {
    div()
        .h(px(68.0))
        .px_6()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(rgb(LINE))
        .child(
            div()
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .when_some(action, |header, action| header.child(action))
        .into_any_element()
}

fn section_label(label: &'static str) -> AnyElement {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(FAINT))
        .child(label)
        .into_any_element()
}

fn toggle(position: f32) -> AnyElement {
    let color_position = position.clamp(0.0, 1.0);
    div()
        .w(px(38.0))
        .h(px(22.0))
        .p(px(3.0))
        .flex_none()
        .flex()
        .items_center()
        .rounded_full()
        .bg(mix_color(rgb(0x353535), rgb(0xe4e4e4), color_position))
        .child(
            div()
                .ml(px(16.0 * position.clamp(-0.04, 1.04)))
                .size(px(16.0))
                .rounded_full()
                .bg(mix_color(rgb(0x9a9a9a), rgb(SURFACE), color_position)),
        )
        .into_any_element()
}

fn mix_color(from: Rgba, to: Rgba, position: f32) -> Rgba {
    Rgba {
        r: from.r + (to.r - from.r) * position,
        g: from.g + (to.g - from.g) * position,
        b: from.b + (to.b - from.b) * position,
        a: from.a + (to.a - from.a) * position,
    }
}

fn empty_message(message: &'static str) -> AnyElement {
    div()
        .p_6()
        .text_size(px(12.0))
        .text_color(rgb(FAINT))
        .child(message)
        .into_any_element()
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

fn error_message(message: &'static str, error: String) -> AnyElement {
    div()
        .p_6()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(px(12.0))
        .text_color(rgb(NEGATIVE))
        .child(message)
        .child(
            div()
                .text_size(px(11.0))
                .line_height(px(17.0))
                .text_color(rgb(MUTED))
                .child(error),
        )
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

fn command_task(command: &CommandInfo) -> &'static str {
    if command.id.starts_with("mode.") {
        "Voice control"
    } else if command.id.starts_with("dictation.") || command.id.starts_with("captains-log.") {
        "Dictation"
    } else if command.id.starts_with("meeting.") {
        "Meetings"
    } else {
        match command.description {
            "Open application" => "Open applications",
            "Open website" => "Open websites",
            "Navigate active tab" => "Browser navigation",
            "Use keyboard shortcut" => "Keyboard",
            "Use application shortcut" | "Open application destination" => "Application navigation",
            _ => "Other",
        }
    }
}

fn command_scope(scope: CommandScope) -> &'static str {
    match scope {
        CommandScope::Sleeping | CommandScope::Global => "Global",
        CommandScope::Application(application) => application,
        CommandScope::Browser(host) => host,
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

fn active_meeting(meetings: &[MeetingManifest]) -> Option<&MeetingManifest> {
    meetings.iter().find(|meeting| {
        matches!(
            meeting.status,
            MeetingStatus::Starting | MeetingStatus::Recording | MeetingStatus::Transcribing
        )
    })
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

fn format_duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

struct RecentEvents {
    events: Vec<VoiceEvent>,
    current_context: (Option<String>, Option<String>),
}

fn read_recent_events(path: &Path) -> Result<RecentEvents, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecentEvents {
                events: Vec::new(),
                current_context: (None, None),
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut events = Vec::with_capacity(ACTIVITY_LIMIT);
    let mut current_context = None;
    for event in contents
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
    {
        if current_context.is_none()
            && let VoiceEvent::Context {
                application,
                browser_url,
                ..
            } = &event
        {
            current_context = Some((
                application.clone(),
                browser_url
                    .as_deref()
                    .and_then(|url| url::Url::parse(url).ok())
                    .and_then(|url| url.host_str().map(str::to_owned)),
            ));
        }
        if events.len() < ACTIVITY_LIMIT {
            events.push(event);
        }
        if events.len() == ACTIVITY_LIMIT && current_context.is_some() {
            break;
        }
    }
    Ok(RecentEvents {
        events,
        current_context: current_context.unwrap_or_default(),
    })
}

fn event_timestamp(event: &VoiceEvent) -> u64 {
    match event {
        VoiceEvent::SessionStarted { timestamp_ms }
        | VoiceEvent::State { timestamp_ms, .. }
        | VoiceEvent::Transcript { timestamp_ms, .. }
        | VoiceEvent::Command { timestamp_ms, .. }
        | VoiceEvent::Dictation { timestamp_ms, .. }
        | VoiceEvent::Context { timestamp_ms, .. } => *timestamp_ms,
    }
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

fn event_kind(event: &VoiceEvent) -> &'static str {
    match event {
        VoiceEvent::SessionStarted { .. } => "session_started",
        VoiceEvent::State { .. } => "state",
        VoiceEvent::Transcript { .. } => "transcript",
        VoiceEvent::Command { .. } => "command",
        VoiceEvent::Dictation { .. } => "dictation",
        VoiceEvent::Context { .. } => "context",
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
        VoiceEvent::Dictation { phase, text, .. } => match phase {
            DictationPhase::Started => "Dictation started".into(),
            DictationPhase::Discarded => "A brief dictation was discarded".into(),
            DictationPhase::Cancelled => "Dictation was cancelled".into(),
            DictationPhase::Transcribing => "Dictation is being transcribed".into(),
            DictationPhase::Pasted => text_or("Dictation was pasted", text),
            DictationPhase::Logged => text_or("Captain's Log was saved", text),
            DictationPhase::Repasted => text_or("The last dictation was pasted again", text),
            DictationPhase::Failed(error) => format!("Dictation failed: {error}"),
        },
        VoiceEvent::Context {
            application,
            browser_url,
            ..
        } => match (application, browser_url) {
            (Some(application), Some(url)) => format!("Context changed to {application} / {url}"),
            (Some(application), None) => format!("Context changed to {application}"),
            _ => "Foreground context became unavailable".into(),
        },
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
    use super::*;

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

        let recent = read_recent_events(&path).unwrap();

        assert_eq!(recent.events.len(), ACTIVITY_LIMIT);
        assert_eq!(
            recent.current_context,
            (Some("Brave Browser".into()), Some("meet.google.com".into()))
        );
        fs::remove_file(path).unwrap();
    }
}
