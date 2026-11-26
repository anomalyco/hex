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
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Loader, Menu as KitMenu,
    MenuItem, Placement, TextInput,
};

use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, card, page, pulsing_dot, segmented_tx,
};
use super::list_detail::{self as ld, Item};
use super::lists::{Tone, dismissible_error_panel, error_panel, tone_tag};
use super::notes_writer::NotesWriter;
use super::platform::MeetingRequest;
use super::{Menu, Ui};
use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill, tr_named};

pub(in crate::rune_ui) mod audio_source;
mod detail;
mod fixture;
mod selection;

use detail::*;

pub(super) static MORE_ICON: rune::Icon = rune::Icon {
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
    /// The published lines this displayed turn covers. Adjacent same-speaker
    /// entries within three seconds coalesce here, so deleting the turn
    /// deletes every line under it.
    pub(super) entries: Vec<crate::meeting::TranscriptEntryKey>,
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
    /// The exported Markdown while it still exists, which `Reveal Markdown`
    /// selects and Trash takes too; otherwise the recording folder, which
    /// only labels Trash.
    pub(super) reveal: crate::meeting::RevealTarget,
    /// The meeting's recording folder, which `Reveal files` always opens.
    pub(super) folder: std::path::PathBuf,
    pub(super) error: Option<String>,
    pub(super) export_error: Option<String>,
    pub(super) notes_error: Option<String>,
    pub(super) live_error: Option<String>,
    /// The microphone was digitally silent for part of this recording.
    pub(super) microphone_warning: Option<crate::meeting::TrackWarning>,
    pub(super) transcript: Transcript,
    pub(super) chapters: Vec<Chapter>,
    /// Imported from an audio file (Labs › Import audio): its one track is
    /// labelled Speaker.
    pub(super) imported: bool,
    /// How long it is kept once finished, as `MeetingRetention::label`;
    /// `None` when it is kept for good.
    pub(super) kept_for: Option<&'static str>,
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
    crate::labs::Feature::Meetings.managed().is_none()
        && (ui.settings.meetings.enabled || ui.platform.developer)
}

/// Whether something can write a meeting's notes when asked: OpenCode once
/// Summaries chooses it, otherwise Apple Intelligence on a Mac that has it.
fn notes_available(ui: &Ui) -> bool {
    ui.settings.meetings.summaries == crate::app_settings::MeetingSummaries::OpenCode
        || apple_intelligence_here(ui)
}

/// Whether this Mac has Apple Intelligence: the other half of the writer
/// choice, which [`NotesWriter`] makes with the Summaries setting.
fn apple_intelligence_here(ui: &Ui) -> bool {
    ui.platform.caps.os == super::platform::Os::Macos && ui.platform.notes.is_available()
}

/// What writes the notes now, as the generator picks it.
fn notes_running_label(ui: &Ui) -> &'static str {
    NotesWriter::for_choice(ui.settings.meetings.summaries, apple_intelligence_here(ui))
        .running_label()
}

#[derive(Default)]
pub(super) struct MeetingsPane {
    pub(super) view: View,
    /// Newest first, as the store lists them.
    pub(super) meetings: Vec<Meeting>,
    error: Option<String>,
    /// The last recording that could not start or finish.
    pub(super) runtime_error: Option<String>,
    /// The last audio file that could not be imported (Labs › Import audio).
    pub(super) import_error: Option<String>,
    /// The audio file dialog while it is open.
    import_chooser: Option<Pending>,
    pub(super) search: TextEdit,
    pub(super) selected: Option<String>,
    pub(super) renaming: Option<String>,
    pub(super) rename: TextEdit,
    pub(super) delete_armed: Option<String>,
    /// The selected transcript lines (Labs › Delete transcript lines).
    pub(super) selection: selection::Selection,
    action_error: Option<String>,
    /// The last transcript edit that failed.
    pub(super) edit_error: Option<String>,
    /// What was last copied, for the button's "Copied".
    copied: Option<(String, String)>,
    pub(super) start_pending: bool,
    pub(super) stop_pending: bool,
    /// Where Linux and Windows perform meeting requests.
    #[cfg(not(target_os = "macos"))]
    pub(super) local: super::meetings_host::LocalMeetings,
}

/// `42 min`, `1 h 05 min`, or `< 1 min`.
pub(super) fn length(milliseconds: u64) -> String {
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

/// Speaker turns as Markdown, delegating to the canonical meeting renderer.
fn transcript_markdown(turns: &[Turn], imported: bool) -> String {
    crate::meeting::render_turns_markdown(
        turns.iter().map(|turn| {
            let source = if turn.mine {
                crate::meeting::MeetingSource::Microphone
            } else {
                crate::meeting::MeetingSource::System
            };
            (source, turn.start_ms, turn.text.as_str())
        }),
        imported,
    )
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
            self.selection.stop();
        }
        self.meetings = meetings;
        self.error = None;
    }

    pub(super) fn select(&mut self, id: String) {
        if self.selected.as_deref() != Some(id.as_str()) {
            self.renaming = None;
            self.delete_armed = None;
            self.action_error = None;
            self.selection.stop();
        }
        self.selected = Some(id);
    }
}

/// Sends `request` where it is carried out (on Linux and Windows, the
/// window's own meetings, not the OS host, which ignores meeting requests).
pub(super) fn ask(ui: &mut Ui, request: MeetingRequest) -> bool {
    match ui.send_meeting(request) {
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
    ui.meetings.rename = TextEdit::new(shown_title(meeting).to_owned());
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

/// The first press arms the confirmation and focuses its Cancel; the
/// confirmation trashes.
fn trash(ui: &mut Ui, tx: &mut Tx, id: String) {
    ui.open = None;
    if ui.meetings.delete_armed.as_deref() != Some(id.as_str()) {
        ui.meetings.delete_armed = Some(id);
        tx.focus("meeting-cancel-delete", tx.focus_visible());
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

/// Escape in the Meetings pane: the selection's confirmation or selection
/// itself, a rename, or a meeting delete's confirmation. False when none is
/// waiting.
pub(super) fn cancel_edit(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.meetings.selection.escape() {
        return true;
    }
    let renaming = ui.meetings.renaming.take().is_some();
    renaming | cancel_delete(ui, tx)
}

/// Escape or Cancel: the meeting delete stops asking, and focus returns to
/// the ⋯ menu it came from.
fn cancel_delete(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.meetings.delete_armed.take().is_some() {
        tx.focus("meeting-more", tx.focus_visible());
        true
    } else {
        false
    }
}

/// The opt-in card while Meetings is off; otherwise the header's view
/// switch and Start/Stop, then the recordings or the settings.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    if !available(ui) {
        if crate::labs::Feature::Meetings.managed().is_none() {
            return (Vec::new(), off_state(p));
        }
        let content = page("content").child(card(
            p,
            "enable",
            vec![super::platform_settings::meetings_toggle(ui, p)],
        ));
        return (Vec::new(), super::scroll_body(p.k, "scroll", content));
    }
    let action = recording_action(ui, p);
    if ui.meetings.view == View::Settings {
        let content =
            page("content").children(super::platform_settings::meetings_settings(ui, p, layer));
        return (
            vec![action],
            super::scroll_body(p.k, "settings-scroll", content),
        );
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
    let action = match active.map(|meeting| meeting.status) {
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
                        text(crate::meeting::format_duration(meeting.duration_ms))
                            .key("elapsed")
                            .font(p.clock())
                            .color(p.muted())
                    })),
            )
            .children(mute_control(p))
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
    };
    // Importing is a second way in, never a competing main action.
    if crate::labs::meeting_import() && active.is_none() && !pane.start_pending {
        return row()
            .key("meeting-actions")
            .gap(8.0)
            .center_y()
            .child(button(
                p,
                "import-meeting",
                tr("Import audio…"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| choose_import(ui),
            ))
            .child(action);
    }
    action
}

/// Labs › Mute the microphone while recording: the recording's own
/// microphone switch beside Stop, only while that lab is on. The call's audio
/// keeps recording, so the recording never stops for it; the label reads the
/// switch the controller set, which the recording's own frames pick up.
fn mute_control(p: Paint) -> Option<El> {
    crate::labs::meeting_microphone_mute().then(|| {
        let muted = crate::meeting::microphone_muted();
        button(
            p,
            "mute-meeting-microphone",
            if muted {
                tr("Unmute microphone")
            } else {
                tr("Mute microphone")
            },
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| {
                ask(ui, MeetingRequest::SetMicrophoneMuted(!muted));
            },
        )
    })
}

/// The Labs › Import audio file dialog: one supported audio file.
fn choose_import(ui: &mut Ui) {
    if ui.meetings.import_chooser.is_some() {
        return;
    }
    ui.meetings.action_error = None;
    let waker = ui.modes.waker.clone();
    let wake = move || {
        if let Some(waker) = &waker {
            waker();
        }
    };
    ui.meetings.import_chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Import audio").into(),
            target: Target::File(vec![Filter {
                label: tr("Audio files").into(),
                extensions: &["wav", "m4a", "mp3", "aiff", "aif", "caf"],
            }]),
            directory: dirs::audio_dir(),
        },
        wake,
    ));
}

/// The audio file dialog's answer: a chosen file is imported where meetings
/// are carried out. True when the answer changed what the pane shows.
pub(super) fn poll_import(ui: &mut Ui) -> bool {
    let Some(choice) = ui.meetings.import_chooser.as_ref().and_then(Pending::poll) else {
        return false;
    };
    ui.meetings.import_chooser = None;
    match choice {
        Ok(Some(path)) => {
            ask(ui, MeetingRequest::Import(path));
        }
        Ok(None) => {}
        Err(error) => ui.meetings.action_error = Some(error),
    }
    true
}

/// An audio file could not be imported; the Meetings pane says why.
pub(crate) fn meeting_import_failed(ui: &mut Ui, error: String) {
    ui.meetings.import_error = Some(error);
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
        Status::Complete if meeting.notes_running => (Some(tr("Summarizing…")), None),
        Status::Complete => (None, None),
    };
    let mut parts: Vec<String> = state.map(String::from).into_iter().collect();
    parts.extend(meeting.app.clone());
    if !meeting.status.is_active() {
        parts.push(length(meeting.duration_ms));
    }
    (parts.join(" · "), ink)
}

/// The title shown in the list and detail header: drops the trailing
/// ` — <clock>` from Hex's default title (`"Zoom call — 14:05"` or
/// `"Meeting — 09:58"`) when it repeats `meeting.clock`, since both the row
/// and the detail meta already show the time.
fn shown_title(meeting: &Meeting) -> &str {
    let Some(prefix) = meeting
        .title
        .strip_suffix(meeting.clock.as_str())
        .and_then(|rest| rest.strip_suffix(" — "))
    else {
        return &meeting.title;
    };
    let is_default = match meeting.app.as_deref() {
        Some(app) => prefix
            .strip_suffix(" call")
            .is_some_and(|head| head.eq_ignore_ascii_case(app)),
        None => prefix == "Meeting",
    };
    if is_default { prefix } else { &meeting.title }
}

/// One concise muted line under a failed meeting row's status, falling back
/// to `"Couldn't record"` when no error text was stored.
fn failure_reason(meeting: &Meeting) -> Option<String> {
    if meeting.status != Status::Failed {
        return None;
    }
    let reason = meeting
        .error
        .as_deref()
        .and_then(|error| error.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| line.strip_suffix('.').unwrap_or(line));
    Some(match reason {
        Some(line) if !line.is_empty() => line.to_owned(),
        _ => tr("Couldn't record").to_owned(),
    })
}

/// The icon before a row's title: the call service's when its app names
/// one and a filled generic call mark otherwise (another app, or a meeting
/// started by hand).
fn platform_icon(meeting: &Meeting) -> &'static rune::Icon {
    match meeting
        .app
        .as_deref()
        .and_then(crate::calendar::CallProvider::from_meeting_app)
    {
        Some(provider) => super::platform_settings::provider_icon(provider),
        None => super::platform_settings::call_icon(),
    }
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
            let markdown = transcript_markdown(meeting.turns(), meeting.imported);
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
            Tone::Danger,
        ));
    }
    if let Some(error) = &pane.runtime_error {
        notices.push(dismissible_error_panel(
            p,
            "meeting-runtime-error",
            tr("Meeting could not be recorded."),
            error,
            |ui: &mut Ui| ui.meetings.runtime_error = None,
        ));
    }
    if let Some(error) = &pane.import_error {
        notices.push(dismissible_error_panel(
            p,
            "meeting-import-error",
            tr("Audio could not be imported."),
            error,
            |ui: &mut Ui| ui.meetings.import_error = None,
        ));
    }
    if let Some(error) = &pane.edit_error {
        notices.push(dismissible_error_panel(
            p,
            "meeting-edit-error",
            tr("Lines could not be deleted."),
            error,
            |ui: &mut Ui| ui.meetings.edit_error = None,
        ));
    }
    // Prototype: Hex can't hear the other side until System Audio
    // Recording is allowed.
    if ui.platform.caps.permissions && !ui.platform.system_audio {
        notices.push(system_audio_notice(p));
    }
    let empty = pane.meetings.is_empty();
    let body = if empty {
        empty_state(ui, p)
    } else {
        let mut list = col().key("meetings-list").label(tr("Meetings"));
        let mut day: Option<&str> = None;
        let visible = pane.visible();
        let count = visible.len();
        for (index, meeting) in visible.into_iter().enumerate() {
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
            list = list.child(
                ld::item_with_icon(
                    p,
                    Item {
                        key: format!("meeting-{}", meeting.id),
                        title: shown_title(meeting).to_owned(),
                        meta,
                        meta_ink,
                        note: failure_reason(meeting),
                        time: meeting.clock.clone(),
                        hover: completed
                            .then(|| copy_action(p, pane, meeting, "row"))
                            .into_iter()
                            .collect(),
                        selected: pane.selected.as_deref() == Some(meeting.id.as_str()),
                    },
                    Some(platform_icon(meeting)),
                    move |ui: &mut Ui| ui.meetings.select(id.clone()),
                )
                .position_in_set(index + 1, count),
            );
        }
        let detail = match pane.selected.as_deref().and_then(|id| pane.meeting(id)) {
            Some(meeting) => detail(ui, p, meeting, layer),
            None if pane.visible().is_empty() => ld::empty_state(
                p,
                "meetings-empty",
                ld::SEARCH_ICON,
                tr("No meetings match"),
                None,
            ),
            None => ld::empty_state(
                p,
                "meeting-detail",
                ld::HISTORY_ICON,
                tr("Select a meeting"),
                None,
            ),
        };
        let column = ld::list_column(p, search(ui, p), "meetings-scroll", list);
        ld::body(p, "meetings-body", column, detail)
    };
    // An empty pane's own Start recording is the one main action.
    let idle = pane.active().is_none() && !pane.start_pending && !pane.stop_pending;
    (
        if empty && idle {
            Vec::new()
        } else {
            vec![action]
        },
        col()
            .key("meetings-pane")
            .grow(1.0)
            .shrink()
            .children((!notices.is_empty()).then(|| ld::notices(notices)))
            .child(body),
    )
}

/// The pane while Meetings is off: what it does and its one
/// action, Turn on Meetings.
fn off_state(p: Paint) -> El {
    col()
        .key("meetings-off")
        .grow(1.0)
        .shrink()
        .center()
        .gap(6.0)
        .pb(40.0)
        .child(
            rune::el()
                .key("badge")
                .size(40.0, 40.0)
                .mb(6.0)
                .radius(p.k.radius.lg)
                .fill(p.k.c.layer_01)
                .center()
                .child(
                    rune::icon(super::sidebar::Pane::Meetings.icon(), 20.0)
                        .key("icon")
                        .color(p.k.c.icon_faint),
                )
                .a11y_hidden(),
        )
        .child(
            text(tr("Record your calls"))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(tr("Local transcripts and notes for your calls."))
                .key("hint")
                .font(DESCRIPTION)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(300.0),
        )
        .child(row().key("cta").mt(12.0).child(button(
            p,
            "meetings-turn-on",
            tr("Turn on Meetings"),
            ButtonKind::Primary,
            |ui: &mut Ui, _| {
                ui.update(|settings| settings.meetings.enabled = true);
            },
        )))
}

/// The first-run pane, with Start recording as its one action
/// and where meetings are kept.
fn empty_state(ui: &Ui, p: Paint) -> El {
    use crate::app_settings::MeetingRetention;
    let kept = ui
        .settings
        .meetings
        .retention
        .unwrap_or(MeetingRetention::DEFAULT);
    let where_kept = match kept {
        MeetingRetention::Forever => tr("Meetings stay on this Mac.").to_owned(),
        kept => tr_fill(
            "Kept in Hex on this Mac for {}, then moved to the Trash. Exported Markdown stays.",
            tr(kept.label()),
        ),
    };
    col()
        .key("meetings-empty")
        .grow(1.0)
        .shrink()
        .center()
        .gap(6.0)
        .pb(40.0)
        .child(
            rune::el()
                .key("badge")
                .size(40.0, 40.0)
                .mb(6.0)
                .radius(p.k.radius.lg)
                .fill(p.k.c.layer_01)
                .center()
                .child(
                    rune::icon(super::sidebar::Pane::Meetings.icon(), 20.0)
                        .key("icon")
                        .color(p.k.c.icon_faint),
                )
                .a11y_hidden(),
        )
        .child(
            text(tr("No meetings yet"))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(tr("Record a call to get its transcript and notes here."))
                .key("hint")
                .font(DESCRIPTION)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(300.0),
        )
        .child(row().key("cta").mt(12.0).child(button(
            p,
            "start-meeting",
            tr("Start recording"),
            ButtonKind::Primary,
            |ui: &mut Ui, _| start(ui),
        )))
        .child(
            row()
                .key("where-kept")
                .mt(16.0)
                .gap(6.0)
                .center_y()
                .child(rune::icon(&rune_theme_opencode::icons::LOCK, 12.0).color(p.k.c.icon_faint))
                .child(text(where_kept).font(p.k.ty.small).color(p.faint())),
        )
}

/// The pane's notice while System Audio Recording is off, with
/// the way to turn it on.
fn system_audio_notice(p: Paint) -> El {
    let c = &p.k.c.warning;
    row()
        .key("system-audio-off")
        .pl(12.0)
        .pr(8.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.md)
        .fill(c.bg)
        .border(0.5, c.border)
        .child(
            col()
                .key("message")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(tr("Hex can't hear the other participants"))
                        .key("title")
                        .font(p.k.ty.ui_medium)
                        .color(c.fg),
                )
                .child(
                    text(tr(
                        "Only your microphone is recorded until Hex is allowed in Screen & System Audio Recording.",
                    ))
                    .key("detail")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
                ),
        )
        .child(button(
            p,
            "open-system-audio",
            tr("Open System Settings"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.ask(super::platform::PlatformRequest::SystemAudio),
        ))
}

/// The detail's two copies, labelled, transcript first.
fn copy_buttons(p: Paint, pane: &MeetingsPane, meeting: &Meeting) -> [El; 2] {
    let copied = |what: &str| {
        pane.copied
            .as_ref()
            .is_some_and(|(id, copied)| id == &meeting.id && copied == what)
    };
    let labelled = |key: &'static str, what: &'static str, glyph, label: &'static str| {
        let done = copied(what);
        let id = meeting.id.clone();
        Button::new(if done { tr("Copied") } else { label })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .icon(if done { p.k.glyph.check_small } else { glyph })
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
                let Some(meeting) = ui.meetings.meeting(&id) else {
                    return;
                };
                let text = if what == "prompt" {
                    let recording = (meeting.status == Status::Recording)
                        .then(|| meeting.turns().last().map_or(0, |turn| turn.start_ms));
                    super::meeting_capsules::meeting_agent_prompt(
                        &meeting.id,
                        shown_title(meeting),
                        recording,
                    )
                } else {
                    transcript_markdown(meeting.turns(), meeting.imported)
                };
                copy(ui, tx, id.clone(), what, text);
            })
            .build(p.k)
            .key(key)
    };
    [
        labelled(
            "meeting-copy-transcript",
            "transcript",
            ld::COPY_ICON,
            tr("Copy transcript"),
        ),
        labelled(
            "meeting-copy-prompt",
            "prompt",
            &rune_theme_opencode::icons::OUTLINE_CONSOLE,
            tr("Copy agent prompt"),
        ),
    ]
}

/// Whether the pane offers selection for this meeting: the lab is on, and
/// the meeting is a finished transcript with turns and no summary running.
fn selectable(ui: &Ui, meeting: &Meeting) -> bool {
    available(ui)
        && crate::labs::Feature::TranscriptEdits.on(&ui.settings)
        && crate::labs::Feature::TranscriptEdits.managed().is_none()
        && meeting.status == Status::Complete
        && !meeting.notes_running
        && !meeting.turns().is_empty()
}

/// Drops a selection the pane no longer offers, before it is drawn: the lab
/// went off, the selected meeting left Complete, or its notes started
/// running. Returns whether anything was dropped.
pub(super) fn settle_selection(ui: &mut Ui) -> bool {
    if !ui.meetings.selection.active() {
        return false;
    }
    let offered = ui
        .meetings
        .selected
        .as_deref()
        .and_then(|id| ui.meetings.meeting(id))
        .is_some_and(|meeting| selectable(ui, meeting));
    if offered {
        return false;
    }
    ui.meetings.selection.stop();
    true
}

fn day_label(day: &str) -> &str {
    match day {
        "Today" => tr("Today"),
        "Yesterday" => tr("Yesterday"),
        other => other,
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use crate::rune_ui::Service;
    use crate::rune_ui::platform::PlatformHost;

    /// An imported meeting has one track: every turn is Speaker, not Them.
    #[test]
    fn an_imported_meeting_labels_its_one_track_speaker() {
        let turn = |mine| Turn {
            mine,
            start_ms: 1_000,
            text: "Hi.".into(),
            pause_before_ms: None,
            entries: Vec::new(),
        };
        assert!(transcript_markdown(&[turn(false)], true).contains("**Speaker** · 00:01"));
        assert!(transcript_markdown(&[turn(true)], true).contains("**Speaker**"));
        assert!(transcript_markdown(&[turn(true)], false).contains("**You**"));
        assert!(transcript_markdown(&[turn(false)], false).contains("**Them**"));
    }

    /// Linux and Windows carry meeting requests out in the window's own
    /// process; the OS host ignored them, so Stop, Rename, and Trash from
    /// the pane did nothing and reported nothing (#566).
    #[test]
    fn pane_requests_reach_the_window_s_own_meetings() {
        let mut ui = Ui::with(
            crate::app_settings::AppSettings::default(),
            Service::Fixture(Vec::new()),
        );
        assert!(matches!(ui.host, PlatformHost::Local(_)));
        ui.meetings = MeetingsPane::fixture();
        // No meeting controller runs in tests, so the request is refused
        // rather than silently accepted.
        stop(&mut ui);
        assert!(!ui.meetings.stop_pending);
        assert!(ui.meetings.action_error.is_some());
    }

    /// A known call service's icon, and the neutral one for any other or
    /// no app (#1290).
    #[test]
    fn platform_icons_fall_back_to_a_neutral_one() {
        let pane = MeetingsPane::platforms_fixture();
        let name = |id: &str| platform_icon(pane.meeting(id).unwrap()).name;
        assert_eq!(name("preview-live"), "hex-provider-zoom");
        assert_eq!(name("preview-meet"), "hex-provider-meet");
        assert_eq!(name("preview-teams"), "hex-provider-teams");
        assert_eq!(name("preview-huddle"), "hex-provider-slack");
        assert_eq!(name("preview-design"), "hex-provider-call");
        assert_eq!(name("preview-failed"), "hex-provider-call");
    }

    #[test]
    fn default_titles_hide_the_clock_the_row_already_shows() {
        let pane = MeetingsPane::fixture();
        assert_eq!(
            shown_title(pane.meeting("preview-live").unwrap()),
            "Zoom call"
        );
        assert_eq!(
            shown_title(pane.meeting("preview-failed").unwrap()),
            "Meeting"
        );
        assert_eq!(
            shown_title(pane.meeting("preview-notes").unwrap()),
            "Arcade launch plan"
        );
        let mut custom = pane.meeting("preview-notes").unwrap().clone();
        custom.title = "Standup — 11:12".into();
        assert_eq!(shown_title(&custom), "Standup — 11:12");
    }

    #[test]
    fn failed_rows_show_one_line_reason_or_fallback() {
        let pane = MeetingsPane::fixture();
        assert_eq!(
            failure_reason(pane.meeting("preview-failed").unwrap()).as_deref(),
            Some("ScreenCaptureKit stopped the capture")
        );
        let mut bare = pane.meeting("preview-failed").unwrap().clone();
        bare.error = None;
        assert_eq!(failure_reason(&bare).as_deref(), Some("Couldn't record"));
        assert_eq!(failure_reason(pane.meeting("preview-notes").unwrap()), None);
    }

    /// Only a recording meeting's transcript follows its newest line (#1332).
    #[test]
    fn only_a_recording_transcript_follows() {
        let settings = crate::app_settings::AppSettings::default();
        let pane = MeetingsPane::fixture();
        let live = pane.meeting("preview-live").unwrap();
        let finished = pane.meeting("preview-notes").unwrap();
        let mut ui = Ui::with(settings, Service::Fixture(Vec::new()));
        assert!(follows_live_transcript(&ui, live));
        assert!(!follows_live_transcript(&ui, finished));
        ui.meetings.stop_pending = true;
        assert!(!follows_live_transcript(&ui, live));
    }

    /// Off a Mac the notes are written by OpenCode, so the progress line
    /// names it, never Apple Intelligence (#1151).
    #[test]
    fn notes_progress_names_opencode_off_a_mac() {
        let ui = Ui::with(
            crate::app_settings::AppSettings::default(),
            Service::Fixture(Vec::new()),
        );
        let label = notes_running_label(&ui);
        assert!(label.contains("OpenCode"), "{label}");
        assert!(!label.contains("Apple Intelligence"), "{label}");
    }
}
