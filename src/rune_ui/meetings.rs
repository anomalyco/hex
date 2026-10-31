//! The Meetings pane (macOS beta): recorded calls by day with their status,
//! search, a detail with notes, the live or final transcript as turns,
//! Copy transcript, Rename, an overflow menu (copy summary, reveal,
//! summarize, delete), and the two-step trash confirmation. Recording is
//! started and stopped from the header, whose `Recordings | Settings` switch
//! also shows the feature's settings (Call recording, Calendar, Detected
//! calls). With Meetings off, the pane is the opt-in card alone.
//!
//! The meetings store and controller are macOS-only, so the pane reads
//! [`Meeting`] values the host loads (formatted with `meeting::day_label`,
//! `meeting::clock`, and the coalesced transcript) and sends
//! [`MeetingRequest`]s back. Scenes feed [`MeetingsPane::fixture`].

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::{IconButton, IconButtonVariant, Menu as KitMenu, MenuItem, Placement, TextInput};

use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, card, page, pulsing_dot, segmented_tx,
};
use super::list_detail::{self as ld, Item};
use super::lists::{Tone, error_panel, section_label, tone_tag};
use super::platform::{MeetingRequest, PlatformRequest};
use super::{Menu, Ui};
use crate::desktop_i18n::{tr, tr_fill, tr_named};

static MORE_ICON: rune::Icon = rune::Icon {
    name: "hex-more",
    view: 16.0,
    body: r#"<circle cx="3.5" cy="8" r="1.25" fill="currentColor"/><circle cx="8" cy="8" r="1.25" fill="currentColor"/><circle cx="12.5" cy="8" r="1.25" fill="currentColor"/>"#,
};

// Only the macOS host reports the states fixtures do not show.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Starting,
    Recording,
    Transcribing,
    Complete,
    Interrupted,
    Failed,
}

impl Status {
    pub(super) fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Recording | Self::Transcribing)
    }
}

/// One speaker turn. `mine` is the microphone side ("You").
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Turn {
    pub(super) mine: bool,
    pub(super) start_ms: u64,
    pub(super) text: String,
    /// A silence of 30 s or more before this turn.
    pub(super) pause_before_ms: Option<u64>,
}

/// One topic of a meeting (Labs), from `start_ms` to
/// the next chapter's start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Chapter {
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) start_ms: u64,
    /// What Copy section copies, from the same transcript lines as the menu
    /// bar and `hex meetings show --chapter`.
    pub(super) markdown: String,
}

// Only the macOS host reports the states fixtures do not show.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Transcript {
    Loaded(Vec<Turn>),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Meeting {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) status: Status,
    /// `Today`, `Yesterday`, or a date, from `meeting::day_label`.
    pub(super) day: String,
    /// The start time, from `meeting::clock`.
    pub(super) clock: String,
    pub(super) duration_ms: u64,
    pub(super) app: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) notes_running: bool,
    /// The exported Markdown still exists, so trashing takes it too.
    pub(super) has_markdown: bool,
    pub(super) error: Option<String>,
    pub(super) export_error: Option<String>,
    pub(super) notes_error: Option<String>,
    pub(super) live_error: Option<String>,
    pub(super) transcript: Transcript,
    pub(super) chapters: Vec<Chapter>,
}

impl Meeting {
    fn turns(&self) -> &[Turn] {
        match &self.transcript {
            Transcript::Loaded(turns) => turns,
            Transcript::Unavailable(_) => &[],
        }
    }

    fn searchable(&self) -> String {
        let mut text = format!(
            "{} {} {}",
            self.title,
            self.app.as_deref().unwrap_or_default(),
            self.summary.as_deref().unwrap_or_default()
        );
        for turn in self.turns() {
            text.push(' ');
            text.push_str(&turn.text);
        }
        text.to_lowercase()
    }
}

/// What the pane shows, picked in its header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Recordings,
    Settings,
}

/// Meetings is on, so the pane lists recordings and settings instead of the
/// opt-in card (`MeetingSettings::available`; scenes leave `developer` off).
pub(super) fn available(ui: &Ui) -> bool {
    ui.settings.meetings.enabled || ui.platform.developer
}

/// Whether something can write a meeting's notes: Apple Intelligence on a
/// Mac that has it, or OpenCode once turned on.
fn notes_available(ui: &Ui) -> bool {
    (ui.platform.caps.os == super::platform::Os::Macos && ui.platform.notes.is_available())
        || ui.settings.meetings.summarize_with_opencode
}

#[derive(Default)]
pub(super) struct MeetingsPane {
    pub(super) view: View,
    /// Newest first, as the store lists them.
    pub(super) meetings: Vec<Meeting>,
    error: Option<String>,
    /// The last recording that could not start or finish.
    pub(super) runtime_error: Option<String>,
    pub(super) search: TextEdit,
    pub(super) selected: Option<String>,
    pub(super) renaming: Option<String>,
    pub(super) rename: TextEdit,
    pub(super) delete_armed: Option<String>,
    action_error: Option<String>,
    /// What was last copied, for the button's "Copied".
    copied: Option<(String, String)>,
    pub(super) start_pending: bool,
    pub(super) stop_pending: bool,
    /// Where Linux and Windows perform meeting requests.
    pub(super) local: super::meetings_host::LocalMeetings,
}

/// `42 min`, `1 h 05 min`, or `< 1 min`.
fn length(milliseconds: u64) -> String {
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

/// `mm:ss`, as `meeting::format_duration`.
fn clock_offset(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Speaker turns as Markdown, as `meeting::transcript_markdown`.
fn transcript_markdown(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(|turn| {
            format!(
                "**{}** · {}\n{}",
                if turn.mine { "You" } else { "Them" },
                clock_offset(turn.start_ms),
                turn.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl MeetingsPane {
    fn meeting(&self, id: &str) -> Option<&Meeting> {
        self.meetings.iter().find(|meeting| meeting.id == id)
    }

    fn active(&self) -> Option<&Meeting> {
        self.meetings
            .iter()
            .find(|meeting| meeting.status.is_active())
    }

    fn visible(&self) -> Vec<&Meeting> {
        let query = self.search.text.trim().to_lowercase();
        self.meetings
            .iter()
            .filter(|meeting| query.is_empty() || meeting.searchable().contains(&query))
            .collect()
    }

    /// Adopts a fresh listing: follows a recording that just started,
    /// settles the Start/Stop buttons, and drops a selection that is gone.
    pub(super) fn adopt(&mut self, listed: Result<Vec<Meeting>, String>) {
        let meetings = match listed {
            Ok(meetings) => meetings,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let active = meetings.iter().find(|meeting| meeting.status.is_active());
        let previous = self.active().map(|meeting| meeting.id.clone());
        if let Some(active) = active {
            self.start_pending = false;
            if previous.as_deref() != Some(active.id.as_str()) {
                self.select(active.id.clone());
            }
        }
        if active.is_none_or(|meeting| meeting.status == Status::Transcribing) {
            self.stop_pending = false;
        }
        if let Some(selected) = &self.selected
            && !meetings.iter().any(|meeting| &meeting.id == selected)
        {
            self.selected = None;
        }
        self.meetings = meetings;
        self.error = None;
    }

    pub(super) fn select(&mut self, id: String) {
        if self.selected.as_deref() != Some(id.as_str()) {
            self.renaming = None;
            self.delete_armed = None;
            self.action_error = None;
        }
        self.selected = Some(id);
    }

    /// One recording, one with notes, one without, and one that failed.
    pub(super) fn fixture() -> Self {
        let turn = |mine, seconds: u64, text: &str| Turn {
            mine,
            start_ms: seconds * 1_000,
            text: text.into(),
            pause_before_ms: None,
        };
        let mut conversation = vec![
            turn(
                false,
                1,
                "Basically, I want Anomaly Arcade to be one login where you add friends and jump into any game.",
            ),
            turn(false, 4, "I need to finish my StarCraft game, too."),
            turn(true, 9, "Oh nice. How far along is it?"),
            turn(
                false,
                13,
                "Close. I feel like over a couple of months we could make twenty to thirty games.",
            ),
            turn(true, 20, "That's basically a new version of Steam."),
            turn(
                false,
                24,
                "If we're seeing Roblox, the idea should be to become Roblox.",
            ),
            turn(
                true,
                31,
                "Apple has a new thing where you hold two phones together and there's this pulse and gravity well. Could be a nice mini gravity well for the transitions.",
            ),
            turn(false, 40, "Prototype it in WebGPU, whatever's faster."),
            turn(
                true,
                95,
                "The build screen looks rough, by the way. We need a proposal for it.",
            ),
            turn(
                false,
                101,
                "Agreed. Write it up and I'll review it tomorrow.",
            ),
        ];
        conversation[8].pause_before_ms = Some(55_000);
        let meeting =
            |id: &str, title: &str, status, day: &str, clock: &str, minutes: u64| Meeting {
                id: id.into(),
                title: title.into(),
                status,
                day: day.into(),
                clock: clock.into(),
                duration_ms: minutes * 60_000 + 20_000,
                app: None,
                summary: None,
                notes_running: false,
                has_markdown: false,
                error: None,
                export_error: None,
                notes_error: None,
                live_error: None,
                transcript: Transcript::Loaded(Vec::new()),
                chapters: Vec::new(),
            };
        let mut live = meeting(
            "preview-live",
            "Zoom call — 14:05",
            Status::Recording,
            "Today",
            "14:05",
            7,
        );
        live.app = Some("Zoom".into());
        live.transcript = Transcript::Loaded(conversation[..6].to_vec());
        let mut notes = meeting(
            "preview-notes",
            "Arcade launch plan",
            Status::Complete,
            "Today",
            "11:12",
            38,
        );
        notes.app = Some("Zoom".into());
        notes.has_markdown = true;
        notes.summary = Some(
            "One login for every Anomaly Arcade game, aiming for twenty to thirty games over the next couple of months. The build screen needs a proposal before more polish."
                .into(),
        );
        notes.transcript = Transcript::Loaded(conversation.clone());
        let mut design = meeting(
            "preview-design",
            "Design review",
            Status::Complete,
            "Yesterday",
            "16:30",
            24,
        );
        design.app = Some("FaceTime".into());
        design.transcript = Transcript::Loaded(conversation[2..6].to_vec());
        let mut failed = meeting(
            "preview-failed",
            "Meeting — 09:58",
            Status::Failed,
            "Yesterday",
            "09:58",
            2,
        );
        failed.error = Some("ScreenCaptureKit stopped the capture.".into());
        Self {
            meetings: vec![live, notes, design, failed],
            selected: Some("preview-notes".into()),
            ..Self::default()
        }
    }

    /// The fixture with live chapters (Labs) on the
    /// meeting with notes, its title extended by them.
    pub(super) fn chapters_fixture() -> Self {
        let mut pane = Self::fixture();
        let chapters = [
            (
                "One login",
                "One login for every Arcade game, twenty to thirty games in a couple of months.",
                0,
            ),
            (
                "Gravity well",
                "A pulse and gravity well for transitions, prototyped in WebGPU.",
                31_000,
            ),
            (
                "Build screen",
                "The build screen needs a written proposal.",
                95_000,
            ),
        ];
        if let Some(notes) = pane.meetings.iter_mut().find(|m| m.id == "preview-notes") {
            notes.title = "Arcade launch plan: One login, Gravity well, Build screen".into();
            let turns = notes.turns().to_vec();
            notes.chapters = chapters
                .iter()
                .enumerate()
                .map(|(index, &(title, summary, start_ms))| {
                    let end_ms = chapters.get(index + 1).map(|next| next.2);
                    let inside = turns
                        .iter()
                        .filter(|turn| {
                            turn.start_ms >= start_ms
                                && end_ms.is_none_or(|end| turn.start_ms < end)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    Chapter {
                        title: title.into(),
                        summary: summary.into(),
                        start_ms,
                        markdown: format!(
                            "## {title}\n\n{summary}\n\n{}",
                            transcript_markdown(&inside)
                        )
                        .trim_end()
                        .into(),
                    }
                })
                .collect();
        }
        pane
    }
}

fn ask(ui: &mut Ui, request: MeetingRequest) -> bool {
    match ui.host.send(PlatformRequest::Meeting(request)) {
        Ok(()) => true,
        Err(error) => {
            ui.meetings.action_error = Some(error);
            false
        }
    }
}

fn start(ui: &mut Ui) {
    ui.meetings.start_pending = ask(ui, MeetingRequest::Start);
}

fn stop(ui: &mut Ui) {
    ui.meetings.stop_pending = ask(ui, MeetingRequest::Stop);
}

fn start_rename(ui: &mut Ui, id: String) {
    let Some(meeting) = ui.meetings.meeting(&id) else {
        return;
    };
    ui.meetings.rename = TextEdit::new(meeting.title.clone());
    ui.meetings.renaming = Some(id);
    ui.open = None;
}

fn finish_rename(ui: &mut Ui) {
    let Some(id) = ui.meetings.renaming.take() else {
        return;
    };
    let title = ui.meetings.rename.text.trim().to_string();
    if title.is_empty() {
        return;
    }
    if ask(
        ui,
        MeetingRequest::Rename {
            id: id.clone(),
            title: title.clone(),
        },
    ) && ui.host.is_fixture()
        && let Some(meeting) = ui.meetings.meetings.iter_mut().find(|m| m.id == id)
    {
        meeting.title = title;
    }
}

/// The first press arms the confirmation; the confirmation trashes.
fn trash(ui: &mut Ui, id: String) {
    ui.open = None;
    if ui.meetings.delete_armed.as_deref() != Some(id.as_str()) {
        ui.meetings.delete_armed = Some(id);
        return;
    }
    ui.meetings.delete_armed = None;
    if ask(ui, MeetingRequest::Trash(id.clone())) && ui.host.is_fixture() {
        ui.meetings.meetings.retain(|meeting| meeting.id != id);
        ui.meetings.selected = None;
    }
}

fn copy(ui: &mut Ui, tx: &mut Tx, id: String, what: impl Into<String>, text: String) {
    tx.copy(text);
    ui.meetings.copied = Some((id, what.into()));
}

/// The status tag; `Ready` only in the detail header, so the list stays
/// quiet for finished meetings.
/// The detail's status, while it is one worth a tag.
pub(super) fn status_tag(p: Paint, status: Status, stop_pending: bool) -> Option<El> {
    let (label, tone) = match status {
        Status::Recording if stop_pending => (tr("Transcribing"), Tone::Accent),
        Status::Starting | Status::Recording => (tr("Recording"), Tone::Danger),
        Status::Transcribing => (tr("Transcribing"), Tone::Accent),
        Status::Complete => return None,
        Status::Interrupted => (tr("Interrupted"), Tone::Neutral),
        Status::Failed => (tr("Failed"), Tone::Danger),
    };
    Some(tone_tag(p, "status", label, tone))
}

/// The opt-in card while Meetings is off; otherwise the header's view
/// switch and Start/Stop, then the recordings or the settings.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    if !available(ui) {
        let content = page("content").child(card(
            p,
            "enable",
            vec![super::platform_settings::meetings_toggle(ui, p)],
        ));
        return (Vec::new(), super::scroll_body("scroll", content));
    }
    let action = recording_action(ui, p);
    if ui.meetings.view == View::Settings {
        let content =
            page("content").children(super::platform_settings::meetings_settings(ui, p, layer));
        return (vec![action], super::scroll_body("settings-scroll", content));
    }
    recordings(ui, p, layer, action)
}

/// The header's `Recordings | Settings` switch, beside the title so it stays
/// put whichever view shows; none while the opt-in card does.
pub(super) fn view_switch(ui: &Ui, p: Paint) -> Option<El> {
    if !available(ui) {
        return None;
    }
    let views = [tr("Recordings"), tr("Settings")];
    let selected = match ui.meetings.view {
        View::Recordings => 0,
        View::Settings => 1,
    };
    Some(segmented_tx(
        p,
        "meetings-view",
        &views,
        selected,
        |ui: &mut Ui, index: usize, tx: &mut Tx| {
            let view = if index == 0 {
                View::Recordings
            } else {
                View::Settings
            };
            if ui.meetings.view != view {
                tx.settle();
                ui.flush_drafts();
                ui.open = None;
                ui.meetings.view = view;
                ui.meetings.renaming = None;
                ui.meetings.delete_armed = None;
            }
        },
    ))
}

fn search(ui: &Ui, p: Paint) -> El {
    TextInput::new(&ui.meetings.search)
        .placeholder(tr("Search meetings"))
        .width(None)
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.meetings.search)
        .build(p.k)
        .key("meetings-search")
}

/// Start recording, or the active recording's elapsed time and Stop.
fn recording_action(ui: &Ui, p: Paint) -> El {
    let pane = &ui.meetings;
    let busy = |label: &str| {
        text(label)
            .key("meeting-busy")
            .font(DESCRIPTION)
            .color(p.muted())
    };
    let active = pane.active();
    match active.map(|meeting| meeting.status) {
        _ if pane.stop_pending => busy(tr("Finalizing…")),
        Some(Status::Transcribing) => busy(tr("Finalizing…")),
        Some(Status::Starting) => busy(tr("Starting…")),
        // Recording is a state, not the screen's main action.
        Some(Status::Recording) => row()
            .key("stop-meeting-control")
            .gap(12.0)
            .center_y()
            .child(
                row()
                    .key("elapsed-group")
                    .gap(6.0)
                    .center_y()
                    .child(pulsing_dot(p.danger()))
                    .children(active.map(|meeting| {
                        text(clock_offset(meeting.duration_ms))
                            .key("elapsed")
                            .font(p.k.ty.mono)
                            .color(p.muted())
                    })),
            )
            .child(button(
                p,
                "stop-meeting",
                tr("Stop & transcribe"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| stop(ui),
            )),
        _ if pane.start_pending => busy(tr("Starting…")),
        _ => button(
            p,
            "start-meeting",
            tr("Start recording"),
            ButtonKind::Primary,
            |ui: &mut Ui, _| start(ui),
        ),
    }
}

/// A row's state line: what went wrong or is happening, else the app and
/// length.
fn row_meta(pane: &MeetingsPane, meeting: &Meeting, p: Paint) -> (String, Option<rune::Color>) {
    let (state, ink) = match meeting.status {
        Status::Recording if pane.stop_pending => (Some(tr("Transcribing")), None),
        Status::Starting | Status::Recording => (Some(tr("Recording")), Some(p.danger())),
        Status::Transcribing => (Some(tr("Transcribing")), None),
        Status::Failed => (Some(tr("Failed")), Some(p.danger())),
        Status::Interrupted => (Some(tr("Interrupted")), None),
        Status::Complete => (None, None),
    };
    let mut parts: Vec<String> = state.map(String::from).into_iter().collect();
    parts.extend(meeting.app.clone());
    if !meeting.status.is_active() {
        parts.push(length(meeting.duration_ms));
    }
    (parts.join(" · "), ink)
}

/// Copy transcript as an icon: a completed row's hover action, or the
/// card's.
fn copy_action(p: Paint, pane: &MeetingsPane, meeting: &Meeting, what: &'static str) -> El {
    let copied = pane
        .copied
        .as_ref()
        .is_some_and(|(id, copied)| id == &meeting.id && copied == what);
    let glyph = if copied {
        p.k.glyph.check_small
    } else {
        ld::COPY_ICON
    };
    let id = meeting.id.clone();
    let press = move |ui: &mut Ui, tx: &mut Tx| {
        if let Some(meeting) = ui.meetings.meeting(&id) {
            let markdown = transcript_markdown(meeting.turns());
            copy(ui, tx, id.clone(), what, markdown);
        }
    };
    if what == "row" {
        ld::row_action(p, "row-copy", glyph, tr("Copy transcript"), press)
    } else {
        ld::ghost_icon(
            p,
            "meeting-copy-transcript",
            glyph,
            tr("Copy transcript"),
            press,
        )
    }
}

/// The day-grouped list and the selected meeting's card.
fn recordings(ui: &Ui, p: Paint, layer: &mut Vec<El>, action: El) -> (Vec<El>, El) {
    let pane = &ui.meetings;
    let mut notices = Vec::new();
    if let Some(error) = &pane.error {
        notices.push(error_panel(
            p,
            "meetings-error",
            tr("Meetings could not be loaded."),
            error,
        ));
    }
    if let Some(error) = &pane.runtime_error {
        notices.push(error_panel(
            p,
            "meeting-runtime-error",
            tr("Meeting could not be recorded."),
            error,
        ));
    }
    let body = if pane.meetings.is_empty() {
        ld::empty_state(
            p,
            "meetings-empty",
            ld::HISTORY_ICON,
            tr("No meetings yet."),
            Some(tr("Record a call to see its notes and transcript here.")),
        )
    } else {
        let mut list = col().key("meetings-list");
        let mut day: Option<&str> = None;
        for (index, meeting) in pane.visible().into_iter().enumerate() {
            if day != Some(meeting.day.as_str()) {
                list = list.child(ld::group(
                    p,
                    &format!("day{index}"),
                    day_label(&meeting.day),
                    day.is_none(),
                ));
                day = Some(&meeting.day);
            }
            let (meta, meta_ink) = row_meta(pane, meeting, p);
            let completed = meeting.status == Status::Complete && !meeting.turns().is_empty();
            let id = meeting.id.clone();
            list = list.child(ld::item(
                p,
                Item {
                    key: format!("meeting-{}", meeting.id),
                    title: meeting.title.clone(),
                    meta,
                    meta_ink,
                    time: meeting.clock.clone(),
                    hover: completed
                        .then(|| copy_action(p, pane, meeting, "row"))
                        .into_iter()
                        .collect(),
                    selected: pane.selected.as_deref() == Some(meeting.id.as_str()),
                },
                move |ui: &mut Ui| ui.meetings.select(id.clone()),
            ));
        }
        let detail = match pane.selected.as_deref().and_then(|id| pane.meeting(id)) {
            Some(meeting) => detail(ui, p, meeting, layer),
            None if pane.visible().is_empty() => ld::empty_state(
                p,
                "meetings-empty",
                ld::SEARCH_ICON,
                tr("No meetings match."),
                None,
            ),
            None => ld::empty_state(
                p,
                "meeting-detail",
                ld::HISTORY_ICON,
                tr("Select a meeting."),
                None,
            ),
        };
        let column = ld::list_column(search(ui, p), "meetings-scroll", list);
        ld::body(p, "meetings-body", column, detail)
    };
    (
        vec![action],
        col()
            .key("meetings-pane")
            .grow(1.0)
            .shrink()
            .children((!notices.is_empty()).then(|| ld::notices(notices)))
            .child(body),
    )
}

fn day_label(day: &str) -> &str {
    match day {
        "Today" => tr("Today"),
        "Yesterday" => tr("Yesterday"),
        other => other,
    }
}

fn detail(ui: &Ui, p: Paint, meeting: &Meeting, layer: &mut Vec<El>) -> El {
    let pane = &ui.meetings;
    let active = meeting.status.is_active();
    let copied = |what: &str| {
        pane.copied
            .as_ref()
            .is_some_and(|(id, copied)| id == &meeting.id && copied == what)
    };
    let armed = pane.delete_armed.as_deref() == Some(meeting.id.as_str());
    let renaming = pane.renaming.as_deref() == Some(meeting.id.as_str());
    let mut actions = Vec::new();
    let mut confirm = None;
    if armed && !active && !meeting.notes_running {
        // The confirmation sits under the title until it is answered.
        let id = meeting.id.clone();
        confirm = Some(
            row()
                .key("delete-confirm")
                .gap(8.0)
                .pt(14.0)
                .child(
                    button(
                        p,
                        "meeting-confirm-delete",
                        if meeting.has_markdown {
                            tr("Trash recording & Markdown")
                        } else {
                            tr("Trash recording")
                        },
                        ButtonKind::Danger,
                        move |ui: &mut Ui, _| trash(ui, id.clone()),
                    )
                    .default_button(),
                )
                .child(button(
                    p,
                    "meeting-cancel-delete",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.meetings.delete_armed = None,
                )),
        );
    } else if !renaming {
        // The title field brings its own Save and Cancel.
        if !meeting.turns().is_empty() {
            actions.push(copy_action(p, pane, meeting, "transcript"));
        }
        if !active {
            let id = meeting.id.clone();
            actions.push(ld::ghost_icon(
                p,
                "meeting-rename",
                ld::EDIT_ICON,
                tr("Rename"),
                move |ui: &mut Ui, _| start_rename(ui, id.clone()),
            ));
        }
        actions.extend(overflow(ui, p, meeting, layer));
    }

    let title = if renaming {
        row()
            .key("rename")
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                TextInput::new(&pane.rename)
                    .width(Some(220.0))
                    .on_edit(|ui: &mut Ui| &mut ui.meetings.rename)
                    .on_submit(|ui: &mut Ui, _| finish_rename(ui))
                    .build(p.k)
                    .key("rename-field"),
            )
            .child(button(
                p,
                "meeting-save-title",
                tr("Save"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| finish_rename(ui),
            ))
            .child(button(
                p,
                "meeting-cancel-title",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.meetings.renaming = None,
            ))
    } else {
        ld::card_title(p, &meeting.title)
    };
    let mut meta_line = vec![
        day_label(&meeting.day).to_string(),
        meeting.clock.clone(),
        clock_offset(meeting.duration_ms),
    ];
    meta_line.extend(meeting.app.clone());
    let mut meta: Vec<El> = status_tag(p, meeting.status, pane.stop_pending)
        .into_iter()
        .collect();
    meta.push(ld::meta_text(p, &meta_line.join(" · ")));

    let errors = [
        pane.action_error
            .as_deref()
            .map(|error| ("action-error", tr("Meeting could not be updated."), error)),
        meeting
            .error
            .as_deref()
            .map(|error| ("recording-error", tr("Recording failed."), error)),
        meeting
            .export_error
            .as_deref()
            .map(|error| ("export-error", tr("Markdown not saved."), error)),
        meeting
            .notes_error
            .as_deref()
            .filter(|_| !meeting.notes_running)
            .map(|error| ("notes-error", tr("Notes could not be written."), error)),
        meeting
            .live_error
            .as_deref()
            .filter(|_| active)
            .map(|error| ("live-error", tr("Live transcript unavailable."), error)),
    ];
    let mut body = col().key("detail-body").gap(24.0).pb(8.0);
    for (key, title, error) in errors.into_iter().flatten() {
        body = body.child(error_panel(p, key, title, error));
    }
    if let Some(summary) = &meeting.summary {
        let summary = summary.trim();
        body = body.child(
            col()
                .key("notes")
                .gap(6.0)
                .child(section_label(p, "summary-label", tr("Summary")))
                .child(
                    text(if summary.is_empty() {
                        tr("Nothing to summarize.")
                    } else {
                        summary
                    })
                    .key("summary")
                    .font(p.k.ty.body)
                    .color(if summary.is_empty() {
                        p.faint()
                    } else {
                        p.text()
                    })
                    .wrap(),
                ),
        );
    } else if meeting.notes_running {
        body = body.child(
            text(tr("Writing a title and summary with Apple Intelligence…"))
                .key("notes-running")
                .font(DESCRIPTION)
                .color(p.faint()),
        );
    }
    if !meeting.chapters.is_empty() {
        body = body.child(chapters(p, meeting, &copied));
    }
    let transcript_label = match meeting.status {
        Status::Starting | Status::Recording if !pane.stop_pending => tr("Live transcript"),
        Status::Starting | Status::Recording | Status::Transcribing => tr("Draft transcript"),
        Status::Complete => tr("Transcript"),
        Status::Interrupted | Status::Failed => tr("Recovered draft"),
    };
    body = body.child(
        col()
            .key("transcript-block")
            .gap(10.0)
            .child(section_label(p, "transcript-label", transcript_label))
            .child(transcript(p, meeting, pane.stop_pending)),
    );
    col()
        .key("meeting-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(title, meta, actions))
        .children(confirm)
        .child(ld::card_scroll("meeting-transcript", body))
}

/// The meeting's chapters, each with its start, name, line, and Copy
/// section.
fn chapters(p: Paint, meeting: &Meeting, copied: &dyn Fn(&str) -> bool) -> El {
    let mut list = col()
        .key("chapters")
        .gap(10.0)
        .pb(16.0)
        .child(section_label(p, "chapters-label", tr("Chapters")));
    for (index, chapter) in meeting.chapters.iter().enumerate() {
        let what = format!("chapter{index}");
        let id = meeting.id.clone();
        let mut words = col().key("words").gap(2.0).grow(1.0).shrink().child(
            text(chapter.title.clone())
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap(),
        );
        if !chapter.summary.trim().is_empty() {
            words = words.child(
                text(chapter.summary.trim())
                    .key("summary")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
            );
        }
        list = list.child(
            row()
                .key(format!("chapter{index}"))
                .gap(12.0)
                .child(
                    text(clock_offset(chapter.start_ms))
                        .key("clock")
                        .font(p.k.ty.mono)
                        .color(p.faint())
                        .pt(1.0),
                )
                .child(words)
                .child(
                    ld::ghost_icon(
                        p,
                        "copy-section",
                        if copied(&what) {
                            p.k.glyph.check_small
                        } else {
                            ld::COPY_ICON
                        },
                        tr("Copy section"),
                        move |ui: &mut Ui, tx: &mut Tx| {
                            if let Some(markdown) = ui
                                .meetings
                                .meeting(&id)
                                .and_then(|meeting| meeting.chapters.get(index))
                                .map(|chapter| chapter.markdown.clone())
                            {
                                copy(ui, tx, id.clone(), what.clone(), markdown);
                            }
                        },
                    )
                    .mt(-4.0),
                ),
        );
    }
    list
}

/// Less common actions behind the ellipsis, Delete last.
fn overflow(ui: &Ui, p: Paint, meeting: &Meeting, layer: &mut Vec<El>) -> Option<El> {
    let active = meeting.status.is_active();
    let mut menu = KitMenu::new("meeting-actions")
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(200.0)
        .open(ui.open == Some(Menu::MeetingActions))
        .on_close(|ui: &mut Ui| ui.open = None);
    let mut items = 0;
    if let Some(summary) = meeting.summary.clone() {
        let id = meeting.id.clone();
        let copied = ui
            .meetings
            .copied
            .as_ref()
            .is_some_and(|(copied, what)| copied == &meeting.id && what == "notes");
        // Stays open so "Copied" confirms the copy.
        menu = menu.item(
            MenuItem::new(if copied {
                tr("Copied")
            } else {
                tr("Copy summary")
            })
            .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
                copy(ui, tx, id.clone(), "notes", summary.trim().to_string());
                ui.open = Some(Menu::MeetingActions);
            }),
        );
        items += 1;
    }
    if !active {
        let id = meeting.id.clone();
        menu = menu.item(
            MenuItem::new(if meeting.has_markdown {
                tr("Reveal Markdown")
            } else {
                tr("Reveal files")
            })
            .on_select(move |ui: &mut Ui| {
                ui.open = None;
                ask(ui, MeetingRequest::Reveal(id.clone()));
            }),
        );
        items += 1;
    }
    if meeting.status == Status::Complete && notes_available(ui) {
        if meeting.notes_running {
            menu = menu.item(MenuItem::new(tr("Summarizing…")).disabled(true));
        } else {
            let id = meeting.id.clone();
            menu = menu.item(
                MenuItem::new(if meeting.summary.is_some() {
                    tr("Regenerate summary")
                } else {
                    tr("Summarize")
                })
                .on_select(move |ui: &mut Ui| {
                    ui.open = None;
                    ask(ui, MeetingRequest::Summarize(id.clone()));
                }),
            );
        }
        items += 1;
    }
    if !active && !meeting.notes_running {
        if items > 0 {
            menu = menu.separator();
        }
        let id = meeting.id.clone();
        menu = menu
            .item(MenuItem::new(tr("Delete…")).on_select(move |ui: &mut Ui| trash(ui, id.clone())));
        items += 1;
    }
    if items == 0 {
        return None;
    }
    let trigger = IconButton::new(&MORE_ICON)
        .variant(IconButtonVariant::Ghost)
        .control_size(rune_kit::ControlSize::Small)
        .label(tr("More"))
        .on_press(|ui: &mut Ui| {
            ui.open = (ui.open != Some(Menu::MeetingActions)).then_some(Menu::MeetingActions);
        })
        .build(p.k)
        .key("meeting-more");
    Some(menu.trigger(trigger).build(p.k, layer))
}

fn transcript(p: Paint, meeting: &Meeting, stop_pending: bool) -> El {
    let empty = |copy: &str| {
        text(copy)
            .key("transcript-empty")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap()
    };
    let turns = match &meeting.transcript {
        Transcript::Unavailable(error) => {
            return error_panel(p, "transcript-error", tr("Transcript unavailable."), error);
        }
        Transcript::Loaded(turns) => turns,
    };
    if turns.is_empty() {
        return empty(match meeting.status {
            Status::Starting => tr("Preparing local audio capture…"),
            Status::Recording if meeting.live_error.is_some() => {
                tr("Live transcription is unavailable. Audio is still recording safely.")
            }
            Status::Recording if stop_pending => tr("Building the final transcript…"),
            Status::Recording => tr("Listening for speech…"),
            Status::Transcribing => tr("Building the final transcript…"),
            Status::Complete => tr("No speech was transcribed."),
            Status::Interrupted | Status::Failed => tr("No recoverable transcript is available."),
        });
    }
    let mut lines = col().key("turns").gap(14.0);
    for (index, turn) in turns.iter().enumerate() {
        lines = lines.children(turn_line(p, index, turn));
    }
    if meeting.status == Status::Recording && !stop_pending {
        lines = lines.child(
            text(tr("Listening…"))
                .key("listening")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    lines
}

/// One turn of a transcript: the pause before it, if any, then the speaker,
/// the time, and the words.
pub(super) fn turn_line(p: Paint, index: usize, turn: &Turn) -> Vec<El> {
    let mut els = Vec::new();
    if let Some(pause) = turn.pause_before_ms {
        els.push(
            text(tr_fill("{} pause", &length(pause)))
                .key(format!("pause{index}"))
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    els.push(
        col()
            .key(format!("turn{index}"))
            .gap(2.0)
            .child(
                row()
                    .key("speaker")
                    .gap(8.0)
                    .child(
                        text(if turn.mine { tr("You") } else { tr("Them") })
                            .key("name")
                            .font(p.k.ty.small_medium)
                            .color(if turn.mine { p.text() } else { p.faint() }),
                    )
                    .child(
                        text(clock_offset(turn.start_ms))
                            .key("clock")
                            .font(p.k.ty.small)
                            .color(p.faint()),
                    ),
            )
            .child(
                text(turn.text.clone())
                    .key("text")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap(),
            ),
    );
    els
}
