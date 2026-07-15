use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, Bounds, Context, FontWeight, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};

use crate::app_settings::AppSettings;
use crate::commands::{CommandConfig, CommandInfo, CommandScope};
use crate::events::{CommandOutcome, DictationPhase, TranscriptPhase, VoiceEvent, VoiceState};
use crate::meeting::{self, MeetingManifest, MeetingStatus};

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
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    if let Some(handle) = app_window.borrow().as_ref().copied()
        && handle
            .update(cx, |this, window, cx| {
                this.event_path = event_path.clone();
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
        |_, cx| cx.new(|_| AppWindow::new(event_path, commands)),
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

#[derive(Clone)]
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

/// Root GPUI entity for the production app shell.
pub struct AppWindow {
    pane: Pane,
    event_path: PathBuf,
    settings: AppSettings,
    meetings: Vec<MeetingManifest>,
    meetings_error: Option<String>,
    selected_meeting: Option<String>,
    meeting_transcript: MeetingTranscript,
    commands: Vec<CatalogCommand>,
    command_filter: ContextFilter,
    selected_command: Option<&'static str>,
    events: Vec<VoiceEvent>,
    current_context: (Option<String>, Option<String>),
    events_error: Option<String>,
    selected_event: Option<usize>,
}

impl AppWindow {
    fn new(event_path: PathBuf, commands: CommandConfig) -> Self {
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
            settings,
            meetings: Vec::new(),
            meetings_error: None,
            selected_meeting: None,
            meeting_transcript: MeetingTranscript::Empty,
            commands,
            command_filter: ContextFilter::All,
            selected_command: None,
            events: Vec::new(),
            current_context: (None, None),
            events_error: None,
            selected_event: None,
        };
        window.reload_meetings();
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

    fn reload_meetings(&mut self) {
        match meeting::list() {
            Ok(meetings) => {
                self.meetings = meetings;
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
        let Some(id) = self.selected_meeting.as_deref() else {
            self.meeting_transcript = MeetingTranscript::Empty;
            return;
        };
        self.meeting_transcript = match meeting::show(id) {
            Ok(markdown) => MeetingTranscript::Loaded(parse_transcript(&markdown)),
            Err(error) => MeetingTranscript::Unavailable(error.to_string()),
        };
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
            .pt_6()
            .bg(rgb(SIDEBAR))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .px_2()
                    .pb_6()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::BOLD)
                    .child("HEX"),
            )
            .child(div().flex().flex_col().gap_1().children(items))
            .into_any_element()
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.settings.sound_effects;
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
                                .child(toggle(enabled))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.sound_effects = !this.settings.sound_effects;
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
        let refresh = div()
            .id("refresh-meetings")
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
                this.reload_meetings();
                cx.notify();
            }))
            .into_any_element();

        let rows: Vec<AnyElement> = self
            .meetings
            .iter()
            .enumerate()
            .map(|(index, meeting)| {
                let id = meeting.id.clone();
                let selected = self.selected_meeting.as_deref() == Some(id.as_str());
                let title = meeting.title.clone();
                let duration = format_duration(meeting.duration_ms.unwrap_or_default());
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
            .child(pane_header("Meetings", Some(refresh)))
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
                empty_message("The transcript is empty.")
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
                                    .child(meeting.title),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_4()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(meeting_status(meeting.status))
                                    .child(format_duration(
                                        meeting.duration_ms.unwrap_or_default(),
                                    )),
                            ),
                    )
                    .child(reveal),
            )
            .child(
                div()
                    .id("meeting-transcript")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_6()
                    .pb_6()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.pane {
            Pane::Meetings => self.render_meetings(cx),
            Pane::Commands => self.render_commands(cx),
            Pane::Activity => self.render_activity(cx),
            Pane::Settings => self.render_settings(cx),
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

fn toggle(enabled: bool) -> AnyElement {
    div()
        .w(px(38.0))
        .h(px(22.0))
        .p(px(3.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_start()
        .rounded_full()
        .when(enabled, |toggle| toggle.justify_end().bg(rgb(0xe4e4e4)))
        .when(!enabled, |toggle| toggle.bg(rgb(0x353535)))
        .child(div().size(px(16.0)).rounded_full().bg(if enabled {
            rgb(SURFACE)
        } else {
            rgb(0x9a9a9a)
        }))
        .into_any_element()
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
        MeetingStatus::Recording => "Recording",
        MeetingStatus::Transcribing => "Transcribing",
        MeetingStatus::Complete => "Complete",
        MeetingStatus::Interrupted => "Interrupted",
        MeetingStatus::Failed => "Failed",
    }
}

fn format_duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn parse_transcript(markdown: &str) -> Vec<TranscriptLine> {
    markdown
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let content = line.strip_prefix("**")?;
            let marker_end = content.find("**")?;
            Some(TranscriptLine {
                source: content[..marker_end].to_string(),
                text: content[marker_end + 2..].trim().to_string(),
            })
        })
        .collect()
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
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seconds = now.saturating_sub(timestamp_ms) / 1_000;
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
