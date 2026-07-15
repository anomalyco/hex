use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event as TerminalEvent, KeyCode};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::commands::{CommandConfig, CommandScope, Mode};
use crate::context::ContextSnapshot;
use crate::events::{CommandOutcome, DictationPhase, TranscriptPhase, VoiceEvent, VoiceState};
use crate::meeting::{self, MeetingManifest, MeetingStatus};

#[derive(Clone, Copy, Eq, PartialEq)]
enum View {
    Activity,
    Commands,
    Meetings,
}

enum RecentAction<'a> {
    Command(&'a str, &'a CommandOutcome, &'a str),
    Dictation(&'a DictationPhase, &'a str),
}

pub fn run(path: PathBuf, commands: CommandConfig) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, path, commands);
    ratatui::restore();
    result.map_err(Into::into)
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    path: PathBuf,
    commands: CommandConfig,
) -> io::Result<()> {
    let mut view = View::Commands;
    let mut selected_meeting = 0;
    loop {
        let events = read_events(&path);
        let meetings = meeting::list().unwrap_or_default();
        selected_meeting = selected_meeting.min(meetings.len().saturating_sub(1));
        terminal.draw(|frame| {
            draw(
                frame,
                &events,
                &commands,
                view,
                &path,
                &meetings,
                selected_meeting,
            )
        })?;
        if event::poll(Duration::from_millis(100))?
            && let TerminalEvent::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('1') => view = View::Commands,
                KeyCode::Char('2') => view = View::Activity,
                KeyCode::Char('3') => view = View::Meetings,
                KeyCode::Down | KeyCode::Char('j') if view == View::Meetings => {
                    selected_meeting = (selected_meeting + 1).min(meetings.len().saturating_sub(1));
                }
                KeyCode::Up | KeyCode::Char('k') if view == View::Meetings => {
                    selected_meeting = selected_meeting.saturating_sub(1);
                }
                KeyCode::Tab => {
                    view = match view {
                        View::Activity => View::Commands,
                        View::Commands => View::Meetings,
                        View::Meetings => View::Activity,
                    }
                }
                _ => {}
            }
        }
    }
}

fn read_events(path: &PathBuf) -> VecDeque<VoiceEvent> {
    let mut events: VecDeque<_> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if let Some(start) = events
        .iter()
        .rposition(|event| matches!(event, VoiceEvent::SessionStarted { .. }))
    {
        events.drain(..start);
    }
    events
}

fn draw(
    frame: &mut ratatui::Frame,
    events: &VecDeque<VoiceEvent>,
    commands: &CommandConfig,
    view: View,
    path: &Path,
    meetings: &[MeetingManifest],
    selected_meeting: usize,
) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let state = events.iter().rev().find_map(|event| match event {
        VoiceEvent::State { state, device, .. } => Some((*state, device.as_str())),
        VoiceEvent::SessionStarted { .. }
        | VoiceEvent::Transcript { .. }
        | VoiceEvent::Command { .. }
        | VoiceEvent::Dictation { .. }
        | VoiceEvent::Context { .. } => None,
    });
    let (state, device) = state.unwrap_or((VoiceState::Stopping, "waiting for listener"));
    let (state_label, state_color) = match state {
        VoiceState::Sleeping => ("SLEEPING", Color::Blue),
        VoiceState::Listening => ("LISTENING", Color::Green),
        VoiceState::Dictating => ("DICTATING", Color::Magenta),
        VoiceState::Transcribing => ("TRANSCRIBING", Color::Cyan),
        VoiceState::Stopping => ("OFFLINE", Color::DarkGray),
    };
    let latest = events.iter().rev().find_map(|event| match event {
        VoiceEvent::Transcript {
            phase,
            latency_ms,
            text,
            ..
        } => Some((*phase, *latency_ms, text.as_str())),
        VoiceEvent::SessionStarted { .. }
        | VoiceEvent::State { .. }
        | VoiceEvent::Command { .. }
        | VoiceEvent::Dictation { .. } => None,
        VoiceEvent::Context { .. } => None,
    });
    let latest_action = events.iter().rev().find_map(|event| match event {
        VoiceEvent::Command {
            command,
            outcome,
            context,
            ..
        } => Some(RecentAction::Command(
            command.as_deref().unwrap_or("no match"),
            outcome,
            context,
        )),
        VoiceEvent::Dictation { phase, text, .. } => Some(RecentAction::Dictation(phase, text)),
        _ => None,
    });
    let context = events
        .iter()
        .rev()
        .find_map(|event| match event {
            VoiceEvent::Context {
                application,
                browser_url,
                ..
            } => Some(ContextSnapshot {
                application: application.clone(),
                browser_url: browser_url
                    .as_deref()
                    .and_then(|url| url::Url::parse(url).ok()),
                window_title: None,
            }),
            _ => None,
        })
        .unwrap_or_default();

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(state_color)),
                Span::styled(
                    state_label,
                    Style::default()
                        .fg(state_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Input  ", Style::default().fg(Color::DarkGray)),
                Span::raw(device),
            ]),
            Line::from(vec![
                Span::styled("Option ", Style::default().fg(Color::DarkGray)),
                Span::raw("hold to dictate, release to transcribe and paste"),
            ]),
            Line::from(vec![
                Span::styled("Context ", Style::default().fg(Color::DarkGray)),
                Span::raw(context.label()),
            ]),
        ])
        .block(Block::default().title(" Voice ").borders(Borders::ALL)),
        areas[0],
    );

    let current_transcript = latest.map_or_else(
        || {
            Line::from(Span::styled(
                "No speech yet",
                Style::default().fg(Color::DarkGray),
            ))
        },
        |(phase, latency, text)| {
            Line::from(vec![
                Span::styled(
                    format!("{}  ", phase_label(phase)),
                    Style::default().fg(phase_color(phase)),
                ),
                Span::raw(text),
                Span::styled(
                    format!("  {latency}ms"),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        },
    );
    let current_action = match latest_action {
        Some(RecentAction::Command(command, outcome, context)) => Line::from(vec![
            Span::styled("command    ", Style::default().fg(Color::DarkGray)),
            Span::styled(command, Style::default().fg(command_color(outcome))),
            Span::styled(
                format!("  {}", command_outcome(outcome)),
                Style::default().fg(command_color(outcome)),
            ),
            Span::styled(format!("  {context}"), Style::default().fg(Color::DarkGray)),
        ]),
        Some(RecentAction::Dictation(phase, text)) => Line::from(vec![
            Span::styled("dictation  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                dictation_label(phase),
                Style::default().fg(dictation_color(phase)),
            ),
            Span::raw(if text.is_empty() {
                String::new()
            } else {
                format!("  {text}")
            }),
        ]),
        None => Line::from(""),
    };
    frame.render_widget(
        Paragraph::new(vec![current_transcript, current_action])
            .wrap(Wrap { trim: false })
            .block(Block::default().title(" Current ").borders(Borders::ALL)),
        areas[1],
    );

    match view {
        View::Activity => draw_activity(frame, events, areas[2]),
        View::Meetings => draw_meetings(frame, meetings, selected_meeting, areas[2]),
        View::Commands => draw_commands(
            frame,
            commands,
            if state == VoiceState::Sleeping {
                Mode::Sleeping
            } else {
                Mode::Listening
            },
            &context,
            areas[2],
        ),
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("q/esc", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("1", tab_style(matches!(view, View::Commands))),
            Span::raw(" commands  "),
            Span::styled("2", tab_style(matches!(view, View::Activity))),
            Span::raw(" log  "),
            Span::styled("3", tab_style(matches!(view, View::Meetings))),
            Span::raw(" meetings  "),
            Span::styled(
                path.display().to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        areas[3],
    );
}

fn draw_meetings(
    frame: &mut ratatui::Frame,
    meetings: &[MeetingManifest],
    selected: usize,
    area: ratatui::layout::Rect,
) {
    let direction = if area.width >= 90 {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };
    let panes = Layout::default()
        .direction(direction)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    let rows: Vec<_> = meetings
        .iter()
        .enumerate()
        .map(|(index, meeting)| {
            let marker = if index == selected { "›" } else { " " };
            let status = match meeting.status {
                MeetingStatus::Recording => "REC",
                MeetingStatus::Transcribing => "ASR",
                MeetingStatus::Complete => "OK ",
                MeetingStatus::Interrupted => "INT",
                MeetingStatus::Failed => "ERR",
            };
            let color = match meeting.status {
                MeetingStatus::Recording => Color::Red,
                MeetingStatus::Transcribing => Color::Cyan,
                MeetingStatus::Complete => Color::Green,
                MeetingStatus::Interrupted => Color::Yellow,
                MeetingStatus::Failed => Color::Red,
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} {status} "), Style::default().fg(color)),
                Span::raw(&meeting.title),
                Span::styled(
                    format!(
                        "  {}",
                        format_duration(meeting.duration_ms.unwrap_or_default())
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(rows).block(Block::default().title(" Meetings ").borders(Borders::ALL)),
        panes[0],
    );

    let transcript = meetings
        .get(selected)
        .and_then(|meeting| meeting::show(&meeting.id).ok())
        .unwrap_or_else(|| "No transcript available".into());
    frame.render_widget(
        Paragraph::new(transcript)
            .wrap(Wrap { trim: false })
            .block(Block::default().title(" Transcript ").borders(Borders::ALL)),
        panes[1],
    );
}

fn format_duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn draw_activity(
    frame: &mut ratatui::Frame,
    events: &VecDeque<VoiceEvent>,
    area: ratatui::layout::Rect,
) {
    let activity: Vec<ListItem> = events
        .iter()
        .filter_map(|event| match event {
            VoiceEvent::Command {
                heard,
                command,
                outcome,
                context,
                ..
            } => {
                let (symbol, label) = outcome_display(outcome);
                let error = match outcome {
                    CommandOutcome::Failed(error) => format!("  {error}"),
                    _ => String::new(),
                };
                Some(Line::from(vec![
                    Span::styled(
                        format!("{symbol} {label:<12}"),
                        Style::default().fg(command_color(outcome)),
                    ),
                    Span::raw(heard),
                    Span::styled(
                        command
                            .as_deref()
                            .map_or_else(String::new, |id| format!("  {id}")),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  [{context}]{error}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }
            VoiceEvent::Dictation { phase, text, .. } => {
                let detail = match phase {
                    DictationPhase::Failed(error) => error.as_str(),
                    _ => text.as_str(),
                };
                Some(Line::from(vec![
                    Span::styled(
                        format!("◆ {:<12}", dictation_label(phase)),
                        Style::default().fg(dictation_color(phase)),
                    ),
                    Span::styled("dictation", Style::default().fg(Color::DarkGray)),
                    Span::raw(if detail.is_empty() {
                        String::new()
                    } else {
                        format!("  {detail}")
                    }),
                ]))
            }
            VoiceEvent::SessionStarted { .. }
            | VoiceEvent::State { .. }
            | VoiceEvent::Transcript { .. }
            | VoiceEvent::Context { .. } => None,
        })
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .map(ListItem::new)
        .collect();
    frame.render_widget(
        List::new(activity).block(Block::default().title(" Log ").borders(Borders::ALL)),
        area,
    );
}

fn draw_commands(
    frame: &mut ratatui::Frame,
    commands: &CommandConfig,
    mode: Mode,
    context: &ContextSnapshot,
    area: ratatui::layout::Rect,
) {
    let phrase_width = if area.width >= 100 { 28 } else { 22 };
    let mut available = commands.available_catalog(mode, context);
    available.sort_by_key(|command| matches!(command.scope, CommandScope::Global));
    let catalog: Vec<ListItem> = available
        .into_iter()
        .flat_map(|command| {
            let scope = match command.scope {
                CommandScope::Sleeping | CommandScope::Global => "global",
                CommandScope::Application(application) => application,
                CommandScope::Browser(host) => host,
            };
            let mut rows = vec![ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<9}", scope.to_lowercase()),
                    Style::default().fg(if scope == "global" {
                        Color::DarkGray
                    } else {
                        Color::Green
                    }),
                ),
                Span::styled(
                    format!("{:<phrase_width$}", command.phrase),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(command.description, Style::default().fg(Color::DarkGray)),
            ]))];
            if area.width >= 100 && !command.aliases.is_empty() {
                rows.push(ListItem::new(Line::from(vec![
                    Span::raw("           "),
                    Span::styled("↳ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        command.aliases.join("  ·  "),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])));
            }
            rows
        })
        .collect();
    let title = format!(" Commands · {} ", context.label());
    frame.render_widget(
        List::new(catalog).block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn phase_label(phase: TranscriptPhase) -> &'static str {
    match phase {
        TranscriptPhase::Started => "started",
        TranscriptPhase::Updated => "updated",
        TranscriptPhase::Completed => "completed",
    }
}

fn phase_color(phase: TranscriptPhase) -> Color {
    match phase {
        TranscriptPhase::Started => Color::Cyan,
        TranscriptPhase::Updated => Color::Yellow,
        TranscriptPhase::Completed => Color::Green,
    }
}

fn command_outcome(outcome: &CommandOutcome) -> &'static str {
    match outcome {
        CommandOutcome::Ignored => "ignored",
        CommandOutcome::Woke => "woke",
        CommandOutcome::Slept => "slept",
        CommandOutcome::Submitted => "submitted",
        CommandOutcome::Executed => "executed",
        CommandOutcome::Failed(_) => "failed",
    }
}

fn outcome_display(outcome: &CommandOutcome) -> (&'static str, &'static str) {
    match outcome {
        CommandOutcome::Ignored => ("○", "ignored"),
        CommandOutcome::Woke => ("↑", "woke"),
        CommandOutcome::Slept => ("↓", "slept"),
        CommandOutcome::Submitted => ("…", "submitted"),
        CommandOutcome::Executed => ("✓", "executed"),
        CommandOutcome::Failed(_) => ("!", "failed"),
    }
}

fn command_color(outcome: &CommandOutcome) -> Color {
    match outcome {
        CommandOutcome::Executed | CommandOutcome::Woke | CommandOutcome::Slept => Color::Green,
        CommandOutcome::Submitted => Color::Cyan,
        CommandOutcome::Failed(_) => Color::Red,
        CommandOutcome::Ignored => Color::DarkGray,
    }
}

fn dictation_label(phase: &DictationPhase) -> &'static str {
    match phase {
        DictationPhase::Started => "recording",
        DictationPhase::Discarded => "discarded",
        DictationPhase::Cancelled => "cancelled",
        DictationPhase::Transcribing => "transcribing",
        DictationPhase::Pasted => "pasted",
        DictationPhase::Logged => "logged",
        DictationPhase::Repasted => "repasted",
        DictationPhase::Failed(_) => "failed",
    }
}

fn dictation_color(phase: &DictationPhase) -> Color {
    match phase {
        DictationPhase::Started => Color::Magenta,
        DictationPhase::Transcribing => Color::Cyan,
        DictationPhase::Pasted => Color::Green,
        DictationPhase::Logged => Color::Green,
        DictationPhase::Repasted => Color::Green,
        DictationPhase::Discarded => Color::DarkGray,
        DictationPhase::Cancelled => Color::Yellow,
        DictationPhase::Failed(_) => Color::Red,
    }
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    }
}
