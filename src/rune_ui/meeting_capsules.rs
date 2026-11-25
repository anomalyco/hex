//! The meeting capsules on Rune: the calendar meeting reminder, the
//! recording offer, "call ended" (`call_ended`), the ready transcript, and the recording
//! capsule. Each is an
//! interactive Rune overlay, which on macOS is a non-activating `NSPanel`:
//! its buttons take clicks while the call app keeps focus and Hex never
//! activates. An offer that times out only closes; nothing starts or stops
//! on a timeout, and the call is offered again once its app releases and
//! retakes the microphone.
//!
//! `macos_rune` and `meeting_runtime` forward controller events; buttons
//! send `ControllerCommand`s back to the controller.
//!
//! The recording capsule's "Copy agent prompt" copies instructions that
//! tell a coding agent which `hex meetings` commands read the meeting, and
//! "Copy all" copies the in-call transcript (Parakeet,
//! `meeting::TranscriptReader`). The ready panel offers the same two under
//! the finished meeting's title and summary.
//!
//! The recording capsule rests as a small pill in its corner, with a recording
//! dot, the dictation HUD's listening field and a timer. It opens into Stop and its
//! copies while the pointer is over it. Dragging it moves the window; on
//! release it glides into a corner, which is saved.
//! It is a function of the controller's current recording and the live
//! Show recording indicator setting ([`PillModel`]): the call-ended prompt
//! stands in for it until Keep recording, and turning the setting on or off
//! mid-recording shows or hides it within a second.
//! The panel capsules (offer, reminder, call ended, ready, recording
//! automatically) show in that same saved corner and drag the same way. A
//! throw carries a capsule toward the corner it heads for ([`throw`]).
//!
//! On a Linux Wayland compositor without layer-shell (GNOME) the capsules
//! float on XWayland only with Labs › Call recording controls on GNOME
//! (`linux::meeting_capsules`); otherwise an offer opens the Meetings pane.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
use rune::OverlayInput;
use rune::{
    Align, Anim, Cursor, Cx, Drag, El, OverlayHandle, OverlaySpec, Phase, ScreenEdge, Spring, Tx,
    col, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use super::controls::{CAPSULE, Paint, PushSize, status_dot};
use crate::app_settings::{MeetingIndicatorCorner, MeetingPolicy, MeetingProviderChoice};
use crate::calendar::CalendarMeeting;
use crate::desktop::i18n::{tr, tr_fill};
use crate::meeting::detection::{MeetingCandidate, OfferId};
use crate::meeting::watcher::{ControllerCommand, ControllerEvent};
use crate::meeting::{self, MeetingManifest, TranscriptEntry};

mod call_ended;
mod handoff;
mod offer;
#[path = "meeting_capsules/offer/surface.rs"]
mod offer_surface;
mod panel;
mod placement;
mod presence;
mod screen;
mod stage;
mod state;
mod stay;
mod throw;
mod waveform;

use panel::{
    agent_prompt, finalizing_window_size, hex_command, ready, ready_window_size, recording,
    recording_window_size,
};
use placement::{place_capsule, place_on};
use screen::{Anchor, primary_screen_size};
use state::{OfferChoices, ReadyHover, State};

/// The offer, call ended, and ready capsules.
const MEETING_PANEL_WIDTH: f32 = 420.0;
/// The offer's "…" choices need room for three buttons beside the name.
#[cfg(test)]
const MEETING_MENU_WIDTH: f32 = 560.0;
const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// Between a capsule's buttons and its right edge.
const ACTIONS_INSET: f32 = CAPSULE.button_inset;
/// The reminder: the meeting's title beside up to three buttons.
const REMINDER_PANEL_WIDTH: f32 = 520.0;
/// The call-ended capsule's width, beside Keep recording and Stop now /
/// Stop & transcribe (#835).
const CALL_ENDED_PANEL_WIDTH: f32 = call_ended::PANEL_WIDTH;
/// How often an open reminder recounts "Starts in N min".
const REMINDER_TICK: Duration = Duration::from_secs(5);
/// Between a capsule's window edges and what it draws.
const CAPSULE_INSET: f32 = CAPSULE.window_inset;
/// Same header at rest and open so the waveform and timer stay put.
const REST_HEIGHT: f32 = HEADER_HEIGHT;
const REST_WIDTH: f32 = 2.0 * REST_PX + waveform::WIDTH + METER_GAP + TIMER_WIDTH;
/// The header's sides, at rest and open, so the timer stays put as the pill
/// opens: the inset the rows' labels, the title and the summary keep.
const REST_PX: f32 = CAPSULE.text_rail();
/// Between the listening field and the time.
const METER_GAP: f32 = 6.0;
/// "00:00" in `Paint::clock()` (13 pt tabular medium), reserved as an exact
/// integer width so layout matches across macOS, Linux, and Windows.
const TIMER_TEXT_WIDTH: f32 = 38.0;
/// `TIMER_TEXT_WIDTH` plus the recording dot and its gap before it.
const TIMER_WIDTH: f32 = TIMER_TEXT_WIDTH + REC_DOT + METER_GAP;
/// The recording dot before the elapsed time, a status dot's size.
const REC_DOT: f32 = 8.0;
/// The open recording pill and the ready panel.
const PANEL_WIDTH: f32 = CAPSULE.panel_w;
const PANEL_RADIUS: f32 = CAPSULE.radius;
/// Around the open panel's rows.
const PANEL_PAD: f32 = CAPSULE.row_inset;
const HEADER_HEIGHT: f32 = CAPSULE.header_h;
const ROW_HEIGHT: f32 = CAPSULE.row_h;
const OPEN_HEADER: f32 = REST_HEIGHT + ROW_HEIGHT;
/// The two copies and the gap under the raised one.
const COPIES_HEIGHT: f32 = 2.0 * ROW_HEIGHT + ROW_GAP;
/// Concentric with the panel across [`PANEL_PAD`].
const ROW_RADIUS: f32 = CAPSULE.inner(CAPSULE.row_inset);
/// A row's icon and label inset; titles and summaries share it.
const ROW_PX: f32 = CAPSULE.row_px;
/// Between two rows that can both be filled: under the raised first copy,
/// and between Stop and the copies.
const ROW_GAP: f32 = CAPSULE.row_gap;
/// "Microphone is silent" or "Started automatically" under the header.
const NOTICE_HEIGHT: f32 = 20.0;
/// The ready panel's summary: the notes writer's two sentences.
const SUMMARY_LINES: usize = 3;
const TITLE_LINES: usize = 3;
const TITLE_INSET: f32 = (REST_HEIGHT - 20.) / 2.;
const RESULT_HEADER_MAX: f32 = REST_HEIGHT + TITLE_LINES as f32 * 20. + 2. * TITLE_INSET;
const SUMMARY_LINE: f32 = 19.5;
/// One transparent window with room for the finished panel's longest summary.
const RECORDING_WINDOW: (f32, f32) = (
    PANEL_WIDTH + 2.0 * CAPSULE_INSET,
    RESULT_HEADER_MAX
        + SUMMARY_LINE * SUMMARY_LINES as f32
        + ROW_PX
        + ROW_GAP
        + ROW_GAP
        + COPIES_HEIGHT
        + PANEL_PAD
        + 2.0 * CAPSULE_INSET,
);
/// The finished meeting's title and summary grow the panel on this.
const RECORDING_OPEN: Spring = Spring::new(0.42, 0.0);
/// A start this quick never says "Starting…": the offer contracts straight
/// into the live pill.
const PENDING_LABEL_AFTER: f64 = 0.18;
/// How long a copy says "Copied".
const COPIED_FEEDBACK: Duration = Duration::from_millis(1_500);
const OFFER_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// How long after an offer Linux checks that its overlay opened.
#[cfg(target_os = "linux")]
const OFFER_CHECK: Duration = Duration::from_secs(2);
/// How long an automatic recording says so before showing its elapsed time.
const AUTOMATIC_LABEL: Duration = Duration::from_secs(10);
const WINDOW_MARGIN: f32 = 12.0;
/// Below the menu bar.
const MENU_BAR: f32 = 28.0;
/// Above the Dock.
const DOCK: f32 = 24.0;

#[derive(Clone)]
enum Kind {
    /// A calendar meeting is about to start; `automatic` when its call
    /// service is set to Always record.
    Reminder {
        meeting: CalendarMeeting,
        automatic: bool,
    },
    Offer(OfferId, MeetingCandidate),
    /// `stops` when the recording stops on its own then, after a grace;
    /// Keep recording cancels that.
    CallEnded {
        app: String,
        application: Option<String>,
        stops: Option<(Instant, call_ended::Countdown)>,
    },
    Ready(Box<MeetingManifest>),
    /// An automatic recording while the recording indicator is off.
    AutoStarted {
        title: String,
    },
    #[cfg(test)]
    Recording(Recording),
}

/// The recording the controller reported, as its capsule draws it.
#[derive(Clone)]
struct Recording {
    id: String,
    title: String,
    started: Instant,
    automatic: bool,
    /// The microphone is digitally silent (#1518).
    warning: Option<crate::meeting::TrackWarning>,
}

/// What the recording capsule is a function of, besides the live Show
/// recording indicator setting: the controller's current recording, and
/// whether the call-ended prompt stands in for it.
#[derive(Default)]
struct PillModel {
    recording: Option<Recording>,
    /// The call-ended prompt shows instead of the pill until Keep recording,
    /// or until another panel capsule replaces it.
    call_ended: bool,
    /// The recording the pill shows now.
    shown: Option<String>,
    /// Where this recording was last shown, kept through transcription for its
    /// summary.
    last_anchor: Option<(String, Anchor)>,
}

impl PillModel {
    fn apply(&mut self, event: &ControllerEvent, now: Instant) {
        match event {
            ControllerEvent::RecordingStarted {
                id,
                title,
                automatic,
            } => {
                self.recording = Some(Recording {
                    id: id.clone(),
                    title: title.clone(),
                    started: now,
                    automatic: *automatic,
                    warning: None,
                });
                self.call_ended = false;
            }
            ControllerEvent::Retitled { title } => {
                if let Some(recording) = &mut self.recording {
                    recording.title = title.clone();
                }
            }
            ControllerEvent::SourceWarning { warning } => {
                if let Some(recording) = &mut self.recording {
                    recording.warning = *warning;
                }
            }
            ControllerEvent::CallEnded { .. } => self.call_ended = true,
            // The call came back before its grace: the notice goes and the
            // recording capsule returns, as Keep recording does.
            ControllerEvent::CallEndedWithdrawn => self.call_ended = false,
            ControllerEvent::Offer(..) | ControllerEvent::Reminder { .. } => {
                self.call_ended = false;
            }
            ControllerEvent::Starting(_)
            | ControllerEvent::Transcribing
            | ControllerEvent::Finished(_)
            | ControllerEvent::Discarded(_)
            | ControllerEvent::Failed { .. } => {
                self.recording = None;
                self.call_ended = false;
            }
            ControllerEvent::OfferWithdrawn(_)
            | ControllerEvent::ReminderWithdrawn { .. }
            | ControllerEvent::Stopping
            | ControllerEvent::Chapters(_)
            | ControllerEvent::ImportFailed { .. }
            | ControllerEvent::TranscriptEditFailed { .. }
            | ControllerEvent::NotesProgress(_)
            | ControllerEvent::NotesFinished(_) => {}
        }
    }

    fn keep_recording(&mut self) {
        self.call_ended = false;
    }

    /// The recording the pill shows with the indicator setting `indicator`.
    fn pill(&self, indicator: bool) -> Option<&Recording> {
        self.recording
            .as_ref()
            .filter(|_| indicator && !self.call_ended)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LiveCopy {
    All,
    Prompt,
}

/// The shown meeting's transcript as last read: the recording's in-call
/// transcript by the ticker, or the ready meeting's when it shows.
#[derive(Default)]
struct Live {
    /// A swipe within a wide action row is not a click, even on release inside it.
    row_swiped: bool,
    entries: Vec<TranscriptEntry>,
    copied: Option<(LiveCopy, Instant)>,
}

impl Live {
    fn copied(&self, kind: LiveCopy) -> bool {
        self.copied
            .is_some_and(|(copied, at)| copied == kind && at.elapsed() < COPIED_FEEDBACK)
    }
}

struct Capsule {
    state: State,
    presentation: offer_surface::Presentation,
    /// The recording host's outline and layers in motion.
    stage: stage::Stage,
    /// The whole surface arriving and leaving, and its lines changing.
    surface: presence::Surface,
    live: Live,
    waveform: Option<Box<waveform::Listening>>,
    commands: SyncSender<ControllerCommand>,
    hide: Option<Arc<dyn Fn() + Send + Sync>>,
    show: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The call-ended prompt's Keep recording brings the recording capsule
    /// back.
    keep_recording: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The ready panel's title opens its meeting in Hex's Meetings pane.
    open_meeting: Option<OpenMeeting>,
    resize: Option<Resize>,
    /// Windows: the size the recording window was last given, which follows
    /// what it draws (see [`panel::WindowFit`]).
    #[cfg(target_os = "windows")]
    recording_window: panel::WindowFit,
    choose: Choose,
    /// Where the capsule rests.
    corner: MeetingIndicatorCorner,
    /// The display it rests on (`meetings.recording_indicator_display`),
    /// while Labs › Indicator on any display is on; `None` is the primary
    /// display, where Rune opens every capsule.
    display: Option<String>,
    /// Moves the capsule's window by a drag's travel from `corner`.
    shift: Option<Shift>,
    /// Moves the capsule's window onto another display at its next placement.
    retarget: Option<Retarget>,
    /// The drag in progress and the glide into a corner after it.
    throw: throw::Throw,
    /// Reduce Motion, as the view last read it: a release jumps to its
    /// corner.
    reduced: Cell<bool>,
    /// The window's scale, as the view last read it: device pixels per point.
    scale: Cell<f32>,
    /// The primary screen in points, for a release's corner; a type that
    /// tests can replace with no screen at all.
    screen: ScreenFn,
    /// Every display in points, with the pointer's first, for a release
    /// while Labs › Indicator on any display is on; a type that tests replace
    /// with no display.
    displays: DisplaysFn,
    save_anchor: SaveAnchor,
}

type Choose = Arc<dyn Fn(MeetingProviderChoice) + Send + Sync>;
/// Opens a finished meeting, by id, in Hex's window.
type OpenMeeting = Arc<dyn Fn(String) + Send + Sync>;
/// Sets the capsule window's size in points.
type Resize = Arc<dyn Fn((f32, f32)) + Send + Sync>;
type Shift = Arc<dyn Fn(MeetingIndicatorCorner, f32, f32) + Send + Sync>;
/// Moves the capsule's window onto the display `None` names the primary for.
type Retarget = Arc<dyn Fn(Option<&str>) + Send + Sync>;
/// Saves where the capsule rests: its corner, and its display.
type SaveAnchor = Arc<dyn Fn(MeetingIndicatorCorner, Option<&str>) + Send + Sync>;
/// The primary screen in points, given the window's pixels-per-point scale.
type ScreenFn = fn(f32) -> Option<(f32, f32)>;
/// Every display in points, the pointer's first, given that same scale.
type DisplaysFn = fn(f32) -> Option<screen::Displays>;

impl Capsule {
    fn send(&self, command: ControllerCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    fn is_shown(&self) -> bool {
        !self.state.is_empty()
    }

    fn hovered(&self) -> bool {
        self.state.hovered()
    }

    #[cfg(test)]
    fn stopping(&self) -> bool {
        self.state.stopping()
    }

    fn more_open(&self) -> bool {
        self.state.more_open()
    }

    /// Refreshes the shown recording's metadata in place without disturbing
    /// hover or stop state.
    fn update_recording(&mut self, next: Recording) {
        match &mut self.state {
            State::Recording { recording, .. } => *recording = next,
            _ => {
                self.state = State::Recording {
                    recording: next,
                    hovered: false,
                    stopping: false,
                    confirming: false,
                };
            }
        }
    }

    /// The recording's pill is held open: by the pointer, or by Stop.
    #[cfg(test)]
    fn open(&self) -> bool {
        self.state.recording_open()
    }

    fn set_hovered(&mut self, inside: bool) {
        if self.presentation.blocks_hover(inside) {
            return;
        }
        match &mut self.state {
            State::Recording {
                hovered,
                confirming,
                ..
            } => {
                *hovered = inside;
                // Leaving answers the question with Keep.
                if !inside {
                    *confirming = false;
                }
                #[cfg(target_os = "windows")]
                self.fit_recording_window();
            }
            State::Finalizing { hovered, .. } => {
                *hovered = inside;
            }
            State::Ready { hover, stay, .. } => {
                // The stay holds while the pointer is inside and runs on from
                // where it was when it leaves; its ring closes the panel.
                if inside {
                    stay.pause();
                } else {
                    stay.resume();
                }
                match (inside, *hover) {
                    (true, ReadyHover::Idle) => *hover = ReadyHover::Hovered,
                    (true, ReadyHover::Hovered | ReadyHover::ExpiredWhileHovered) => {}
                    (false, ReadyHover::Hovered | ReadyHover::ExpiredWhileHovered) => {
                        *hover = ReadyHover::Idle;
                    }
                    (false, ReadyHover::Idle) => {}
                }
            }
            _ => {}
        }
    }

    /// Saves the offered provider's choice; returns whether there was one.
    fn choose(&self, policy: MeetingPolicy) -> bool {
        let State::Offer { candidate, .. } = &self.state else {
            return false;
        };
        let Some(provider) = candidate.provider() else {
            return false;
        };
        tracing::info!(key = %provider.key, ?policy, "meeting provider choice made from an offer");
        (self.choose)(provider.choose(policy));
        true
    }

    /// Closed from this moment; the window hides once the surface has gone.
    fn close(&mut self) {
        let mut state = std::mem::replace(&mut self.state, State::Empty);
        if let State::Offer { rearm, .. } = &mut state {
            *rearm = None;
        }
        self.surface.leave(state);
    }

    /// Notes update an existing summary; they never present one.
    fn refresh_ready(&mut self, manifest: Box<MeetingManifest>) {
        if let State::Ready {
            manifest: ready, ..
        } = &mut self.state
            && ready.id == manifest.id
        {
            *ready = manifest;
            // A longer summary grows the window here; one that goes shrinks
            // it only once the surface has landed.
            #[cfg(target_os = "windows")]
            self.fit_recording_window();
        }
    }

    /// The pending stop the notice named was withdrawn: the automatic stop's
    /// notice goes. The "still recording" notice names no stop, and a newer
    /// panel of another kind stays, as [`ControllerEvent::ReminderWithdrawn`]
    /// leaves another reminder alone.
    fn withdraw_call_ended(&mut self) -> bool {
        if !matches!(self.state, State::CallEnded { stops: Some(_), .. }) {
            return false;
        }
        self.close();
        true
    }

    fn copy_all(&mut self, tx: &mut Tx) {
        let markdown = match &self.state {
            State::Recording { .. } if self.live.entries.is_empty() => return,
            State::Recording { .. } => meeting::live_transcript_markdown(&self.live.entries),
            State::Ready { manifest, .. } => {
                match meeting::transcript_markdown_for_manifest(manifest) {
                    Ok(markdown) => markdown,
                    Err(error) => {
                        tracing::warn!(%error, "could not copy meeting transcript");
                        return;
                    }
                }
            }
            _ => return,
        };
        tx.copy(markdown);
        self.live.copied = Some((LiveCopy::All, Instant::now()));
    }

    fn copy_prompt(&mut self, tx: &mut Tx) {
        let prompt = match &self.state {
            State::Recording {
                recording: Recording { id, title, .. },
                ..
            } => {
                let reached = self.live.entries.last().map_or(0, |entry| entry.end_ms);
                agent_prompt(&hex_command(), id, title, Some(reached))
            }
            State::Ready { manifest, .. } => agent_prompt(
                &hex_command(),
                &manifest.id,
                &meeting::display_title(manifest),
                None,
            ),
            _ => return,
        };
        tx.copy(prompt);
        self.live.copied = Some((LiveCopy::Prompt, Instant::now()));
    }

    /// The capsule's window size, for snapping a drag.
    fn window_size(&self) -> (f32, f32) {
        // On Windows the recording window follows what it draws: snap by the
        // size it was last given, not the one the surface is heading to.
        #[cfg(target_os = "windows")]
        if matches!(
            self.state,
            State::Recording { .. } | State::Finalizing { .. } | State::Ready { .. }
        ) && let Some(size) = self.recording_window.size()
        {
            return size;
        }
        if self.presentation.inherits_offer_window() {
            return self.presentation.window();
        }
        match &self.state {
            State::Finalizing { .. } => finalizing_window_size(),
            State::Recording {
                recording: shown,
                hovered,
                stopping,
                ..
            } => recording_window_size(*hovered || *stopping, Some(shown)),
            State::Ready { manifest, .. } => ready_window_size(manifest),
            State::Offer { .. } => self.presentation.window(),
            State::Reminder { .. } => (REMINDER_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
            State::CallEnded { .. } => (CALL_ENDED_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
            State::AutoStarted { .. } | State::Empty => (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
        }
    }
}

/// Linux: lets the capsule's next show float on XWayland where Wayland has
/// no layer-shell, only by Labs › Call recording controls on GNOME (#1274).
/// Off, Rune can't open it there and the Meetings pane stands in (#1271).
fn float_on_xwayland(capsule: &OverlayHandle<Capsule>) {
    #[cfg(target_os = "linux")]
    capsule.set_xwayland_input(crate::linux::meeting_capsules::on_xwayland());
    #[cfg(not(target_os = "linux"))]
    let _ = capsule;
}

/// The two capsules: the timed panel (offer, call ended, ready) and the
/// recording capsule in the configured corner.
pub(crate) struct MeetingCapsules {
    panel: OverlayHandle<Capsule>,
    /// Who currently owns the recording window (`pill.handle`) outside an active
    /// recording: none (`Panel`), an offer, an accepted offer, or a finalizing /
    /// ready meeting.
    host: offer::Host,
    pill: Pill,
    /// Bumped whenever the panel shows something new, so a stale timeout
    /// doesn't close it.
    generation: Arc<AtomicU64>,
}

/// The recording capsule, rebuilt from its [`PillModel`] and the live Show
/// recording indicator setting whenever either may have changed: on each
/// controller event, on Keep recording, and each second while recording.
#[derive(Clone)]
struct Pill {
    handle: OverlayHandle<Capsule>,
    model: Arc<Mutex<PillModel>>,
}

impl Pill {
    fn keep_recording(&self) {
        self.lock().keep_recording();
        self.reconcile();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PillModel> {
        self.model.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Shows, refreshes, or hides the pill to match the model; returns the
    /// recording it shows.
    fn reconcile(&self) -> Option<String> {
        self.reconcile_locked(&mut self.lock(), false)
    }

    fn reconcile_locked(&self, model: &mut PillModel, inherit: bool) -> Option<String> {
        let Some(recording) = model
            .pill(crate::app_settings::meeting_indicator_enabled())
            .cloned()
        else {
            if model.shown.take().is_some() || inherit {
                self.handle.update(|capsule, tx| capsule.dismiss(tx));
            }
            return None;
        };
        let id = recording.id.clone();
        if model.shown.as_ref() == Some(&id) {
            self.handle.update(move |capsule, _| {
                capsule.update_recording(recording);
                // A notice coming or going changes the resting pill's width.
                #[cfg(target_os = "windows")]
                capsule.fit_recording_window();
            });
            return Some(id);
        }
        model.shown = Some(id.clone());
        let corner = crate::app_settings::meeting_indicator_corner();
        let anchor = Anchor {
            corner,
            display: crate::app_settings::saved_meeting_indicator_display(),
        };
        model.last_anchor = Some((id.clone(), anchor.clone()));
        if inherit {
            // The accepted offer's window is this capsule's, so it already
            // rests in the corner and display the offer was dragged to: the
            // pill keeps exactly that, not the saved one, in case the offer's
            // drag has not reached settings yet.
            self.handle.update(move |c, tx| {
                let anchor = Anchor {
                    corner: c.corner,
                    display: c.display.clone(),
                };
                c.start_recording(recording, anchor, tx);
            });
            return Some(id);
        }
        place_on(&self.handle, anchor.display.as_deref());
        place_capsule(&self.handle, corner, 0.0, 0.0);
        // Windows: a shown pill starts at rest.
        self.handle
            .resize(recording_window_size(false, Some(&recording)));
        #[cfg(target_os = "windows")]
        self.handle.set_input(rune::OverlayInput::All);
        self.handle.update(move |capsule, tx| {
            capsule.presentation = Default::default();
            capsule.stage = Default::default();
            capsule.start_recording(recording, anchor, tx);
        });
        float_on_xwayland(&self.handle);
        self.handle.show();
        Some(id)
    }

    /// Whether the controller still records `id`.
    fn records(&self, id: &str) -> bool {
        self.lock()
            .recording
            .as_ref()
            .is_some_and(|recording| recording.id == id)
    }

    /// While recording `id`: each second, reconciles the pill (the elapsed
    /// time, the indicator setting) and, while it shows, reads the in-call
    /// transcript here rather than on the main thread.
    fn tick(&self, id: String) {
        let pill = self.clone();
        std::thread::spawn(move || {
            let mut reader = meeting::TranscriptReader::default();
            while pill.records(&id) {
                std::thread::sleep(Duration::from_secs(1));
                if pill.reconcile().as_ref() != Some(&id) {
                    continue;
                }
                let entries = match reader.read(&id) {
                    Ok(entries) => entries,
                    Err(error) => {
                        tracing::debug!(%error, "could not read the in-call transcript");
                        continue;
                    }
                };
                let id = id.clone();
                pill.handle.update(move |capsule, _| {
                    if matches!(&capsule.state, State::Recording { recording: Recording { id: shown, .. }, .. } if *shown == id)
                    {
                        capsule.live.entries = entries;
                    }
                });
            }
        });
    }
}

impl MeetingCapsules {
    /// `choose` saves a "Don't offer" or "Always record" choice (the window
    /// keeps the settings it saves).
    pub(crate) fn install<S: 'static>(
        app: &mut rune::App<S>,
        commands: SyncSender<ControllerCommand>,
        choose: impl Fn(MeetingProviderChoice) + Send + Sync + 'static,
        save_anchor: impl Fn(MeetingIndicatorCorner, Option<&str>) + Send + Sync + 'static,
        open_meeting: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        let choose: Choose = Arc::new(choose);
        let open_meeting: OpenMeeting = Arc::new(open_meeting);
        let save_anchor: SaveAnchor = Arc::new(save_anchor);
        let overlay = |app: &mut rune::App<S>, name, size: (f32, f32), recording: bool| {
            let spec = OverlaySpec {
                name,
                size,
                edge: ScreenEdge::Top,
                margin: WINDOW_MARGIN + MENU_BAR,
                align: Align::End,
                inset: WINDOW_MARGIN,
                interactive: true,
                view,
            }
            .options(Default::default());
            let spec = if recording {
                spec.compositor(waveform::Ink::default())
            } else {
                spec
            };
            let handle = app.overlay(
                spec.poll(poll),
                Capsule {
                    state: State::Empty,
                    presentation: Default::default(),
                    stage: Default::default(),
                    surface: Default::default(),
                    live: Live::default(),
                    waveform: None,
                    commands: commands.clone(),
                    hide: None,
                    show: None,
                    keep_recording: None,
                    open_meeting: Some(open_meeting.clone()),
                    resize: None,
                    #[cfg(target_os = "windows")]
                    recording_window: Default::default(),
                    choose: choose.clone(),
                    corner: MeetingIndicatorCorner::default(),
                    display: None,
                    shift: None,
                    retarget: None,
                    throw: throw::Throw::default(),
                    reduced: Cell::new(false),
                    scale: Cell::new(1.0),
                    screen: primary_screen_size,
                    displays: screen::displays,
                    save_anchor: save_anchor.clone(),
                },
            );
            // Only the drawn capsule (its `.hit()` body) takes clicks; the
            // window's transparent rest passes them to the app beneath.
            // Windows instead sizes each window to what it draws, so the
            // whole window may take the pointer there (`recording_window_size`
            // and `show_recording`); Regions' sampled pointer positions can
            // leave the transparent rest of a wide window hit-testable.
            #[cfg(not(target_os = "windows"))]
            handle.set_input(OverlayInput::Regions);
            let (hide, resize, shift) = (handle.clone(), handle.clone(), handle.clone());
            let (show, retarget) = (handle.clone(), handle.clone());
            handle.update(move |capsule, _| {
                capsule.hide = Some(Arc::new(move || hide.hide()));
                capsule.show = Some(Arc::new(move || show.show()));
                capsule.resize = Some(Arc::new(move |size| resize.resize(size)));
                capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                    place_capsule(&shift, corner, dx, dy);
                }));
                capsule.retarget = Some(Arc::new(move |display| {
                    place_on(&retarget, display);
                }));
            });
            handle
        };
        let panel = overlay(
            app,
            "Hex meeting",
            (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
            false,
        );
        let pill = Pill {
            handle: overlay(
                app,
                "Hex meeting recording",
                recording_window_size(false, None),
                true,
            ),
            model: Arc::default(),
        };
        let kept = pill.clone();
        let anchors = pill.model.clone();
        pill.handle.update(move |capsule, _| {
            capsule.save_anchor = Arc::new(move |corner, display| {
                if let Some((_, anchor)) = &mut anchors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .last_anchor
                {
                    anchor.corner = corner;
                    anchor.display = display.map(str::to_owned);
                }
                save_anchor(corner, display);
            });
        });
        panel.update(move |capsule, _| {
            capsule.keep_recording = Some(Arc::new(move || kept.keep_recording()));
        });
        Self {
            panel,
            host: Default::default(),
            pill,
            generation: Arc::default(),
        }
    }

    /// Shows, updates, or closes a capsule for each controller event.
    pub(crate) fn event(&mut self, event: &ControllerEvent) {
        let route = self.host.route(event);
        let inherit = route == offer::Route::Inherit;
        if inherit {
            self.host = offer::Host::Panel;
        } else if route == offer::Route::Close {
            self.close_panel();
        }
        self.advance_handoff(event, inherit);
        match event {
            ControllerEvent::Reminder { meeting, automatic } => {
                // Unanswered, it closes without recording or snoozing
                // anything, when the scheduler lets the next one show.
                let now_ms = crate::calendar::wall_clock_ms();
                let until_ms = crate::calendar::reminder_until_ms(meeting.start_ms, now_ms);
                let timeout = Duration::from_millis((until_ms - now_ms).max(0) as u64);
                self.show_panel(
                    Kind::Reminder {
                        meeting: meeting.clone(),
                        automatic: *automatic,
                    },
                    Some(timeout),
                );
            }
            ControllerEvent::ReminderWithdrawn { occurrence } => {
                let (occurrence, generation) = (occurrence.clone(), self.generation.clone());
                self.panel.update(move |capsule, tx| {
                    if matches!(&capsule.state, State::Reminder { meeting, .. } if meeting.occurrence() == occurrence)
                    {
                        generation.fetch_add(1, Ordering::SeqCst);
                        capsule.dismiss(tx);
                    }
                });
            }
            ControllerEvent::Offer(id, candidate) => {
                tracing::info!(
                    source = candidate.source.label(),
                    title = %candidate.title,
                    application = ?candidate.application,
                    "meeting recording offer shown"
                );
                self.show_panel(Kind::Offer(*id, candidate.clone()), None);
            }
            ControllerEvent::OfferWithdrawn(_) => {
                // The shown offer's own route closes it.
            }
            ControllerEvent::Starting(_) | ControllerEvent::Transcribing
                if route != offer::Route::Ordinary => {}
            ControllerEvent::Starting(_) | ControllerEvent::Transcribing => self.close_panel(),
            ControllerEvent::Stopping => self.pill.handle.update(|c, tx| {
                c.mark_stopping();
                c.stir(tx);
            }),
            ControllerEvent::RecordingStarted {
                title, automatic, ..
            } => {
                if *automatic && !crate::app_settings::meeting_indicator_enabled() {
                    // Nothing else on screen would say a recording began.
                    self.show_panel(
                        Kind::AutoStarted {
                            title: title.clone(),
                        },
                        Some(AUTOMATIC_LABEL),
                    );
                }
            }
            ControllerEvent::CallEnded {
                app,
                application,
                stops_at,
            } => {
                self.show_panel(
                    Kind::CallEnded {
                        app: app.clone(),
                        application: application.clone(),
                        stops: stops_at.map(|at| {
                            (
                                at,
                                call_ended::Countdown::new(
                                    at.saturating_duration_since(Instant::now()),
                                ),
                            )
                        }),
                    },
                    None,
                );
            }
            ControllerEvent::CallEndedWithdrawn => {
                let generation = self.generation.clone();
                self.panel.update(move |capsule, tx| {
                    if capsule.withdraw_call_ended() {
                        generation.fetch_add(1, Ordering::SeqCst);
                        tx.wake_after(Duration::ZERO);
                    }
                });
            }
            ControllerEvent::Finished(manifest) => {
                self.finish_meeting(manifest);
            }
            // Nothing to show: the meeting it would have offered was trashed
            // as it finished. `advance_handoff` already dismissed the pill.
            ControllerEvent::Discarded(_) => {}
            ControllerEvent::NotesFinished(manifest) => {
                let manifest = manifest.clone();
                let panel = self.handle_for(&manifest.id);
                panel.update(move |capsule, _| capsule.refresh_ready(manifest));
            }
            ControllerEvent::NotesProgress(id) => self.notes_progress(id),
            ControllerEvent::Retitled { .. }
            | ControllerEvent::SourceWarning { .. }
            | ControllerEvent::Failed { .. }
            | ControllerEvent::ImportFailed { .. }
            | ControllerEvent::TranscriptEditFailed { .. }
            | ControllerEvent::Chapters(_) => {}
        }
    }

    fn show_panel(&mut self, kind: Kind, timeout: Option<Duration>) {
        match &kind {
            Kind::Offer(id, _) => {
                if !self.host.offer_on_pill() {
                    self.clear_finishing();
                    self.close_panel();
                }
                self.host = offer::Host::Offered(*id);
            }
            _ => {
                if self.host.offer_on_pill() {
                    self.close_panel();
                } else {
                    self.clear_finishing();
                }
            }
        }
        let panel = self.panel_handle().clone();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let reminder = matches!(kind, Kind::Reminder { .. });
        let (stops_at, call_ended_app) = match &kind {
            Kind::CallEnded {
                stops, application, ..
            } => (stops.as_ref().map(|(at, _)| *at), application.clone()),
            _ => (None, None),
        };
        // The saved corner and display, and a finished meeting its
        // recording's own.
        let anchor = panel::show_anchor(&kind, self.pill.lock().last_anchor.as_ref(), || Anchor {
            corner: crate::app_settings::meeting_indicator_corner(),
            display: crate::app_settings::saved_meeting_indicator_display(),
        });
        place_on(&panel, anchor.display.as_deref());
        place_capsule(&panel, anchor.corner, 0.0, 0.0);
        let presentation = offer_surface::Presentation::for_kind(&kind, false);
        // Only the detected offer reserves space for its wider page. The
        // other Windows panels retain their original, window-sized input.
        #[cfg(target_os = "windows")]
        panel.set_input(if matches!(kind, Kind::Offer(..)) {
            rune::OverlayInput::Regions
        } else {
            rune::OverlayInput::All
        });
        panel.resize(match &kind {
            Kind::Offer(..) => presentation.window(),
            Kind::Ready(manifest) => ready_window_size(manifest),
            kind => (panel_width(kind), MEETING_PANEL_HEIGHT),
        });
        let offer_id = match kind {
            Kind::Offer(id, _) => Some(id),
            _ => None,
        };
        let rearm = offer_id
            .is_some()
            .then(|| offer::timer(panel.clone(), self.generation.clone(), generation));
        panel.update(move |capsule, tx| capsule.show(kind, presentation, anchor, rearm, tx));
        float_on_xwayland(&panel);
        panel.show();
        if let Some(id) = offer_id {
            offer_surface::load_icon(panel.clone(), id);
        }
        if let Some(bundle) = call_ended_app {
            let (panel, current) = (panel.clone(), self.generation.clone());
            std::thread::spawn(move || {
                if let Some(icon) = offer_surface::native_icon_sized(&bundle, call_ended::ICON_SIZE)
                {
                    panel.update(move |capsule, _| {
                        if current.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        if let State::CallEnded { icon: slot, .. } = &mut capsule.state {
                            *slot = Some(icon);
                        }
                    });
                }
            });
        }
        if let Some(stops_at) = stops_at {
            // Recount "Recording stops in N s" as each second passes; the
            // drain moves on its own on the frame clock.
            let (panel, current) = (panel.clone(), self.generation.clone());
            std::thread::spawn(move || {
                while current.load(Ordering::SeqCst) == generation {
                    let left = stops_at.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    let fraction = left.as_secs_f64().fract();
                    std::thread::sleep(Duration::from_secs_f64(if fraction > 0.0 {
                        fraction
                    } else {
                        1.0
                    }));
                    if current.load(Ordering::SeqCst) != generation {
                        break;
                    }
                    panel.update(|_, _| {});
                }
            });
        }
        if reminder && let Some(timeout) = timeout {
            // Recount "Starts in N min" while this reminder may show.
            let (panel, current) = (self.panel.clone(), self.generation.clone());
            let until = Instant::now() + timeout;
            std::thread::spawn(move || {
                while current.load(Ordering::SeqCst) == generation && Instant::now() < until {
                    std::thread::sleep(REMINDER_TICK);
                    panel.update(|_, _| {});
                }
            });
        }
        if let Some(timeout) = timeout {
            self.timeout(self.panel.clone(), generation, timeout);
        }
    }

    /// Closes the capsule shown for `generation` after `timeout`, unless a
    /// newer one replaced it or the pointer holds a ready panel open.
    fn timeout(&self, panel: OverlayHandle<Capsule>, generation: u64, timeout: Duration) {
        let current = self.generation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if current.load(Ordering::SeqCst) == generation {
                panel.update(move |capsule, _| {
                    // Checked again when applied: a newer capsule may
                    // have been shown since.
                    if current.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    // A ready panel waits for the pointer to leave it.
                    capsule.expire_panel();
                });
            }
        });
    }

    /// Linux: calls `unshown` with Rune's reason when the offer just shown
    /// could not open its overlay (GNOME on Wayland has no layer-shell for
    /// interactive overlays, #1271), unless a newer capsule replaced it.
    #[cfg(target_os = "linux")]
    pub(crate) fn when_offer_unshown(&self, unshown: impl FnOnce(String) + Send + 'static) {
        let (panel, current) = (self.panel_handle().clone(), self.generation.clone());
        let generation = current.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            std::thread::sleep(OFFER_CHECK);
            if current.load(Ordering::SeqCst) != generation
                || panel.availability() != rune::Availability::Unavailable
            {
                return;
            }
            unshown(
                panel
                    .failure()
                    .unwrap_or_else(|| "no reason given".to_owned()),
            );
        });
    }

    // The capsule hides its own window once its surface has gone, and only
    // while it still shows nothing, so closing then showing stays shown.
    fn close_panel(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.host.offer_on_pill() {
            self.pill.handle.update(|capsule, tx| capsule.dismiss(tx));
            self.host = offer::Host::Panel;
        } else {
            self.panel.update(|capsule, tx| capsule.dismiss(tx));
        }
    }

    fn panel_handle(&self) -> &OverlayHandle<Capsule> {
        if self.host.offer_on_pill() {
            &self.pill.handle
        } else {
            &self.panel
        }
    }
}

fn button(
    p: Paint,
    key: &str,
    label: &str,
    primary: bool,
    press: impl Fn(&mut Capsule) + 'static,
) -> El {
    Button::new(label)
        .variant(if primary {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        })
        .build_push(p.k)
        .key(key)
        // The capsule's body drags; its buttons are buttons.
        .cursor(Cursor::Pointer)
        // The surface answers in the frame of the press.
        .on_press_tx(move |c: &mut Capsule, tx| {
            press(c);
            c.stir(tx);
        })
}

static CLOSE: &rune::Icon = &rune_theme_opencode::icons::CLOSE;

/// The ✕ that closes a capsule whose actions have no way to. It only hides
/// the capsule: nothing is sent to the controller (#996).
fn dismiss_button(p: Paint) -> El {
    IconButton::new(CLOSE)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(tr("Close"))
        .build(p.k)
        .key("dismiss")
        .cursor(Cursor::Pointer)
        .on_press_tx(|c: &mut Capsule, tx| {
            c.close();
            c.stir(tx);
        })
}

/// The overlay's frame hook: steps the surface, and a glide into its corner,
/// a frame at a time until they rest, and keeps a recording's waveform fed.
fn poll(capsule: &mut Capsule, now: f64, tx: &mut Tx) -> bool {
    // The surface first: the field is drawn once a frame, at the width the
    // wall has just reached.
    let staged = capsule.step_stage(now, tx);
    let mut changed = staged;
    changed |= waveform::poll(capsule, now, tx);
    // Windows: once the surface has landed, a window kept large for a
    // closing panel can shrink to the pill it now draws. The waveform may
    // still be moving, so this reads the surface alone.
    #[cfg(target_os = "windows")]
    if !staged {
        capsule.settle_recording_window();
    }
    if capsule.stay_over() {
        capsule.stir(tx);
        changed = true;
    }
    if !capsule.throw.gliding() {
        return changed;
    }
    let offset = capsule.throw.step(now);
    if let Some(shift) = &capsule.shift {
        let (x, y) = offset.unwrap_or_default();
        shift(capsule.corner, x, y);
    }
    if offset.is_some() {
        tx.wake_after(Duration::ZERO);
    }
    changed
}

fn view(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    capsule.reduced.set(cx.is_reduced_motion());
    capsule.scale.set(cx.env().scale);
    presence::present(capsule, view_body(capsule, cx))
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
}

fn view_body(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    // Recording and its summary stay dark; offers and reminders follow the host.
    let dark = matches!(
        capsule.drawn(),
        Some(State::Recording { .. } | State::Finalizing { .. } | State::Ready { .. })
    ) || cx.system_dark();
    let p = Paint::new(dark, cx.env().scale);
    if let Some(el) = offer_surface::transition(
        capsule,
        Paint::new(cx.system_dark(), cx.env().scale),
        Paint::new(true, cx.env().scale),
    ) {
        return el;
    }
    let Some(state) = capsule.drawn() else {
        return row().key("empty");
    };
    let parts = match state {
        State::Empty => return row().key("empty"),
        State::Recording {
            recording: shown,
            hovered,
            stopping,
            confirming,
        } => {
            return recording(
                p,
                capsule,
                shown,
                panel::Open {
                    hovered: *hovered,
                    stopping: *stopping,
                    confirming: *confirming,
                },
            );
        }
        State::Finalizing { duration_ms, .. } => {
            return panel::finalizing(p, capsule, *duration_ms);
        }
        State::AutoStarted { title } => (
            p.danger(),
            tr("Recording automatically").to_owned(),
            title.clone(),
            // Dismissing leaves the recording going; the menu bar still
            // shows it.
            vec![
                button(p, "stop", tr("Stop"), true, |c| {
                    if c.send(ControllerCommand::Stop) {
                        c.close();
                    }
                }),
                dismiss_button(p),
            ],
        ),
        State::Reminder { meeting, automatic } => {
            reminder(p, meeting, *automatic, crate::calendar::wall_clock_ms())
        }
        State::Offer { id, candidate, .. } => {
            return offer_surface::view(p, capsule, *id, candidate);
        }
        State::CallEnded { app, stops, icon } => {
            let now = capsule.surface.clock();
            let countdown = stops
                .as_ref()
                .map(|(at, countdown)| (countdown, at.saturating_duration_since(now)));
            return call_ended::view(
                p,
                capsule,
                call_ended::Content {
                    app,
                    icon: icon.as_ref(),
                    countdown,
                },
                cx.is_reduced_motion(),
                |c| {
                    if matches!(c.state, State::CallEnded { stops: Some(_), .. }) {
                        c.send(ControllerCommand::KeepRecording);
                    }
                    if let Some(keep_recording) = &c.keep_recording {
                        keep_recording();
                    }
                    c.close();
                },
                |c| {
                    if c.send(ControllerCommand::Stop) {
                        c.close();
                    }
                },
            );
        }
        State::Ready { manifest, .. } => return ready(p, capsule, manifest),
    };
    pill(p, capsule, parts)
}

/// The panel capsule's width for `kind`, room for its title beside its
/// buttons.
fn panel_width(kind: &Kind) -> f32 {
    match kind {
        Kind::Reminder { .. } => REMINDER_PANEL_WIDTH,
        Kind::CallEnded { .. } => CALL_ENDED_PANEL_WIDTH,
        _ => MEETING_PANEL_WIDTH,
    }
}

/// "Starts in 1 min", "Starting now", or "Started 2 min ago".
fn starts_label(start_ms: i64, now_ms: i64) -> String {
    let minutes = crate::calendar::minutes_until(start_ms, now_ms);
    if minutes > 0 {
        tr_fill("Starts in {} min", &minutes.to_string())
    } else {
        let ago = (now_ms - start_ms) / 60_000;
        if ago < 1 {
            tr("Starting now").to_owned()
        } else {
            tr_fill("Started {} min ago", &ago.to_string())
        }
    }
}

/// The calendar reminder: Record, or Join & record when the invitation has
/// a call link. With Always record set for its service, it says so and
/// offers Join and Don't record instead.
fn reminder(
    p: Paint,
    meeting: &CalendarMeeting,
    automatic: bool,
    now_ms: i64,
) -> (rune::Color, String, String, Vec<El>) {
    let when = starts_label(meeting.start_ms, now_ms);
    let has_link = meeting.link.is_some();
    let send_meeting = |command: fn(CalendarMeeting) -> ControllerCommand| {
        move |c: &mut Capsule| {
            let State::Reminder { meeting, .. } = &c.state else {
                return;
            };
            let command = command(meeting.clone());
            c.send(command);
            c.close();
        }
    };
    if automatic {
        let mut actions = vec![button(
            p,
            "dont-record",
            tr("Don't record"),
            false,
            send_meeting(ControllerCommand::DismissReminder),
        )];
        if has_link {
            actions.push(button(
                p,
                "join",
                tr("Join"),
                true,
                send_meeting(ControllerCommand::JoinMeeting),
            ));
        }
        return (
            p.accent(),
            meeting.display_title(),
            tr_fill("{} · Hex records automatically", &when),
            actions,
        );
    }
    let mut actions = vec![button(
        p,
        "not-now",
        tr("Not now"),
        false,
        send_meeting(ControllerCommand::DismissReminder),
    )];
    if has_link {
        actions.push(button(
            p,
            "join-record",
            tr("Join & record"),
            false,
            send_meeting(|meeting| ControllerCommand::RecordMeeting {
                meeting,
                join: true,
            }),
        ));
    }
    actions.push(button(
        p,
        "record",
        tr("Record"),
        true,
        send_meeting(|meeting| ControllerCommand::RecordMeeting {
            meeting,
            join: false,
        }),
    ));
    let subtitle = match &meeting.link {
        Some(link) => format!("{when} · {}", link.provider.label()),
        None => when,
    };
    (p.accent(), meeting.display_title(), subtitle, actions)
}

/// The panel capsule (reminder, auto-started): status dot, two
/// truncated lines, and its actions, in a rounded rectangle. It drags to
/// another corner like the recording capsule.
fn pill(
    p: Paint,
    capsule: &Capsule,
    (dot, title, subtitle, actions): (rune::Color, String, String, Vec<El>),
) -> El {
    // Only the body takes the pointer, not the window's margin around it.
    let body = pill_body(p, status_dot(dot), &title, &subtitle)
        .w_full()
        .child(row().key("actions").gap(8.0).children(actions))
        .hit()
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(CAPSULE_INSET)
        .py(CAPSULE_INSET)
        .child(presence::anchored(capsule, body))
}

/// The panel capsule's corners, concentric with its buttons' (`radius.md`)
/// across the [`ACTIONS_INSET`] between them and the edge.
fn panel_radius(_: Paint) -> f32 {
    CAPSULE.radius
}

/// The panel capsule's body: its dot and two truncated lines in a rounded
/// rectangle.
fn pill_body(p: Paint, dot: El, title: &str, subtitle: &str) -> El {
    let c = &p.k.c;
    // A status named by its title, announced when the capsule's title
    // changes; the subtitle stays readable inside it.
    row()
        .key("pill")
        .role("status")
        .label(title)
        .h_full()
        .gap(12.0)
        .pl(CAPSULE.text_rail())
        .pr(ACTIONS_INSET)
        .center_y()
        .radius(panel_radius(p))
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(dot)
        .child(
            col()
                .key("lines")
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(title.to_owned())
                        .key("title")
                        .truncate()
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .a11y_hidden(),
                )
                .children((!subtitle.is_empty()).then(|| {
                    text(subtitle.to_owned())
                        .key("subtitle")
                        .truncate()
                        .font(p.k.ty.small)
                        .color(p.faint())
                })),
        )
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};

    use rune::text::TextSystem;
    use rune::{Env, Runtime};

    use super::panel::shell_word;
    use super::placement::corner_placement;
    use super::throw::nearest_corner;
    use super::*;
    use crate::meeting::detection::MeetingSource;

    type Chosen = Arc<Mutex<Vec<MeetingProviderChoice>>>;

    const PANEL: (f32, f32) = (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT);
    const MENU: (f32, f32) = (MEETING_MENU_WIDTH, MEETING_PANEL_HEIGHT);
    const RECORDING: (f32, f32) = RECORDING_WINDOW;
    const REMINDER: (f32, f32) = (REMINDER_PANEL_WIDTH, MEETING_PANEL_HEIGHT);
    const CALL_ENDED: (f32, f32) = (CALL_ENDED_PANEL_WIDTH, MEETING_PANEL_HEIGHT);

    /// "Design sync" a minute from now, with a Zoom link or none.
    fn design_sync(link: bool) -> CalendarMeeting {
        let start_ms = crate::calendar::wall_clock_ms() + 59_000;
        CalendarMeeting {
            id: "design-sync".into(),
            title: "Design sync".into(),
            start_ms,
            end_ms: start_ms + 30 * 60_000,
            attendees: vec!["Dax".into(), "Adam".into()],
            link: link.then(|| crate::calendar::CallLink {
                provider: crate::calendar::CallProvider::Zoom,
                url: "https://anomaly.zoom.us/j/123456789".into(),
            }),
        }
    }

    fn reminder(link: bool, automatic: bool) -> Kind {
        Kind::Reminder {
            meeting: design_sync(link),
            automatic,
        }
    }

    fn slack_huddle() -> MeetingCandidate {
        MeetingCandidate {
            key: "meeting-slack".into(),
            title: "Slack huddle".into(),
            source: MeetingSource::SlackHuddle,
            application: Some("com.tinyspeck.slackmacgap".into()),
            named: false,
            site: None,
            application_name: Some("Slack".into()),
            call: None,
        }
    }

    fn offer(candidate: MeetingCandidate) -> Kind {
        Kind::Offer(OfferId::next(), candidate)
    }

    fn bare_browser() -> MeetingCandidate {
        MeetingCandidate {
            key: "meeting-brave".into(),
            title: "Browser call".into(),
            source: MeetingSource::BrowserCall,
            application: Some("com.brave.Browser".into()),
            named: false,
            site: None,
            application_name: Some("Brave Browser".into()),
            call: None,
        }
    }

    fn runtime(
        kind: Kind,
        more_open: bool,
        dark: bool,
        size: (f32, f32),
    ) -> (Runtime<Capsule>, Receiver<ControllerCommand>, Chosen) {
        let (commands, received) = mpsc::sync_channel(4);
        let chosen = Chosen::default();
        let saved = chosen.clone();
        let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let presentation = offer_surface::Presentation::for_kind(&kind, more_open);
        let size = if matches!(kind, Kind::Offer(..)) {
            presentation.window()
        } else {
            size
        };
        let capsule = Capsule {
            state: State::from_kind(kind, more_open, None),
            presentation,
            stage: Default::default(),
            surface: presence::Surface::shown(),
            live: Live::default(),
            waveform: None,
            commands,
            hide: None,
            show: None,
            keep_recording: None,
            open_meeting: None,
            resize: None,
            #[cfg(target_os = "windows")]
            recording_window: Default::default(),
            choose: Arc::new(move |choice| saved.lock().unwrap().push(choice)),
            corner: MeetingIndicatorCorner::TopRight,
            display: None,
            shift: None,
            retarget: None,
            throw: throw::Throw::default(),
            reduced: Cell::new(false),
            scale: Cell::new(1.0),
            // No screen: a release lands back in the corner it started in.
            screen: |_| None,
            displays: |_| None,
            save_anchor: Arc::new(|_, _| {}),
        };
        let env = Env {
            system_dark: dark,
            scale: 2.0,
        };
        let mut rt = crate::rune_ui::offscreen::runtime(capsule, view, size, env, text);
        // The native overlay polls each frame; here only the surface steps,
        // so no test reads the wall clock or the capture meter.
        rt.poll = Some(|c: &mut Capsule, now, tx: &mut Tx| c.step_stage(now, tx));
        rt.frame(0.0);
        (rt, received, chosen)
    }

    fn tap(rt: &mut Runtime<Capsule>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn texts(rt: &Runtime<Capsule>) -> Vec<String> {
        rt.elements().into_iter().filter_map(|e| e.text).collect()
    }

    #[test]
    fn an_offer_names_the_call_without_asking_about_consent() {
        let (rt, _, _) = runtime(offer(slack_huddle()), false, true, PANEL);
        assert_eq!(texts(&rt)[0], "Slack huddle detected");
        assert!(rt.find("subtitle").is_none());
        assert!(rt.find("offer-more").is_some());
    }

    /// The offscreen runtime has no host poll. Step the same presentation
    /// clock the native overlay polls, on the original surface.
    fn navigate_offer(rt: &mut Runtime<Capsule>, key: &str) {
        tap(rt, key);
        for _ in 0..60 {
            advance_offer(rt);
        }
    }

    fn advance_offer(rt: &mut Runtime<Capsule>) {
        rt.frame(1.0 / 60.0);
    }

    /// Lands everything in flight, as `settle_all` lands Rune's tracks.
    fn settle_stage(rt: &mut Runtime<Capsule>) {
        rt.update(|c, tx| {
            let now = tx.clock();
            c.settle_stage(now);
        });
    }

    /// The runtime's surface, stage and Rune's tracks all at rest.
    fn landed(rt: &mut Runtime<Capsule>) {
        settle_stage(rt);
        rt.settle_all();
        rt.frame(0.);
    }

    #[test]
    fn the_offer_menu_stays_usable_past_the_unanswered_deadline() {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
        let (arm, armed) = mpsc::channel();
        rt.state.set_offer_timer(Some(Box::new(move |delay, fire| {
            arm.send((delay, fire)).unwrap()
        })));
        let (delay, fire) = armed.try_recv().unwrap();
        assert_eq!(delay, Duration::from_secs(20));
        rt.frame(19.0);
        navigate_offer(&mut rt, "offer-more");
        assert!(rt.find("always-record").is_some());
        assert!(commands.try_recv().is_err());
        assert!(chosen.lock().unwrap().is_empty());

        // Deliver the original 20 s timer on this same widened capsule.
        rt.frame(delay.as_secs_f64() - 19.0);
        rt.update(|c, _| fire(c));
        rt.frame(0.0);
        assert!(rt.find("always-record").is_some(), "menu closed at expiry");
        assert!(commands.try_recv().is_err());
        assert!(chosen.lock().unwrap().is_empty());
        tap(&mut rt, "never-offer");
        assert_eq!(chosen.lock().unwrap()[0].policy, MeetingPolicy::NeverOffer);
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::Dismiss)
        ));
    }

    #[test]
    fn always_record_saves_the_choice_and_records_this_call() {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
        navigate_offer(&mut rt, "offer-more");
        rt.update(|c, _| c.expire_offer());
        assert_eq!(texts(&rt)[0], "Slack huddles");
        tap(&mut rt, "always-record");
        let chosen = chosen.lock().unwrap();
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].key, "com.tinyspeck.slackmacgap");
        assert_eq!(chosen[0].policy, MeetingPolicy::AlwaysRecord);
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::Record(candidate, Some(_))) if candidate.key == "meeting-slack"
        ));
        assert!(rt.state.presentation.inherits_offer_window());
    }

    #[test]
    fn an_old_offer_timeout_cannot_close_a_replacement() {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
        let current = Arc::new(AtomicU64::new(1));
        let queued = offer::delivery(current.clone(), 1, Capsule::expire_offer);
        navigate_offer(&mut rt, "offer-more");
        // A new offer of the very same call replaces the open choices
        // before Rune gets to the first offer's timeout update.
        current.fetch_add(1, Ordering::SeqCst);
        rt.update(|c, _| {
            let next = offer(slack_huddle());
            c.presentation = offer_surface::Presentation::for_kind(&next, false);
            c.state = State::from_kind(next, false, None);
        });
        rt.set_viewport(PANEL, 2.0);
        rt.update(queued);
        rt.frame(0.0);
        assert!(rt.find("offer-more").is_some());
        assert!(rt.find("always-record").is_none());
        assert!(commands.try_recv().is_err());
        assert!(chosen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_untouched_offer_times_out_without_a_choice() {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
        rt.update(|c, _| c.expire_offer());
        rt.frame(0.0);
        // Closed at once; what it showed leaves over the next frames.
        assert!(!rt.state.is_shown());
        for _ in 0..40 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.find("offer-more").is_none());
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::OfferTimedOut(_))
        ));
        assert!(commands.try_recv().is_err());
        assert!(chosen.lock().unwrap().is_empty());
    }

    #[test]
    fn cancel_returns_to_a_timed_clickable_offer() {
        for deadline_passed in [false, true] {
            let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
            let (arm, armed) = mpsc::channel();
            rt.state.set_offer_timer(Some(Box::new(move |delay, fire| {
                arm.send((delay, fire)).unwrap()
            })));
            let (delay, first_timeout) = armed.try_recv().unwrap();
            assert_eq!(delay, Duration::from_secs(20));
            navigate_offer(&mut rt, "offer-more");
            if deadline_passed {
                rt.update(|c, _| first_timeout(c));
            }
            navigate_offer(&mut rt, "offer-cancel");
            assert!(rt.find("record").is_some());
            assert!(commands.try_recv().is_err());
            assert!(chosen.lock().unwrap().is_empty());
            if deadline_passed {
                let (delay, new_timeout) =
                    armed.try_recv().expect("Cancel rearms the expired timer");
                assert_eq!(delay, Duration::from_secs(20));
                rt.update(|c, _| new_timeout(c));
                assert!(matches!(
                    commands.try_recv(),
                    Ok(ControllerCommand::OfferTimedOut(_))
                ));
            } else {
                assert!(armed.try_recv().is_err(), "original timer is still pending");
                tap(&mut rt, "not-now");
                assert!(matches!(
                    commands.try_recv(),
                    Ok(ControllerCommand::Dismiss)
                ));
                rt.update(|c, _| first_timeout(c));
            }
            assert!(!rt.state.is_shown());
            assert!(commands.try_recv().is_err());
        }
    }

    #[test]
    fn never_offer_saves_the_choice_and_dismisses() {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), true, true, MENU);
        tap(&mut rt, "never-offer");
        assert_eq!(chosen.lock().unwrap()[0].policy, MeetingPolicy::NeverOffer);
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::Dismiss)
        ));
    }

    #[test]
    fn a_browser_on_an_unknown_page_cannot_be_set_to_always_record() {
        let (rt, _, _) = runtime(offer(bare_browser()), true, true, MENU);
        assert!(rt.find("never-offer").is_some());
        assert!(rt.find("always-record").is_none());
    }

    #[test]
    fn an_automatic_recording_says_so_at_first() {
        let recording = |started| {
            Kind::Recording(Recording {
                id: "m".into(),
                title: "Slack Huddle call — 2:30 PM".into(),
                started,
                automatic: true,
                warning: None,
            })
        };
        let (mut rt, _, _) = runtime(recording(Instant::now()), false, true, RECORDING);
        assert!(texts(&rt).contains(&"Started automatically".to_string()));
        // Open, it says so under the header.
        open_recording(&mut rt);
        assert!(rt.find("notice-line").is_some());
        let earlier = Instant::now() - AUTOMATIC_LABEL - Duration::from_secs(1);
        let (rt, _, _) = runtime(recording(earlier), false, true, RECORDING);
        assert!(!texts(&rt).iter().any(|text| text.contains("automatically")));
    }

    #[test]
    fn a_recording_says_when_its_microphone_is_silent() {
        let mut kind = live_recording("m");
        let Kind::Recording(Recording { warning, .. }) = &mut kind else {
            panic!("a recording");
        };
        *warning = Some(crate::meeting::TrackWarning::DigitalSilence);
        let (rt, _, _) = runtime(kind, false, true, RECORDING);
        assert!(
            texts(&rt).contains(&"Microphone is silent".to_string()),
            "{:?}",
            texts(&rt)
        );
    }

    #[test]
    fn a_reminder_with_a_link_joins_and_records_on_one_click() {
        let (mut rt, commands, _) = runtime(reminder(true, false), false, true, REMINDER);
        let shown = texts(&rt);
        assert!(shown.contains(&"Design sync".to_string()), "{shown:?}");
        assert!(
            shown.contains(&"Starts in 1 min · Zoom".to_string()),
            "{shown:?}"
        );
        assert!(rt.find("record").is_some());
        tap(&mut rt, "join-record");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::RecordMeeting { meeting, join: true }) if meeting.id == "design-sync"
        ));
        assert!(!rt.state.is_shown());
    }

    #[test]
    fn a_reminder_without_a_link_only_records() {
        let (mut rt, commands, _) = runtime(reminder(false, false), false, false, REMINDER);
        assert!(texts(&rt).contains(&"Starts in 1 min".to_string()));
        assert!(rt.find("join-record").is_none());
        tap(&mut rt, "record");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::RecordMeeting { join: false, .. })
        ));
    }

    #[test]
    fn dismissing_a_reminder_tells_the_controller() {
        let (mut rt, commands, _) = runtime(reminder(true, false), false, true, REMINDER);
        tap(&mut rt, "not-now");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::DismissReminder(meeting)) if meeting.id == "design-sync"
        ));
        assert!(!rt.state.is_shown());
    }

    #[test]
    fn an_always_record_reminder_says_so_and_never_offers_record() {
        let (mut rt, commands, _) = runtime(reminder(true, true), false, true, REMINDER);
        assert!(texts(&rt).contains(&"Starts in 1 min · Hex records automatically".to_string()));
        assert!(rt.find("record").is_none());
        assert!(rt.find("join-record").is_none());
        tap(&mut rt, "join");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::JoinMeeting(_))
        ));

        let (mut rt, commands, _) = runtime(reminder(false, true), false, true, REMINDER);
        assert!(rt.find("join").is_none());
        tap(&mut rt, "dont-record");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::DismissReminder(_))
        ));
    }

    #[test]
    fn a_reminders_buttons_take_the_pointer_while_its_body_grabs() {
        let (mut rt, _, _) = runtime(reminder(true, false), false, true, REMINDER);
        landed(&mut rt);
        for key in ["not-now", "join-record", "record"] {
            let (x, y) = rt.find(key).unwrap();
            rt.pointer_move(x, y);
            rt.frame(1. / 60.);
            assert_eq!(rt.cursor(), Cursor::Pointer, "{key}");
        }
        // The name and the time between the dot and the buttons drag it.
        let (x, y) = rt.find("title").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1. / 60.);
        assert_eq!(rt.cursor(), Cursor::Grab);
    }

    #[test]
    fn the_reminder_counts_down_to_the_start() {
        let now = 1_000_000;
        assert_eq!(starts_label(now + 4 * 60_000 + 1, now), "Starts in 5 min");
        assert_eq!(starts_label(now + 60_000, now), "Starts in 1 min");
        assert_eq!(starts_label(now, now), "Starting now");
        assert_eq!(starts_label(now - 59_000, now), "Starting now");
        assert_eq!(starts_label(now - 2 * 60_000, now), "Started 2 min ago");
    }

    fn live_recording(id: &str) -> Kind {
        Kind::Recording(Recording {
            id: id.into(),
            title: "Q4 launch sync".into(),
            started: Instant::now() - Duration::from_secs(258),
            automatic: false,
            warning: None,
        })
    }

    fn call() -> Vec<TranscriptEntry> {
        let entry = |source, start_ms, text: &str| TranscriptEntry {
            source,
            start_ms,
            end_ms: start_ms + 2_000,
            text: text.into(),
        };
        vec![
            entry(
                meeting::MeetingSource::System,
                0,
                "Kickoff is at two thirty.",
            ),
            entry(
                meeting::MeetingSource::Microphone,
                3_000,
                "Two thirty works.",
            ),
            entry(meeting::MeetingSource::System, 6_000, "Then let's ship it."),
        ]
    }

    #[test]
    fn the_recording_capsule_copies_the_in_call_transcript() {
        let id = "capsule-copy-test";
        let (mut rt, _, _) = runtime(live_recording(id), false, true, RECORDING);
        open_recording(&mut rt);
        // Nothing transcribed yet: Copy all is unavailable.
        tap(&mut rt, "copy-all");
        assert!(rt.copy_request.is_none());

        rt.update(|capsule, _| capsule.live.entries = call());
        rt.frame(1.0 / 60.0);
        tap(&mut rt, "copy-all");
        let all = rt.copy_request.take().unwrap();
        assert!(
            all.starts_with("**Them** · 00:00\nKickoff is at two thirty."),
            "{}",
            &*all
        );
        assert!(texts(&rt).contains(&"Copied".to_string()));
    }

    fn open_recording(rt: &mut Runtime<Capsule>) {
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.frame(0.0);
        settle_stage(rt);
        rt.settle_all();
        rt.frame(0.0);
    }

    /// A stopped capture snaps by the window it was resized to, not the
    /// largest one: `capture_stopped` and the drag's `window_size` read
    /// one size.
    #[test]
    fn a_stopped_capture_snaps_by_the_window_it_was_resized_to() {
        let (mut rt, _, _) = runtime(live_recording("finalizing-size"), false, true, RECORDING);
        rt.update(|capsule, _| capsule.capture_stopped());
        rt.update(|capsule, _| {
            assert!(matches!(capsule.state, State::Finalizing { .. }));
            assert_eq!(capsule.window_size(), finalizing_window_size());
        });
    }

    #[test]
    fn the_recording_capsule_rests_compact_and_opens_under_the_pointer() {
        let (mut rt, _, _) = runtime(live_recording("capsule-hover"), false, true, RECORDING);
        rt.update(|capsule, _| capsule.live.entries = call());
        landed(&mut rt);
        // At rest, the meter and time, no actions.
        assert!(rt.find("copy-prompt").is_none());
        assert!(rt.find("stop").is_none());
        assert_eq!(texts(&rt), ["04:18"]);
        let (x, _) = rt.find("pill").unwrap();
        assert!(
            x > RECORDING.0 / 2.0,
            "a right corner's pill rests on the right"
        );

        open_recording(&mut rt);
        assert!(rt.state.hovered());
        assert!(rt.find("copy-prompt").is_some());
        // Moving onto a button inside the pill keeps it open.
        let (x, y) = rt.find("stop").unwrap();
        rt.pointer_move(x, y);
        rt.frame(0.0);
        assert!(rt.state.hovered());

        rt.pointer_leave();
        rt.frame(0.0);
        assert!(!rt.state.hovered());
    }

    #[test]
    fn dragging_the_recording_capsule_moves_it_and_snaps_to_a_corner() {
        type Moves = Arc<Mutex<Vec<(MeetingIndicatorCorner, f32, f32)>>>;
        let (mut rt, _, _) = runtime(live_recording("capsule-drag"), false, true, RECORDING);
        let (moves, saved): (Moves, Arc<Mutex<Vec<MeetingIndicatorCorner>>>) = Default::default();
        let (record, save) = (moves.clone(), saved.clone());
        rt.update(move |capsule, _| {
            capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                record.lock().unwrap().push((corner, dx, dy));
            }));
            capsule.save_anchor = Arc::new(move |corner, _| save.lock().unwrap().push(corner));
        });
        rt.frame(0.0);
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x - 20.0, y + 30.0);
        rt.frame(0.0);
        assert_eq!(
            moves.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopRight, -20.0, 30.0))
        );
        rt.pointer_up(x - 20.0, y + 30.0);
        rt.frame(0.0);
        // Off the main thread there is no screen to snap against, so it
        // returns to its corner.
        assert_eq!(
            moves.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopRight, 0.0, 0.0))
        );
        assert!(saved.lock().unwrap().is_empty());
    }

    /// Two displays in one point space: the 1512 × 982 primary laptop, and
    /// the 1920 × 1080 monitor to its right, the pointer's (so listed first).
    fn two_displays() -> Vec<screen::Display> {
        let display = |id: &str, x: f32, size: (f32, f32), primary: bool| screen::Display {
            target: if primary {
                rune::DisplayTarget::Primary
            } else {
                rune::DisplayTarget::Native(rune::NativeDisplayId::Windows(7))
            },
            id: id.to_owned(),
            frame: (x, 0.0, size.0, size.1),
            origin: (x, 0.0),
            size,
            scale: 1.0,
            primary,
        };
        vec![
            display("monitor", 1512.0, (1920.0, 1080.0), false),
            display("laptop", 0.0, (1512.0, 982.0), true),
        ]
    }

    #[test]
    fn the_display_lab_lands_a_release_on_the_pointer_s_display() {
        type Moves = Arc<Mutex<Vec<(MeetingIndicatorCorner, f32, f32)>>>;
        type Anchors = Arc<Mutex<Vec<(MeetingIndicatorCorner, Option<String>)>>>;
        type Targets = Arc<Mutex<Vec<Option<String>>>>;
        let (mut rt, _, _) = runtime(live_recording("capsule-displays"), false, true, RECORDING);
        let (moves, anchors, targets): (Moves, Anchors, Targets) = Default::default();
        let (record, save, target) = (moves.clone(), anchors.clone(), targets.clone());
        rt.update(move |capsule, _| {
            capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                record.lock().unwrap().push((corner, dx, dy));
            }));
            capsule.retarget = Some(Arc::new(move |display| {
                target.lock().unwrap().push(display.map(str::to_owned));
            }));
            capsule.save_anchor = Arc::new(move |corner, display| {
                save.lock()
                    .unwrap()
                    .push((corner, display.map(str::to_owned)));
            });
            capsule.screen = |_| Some((1512.0, 982.0));
            capsule.displays = |_| {
                Some(screen::Displays {
                    all: two_displays(),
                    pointer: true,
                })
            };
        });
        placement::force_anywhere(Some(true));
        rt.frame(0.0);
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x + 500.0, y);
        rt.frame(0.0);
        rt.pointer_up(x + 500.0, y);
        rt.frame(0.0);
        placement::force_anywhere(None);
        // It jumped onto the monitor, into that display's nearest corner,
        // and saved both.
        assert_eq!(
            targets.lock().unwrap().last(),
            Some(&Some("monitor".to_owned()))
        );
        assert_eq!(
            moves.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopLeft, 0.0, 0.0))
        );
        assert_eq!(
            anchors.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopLeft, Some("monitor".to_owned())))
        );
    }

    #[test]
    fn without_the_display_lab_a_release_keeps_the_primary_display() {
        type Anchors = Arc<Mutex<Vec<(MeetingIndicatorCorner, Option<String>)>>>;
        let (mut rt, _, _) = runtime(live_recording("capsule-primary"), false, true, RECORDING);
        let anchors: Anchors = Default::default();
        let save = anchors.clone();
        let targets: Arc<Mutex<Vec<Option<String>>>> = Default::default();
        let target = targets.clone();
        rt.update(move |capsule, _| {
            capsule.shift = Some(Arc::new(|_, _, _| {}));
            capsule.retarget = Some(Arc::new(move |display| {
                target.lock().unwrap().push(display.map(str::to_owned));
            }));
            capsule.save_anchor = Arc::new(move |corner, display| {
                save.lock()
                    .unwrap()
                    .push((corner, display.map(str::to_owned)));
            });
            capsule.screen = |_| Some((1512.0, 982.0));
            capsule.displays = |_| {
                Some(screen::Displays {
                    all: two_displays(),
                    pointer: true,
                })
            };
        });
        rt.frame(0.0);
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x - 700.0, y);
        rt.frame(0.0);
        rt.pointer_up(x - 700.0, y);
        rt.frame(0.0);
        // The primary display measured it, as before, and no display was
        // written.
        assert!(targets.lock().unwrap().is_empty());
        assert_eq!(
            anchors.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopLeft, None))
        );
    }

    #[test]
    fn a_cancelled_capsule_drag_returns_to_its_corner_without_saving() {
        type Moves = Arc<Mutex<Vec<(MeetingIndicatorCorner, f32, f32)>>>;
        let (mut rt, _, _) = runtime(live_recording("capsule-cancel"), false, true, RECORDING);
        let (moves, saved): (Moves, Arc<Mutex<Vec<MeetingIndicatorCorner>>>) = Default::default();
        let (record, save) = (moves.clone(), saved.clone());
        rt.update(move |capsule, _| {
            capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                record.lock().unwrap().push((corner, dx, dy));
            }));
            capsule.save_anchor = Arc::new(move |corner, _| save.lock().unwrap().push(corner));
        });
        rt.frame(0.0);
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x - 400.0, y + 300.0);
        rt.frame(0.0);
        rt.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        });
        rt.frame(0.0);
        assert_eq!(
            moves.lock().unwrap().last(),
            Some(&(MeetingIndicatorCorner::TopRight, 0.0, 0.0))
        );
        assert!(saved.lock().unwrap().is_empty());
    }

    #[test]
    fn a_released_capsule_snaps_to_the_corner_nearest_its_centre() {
        use MeetingIndicatorCorner::*;
        let screen = (1512.0, 982.0);
        let nearest = |from, dx, dy| nearest_corner(from, dx, dy, screen, RECORDING);
        assert_eq!(nearest(TopRight, -100.0, 40.0), TopRight);
        assert_eq!(nearest(TopRight, -700.0, 0.0), TopLeft);
        assert_eq!(nearest(TopRight, 0.0, 700.0), BottomRight);
        assert_eq!(nearest(BottomLeft, 700.0, -600.0), TopRight);
        assert_eq!(nearest(BottomLeft, 10.0, -10.0), BottomLeft);
        let (edge, margin, align, inset) = corner_placement(BottomLeft, 10.0, -10.0);
        assert!(matches!((edge, align), (ScreenEdge::Bottom, Align::Start)));
        assert_eq!(
            (margin, inset),
            (WINDOW_MARGIN + DOCK + 10.0, WINDOW_MARGIN + 10.0)
        );
    }

    #[test]
    fn a_released_panel_snaps_by_its_own_size() {
        use MeetingIndicatorCorner::*;
        let screen = (1512.0, 982.0);
        // The panel's centre sits 222 pt in from the right edge, the
        // recording capsule's 142 pt: the same drag crosses the middle
        // only for the wider window.
        assert_eq!(
            nearest_corner(TopRight, -560.0, 0.0, screen, PANEL),
            TopLeft
        );
        assert_eq!(
            nearest_corner(TopRight, -560.0, 0.0, screen, RECORDING),
            TopRight
        );
        assert_eq!(
            nearest_corner(TopRight, -600.0, 0.0, screen, PANEL),
            TopLeft
        );
        assert_eq!(
            nearest_corner(TopLeft, 0.0, 500.0, screen, PANEL),
            BottomLeft
        );
        assert_eq!(
            nearest_corner(BottomRight, 0.0, -400.0, screen, PANEL),
            BottomRight
        );
        assert_eq!(
            nearest_corner(BottomRight, 0.0, -500.0, screen, PANEL),
            TopRight
        );
    }

    #[test]
    fn each_corner_places_the_panel_at_its_edges() {
        use MeetingIndicatorCorner::*;
        let top = WINDOW_MARGIN + MENU_BAR;
        let bottom = WINDOW_MARGIN + DOCK;
        for (corner, margin) in [
            (TopRight, top),
            (TopLeft, top),
            (BottomRight, bottom),
            (BottomLeft, bottom),
        ] {
            let (edge, placed_margin, align, inset) = corner_placement(corner, 0.0, 0.0);
            assert_eq!(
                matches!(edge, ScreenEdge::Top),
                corner.is_top(),
                "{corner:?}"
            );
            assert_eq!(matches!(align, Align::End), corner.is_right(), "{corner:?}");
            assert_eq!(
                (placed_margin, inset),
                (margin, WINDOW_MARGIN),
                "{corner:?}"
            );
        }
    }

    #[test]
    fn a_panel_drags_like_the_recording_capsule() {
        type Moves = Arc<Mutex<Vec<(MeetingIndicatorCorner, f32, f32)>>>;
        let (mut rt, _, _) = runtime(ready_transcript(), false, true, ready_size());
        let moves: Moves = Moves::default();
        let record = moves.clone();
        rt.update(move |capsule, _| {
            capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                record.lock().unwrap().push((corner, dx, dy));
            }));
        });
        rt.frame(0.0);
        // The panel's own margin, clear of its link and copies.
        let [px, py, _, _] = laid_out(&rt, "pill");
        let (x, y) = (px + 2.0, py + 2.0);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x - 20.0, y + 30.0);
        rt.frame(0.0);
        rt.pointer_up(x - 20.0, y + 30.0);
        rt.frame(0.0);
        let moves = moves.lock().unwrap();
        assert!(
            moves.contains(&(MeetingIndicatorCorner::TopRight, -20.0, 30.0)),
            "{moves:?}"
        );
        assert_eq!(
            moves.last(),
            Some(&(MeetingIndicatorCorner::TopRight, 0.0, 0.0))
        );
        // The drag pressed nothing.
        assert!(rt.state.is_shown());
    }

    fn call_ended() -> Kind {
        Kind::CallEnded {
            app: MeetingSource::Teams.label().into(),
            application: None,
            stops: None,
        }
    }

    fn call_ended_countdown(left: u64) -> Kind {
        let left = Duration::from_secs(left);
        Kind::CallEnded {
            app: MeetingSource::Teams.label().into(),
            application: None,
            stops: Some((Instant::now() + left, call_ended::Countdown::new(left))),
        }
    }

    fn ready_transcript() -> Kind {
        ready_with(serde_json::Value::Null)
    }

    /// "Lantern design sync", with Apple Intelligence's summary.
    fn ready_summarized() -> Kind {
        ready_with(serde_json::json!({
            "title": "Lantern design sync",
            "summary": "March 26 depends on passing the soak test by March 5. Friday's playtest decides the festival demo.",
        }))
    }

    fn ready_size_of(kind: &Kind) -> (f32, f32) {
        let Kind::Ready(manifest) = kind else {
            panic!("a ready meeting");
        };
        ready_window_size(manifest)
    }

    fn ready_size() -> (f32, f32) {
        ready_size_of(&ready_transcript())
    }

    fn ready_with(notes: serde_json::Value) -> Kind {
        Kind::Ready(Box::new(
            serde_json::from_value(serde_json::json!({
                "schema_version": 1,
                "id": "ready-snap",
                "title": "Design sync",
                "status": "complete",
                "created_at_ms": 1_760_000_000_000_u64,
                "ended_at_ms": null,
                "duration_ms": 536_000,
                "notes": notes,
                "title_source": if notes.is_null() { "user" } else { "default" },
                "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
                "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
                "error": null,
                "export_path": "/Users/alex/Documents/Hex Meetings/Design sync.md"
            }))
            .unwrap(),
        ))
    }

    fn auto_started() -> Kind {
        Kind::AutoStarted {
            title: "Slack Huddle call — 2:30 PM".into(),
        }
    }

    #[test]
    fn the_ready_panel_copies_the_finished_meeting() {
        let kind = ready_summarized();
        let size = ready_size_of(&kind);
        let (mut rt, commands, _) = runtime(kind, false, true, size);
        let shown = texts(&rt);
        for expected in [
            "Lantern design sync",
            "08:56",
            "Copy transcript",
            "Copy agent prompt",
        ] {
            assert!(shown.contains(&expected.to_string()), "{shown:?}");
        }
        assert!(shown.iter().any(|text| text.starts_with("March 26")));
        tap(&mut rt, "copy-prompt");
        let prompt = rt.copy_request.take().unwrap();
        assert!(
            prompt.contains("meetings show ready-snap --md"),
            "{}",
            &*prompt
        );
        assert!(commands.try_recv().is_err());
        assert!(rt.state.is_shown());
    }

    #[test]
    fn summary_inherits_the_recordings_outer_anchor_without_the_drag_lab() {
        use MeetingIndicatorCorner::{BottomLeft, TopRight};
        let anchor = (
            "ready-snap".to_owned(),
            Anchor {
                corner: BottomLeft,
                display: None,
            },
        );
        let kind = ready_summarized();
        let saved = || Anchor {
            corner: TopRight,
            display: None,
        };
        assert_eq!(
            panel::show_anchor(&kind, Some(&anchor), saved).corner,
            BottomLeft
        );
        assert_eq!(
            panel::show_anchor(&call_ended(), Some(&anchor), saved).corner,
            TopRight
        );
        let (mut live, _, _) = runtime(live_recording("ready-snap"), false, false, RECORDING);
        live.update(|c, tx| {
            if let State::Recording {
                recording: shown,
                hovered,
                ..
            } = &mut c.state
            {
                shown.started = Instant::now() - Duration::from_secs(536);
                *hovered = true;
            }
            c.live.entries = call();
            c.corner = BottomLeft;
            fixture_scope(c);
            tx.instant();
        });
        let size = ready_size_of(&kind);
        let (mut ready, commands, _) = runtime(kind, false, false, size);
        ready.update(move |c, tx| {
            c.corner = BottomLeft;
            c.live.entries = call();
            tx.instant();
        });
        let outer = |rt: &mut Runtime<Capsule>, window: (f32, f32)| {
            settle_stage(rt);
            rt.settle_all();
            rt.frame(0.0);
            let [x, y, _, h] = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("pill"))
                .unwrap()
                .rect;
            // Both windows target the primary screen. Convert their local
            // bottom-left point to screen-edge distances, including the dock margin.
            let (edge, margin, align, inset) = corner_placement(rt.state.corner, 0.0, 0.0);
            assert!(matches!((edge, align), (ScreenEdge::Bottom, Align::Start)));
            (x + inset, window.1 - (y + h) + margin)
        };
        assert_eq!(outer(&mut live, RECORDING), outer(&mut ready, size));
        let (x, y) = ready.find("dismiss").unwrap();
        ready.pointer_move(x, y);
        ready.frame(0.0);
        assert!(ready.state.hovered());
        if let Some(dir) = std::env::var_os("HEX_SUMMARY_SNAPS") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
            for (name, rt, size) in [
                ("recording", &mut live, RECORDING),
                ("summary", &mut ready, size),
            ] {
                rt.transparent = true;
                let (w, h, pixels) = render(&gpu, rt, size);
                rune::render::write_png(&dir.join(format!("{name}.png")), w, h, &pixels).unwrap();
            }
        }
        tap(&mut ready, "dismiss");
        assert!(!ready.state.is_shown());
        assert!(ready.copy_request.is_none() && commands.try_recv().is_err());
        let Kind::Ready(notes) = ready_summarized() else {
            unreachable!()
        };
        ready.update(move |c, _| c.refresh_ready(notes));
        assert!(!ready.state.is_shown());
    }

    /// Gives a shown ready panel a stay that ends almost at once.
    fn nearly_over(capsule: &mut Capsule) {
        if let State::Ready { stay, .. } = &mut capsule.state {
            *stay = stay::Stay::new(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_ready_panels_stay_holds_under_the_pointer_and_runs_out_after_it_leaves() {
        let (mut rt, _, _) = runtime(ready_transcript(), false, true, ready_size());
        rt.update(|capsule, _| {
            nearly_over(capsule);
            capsule.set_hovered(true);
            std::thread::sleep(Duration::from_millis(20));
            // Held: the timer firing under the pointer closes nothing, and
            // neither does the frame hook.
            capsule.expire_panel();
            assert!(!capsule.stay_over());
        });
        assert!(rt.state.is_shown());
        rt.update(|capsule, _| {
            capsule.set_hovered(false);
            // It runs on from where it was held, not from the start.
            assert!(capsule.is_shown());
            std::thread::sleep(Duration::from_millis(20));
            assert!(capsule.stay_over());
        });
        assert!(!rt.state.is_shown());
    }

    #[test]
    fn published_after_stop_clears_stopping_and_preserves_hover() {
        let (mut rt, _, _) = runtime(live_recording("m"), false, true, RECORDING);
        let Kind::Ready(manifest) = ready_transcript() else {
            unreachable!()
        };
        rt.update(move |c, _| {
            c.set_hovered(true);
            c.mark_stopping();
            assert!(c.stopping() && c.open());
            c.capture_stopped();
            assert!(!c.stopping() && !c.open() && c.hovered());
            c.published(manifest);
            assert!(matches!(
                c.state,
                State::Ready {
                    hover: ReadyHover::Hovered,
                    ..
                }
            ));
            assert!(!c.stopping() && !c.open());
            nearly_over(c);
            c.expire_panel();
            assert!(c.is_shown());
            c.set_hovered(false);
            std::thread::sleep(Duration::from_millis(20));
            assert!(c.stay_over() && !c.is_shown());
        });
    }

    #[test]
    fn closing_an_expired_offer_drops_its_choices_and_timer() {
        let (mut rt, _, _) = runtime(offer(slack_huddle()), false, true, PANEL);
        let (arm, armed) = mpsc::channel();
        rt.state.set_offer_timer(Some(Box::new(move |delay, fire| {
            arm.send((delay, fire)).unwrap()
        })));
        let (_, first_timeout) = armed.try_recv().unwrap();
        rt.update(|c, _| {
            c.set_more_open(true);
            first_timeout(c);
            c.close();
            assert!(!c.is_shown() && !c.more_open());
            c.set_more_open(false);
        });
        assert!(armed.try_recv().is_err(), "closed offer never rearms");
    }

    #[test]
    fn the_agent_prompt_names_real_commands() {
        assert_eq!(
            agent_prompt("hex", "2026-11-10-1430", "Q4 launch sync", Some(258_000)),
            "Use my Hex meeting \"Q4 launch sync\" as context. It is still recording.\n\n\
             Read the transcript so far:\n  hex meetings show 2026-11-10-1430 --md\n\n\
             Catch up on what was said since this prompt (the first turn may repeat):\n  \
             hex meetings show 2026-11-10-1430 --from 04:18 --md\n\n\
             Find a topic, then read around the time it came up:\n  \
             hex meetings search \"<words>\"\n  \
             hex meetings show 2026-11-10-1430 --from <mm:ss> --to <mm:ss> --md\n"
        );
        let finished = agent_prompt("hex", "m", "Design sync", None);
        assert!(finished.starts_with("Use my Hex meeting \"Design sync\" as context.\n\n"));
        assert!(finished.contains("  hex meetings show m --md\n"));
        assert!(!finished.contains("--from 0"));
    }

    #[test]
    fn hex_commands_quote_paths_with_spaces() {
        assert_eq!(
            shell_word("/Applications/Hex.app/Contents/MacOS/hex"),
            "/Applications/Hex.app/Contents/MacOS/hex"
        );
        assert_eq!(
            shell_word("/Users/alex/Hex Builds/hex"),
            "'/Users/alex/Hex Builds/hex'"
        );
        assert_eq!(shell_word("it's"), r"'it'\''s'");
    }

    #[test]
    fn a_bottom_corner_opens_upward() {
        let (mut rt, _, _) = runtime(live_recording("bottom"), false, true, RECORDING);
        rt.update(|capsule, _| capsule.corner = MeetingIndicatorCorner::BottomRight);
        open_recording(&mut rt);
        let (_, header) = rt.find("header").unwrap();
        let (_, copy_prompt) = rt.find("copy-prompt").unwrap();
        assert!(
            header > copy_prompt,
            "the header sits nearest the screen edge"
        );
    }

    #[test]
    fn dismissing_an_automatic_recording_never_stops_it() {
        let (mut rt, commands, _) = runtime(auto_started(), false, true, PANEL);
        tap(&mut rt, "dismiss");
        assert!(commands.try_recv().is_err());
        assert!(!rt.state.is_shown());
    }

    #[test]
    fn an_automatic_stop_says_so_and_keep_recording_cancels_it() {
        let auto = call_ended_countdown(30);
        let (mut rt, commands, _) = runtime(auto, false, true, CALL_ENDED);
        assert!(texts(&rt).contains(&"Recording stops in 30 s".to_string()));
        assert!(texts(&rt).contains(&"Stop now".to_string()));
        tap(&mut rt, "keep-recording");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::KeepRecording)
        ));
        assert!(!rt.state.is_shown());

        // Without the lab, Keep recording sends nothing.
        let (mut rt, commands, _) = runtime(call_ended(), false, true, CALL_ENDED);
        assert!(texts(&rt).contains(&"Hex is still recording.".to_string()));
        let kept = Arc::new(AtomicU64::new(0));
        let counted = kept.clone();
        rt.state.keep_recording = Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        tap(&mut rt, "keep-recording");
        assert!(commands.try_recv().is_err());
        assert_eq!(
            kept.load(Ordering::SeqCst),
            1,
            "the recording capsule returns"
        );
        assert!(!rt.state.is_shown());
    }

    fn started(id: &str) -> ControllerEvent {
        ControllerEvent::RecordingStarted {
            id: id.into(),
            title: "Design sync".into(),
            automatic: false,
        }
    }

    fn shown(model: &PillModel, indicator: bool) -> Option<(String, String, Instant)> {
        model
            .pill(indicator)
            .map(|shown| (shown.id.clone(), shown.title.clone(), shown.started))
    }

    #[test]
    fn keep_recording_brings_back_the_recording_capsule_as_it_was() {
        let mut model = PillModel::default();
        let start = Instant::now();
        model.apply(&started("m"), start);
        model.apply(
            &ControllerEvent::CallEnded {
                app: "Zoom".into(),
                application: None,
                stops_at: None,
            },
            start,
        );
        assert!(model.pill(true).is_none(), "the prompt stands in for it");
        model.apply(
            &ControllerEvent::Retitled {
                title: "Q4 launch sync".into(),
            },
            start,
        );
        model.keep_recording();
        assert_eq!(
            shown(&model, true),
            Some(("m".into(), "Q4 launch sync".into(), start))
        );

        // Another panel replacing the prompt brings it back too.
        model.apply(
            &ControllerEvent::CallEnded {
                app: "Zoom".into(),
                application: None,
                stops_at: None,
            },
            start,
        );
        model.apply(
            &ControllerEvent::Offer(OfferId::next(), slack_huddle()),
            start,
        );
        assert!(model.pill(true).is_some());

        // Stop & transcribe leaves it closed until the recording ends.
        model.apply(
            &ControllerEvent::CallEnded {
                app: "Zoom".into(),
                application: None,
                stops_at: None,
            },
            start,
        );
        model.apply(&ControllerEvent::Transcribing, start);
        assert!(model.pill(true).is_none());
        model.keep_recording();
        assert!(model.pill(true).is_none());
    }

    #[test]
    fn a_withdrawn_stop_brings_back_the_recording_capsule_and_closes_its_notice() {
        let mut model = PillModel::default();
        let start = Instant::now();
        model.apply(&started("m"), start);
        model.apply(
            &ControllerEvent::CallEnded {
                app: "Zoom".into(),
                application: None,
                stops_at: Some(start + Duration::from_secs(30)),
            },
            start,
        );
        assert!(model.pill(true).is_none(), "the notice stands in for it");
        model.apply(&ControllerEvent::CallEndedWithdrawn, start);
        assert_eq!(
            shown(&model, true),
            Some(("m".into(), "Design sync".into(), start))
        );

        // The notice itself closes with it.
        let (mut rt, _, _) = runtime(call_ended_countdown(30), false, true, CALL_ENDED);
        rt.update(|capsule, _| assert!(capsule.withdraw_call_ended()));
        assert!(!rt.state.is_shown());

        // The "still recording" notice names no stop, so it stays.
        let (mut rt, _, _) = runtime(call_ended(), false, true, CALL_ENDED);
        rt.update(|capsule, _| assert!(!capsule.withdraw_call_ended()));
        assert!(matches!(
            rt.state.state,
            State::CallEnded { stops: None, .. }
        ));

        // A newer panel of another kind stays.
        let (mut rt, _, _) = runtime(live_recording("kept"), false, true, RECORDING);
        rt.update(|capsule, _| assert!(!capsule.withdraw_call_ended()));
        assert!(matches!(rt.state.state, State::Recording { .. }));
    }

    #[test]
    fn the_recording_capsule_follows_the_indicator_setting_mid_recording() {
        let mut model = PillModel::default();
        let start = Instant::now();
        model.apply(&started("m"), start);
        assert!(model.pill(false).is_none());
        assert_eq!(
            shown(&model, true),
            Some(("m".into(), "Design sync".into(), start))
        );
        model.apply(
            &ControllerEvent::Failed {
                title: "Meeting recording failed".into(),
                error: "the microphone went away".into(),
            },
            start,
        );
        assert!(model.pill(true).is_none());
    }

    #[test]
    fn the_call_ended_lines_fit_beside_its_buttons() {
        let style = rune_theme_opencode::kit(false).ty;
        for kind in [call_ended_countdown(23), call_ended()] {
            assert_eq!(panel_width(&kind), CALL_ENDED.0);
            let (mut rt, _, _) = runtime(kind, false, false, CALL_ENDED);
            landed(&mut rt);
            for (key, font) in [("title", style.ui_medium), ("subtitle", style.small)] {
                let line = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap();
                let words = line.text.clone().unwrap();
                let (width, _) = rt.text.measure(&words, font, 2.0);
                assert!(
                    width <= line.rect[2] + 0.5,
                    "{words:?} needs {width} pt, has {}",
                    line.rect[2]
                );
            }
            let title = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("title"));
            assert_eq!(
                title.and_then(|e| e.text).as_deref(),
                Some("Teams call ended")
            );
        }
    }

    #[test]
    fn call_ended_drain_omits_head_under_reduce_motion_and_omits_ring_when_manual() {
        // Countdown: ring has track, line, and head.
        let (mut rt, _, _) = runtime(call_ended_countdown(23), false, true, CALL_ENDED);
        landed(&mut rt);
        let keys = |rt: &Runtime<Capsule>| -> Vec<String> {
            rt.elements().into_iter().filter_map(|e| e.key).collect()
        };
        let normal = keys(&rt);
        assert!(normal.contains(&"track".to_string()));
        assert!(normal.contains(&"line".to_string()));
        assert!(normal.contains(&"head".to_string()));

        // Reduce Motion: keeps track and line, drops bright head.
        let (mut rt_reduced, _, _) = runtime(call_ended_countdown(23), false, true, CALL_ENDED);
        rt_reduced.set_system_reduced_motion(true);
        rt_reduced.update(|_, tx| tx.instant());
        landed(&mut rt_reduced);
        let reduced = keys(&rt_reduced);
        assert!(reduced.contains(&"track".to_string()));
        assert!(reduced.contains(&"line".to_string()));
        assert!(!reduced.contains(&"head".to_string()));

        // Manual recording (no countdown): icon stays, ring track and drain are omitted.
        let (mut rt, _, _) = runtime(call_ended(), false, true, CALL_ENDED);
        landed(&mut rt);
        let manual = keys(&rt);
        assert!(manual.contains(&"icon".to_string()));
        assert!(!manual.contains(&"track".to_string()));
        assert!(!manual.contains(&"drain".to_string()));
    }

    include!("meeting_capsules/motion_fixture.rs");
    include!("meeting_capsules/offer/proof.rs");
    include!("meeting_capsules/replay_fixture.rs");
    include!("meeting_capsules/exits_tests.rs");
    include!("meeting_capsules/pill_tests.rs");

    /// No `--snap` scene draws an overlay, so this renders the capsules to
    /// PNGs in `HEX_CAPSULE_SNAPS` for review; without it, or a GPU, it skips.
    #[test]
    fn capsule_snapshots() {
        use rune::render::Gpu;
        let Some(folder) = std::env::var_os("HEX_CAPSULE_SNAPS") else {
            return;
        };
        let Ok(gpu) = Gpu::try_new(Gpu::instance(), None) else {
            eprintln!("skipping capsule snapshots: no GPU adapter");
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mut google_meet = slack_huddle();
        google_meet.source = MeetingSource::GoogleMeet;
        google_meet.site = Some("meet.google.com".into());
        google_meet.title = "Design sync".into();
        google_meet.named = true;
        let states = [
            ("offer", offer(slack_huddle()), false, PANEL),
            ("offer-choices", offer(slack_huddle()), true, MENU),
            ("offer-choices-site", offer(google_meet), true, MENU),
            ("reminder", reminder(true, false), false, REMINDER),
            ("reminder-no-link", reminder(false, false), false, REMINDER),
            ("reminder-automatic", reminder(true, true), false, REMINDER),
            ("call-ended", call_ended_countdown(23), false, CALL_ENDED),
            ("call-ended-manual", call_ended(), false, CALL_ENDED),
            (
                "recording-automatic",
                Kind::Recording(Recording {
                    id: "m".into(),
                    title: "Slack Huddle call — 2:30 PM".into(),
                    started: Instant::now() - Duration::from_secs(4),
                    automatic: true,
                    warning: None,
                }),
                false,
                RECORDING,
            ),
            ("auto-started", auto_started(), false, PANEL),
            ("ready", ready_transcript(), false, ready_size()),
            (
                "ready-summary",
                ready_summarized(),
                false,
                ready_size_of(&ready_summarized()),
            ),
        ];
        type Setup = fn(&mut Capsule);
        let nothing: Setup = |_| {};
        let states = states
            .into_iter()
            .map(|(name, kind, more_open, size)| (name, kind, more_open, size, nothing))
            .chain::<[(&str, Kind, bool, (f32, f32), Setup); 8]>([
                (
                    "recording-rest",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    fixture_scope,
                ),
                (
                    "recording-silent",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        if let State::Recording {
                            recording: shown, ..
                        } = &mut capsule.state
                        {
                            shown.warning = Some(crate::meeting::TrackWarning::DigitalSilence);
                        }
                    },
                ),
                (
                    "recording-hover",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        fixture_scope(capsule);
                        capsule.set_hovered(true);
                    },
                ),
                (
                    "recording-hover-bottom-left",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.corner = MeetingIndicatorCorner::BottomLeft;
                        capsule.set_hovered(true);
                    },
                ),
                (
                    "recording-hover-silent",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        if let State::Recording {
                            recording: shown, ..
                        } = &mut capsule.state
                        {
                            shown.warning = Some(crate::meeting::TrackWarning::DigitalSilence);
                        }
                        capsule.set_hovered(true);
                    },
                ),
                (
                    "recording-empty",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| capsule.set_hovered(true),
                ),
                (
                    "recording-copied",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.copied = Some((LiveCopy::Prompt, Instant::now()));
                        capsule.set_hovered(true);
                    },
                ),
                (
                    "ready-finished",
                    ready_summarized(),
                    false,
                    ready_size_of(&ready_summarized()),
                    |capsule: &mut Capsule| capsule.live.entries = call(),
                ),
            ]);
        for (name, kind, more_open, size, setup) in states {
            for dark in [false, true] {
                let (mut rt, _, _) = runtime(kind.clone(), more_open, dark, size);
                let size = if matches!(kind, Kind::Offer(..)) {
                    rt.state.presentation.window()
                } else {
                    size
                };
                rt.update(move |capsule, _| {
                    if let State::CallEnded {
                        stops: Some((at, _)),
                        ..
                    } = &mut capsule.state
                    {
                        *at = Instant::now() + Duration::from_secs(23);
                    }
                    setup(capsule);
                });
                let (w, h, pixels) = render(&gpu, &mut rt, size);
                let theme = if dark { "dark" } else { "light" };
                let path = folder.join(format!("capsule-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }

    /// Review stills of the recording pill, its discard question, and the
    /// ready panel, into `HEX_CAPSULE_SNAPS` as `capsule-*`; a no-op without
    /// that variable or a GPU.
    #[test]
    fn capsule_stills() {
        use rune::render::Gpu;
        let Some(folder) = std::env::var_os("HEX_CAPSULE_SNAPS") else {
            return;
        };
        let Ok(gpu) = Gpu::try_new(Gpu::instance(), None) else {
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        // Name, capsule, size, open, asking to discard, title hovered.
        type Still = (&'static str, Kind, (f32, f32), bool, bool, bool);
        let states: [Still; 5] = [
            (
                "recording-rest",
                live_recording("fixture"),
                RECORDING,
                false,
                false,
                false,
            ),
            (
                "recording-open",
                live_recording("fixture"),
                RECORDING,
                true,
                false,
                false,
            ),
            (
                "recording-discard",
                live_recording("fixture"),
                RECORDING,
                true,
                true,
                false,
            ),
            (
                "ready",
                ready_summarized(),
                ready_size_of(&ready_summarized()),
                false,
                false,
                false,
            ),
            (
                "ready-hover",
                ready_summarized(),
                ready_size_of(&ready_summarized()),
                false,
                false,
                true,
            ),
        ];
        for (name, kind, size, open, discard, title_hover) in states {
            for dark in [false, true] {
                let size = (size.0 + 64.0, size.1.max(RECORDING.1) + 160.0);
                let (mut rt, _, _) = runtime(kind.clone(), false, dark, size);
                rt.update(move |c, _| {
                    if let State::Recording {
                        recording: shown,
                        hovered,
                        confirming,
                        ..
                    } = &mut c.state
                    {
                        shown.started = Instant::now() - Duration::from_secs(258);
                        *hovered = open;
                        *confirming = discard;
                    }
                    fixture_scope(c);
                    c.live.entries = call();
                });
                if title_hover {
                    rt.frame(0.0);
                    if let Some((x, y)) = rt.find("open-meeting") {
                        rt.pointer_move(x, y);
                    }
                    rt.update(|_, _| {});
                }
                let (w, h, pixels) = render(&gpu, &mut rt, size);
                let theme = if dark { "dark" } else { "light" };
                let full = (size.0 - 1.0, size.1 - 1.0);
                let bounds = rt
                    .elements()
                    .into_iter()
                    .filter(|e| !e.exiting && e.opacity > 0.01)
                    .filter(|e| e.fill.a > 0.01 || e.text.is_some())
                    .filter(|e| !(e.rect[2] >= full.0 && e.rect[3] >= full.1))
                    .fold(None::<[f32; 4]>, |acc, e| {
                        let [x, y, w, h] = e.rect;
                        Some(match acc {
                            None => [x, y, x + w, y + h],
                            Some([a, b, c, d]) => [a.min(x), b.min(y), c.max(x + w), d.max(y + h)],
                        })
                    })
                    .expect("no capsule drawn");
                let (w, h, pixels) = framed(w, h, &pixels, bounds, &format!("{name}-{theme}"));
                let path = folder.join(format!("capsule-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }

    /// Crops a still to the capsule's bounds (points) with an
    /// even margin on black, and fails when the capsule leaves the canvas.
    fn framed(w: u32, h: u32, px: &[u8], bounds: [f32; 4], name: &str) -> (u32, u32, Vec<u8>) {
        let (w, h) = (w as isize, h as isize);
        let [x0, y0, x1, y1] = bounds.map(|v| (v * 2.0).round() as isize);
        eprintln!("still {name}: canvas {w}x{h} capsule x {x0}..{x1} y {y0}..{y1}");
        assert!(
            x0 >= 0 && y0 >= 0 && x1 <= w && y1 <= h,
            "{name}: leaves the canvas"
        );
        const MARGIN: isize = 48;
        let (cw, ch) = (x1 - x0 + 2 * MARGIN, y1 - y0 + 2 * MARGIN);
        let mut out = vec![0u8; (cw * ch * 4) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let o = ((y * cw + x) * 4) as usize;
                let (sx, sy) = (x + x0 - MARGIN, y + y0 - MARGIN);
                if sx >= 0 && sy >= 0 && sx < w && sy < h {
                    let i = ((sy * w + sx) * 4) as usize;
                    let a = px[i + 3] as f32 / 255.0;
                    for c in 0..3 {
                        out[o + c] = (px[i + c] as f32 * a).round() as u8;
                    }
                }
                out[o + 3] = 255;
            }
        }
        (cw as u32, ch as u32, out)
    }

    fn render(
        gpu: &rune::render::Gpu,
        rt: &mut Runtime<Capsule>,
        size: (f32, f32),
    ) -> (u32, u32, Vec<u8>) {
        settle_stage(rt);
        rt.settle_all();
        rt.frame(0.0);
        let (w, h) = ((size.0 * 2.0) as u32, (size.1 * 2.0) as u32);
        let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
        let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
        if matches!(rt.state.state, State::Recording { .. }) {
            if rt.state.waveform.is_none() {
                let now = Instant::now();
                let mut wave = waveform::Listening::new(now);
                wave.sample(now, [0.0; 2]);
                rt.state.waveform = Some(Box::new(wave));
            }
            return (
                w,
                h,
                paint_recording(
                    gpu,
                    &mut renderer,
                    &target,
                    rt,
                    &mut waveform::Ink::default(),
                ),
            );
        }
        renderer.prepare(
            &gpu.device,
            &gpu.queue,
            &mut rt.text.atlas,
            &rt.instances,
            (w, h),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.draw(&mut encoder, &target.view, rt.clear_color());
        target.copy(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        (w, h, target.read(&gpu.device))
    }

    /// The call-ended, ready, and automatic-recording capsules against
    /// `tests/golden/meeting-capsule-*.png`; `HEX_UPDATE_GOLDENS=1` rewrites
    /// them. Without a GPU adapter it skips. Like every golden it is checked
    /// on Linux only: glyph coverage on Metal differs past the tolerance.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "goldens are checked on Linux")]
    fn panel_goldens() {
        let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
            eprintln!("skipping meeting capsule golden: no GPU adapter");
            return;
        };
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        let states = [
            ("call-ended", call_ended_countdown(23), CALL_ENDED),
            ("call-ended-manual", call_ended(), CALL_ENDED),
            ("ready", ready_transcript(), ready_size()),
            ("auto-started", auto_started(), PANEL),
            ("recording-rest", live_recording("fixture"), RECORDING),
            ("recording-open", live_recording("fixture"), RECORDING),
        ];
        let mut misses = Vec::new();
        for ((name, kind, size), dark) in states
            .into_iter()
            .flat_map(|state| [false, true].map(|dark| (state.clone(), dark)))
        {
            let theme = if dark { "dark" } else { "light" };
            let file = format!("meeting-capsule-{name}-{theme}");
            let path = folder.join(format!("{file}.png"));
            let (mut rt, _, _) = runtime(kind, false, dark, size);
            if name == "call-ended" {
                rt.update(|c, _| {
                    if let State::CallEnded {
                        stops: Some((at, _)),
                        ..
                    } = &mut c.state
                    {
                        *at = Instant::now() + Duration::from_secs(23);
                    }
                });
            }
            if name.starts_with("recording-") {
                rt.update(move |c, _| {
                    if let State::Recording {
                        recording: shown,
                        hovered,
                        ..
                    } = &mut c.state
                    {
                        shown.started = Instant::now() - Duration::from_secs(258);
                        *hovered = name == "recording-open";
                    }
                    fixture_scope(c);
                    c.live.entries = call();
                });
            }
            let (w, h, pixels) = render(&gpu, &mut rt, size);
            if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
                rune::render::write_png(&path, w, h, &pixels).unwrap();
                continue;
            }
            // Where CI's golden-actual artifact picks it up.
            let actual = std::env::temp_dir().join(format!("rune-settings-{file}-actual.png"));
            if !path.exists() {
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!("{file} is missing; actual at {}", actual.display()));
                continue;
            }
            let decoder =
                png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
            let mut reader = decoder.read_info().unwrap();
            let mut golden = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut golden).unwrap();
            if (info.width, info.height) != (w, h) {
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!(
                    "{name}-{theme}: size changed; actual at {}",
                    actual.display()
                ));
                continue;
            }
            // GPUs differ in the last bit of blending and glyph coverage.
            let differing = pixels
                .chunks(4)
                .zip(golden.chunks(4))
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 12))
                .count();
            if differing > (w * h) as usize / 500 {
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!(
                    "{name}-{theme}: {differing} pixels differ; actual at {}",
                    actual.display()
                ));
            }
        }
        assert!(misses.is_empty(), "{}", misses.join("\n"));
    }
}
