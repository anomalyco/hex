//! The meeting capsules on Rune: the calendar meeting reminder, the
//! recording offer, "left the call", the ready transcript, and the recording
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
//! The recording capsule's "Copy new" and "Copy all" copy the in-call
//! transcript (Parakeet, `meeting::TranscriptReader`); "Copy new" shares
//! Option-Control-V's cursor and first transcribes the call up to the click
//! (`meeting::entries_for_copy`), saying "Transcribing…" meanwhile. "Copy
//! agent prompt" copies instructions that tell a coding agent which `hex
//! meetings` commands read the meeting. The ready panel offers the same three
//! under the finished meeting's title and summary.
//!
//! The recording capsule rests as a small pill in its corner, only its voice
//! bars and elapsed time, and springs open into a panel with Stop and its
//! copies while the pointer is over it. Dragging it moves the window; on
//! release it snaps to the nearest corner, which is saved.
//! It is a function of the controller's current recording and the live
//! Show recording indicator setting ([`PillModel`]): the call-ended prompt
//! stands in for it until Keep recording, and turning the setting on or off
//! mid-recording shows or hides it within a second.
//! With Labs › Movable meeting capsules on, the panel capsules (offer,
//! reminder, call ended, ready, recording automatically) show in that same
//! saved corner and drag the same way; off, they stay in the top right.
//!
//! On a Linux Wayland compositor without layer-shell (GNOME) the capsules
//! float on XWayland only with Labs › Floating meeting capsules on GNOME
//! (`linux::meeting_capsules`); otherwise an offer opens the Meetings pane.

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

use super::controls::{Paint, PushSize, status_dot};
use crate::app_settings::{MeetingIndicatorCorner, MeetingPolicy, MeetingProviderChoice};
use crate::calendar::CalendarMeeting;
use crate::desktop::i18n::{tr, tr_fill};
use crate::meeting::detection::MeetingCandidate;
use crate::meeting::watcher::{ControllerCommand, ControllerEvent};
use crate::meeting::{self, MeetingManifest, TranscriptEntry};

mod panel;

use panel::{Bars, agent_prompt, hex_command, ready, ready_summary, recording, recording_notice};

/// The offer, call ended, and ready capsules.
const MEETING_PANEL_WIDTH: f32 = 420.0;
/// The offer's "…" choices need room for three buttons beside the name.
const MEETING_MENU_WIDTH: f32 = 560.0;
const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// Between a capsule's buttons and its right edge.
const ACTIONS_INSET: f32 = 8.0;
/// The reminder: the meeting's title beside up to three buttons.
const REMINDER_PANEL_WIDTH: f32 = 520.0;
/// "Microsoft Teams left the call" beside Keep recording and Stop &
/// transcribe (#835).
const CALL_ENDED_PANEL_WIDTH: f32 = 520.0;
/// How often an open reminder recounts "Starts in N min".
const REMINDER_TICK: Duration = Duration::from_secs(5);
/// Between a capsule's window edges and what it draws.
const CAPSULE_INSET: f32 = 4.0;
/// The resting pill: the voice bars and the elapsed time.
const REST_HEIGHT: f32 = 38.0;
/// The resting pill's sides, and the open panel header's right side, so the
/// timer stays put as the pill opens.
const REST_PX: f32 = 11.0;
/// The open recording pill and the ready panel.
const PANEL_WIDTH: f32 = 252.0;
const PANEL_RADIUS: f32 = 13.0;
/// Around the open panel's rows.
const PANEL_PAD: f32 = 5.0;
const HEADER_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 36.0;
/// Concentric with the panel across [`PANEL_PAD`].
const ROW_RADIUS: f32 = PANEL_RADIUS - PANEL_PAD;
/// A row's icon and label inset; titles and summaries share it.
const ROW_PX: f32 = 9.0;
/// Under the first row, which is raised.
const FIRST_ROW_GAP: f32 = 2.0;
/// "Microphone is silent" or "Started automatically" under the header.
const NOTICE_HEIGHT: f32 = 20.0;
/// The ready panel's summary: Apple Intelligence's two sentences.
const SUMMARY_LINES: usize = 3;
const SUMMARY_LINE: f32 = 19.5;
/// The resting pill fits a timer up to `100:00`.
#[cfg(target_os = "windows")]
const REST_WINDOW_WIDTH: f32 = 116.0;
/// The recording capsule's window at its largest: the open panel with a
/// notice.
const RECORDING_WINDOW: (f32, f32) = (
    PANEL_WIDTH + 2.0 * CAPSULE_INSET,
    HEADER_HEIGHT
        + NOTICE_HEIGHT
        + 3.0 * ROW_HEIGHT
        + FIRST_ROW_GAP
        + PANEL_PAD
        + 2.0 * CAPSULE_INSET,
);
/// Opening under the pointer, and closing after it leaves (GPUI's snap
/// springs), with no overshoot.
const RECORDING_OPEN: Spring = Spring::new(0.25, 0.0);
const RECORDING_CLOSE: Spring = Spring::new(0.2, 0.0);
/// The voice bars: five 2 pt bars 2 pt apart, from a 3 pt dot to 17 pt.
const BARS: usize = 5;
const BAR_WIDTH: f32 = 2.0;
const BAR_STEP: f32 = 4.0;
const BAR_REST: f32 = 3.0;
const BAR_MAX: f32 = 17.0;
const BARS_BOX: (f32, f32) = (BAR_STEP * (BARS - 1) as f32 + BAR_WIDTH, BAR_MAX);
/// The newest level in the middle, older ones outward and quieter.
const BAR_SHAPE: [f32; BARS] = [0.55, 0.8, 1.0, 0.8, 0.55];
/// How often the bars read the meeting's level.
const BAR_TICK: Duration = Duration::from_millis(50);
const BAR_SPRING: Spring = Spring::new(0.12, 0.0);
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
    Offer(MeetingCandidate),
    /// `auto_stop` when the recording stops on its own after a grace (Labs
    /// › Stop when the call ends); Keep recording then cancels that.
    CallEnded {
        app: String,
        auto_stop: bool,
    },
    Ready(Box<MeetingManifest>),
    /// An automatic recording while the recording indicator is off.
    AutoStarted {
        title: String,
    },
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
            ControllerEvent::Offer(_) | ControllerEvent::Reminder { .. } => {
                self.call_ended = false;
            }
            ControllerEvent::Starting
            | ControllerEvent::Transcribing
            | ControllerEvent::Finished(_)
            | ControllerEvent::Failed { .. } => {
                self.recording = None;
                self.call_ended = false;
            }
            ControllerEvent::ReminderWithdrawn { .. }
            | ControllerEvent::Chapters(_)
            | ControllerEvent::ImportFailed { .. }
            | ControllerEvent::NotesStarted
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
    New,
    All,
    Prompt,
}

/// The shown meeting's transcript as last read: the recording's in-call
/// transcript by the ticker, or the ready meeting's when it shows.
#[derive(Default)]
struct Live {
    entries: Vec<TranscriptEntry>,
    /// What Copy new would copy now.
    pending: meeting::PendingDelta,
    copied: Option<(LiveCopy, Instant)>,
    /// Copy new is transcribing up to the click.
    copying: bool,
}

impl Live {
    fn copied(&self, kind: LiveCopy) -> bool {
        self.copied
            .is_some_and(|(copied, at)| copied == kind && at.elapsed() < COPIED_FEEDBACK)
    }
}

struct Capsule {
    kind: Option<Kind>,
    /// The offer shows its Never offer / Always record choices in place of
    /// its actions.
    more_open: bool,
    live: Live,
    bars: Bars,
    commands: SyncSender<ControllerCommand>,
    hide: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The call-ended prompt's Keep recording brings the recording capsule
    /// back.
    keep_recording: Option<Arc<dyn Fn() + Send + Sync>>,
    resize: Option<Resize>,
    choose: Choose,
    /// The pointer is over the recording pill (or holding it), or over the
    /// ready panel.
    hovered: bool,
    /// The ready panel's time ran out while the pointer was over it; it
    /// closes when the pointer leaves.
    expired: bool,
    /// Where the recording capsule rests, and the panel while
    /// [`Capsule::movable`].
    corner: MeetingIndicatorCorner,
    /// The panel capsule can be dragged to another corner (Labs › Movable
    /// meeting capsules); the recording capsule always can.
    movable: bool,
    /// Moves the capsule's window by a drag's travel from `corner`.
    shift: Option<Shift>,
    save_corner: SaveCorner,
    /// Reads a recording's transcript through now off the main thread, then
    /// calls [`Capsule::finish_copy_new`]. Without it, Copy new copies what
    /// the ticker last read.
    copy_new: Option<CopyNew>,
}

type Choose = Arc<dyn Fn(MeetingProviderChoice) + Send + Sync>;
type CopyNew = Arc<dyn Fn(String) + Send + Sync>;
/// Sets the capsule window's size in points.
type Resize = Arc<dyn Fn((f32, f32)) + Send + Sync>;
type Shift = Arc<dyn Fn(MeetingIndicatorCorner, f32, f32) + Send + Sync>;
type SaveCorner = Arc<dyn Fn(MeetingIndicatorCorner) + Send + Sync>;

impl Capsule {
    fn send(&self, command: ControllerCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    fn set_more_open(&mut self, open: bool) {
        self.more_open = open;
        if let Some(resize) = &self.resize {
            resize((
                if open {
                    MEETING_MENU_WIDTH
                } else {
                    MEETING_PANEL_WIDTH
                },
                MEETING_PANEL_HEIGHT,
            ));
        }
    }

    /// The meeting whose transcript the capsule copies: the recording, or
    /// the ready meeting.
    fn meeting_id(&self) -> Option<&str> {
        match &self.kind {
            Some(Kind::Recording(Recording { id, .. })) => Some(id),
            Some(Kind::Ready(manifest)) => Some(&manifest.id),
            _ => None,
        }
    }

    /// Windows sizes the recording capsule's window to the pill it draws.
    /// Elsewhere the window stays at its largest and only the `.hit()` pill
    /// takes the pointer.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn fit_recording_window(&self) {
        if let (Some(resize), Some(Kind::Recording(shown))) = (&self.resize, &self.kind) {
            resize(recording_window_size(
                self.hovered,
                recording_notice(shown).is_some(),
            ));
        }
    }

    fn set_hovered(&mut self, inside: bool) {
        self.hovered = inside;
        #[cfg(target_os = "windows")]
        self.fit_recording_window();
        if !inside && self.expired {
            self.close();
        }
    }

    /// Saves the offered provider's choice; returns whether there was one.
    fn choose(&self, policy: MeetingPolicy) -> bool {
        let Some(Kind::Offer(candidate)) = &self.kind else {
            return false;
        };
        let Some(provider) = candidate.provider(crate::labs::more_call_apps()) else {
            return false;
        };
        tracing::info!(key = %provider.key, ?policy, "meeting provider choice made from an offer");
        (self.choose)(provider.choose(policy));
        true
    }

    fn close(&mut self) {
        self.kind = None;
        if let Some(hide) = &self.hide {
            hide();
        }
    }

    fn press_copy_new(&mut self, tx: &mut Tx) {
        let Some(id) = self.meeting_id().map(str::to_owned) else {
            return;
        };
        if self.live.copying {
            return;
        }
        match self.copy_new.clone() {
            Some(copy_new) => {
                self.live.copying = true;
                copy_new(id);
            }
            None => {
                let entries = self.live.entries.clone();
                self.copy_new_from(&id, &entries, tx);
            }
        }
    }

    /// Copies the new turns of `entries`, read through the click for the
    /// meeting `id`, if it still shows.
    fn finish_copy_new(
        &mut self,
        id: &str,
        entries: color_eyre::Result<Vec<TranscriptEntry>>,
        tx: &mut Tx,
    ) {
        // A reply for an earlier meeting leaves the shown one's copy alone.
        if self.meeting_id() != Some(id) {
            return;
        }
        self.live.copying = false;
        match entries {
            Ok(entries) => self.copy_new_from(id, &entries, tx),
            Err(error) => tracing::warn!(%error, "could not read new meeting transcript turns"),
        }
    }

    fn copy_new_from(&mut self, id: &str, entries: &[TranscriptEntry], tx: &mut Tx) {
        // Rune's clipboard write has no acknowledgment, so the cursor commits
        // once it is queued (as GPUI's Copy new did).
        match meeting::try_consume_delta_from(id, entries, |text| {
            tx.copy(text.to_owned());
            Ok(())
        }) {
            Ok(_) => {
                if entries.len() > self.live.entries.len() {
                    self.live.entries = entries.to_vec();
                }
                self.live.pending = meeting::PendingDelta::default();
                self.live.copied = Some((LiveCopy::New, Instant::now()));
            }
            Err(error) => tracing::warn!(%error, "could not copy new meeting transcript turns"),
        }
    }

    fn copy_all(&mut self, tx: &mut Tx) {
        let markdown = match &self.kind {
            Some(Kind::Recording(_)) if self.live.entries.is_empty() => return,
            Some(Kind::Recording(_)) => meeting::transcript_markdown(&self.live.entries),
            Some(Kind::Ready(manifest)) => {
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
        let prompt = match &self.kind {
            Some(Kind::Recording(Recording { id, title, .. })) => {
                let reached = self.live.entries.last().map_or(0, |entry| entry.end_ms);
                agent_prompt(&hex_command(), id, title, Some(reached))
            }
            Some(Kind::Ready(manifest)) => agent_prompt(
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
        match &self.kind {
            Some(Kind::Recording(shown)) => {
                recording_window_size(self.hovered, recording_notice(shown).is_some())
            }
            Some(Kind::Ready(manifest)) => ready_window_size(manifest),
            Some(Kind::Offer(_)) if self.more_open => (MEETING_MENU_WIDTH, MEETING_PANEL_HEIGHT),
            Some(kind) => (panel_width(kind), MEETING_PANEL_HEIGHT),
            None => (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
        }
    }

    /// The recording pill (and a movable panel) follows the pointer and
    /// snaps to the nearest corner on release. A cancelled drag (Escape,
    /// window blur) returns to the corner it started from and saves nothing.
    fn drag(&mut self, drag: &Drag, tx: &mut Tx) {
        let Some(shift) = self.shift.clone() else {
            return;
        };
        match drag.phase {
            Phase::Down => {}
            Phase::Move => shift(self.corner, drag.dx, drag.dy),
            Phase::Up if drag.cancelled() => shift(self.corner, 0.0, 0.0),
            Phase::Up => {
                let size = self.window_size();
                let corner = primary_screen_size().map_or(self.corner, |screen| {
                    nearest_corner(self.corner, drag.dx, drag.dy, screen, size)
                });
                shift(corner, 0.0, 0.0);
                if corner != self.corner {
                    tracing::info!(?corner, "meeting capsule moved");
                    self.corner = corner;
                    (self.save_corner)(corner);
                    // The pill changes sides with the window; it must not
                    // slide across it.
                    tx.instant();
                }
            }
        }
    }
}

/// The recording capsule's window. On Windows it is the size of what it
/// draws: the resting pill (as wide as the open panel while it says
/// `noticed`), then the open panel. Elsewhere it stays at its largest, and
/// only the `.hit()` pill takes the pointer.
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
fn recording_window_size(open: bool, noticed: bool) -> (f32, f32) {
    #[cfg(target_os = "windows")]
    if !open {
        let width = if noticed {
            RECORDING_WINDOW.0
        } else {
            REST_WINDOW_WIDTH
        };
        return (width, REST_HEIGHT + 2.0 * CAPSULE_INSET);
    }
    RECORDING_WINDOW
}

/// The ready panel's window: its header, a summary while there is one or one
/// is being written, and the three copies.
fn ready_window_size(manifest: &MeetingManifest) -> (f32, f32) {
    let summary = if ready_summary(manifest).is_some() {
        SUMMARY_LINE * SUMMARY_LINES as f32 + ROW_PX + FIRST_ROW_GAP
    } else {
        0.0
    };
    (
        PANEL_WIDTH + 2.0 * CAPSULE_INSET,
        HEADER_HEIGHT
            + summary
            + 3.0 * ROW_HEIGHT
            + FIRST_ROW_GAP
            + PANEL_PAD
            + 2.0 * CAPSULE_INSET,
    )
}

/// Where a corner's capsule window sits: its edge and margin, and its
/// alignment and inset along that edge, after a drag of (dx, dy) points.
fn corner_placement(
    corner: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
) -> (ScreenEdge, f32, Align, f32) {
    let (edge, margin) = if corner.is_top() {
        (ScreenEdge::Top, WINDOW_MARGIN + MENU_BAR + dy)
    } else {
        (ScreenEdge::Bottom, WINDOW_MARGIN + DOCK - dy)
    };
    let (align, inset) = if corner.is_right() {
        (Align::End, WINDOW_MARGIN - dx)
    } else {
        (Align::Start, WINDOW_MARGIN + dx)
    };
    (edge, margin, align, inset)
}

/// The corner nearest the centre of a capsule window of `size` points after
/// a drag of (dx, dy) points from `from`, on a screen of `screen` points.
fn nearest_corner(
    from: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
    screen: (f32, f32),
    size: (f32, f32),
) -> MeetingIndicatorCorner {
    let (w, h) = size;
    let (_, margin, _, inset) = corner_placement(from, dx, dy);
    let x = if from.is_right() {
        screen.0 - inset - w / 2.0
    } else {
        inset + w / 2.0
    };
    let y = if from.is_top() {
        margin + h / 2.0
    } else {
        screen.1 - margin - h / 2.0
    };
    match (y < screen.1 / 2.0, x >= screen.0 / 2.0) {
        (true, true) => MeetingIndicatorCorner::TopRight,
        (true, false) => MeetingIndicatorCorner::TopLeft,
        (false, true) => MeetingIndicatorCorner::BottomRight,
        (false, false) => MeetingIndicatorCorner::BottomLeft,
    }
}

/// The primary screen in points, where Rune places the capsules.
#[cfg(target_os = "macos")]
fn primary_screen_size() -> Option<(f32, f32)> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::screens(mtm).iter().next()?;
    let size = screen.frame().size;
    Some((size.width as f32, size.height as f32))
}

/// Linux and Windows leave a dragged capsule in its corner.
#[cfg(not(target_os = "macos"))]
fn primary_screen_size() -> Option<(f32, f32)> {
    None
}

fn place_capsule(
    handle: &OverlayHandle<Capsule>,
    corner: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
) {
    let (edge, margin, align, inset) = corner_placement(corner, dx, dy);
    handle.place(edge, margin);
    handle.set_align(align, inset);
}

/// Linux: lets the capsule's next show float on XWayland where Wayland has
/// no layer-shell, only by Labs › Floating meeting capsules on GNOME (#1274).
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
    /// The pill's voice bars, the same ones its capsule draws.
    bars: Bars,
}

impl Pill {
    fn apply(&self, event: &ControllerEvent) {
        self.lock().apply(event, Instant::now());
        self.reconcile();
        if let ControllerEvent::RecordingStarted { id, .. } = event {
            self.tick(id.clone());
        }
    }

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
        let mut model = self.lock();
        let Some(recording) = model
            .pill(crate::app_settings::meeting_indicator_enabled())
            .cloned()
        else {
            if model.shown.take().is_some() {
                self.handle.update(|capsule, _| capsule.kind = None);
                self.handle.hide();
            }
            return None;
        };
        let id = recording.id.clone();
        if model.shown.as_ref() == Some(&id) {
            self.handle.update(move |capsule, _| {
                capsule.kind = Some(Kind::Recording(recording));
                // A notice coming or going changes the resting pill's width.
                #[cfg(target_os = "windows")]
                capsule.fit_recording_window();
            });
            return Some(id);
        }
        model.shown = Some(id.clone());
        let corner = crate::app_settings::meeting_indicator_corner();
        place_capsule(&self.handle, corner, 0.0, 0.0);
        // Windows: a shown pill starts at rest.
        #[cfg(target_os = "windows")]
        self.handle.resize(recording_window_size(
            false,
            recording_notice(&recording).is_some(),
        ));
        self.bars.show([0.0; 3]);
        self.handle.update(move |capsule, tx| {
            capsule.kind = Some(Kind::Recording(recording));
            capsule.live = Live::default();
            capsule.hovered = false;
            capsule.corner = corner;
            tx.instant();
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
    /// transcript here rather than on the main thread; and every
    /// [`BAR_TICK`], moves the voice bars to the meeting's level.
    fn tick(&self, id: String) {
        let bars = self.clone();
        let bars_id = id.clone();
        std::thread::spawn(move || {
            let mut levels = [0.0; 3];
            while bars.records(&bars_id) {
                std::thread::sleep(BAR_TICK);
                if bars.lock().shown.as_deref() != Some(bars_id.as_str()) {
                    continue;
                }
                levels.rotate_right(1);
                levels[0] = meeting::voice_level();
                bars.bars.show(levels);
            }
        });
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
                let pending = meeting::pending_delta(&id, &entries);
                let id = id.clone();
                pill.handle.update(move |capsule, _| {
                    if matches!(&capsule.kind, Some(Kind::Recording(Recording { id: shown, .. })) if *shown == id)
                    {
                        capsule.live.entries = entries;
                        capsule.live.pending = pending;
                    }
                });
            }
        });
    }
}

impl MeetingCapsules {
    /// `choose` saves a "Never offer" or "Always record" choice (the window
    /// keeps the settings it saves).
    pub(crate) fn install<S: 'static>(
        app: &mut rune::App<S>,
        commands: SyncSender<ControllerCommand>,
        choose: impl Fn(MeetingProviderChoice) + Send + Sync + 'static,
        save_corner: impl Fn(MeetingIndicatorCorner) + Send + Sync + 'static,
    ) -> Self {
        let choose: Choose = Arc::new(choose);
        let save_corner: SaveCorner = Arc::new(save_corner);
        let bars = Bars::default();
        let overlay = |app: &mut rune::App<S>, name, size: (f32, f32)| {
            let handle = app.overlay(
                OverlaySpec {
                    name,
                    size,
                    edge: ScreenEdge::Top,
                    margin: WINDOW_MARGIN + MENU_BAR,
                    align: Align::End,
                    inset: WINDOW_MARGIN,
                    interactive: true,
                    view,
                },
                Capsule {
                    kind: None,
                    more_open: false,
                    live: Live::default(),
                    bars: bars.clone(),
                    commands: commands.clone(),
                    hide: None,
                    keep_recording: None,
                    resize: None,
                    choose: choose.clone(),
                    hovered: false,
                    expired: false,
                    corner: MeetingIndicatorCorner::default(),
                    movable: false,
                    shift: None,
                    save_corner: save_corner.clone(),
                    copy_new: None,
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
            let copied = handle.clone();
            let copy_new: CopyNew = Arc::new(move |id: String| {
                let copied = copied.clone();
                // The wait for the transcript through the click stays off
                // the main thread.
                std::thread::spawn(move || {
                    let entries = meeting::entries_for_copy(&id);
                    copied.update(move |capsule, tx| capsule.finish_copy_new(&id, entries, tx));
                });
            });
            handle.update(move |capsule, _| {
                capsule.copy_new = Some(copy_new);
                capsule.hide = Some(Arc::new(move || hide.hide()));
                capsule.resize = Some(Arc::new(move |size| resize.resize(size)));
                capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                    place_capsule(&shift, corner, dx, dy);
                }));
            });
            handle
        };
        let panel = overlay(
            app,
            "Hex meeting",
            (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
        );
        let pill = Pill {
            handle: overlay(
                app,
                "Hex meeting recording",
                recording_window_size(false, false),
            ),
            model: Arc::default(),
            bars,
        };
        let kept = pill.clone();
        panel.update(move |capsule, _| {
            capsule.keep_recording = Some(Arc::new(move || kept.keep_recording()));
        });
        Self {
            panel,
            pill,
            generation: Arc::default(),
        }
    }

    /// Shows, updates, or closes a capsule for each controller event.
    pub(crate) fn event(&mut self, event: &ControllerEvent) {
        self.pill.apply(event);
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
                let panel = self.panel.clone();
                self.panel.update(move |capsule, _| {
                    if matches!(&capsule.kind, Some(Kind::Reminder { meeting, .. }) if meeting.occurrence() == occurrence)
                    {
                        generation.fetch_add(1, Ordering::SeqCst);
                        capsule.kind = None;
                        panel.hide();
                    }
                });
            }
            ControllerEvent::Offer(candidate) => {
                tracing::info!(
                    source = candidate.source.label(),
                    title = %candidate.title,
                    application = ?candidate.application,
                    "meeting recording offer shown"
                );
                self.show_panel(Kind::Offer(candidate.clone()), Some(OFFER_TIMEOUT));
            }
            ControllerEvent::Starting | ControllerEvent::Transcribing => self.close_panel(),
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
            ControllerEvent::CallEnded { app, auto_stop } => {
                self.show_panel(
                    Kind::CallEnded {
                        app: app.clone(),
                        auto_stop: *auto_stop,
                    },
                    None,
                );
            }
            ControllerEvent::Finished(manifest) => {
                self.show_panel(Kind::Ready(manifest.clone()), Some(READY_TIMEOUT));
                // Copy new's hint reads the final transcript, off the main
                // thread.
                let (panel, id) = (self.panel.clone(), manifest.id.clone());
                std::thread::spawn(move || {
                    let entries = match meeting::TranscriptReader::default().read(&id) {
                        Ok(entries) => entries,
                        Err(error) => {
                            tracing::debug!(%error, "could not read the finished transcript");
                            return;
                        }
                    };
                    let pending = meeting::pending_delta(&id, &entries);
                    panel.update(move |capsule, _| {
                        if capsule.meeting_id() == Some(id.as_str()) {
                            capsule.live.entries = entries;
                            capsule.live.pending = pending;
                        }
                    });
                });
            }
            ControllerEvent::NotesFinished(manifest) => {
                let manifest = manifest.clone();
                let panel = self.panel.clone();
                self.panel.update(move |capsule, _| {
                    if let Some(Kind::Ready(ready)) = &mut capsule.kind
                        && ready.id == manifest.id
                    {
                        *ready = manifest;
                        // The summary may have arrived or gone.
                        panel.resize(ready_window_size(ready));
                    }
                });
            }
            ControllerEvent::Retitled { .. }
            | ControllerEvent::SourceWarning { .. }
            | ControllerEvent::Failed { .. }
            | ControllerEvent::ImportFailed { .. }
            | ControllerEvent::Chapters(_)
            | ControllerEvent::NotesStarted => {}
        }
    }

    fn show_panel(&self, kind: Kind, timeout: Option<Duration>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let reminder = matches!(kind, Kind::Reminder { .. });
        // With Labs › Movable meeting capsules the panel shows in the saved
        // capsule corner; otherwise in the top right, as it always has.
        let movable = crate::labs::movable_meeting_capsules();
        let corner = if movable {
            crate::app_settings::meeting_indicator_corner()
        } else {
            MeetingIndicatorCorner::TopRight
        };
        place_capsule(&self.panel, corner, 0.0, 0.0);
        self.panel.resize(match &kind {
            Kind::Ready(manifest) => ready_window_size(manifest),
            kind => (panel_width(kind), MEETING_PANEL_HEIGHT),
        });
        self.panel.update(move |capsule, tx| {
            capsule.kind = Some(kind);
            capsule.more_open = false;
            capsule.live = Live::default();
            capsule.hovered = false;
            capsule.expired = false;
            capsule.movable = movable;
            capsule.corner = corner;
            tx.instant();
        });
        float_on_xwayland(&self.panel);
        self.panel.show();
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
            let (panel, current) = (self.panel.clone(), self.generation.clone());
            std::thread::spawn(move || {
                std::thread::sleep(timeout);
                if current.load(Ordering::SeqCst) == generation {
                    panel.update(move |capsule, _| {
                        // Checked again when applied: a newer capsule may
                        // have been shown since.
                        if current.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        if let Some(Kind::Offer(candidate)) = &capsule.kind {
                            tracing::info!(
                                source = candidate.source.label(),
                                title = %candidate.title,
                                "meeting recording offer timed out"
                            );
                            // Unanswered: offered again when the call app
                            // retakes the microphone (#1267).
                            if candidate.key != "preview" {
                                capsule.send(ControllerCommand::OfferTimedOut);
                            }
                        }
                        // A ready panel waits for the pointer to leave it.
                        if capsule.hovered && matches!(capsule.kind, Some(Kind::Ready(_))) {
                            capsule.expired = true;
                            return;
                        }
                        capsule.close();
                    });
                }
            });
        }
    }

    /// Linux: calls `unshown` with Rune's reason when the offer just shown
    /// could not open its overlay (GNOME on Wayland has no layer-shell for
    /// interactive overlays, #1271), unless a newer capsule replaced it.
    #[cfg(target_os = "linux")]
    pub(crate) fn when_offer_unshown(&self, unshown: impl FnOnce(String) + Send + 'static) {
        let (panel, current) = (self.panel.clone(), self.generation.clone());
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

    // Hide through the handle, not `Capsule::close`: a hide requested inside
    // an update runs on the event loop after a show queued in the same batch,
    // so closing then showing a capsule would leave it hidden.
    fn close_panel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.panel.update(|capsule, _| capsule.kind = None);
        self.panel.hide();
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
        .on_press(press)
        .build_push(p.k)
        .key(key)
}

static CLOSE: &rune::Icon = &rune_theme_opencode::icons::CLOSE;

/// The ✕ that closes a capsule whose actions have no way to. It only hides
/// the capsule: nothing is sent to the controller (#996).
fn dismiss_button(p: Paint) -> El {
    IconButton::new(CLOSE)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(tr("Close"))
        .on_press(|c: &mut Capsule| c.close())
        .build(p.k)
        .key("dismiss")
}

fn view(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    view_body(capsule, cx).hover_motion_within(Anim::INSTANT, Anim::INSTANT)
}

fn view_body(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let Some(kind) = &capsule.kind else {
        return row().key("empty");
    };
    let (dot, title, subtitle, actions) = match kind {
        Kind::Recording(shown) => return recording(p, capsule, shown),
        Kind::AutoStarted { title } => (
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
        Kind::Reminder { meeting, automatic } => {
            reminder(p, meeting, *automatic, crate::calendar::wall_clock_ms())
        }
        Kind::Offer(candidate) if capsule.more_open => offer_choices(p, candidate),
        Kind::Offer(candidate) => {
            let mut actions = Vec::new();
            if candidate.provider(crate::labs::more_call_apps()).is_some() {
                actions.push(
                    button(p, "offer-more", "…", false, |c| {
                        c.set_more_open(true);
                    })
                    .label(tr("More")),
                );
            }
            actions.push(button(p, "not-now", tr("Not now"), false, |c| {
                if let Some(Kind::Offer(candidate)) = &c.kind {
                    tracing::info!(
                        source = candidate.source.label(),
                        title = %candidate.title,
                        "meeting recording offer dismissed"
                    );
                }
                c.send(ControllerCommand::Dismiss);
                c.close();
            }));
            actions.push(button(p, "record", tr("Record"), true, |c| {
                let Some(Kind::Offer(candidate)) = &c.kind else {
                    return;
                };
                let command = if candidate.key == "preview" {
                    ControllerCommand::Dismiss
                } else {
                    ControllerCommand::Record(candidate.clone())
                };
                if c.send(command) {
                    c.close();
                }
            }));
            (p.muted(), candidate.offer_title(), String::new(), actions)
        }
        Kind::CallEnded { app, auto_stop } => (
            p.danger(),
            tr_fill("{} left the call", app),
            if *auto_stop {
                tr("Hex stops recording in 30 s unless you keep recording.")
            } else {
                tr("Hex is still recording.")
            }
            .to_owned(),
            vec![
                button(p, "keep-recording", tr("Keep recording"), false, |c| {
                    if matches!(
                        c.kind,
                        Some(Kind::CallEnded {
                            auto_stop: true,
                            ..
                        })
                    ) {
                        c.send(ControllerCommand::KeepRecording);
                    }
                    if let Some(keep_recording) = &c.keep_recording {
                        keep_recording();
                    }
                    c.close();
                }),
                button(p, "stop-meeting", tr("Stop & transcribe"), true, |c| {
                    if c.send(ControllerCommand::Stop) {
                        c.close();
                    }
                }),
            ],
        ),
        Kind::Ready(manifest) => return ready(p, capsule, manifest),
    };
    pill(p, dot, &title, &subtitle, actions, capsule.movable)
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
            let Some(Kind::Reminder { meeting, .. }) = &c.kind else {
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

/// The offer's "…": never offer this provider again, or always record it
/// (which records this call too).
fn offer_choices(p: Paint, candidate: &MeetingCandidate) -> (rune::Color, String, String, Vec<El>) {
    let more_call_apps = crate::labs::more_call_apps();
    let label = candidate
        .provider(more_call_apps)
        .map_or_else(String::new, |provider| provider.label);
    let mut actions = vec![
        button(p, "offer-cancel", tr("Cancel"), false, |c| {
            c.set_more_open(false);
        }),
        button(p, "never-offer", tr("Never offer"), false, |c| {
            c.choose(MeetingPolicy::NeverOffer);
            c.send(ControllerCommand::Dismiss);
            c.close();
        }),
    ];
    if candidate.can_always_record(more_call_apps) {
        actions.push(button(p, "always-record", tr("Always record"), true, |c| {
            let Some(Kind::Offer(candidate)) = &c.kind else {
                return;
            };
            let command = if candidate.key == "preview" {
                ControllerCommand::Dismiss
            } else {
                ControllerCommand::Record(candidate.clone())
            };
            c.choose(MeetingPolicy::AlwaysRecord);
            if c.send(command) {
                c.close();
            }
        }));
    }
    (
        p.muted(),
        label,
        tr("Change anytime in Settings").to_owned(),
        actions,
    )
}

/// The panel capsule (reminder, offer, call ended): status dot, two
/// truncated lines, and its actions, in a rounded rectangle. A `movable`
/// one drags to another corner like the recording capsule.
fn pill(
    p: Paint,
    dot: rune::Color,
    title: &str,
    subtitle: &str,
    actions: Vec<El>,
    movable: bool,
) -> El {
    // Only the body takes the pointer, not the window's margin around it.
    let body = pill_body(p, status_dot(dot), title, subtitle)
        .w_full()
        .child(row().key("actions").gap(8.0).children(actions))
        .hit();
    let body = if movable {
        body.cursor(Cursor::Grab)
            .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx))
    } else {
        body
    };
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .child(body)
}

/// The panel capsule's corners, concentric with its buttons' (`radius.md`)
/// across the [`ACTIONS_INSET`] between them and the edge.
fn panel_radius(p: Paint) -> f32 {
    p.k.radius.md + ACTIONS_INSET
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
        .pl(16.0)
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
        let capsule = Capsule {
            kind: Some(kind),
            more_open,
            live: Live::default(),
            bars: Bars::default(),
            commands,
            hide: None,
            keep_recording: None,
            resize: None,
            choose: Arc::new(move |choice| saved.lock().unwrap().push(choice)),
            hovered: false,
            expired: false,
            corner: MeetingIndicatorCorner::TopRight,
            movable: false,
            shift: None,
            save_corner: Arc::new(|_| {}),
            copy_new: None,
        };
        let env = Env {
            system_dark: dark,
            scale: 2.0,
        };
        let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let mut rt = crate::rune_ui::offscreen::runtime(capsule, view, size, env, text);
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
        let (rt, _, _) = runtime(Kind::Offer(slack_huddle()), false, true, PANEL);
        assert_eq!(texts(&rt)[0], "Slack huddle detected");
        assert!(rt.find("subtitle").is_none());
        assert!(rt.find("offer-more").is_some());
    }

    #[test]
    fn always_record_saves_the_choice_and_records_this_call() {
        let (mut rt, _, _) = runtime(Kind::Offer(slack_huddle()), false, true, PANEL);
        tap(&mut rt, "offer-more");
        assert!(rt.state.more_open);
        // The overlay widens for the choices; render them at that width.
        let (mut rt, commands, chosen) = runtime(Kind::Offer(slack_huddle()), true, true, MENU);
        assert_eq!(texts(&rt)[0], "Slack huddles");
        tap(&mut rt, "always-record");
        let chosen = chosen.lock().unwrap();
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].key, "com.tinyspeck.slackmacgap");
        assert_eq!(chosen[0].policy, MeetingPolicy::AlwaysRecord);
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::Record(candidate)) if candidate.key == "meeting-slack"
        ));
        assert!(rt.state.kind.is_none());
    }

    #[test]
    fn never_offer_saves_the_choice_and_dismisses() {
        let (mut rt, commands, chosen) = runtime(Kind::Offer(slack_huddle()), true, true, MENU);
        tap(&mut rt, "never-offer");
        assert_eq!(chosen.lock().unwrap()[0].policy, MeetingPolicy::NeverOffer);
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::Dismiss)
        ));
    }

    #[test]
    fn a_browser_on_an_unknown_page_cannot_be_set_to_always_record() {
        let (rt, _, _) = runtime(Kind::Offer(bare_browser()), true, true, MENU);
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
        assert!(rt.state.kind.is_none());
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
        assert!(rt.state.kind.is_none());
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

    /// The meeting paste cursor follows one meeting at a time, so the tests
    /// that copy take turns with it.
    static PASTE_CURSOR: Mutex<()> = Mutex::new(());

    #[test]
    fn the_recording_capsule_copies_the_in_call_transcript() {
        let _cursor = PASTE_CURSOR
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let id = "capsule-copy-test";
        let (mut rt, _, _) = runtime(live_recording(id), false, true, RECORDING);
        open_recording(&mut rt);
        // Nothing transcribed yet: both copies are unavailable.
        assert!(texts(&rt).contains(&"Copy new".to_string()));
        assert!(texts(&rt).contains(&"No speech yet".to_string()));
        tap(&mut rt, "copy-new");
        tap(&mut rt, "copy-all");
        assert!(rt.copy_request.is_none());

        rt.update(|capsule, _| {
            capsule.live.entries = call();
            capsule.live.pending = meeting::pending_delta(id, &call());
        });
        rt.frame(1.0 / 60.0);
        assert!(texts(&rt).contains(&"8 sec new".to_string()));
        tap(&mut rt, "copy-new");
        assert_eq!(
            rt.copy_request.take().as_deref(),
            Some(
                "Them: Kickoff is at two thirty.\n\nYou: Two thirty works.\n\nThem: Then let's ship it."
            )
        );
        assert!(texts(&rt).contains(&"Copied".to_string()));
        // What was copied is not new again, here or for Option-Control-V.
        assert_eq!(meeting::pending_delta(id, &call()).turns, 0);
        rt.update(|capsule, _| capsule.live.copied = None);
        rt.frame(1.0 / 60.0);
        assert!(texts(&rt).contains(&"Up to date".to_string()));

        tap(&mut rt, "copy-all");
        let all = rt.copy_request.take().unwrap();
        assert!(
            all.starts_with("**Them** · 00:00\nKickoff is at two thirty."),
            "{}",
            &*all
        );
    }

    #[test]
    fn copy_new_copies_the_transcript_read_through_the_click() {
        let _cursor = PASTE_CURSOR
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let id = "capsule-copy-through-click";
        let (mut rt, _, _) = runtime(live_recording(id), false, true, RECORDING);
        let asked: Arc<Mutex<Vec<String>>> = Arc::default();
        let record = asked.clone();
        open_recording(&mut rt);
        rt.update(move |capsule, _| {
            capsule.copy_new = Some(Arc::new(move |id| record.lock().unwrap().push(id)));
        });
        rt.frame(1.0 / 60.0);
        // The ticker has shown nothing yet, but the call may hold speech.
        tap(&mut rt, "copy-new");
        assert_eq!(*asked.lock().unwrap(), [id]);
        assert!(texts(&rt).contains(&"Transcribing…".to_string()));
        assert!(rt.copy_request.is_none());
        tap(&mut rt, "copy-new");
        assert_eq!(asked.lock().unwrap().len(), 1, "one copy at a time");

        rt.update(move |capsule, tx| capsule.finish_copy_new(id, Ok(call()), tx));
        rt.frame(1.0 / 60.0);
        assert_eq!(
            rt.copy_request.take().as_deref(),
            Some(
                "Them: Kickoff is at two thirty.\n\nYou: Two thirty works.\n\nThem: Then let's ship it."
            )
        );
        assert!(texts(&rt).contains(&"Copied".to_string()));
        assert_eq!(meeting::pending_delta(id, &call()).turns, 0);

        // A copy still running when the next recording shows: its late reply
        // neither copies nor ends the new recording's copy.
        let next = "capsule-copy-through-click-next";
        tap(&mut rt, "copy-new");
        rt.update(move |capsule, _| {
            capsule.kind = Some(live_recording(next));
            capsule.live = Live::default();
        });
        rt.frame(1.0 / 60.0);
        tap(&mut rt, "copy-new");
        assert_eq!(*asked.lock().unwrap(), [id, id, next]);
        rt.update(move |capsule, tx| capsule.finish_copy_new(id, Ok(call()), tx));
        rt.frame(1.0 / 60.0);
        assert!(rt.copy_request.is_none());
        assert!(rt.state.live.copying);
        assert!(texts(&rt).contains(&"Transcribing…".to_string()));
        rt.update(move |capsule, tx| capsule.finish_copy_new(next, Ok(call()), tx));
        rt.frame(1.0 / 60.0);
        assert!(rt.copy_request.take().is_some());
        assert!(!rt.state.live.copying);
    }

    fn open_recording(rt: &mut Runtime<Capsule>) {
        let (x, y) = rt.find("pill").unwrap();
        rt.pointer_move(x, y);
        rt.frame(0.0);
        rt.settle_all();
        rt.frame(0.0);
    }

    #[test]
    fn the_recording_capsule_rests_compact_and_opens_under_the_pointer() {
        let (mut rt, _, _) = runtime(live_recording("capsule-hover"), false, true, RECORDING);
        rt.update(|capsule, _| {
            capsule.live.entries = call();
            capsule.live.pending = meeting::PendingDelta {
                turns: 2,
                span_ms: 4 * 60_000 + 59_000,
            };
        });
        rt.settle_all();
        rt.frame(0.0);
        // At rest, only the bars and the time.
        assert!(rt.find("copy-new").is_none());
        assert!(rt.find("stop").is_none());
        assert_eq!(texts(&rt), ["04:18"]);
        let (x, _) = rt.find("pill").unwrap();
        assert!(
            x > RECORDING.0 / 2.0,
            "a right corner's pill rests on the right"
        );

        open_recording(&mut rt);
        assert!(rt.state.hovered);
        assert!(rt.find("copy-new").is_some());
        assert!(texts(&rt).contains(&"4 min new".to_string()));
        // Moving onto a button inside the pill keeps it open.
        let (x, y) = rt.find("stop").unwrap();
        rt.pointer_move(x, y);
        rt.frame(0.0);
        assert!(rt.state.hovered);

        rt.pointer_leave();
        rt.frame(0.0);
        assert!(!rt.state.hovered);
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
            capsule.save_corner = Arc::new(move |corner| save.lock().unwrap().push(corner));
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
            capsule.save_corner = Arc::new(move |corner| save.lock().unwrap().push(corner));
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
        // Off, the panel keeps the top-right spot its overlay opens at.
        let (edge, margin, align, inset) = corner_placement(TopRight, 0.0, 0.0);
        assert!(matches!((edge, align), (ScreenEdge::Top, Align::End)));
        assert_eq!((margin, inset), (WINDOW_MARGIN + MENU_BAR, WINDOW_MARGIN));
    }

    #[test]
    fn only_a_movable_panel_drags() {
        type Moves = Arc<Mutex<Vec<(MeetingIndicatorCorner, f32, f32)>>>;
        for movable in [false, true] {
            let (mut rt, _, _) = runtime(ready_transcript(), false, true, ready_size());
            let moves: Moves = Moves::default();
            let record = moves.clone();
            rt.update(move |capsule, _| {
                capsule.movable = movable;
                capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                    record.lock().unwrap().push((corner, dx, dy));
                }));
            });
            rt.frame(0.0);
            let (x, y) = rt.find("header").unwrap();
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.pointer_move(x - 20.0, y + 30.0);
            rt.frame(0.0);
            rt.pointer_up(x - 20.0, y + 30.0);
            rt.frame(0.0);
            let moves = moves.lock().unwrap();
            if movable {
                assert!(
                    moves.contains(&(MeetingIndicatorCorner::TopRight, -20.0, 30.0)),
                    "{moves:?}"
                );
                assert_eq!(
                    moves.last(),
                    Some(&(MeetingIndicatorCorner::TopRight, 0.0, 0.0))
                );
            } else {
                assert!(moves.is_empty(), "{moves:?}");
            }
            // The drag pressed nothing.
            assert!(rt.state.kind.is_some());
        }
    }

    fn call_ended() -> Kind {
        Kind::CallEnded {
            app: MeetingSource::Teams.label().into(),
            auto_stop: false,
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
        let _cursor = PASTE_CURSOR
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let kind = ready_summarized();
        let size = ready_size_of(&kind);
        let (mut rt, commands, _) = runtime(kind, false, true, size);
        let shown = texts(&rt);
        for expected in [
            "Lantern design sync",
            "08:56",
            "Copy all",
            "Copy agent prompt",
        ] {
            assert!(shown.contains(&expected.to_string()), "{shown:?}");
        }
        assert!(shown.iter().any(|text| text.starts_with("March 26")));
        // The transcript reads after the panel shows; until then Copy new
        // has nothing to say.
        tap(&mut rt, "copy-new");
        assert!(rt.copy_request.is_none());

        rt.update(|capsule, _| {
            capsule.live.entries = call();
            capsule.live.pending = meeting::pending_delta("ready-snap", &call());
        });
        rt.frame(1.0 / 60.0);
        assert!(texts(&rt).contains(&"8 sec new".to_string()));
        tap(&mut rt, "copy-new");
        assert!(rt.copy_request.take().unwrap().starts_with("Them: Kickoff"));
        assert_eq!(meeting::pending_delta("ready-snap", &call()).turns, 0);

        tap(&mut rt, "copy-prompt");
        let prompt = rt.copy_request.take().unwrap();
        assert!(
            prompt.contains("meetings show ready-snap --md"),
            "{}",
            &*prompt
        );
        assert!(commands.try_recv().is_err());
        assert!(rt.state.kind.is_some());
    }

    #[test]
    fn a_ready_panel_whose_time_ran_out_closes_when_the_pointer_leaves() {
        let (mut rt, _, _) = runtime(ready_transcript(), false, true, ready_size());
        rt.update(|capsule, _| {
            capsule.set_hovered(true);
            capsule.expired = true;
        });
        assert!(rt.state.kind.is_some());
        rt.update(|capsule, _| capsule.set_hovered(false));
        assert!(rt.state.kind.is_none());
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
    fn copy_new_says_how_much_is_new() {
        let (mut rt, _, _) = runtime(live_recording("hint"), false, true, RECORDING);
        open_recording(&mut rt);
        for (span_ms, expected) in [
            (400, "1 sec new"),
            (1_000, "1 sec new"),
            (59_999, "59 sec new"),
            (60_000, "1 min new"),
            (61 * 60_000, "61 min new"),
        ] {
            rt.update(move |capsule, _| {
                capsule.live.entries = call();
                capsule.live.pending = meeting::PendingDelta { turns: 1, span_ms };
            });
            rt.frame(1.0 / 60.0);
            assert!(
                texts(&rt).contains(&expected.to_string()),
                "{span_ms}: {:?}",
                texts(&rt)
            );
        }
    }

    #[test]
    fn a_bottom_corner_opens_upward() {
        let (mut rt, _, _) = runtime(live_recording("bottom"), false, true, RECORDING);
        rt.update(|capsule, _| capsule.corner = MeetingIndicatorCorner::BottomRight);
        open_recording(&mut rt);
        let (_, header) = rt.find("header").unwrap();
        let (_, copy_new) = rt.find("copy-new").unwrap();
        assert!(header > copy_new, "the header sits nearest the screen edge");
    }

    #[test]
    fn dismissing_an_automatic_recording_never_stops_it() {
        let (mut rt, commands, _) = runtime(auto_started(), false, true, PANEL);
        tap(&mut rt, "dismiss");
        assert!(commands.try_recv().is_err());
        assert!(rt.state.kind.is_none());
    }

    #[test]
    fn an_automatic_stop_says_so_and_keep_recording_cancels_it() {
        let auto = Kind::CallEnded {
            app: MeetingSource::WhatsApp.label().into(),
            auto_stop: true,
        };
        let (mut rt, commands, _) = runtime(auto, false, true, CALL_ENDED);
        assert!(
            texts(&rt)
                .contains(&"Hex stops recording in 30 s unless you keep recording.".to_string())
        );
        tap(&mut rt, "keep-recording");
        assert!(matches!(
            commands.try_recv(),
            Ok(ControllerCommand::KeepRecording)
        ));
        assert!(rt.state.kind.is_none());

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
        assert!(rt.state.kind.is_none());
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
                auto_stop: false,
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
                auto_stop: false,
            },
            start,
        );
        model.apply(&ControllerEvent::Offer(slack_huddle()), start);
        assert!(model.pill(true).is_some());

        // Stop & transcribe leaves it closed until the recording ends.
        model.apply(
            &ControllerEvent::CallEnded {
                app: "Zoom".into(),
                auto_stop: false,
            },
            start,
        );
        model.apply(&ControllerEvent::Transcribing, start);
        assert!(model.pill(true).is_none());
        model.keep_recording();
        assert!(model.pill(true).is_none());
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
    fn the_call_ended_title_fits_beside_its_buttons() {
        let kind = call_ended();
        assert_eq!(panel_width(&kind), CALL_ENDED.0);
        let (mut rt, _, _) = runtime(kind, false, false, CALL_ENDED);
        rt.settle_all();
        rt.frame(0.0);
        let title = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("title"))
            .unwrap();
        assert_eq!(title.text.as_deref(), Some("Microsoft Teams left the call"));
        let style = rune_theme_opencode::kit(false).ty.ui_medium;
        let (width, _) = rt.text.measure("Microsoft Teams left the call", style, 2.0);
        assert!(
            width <= title.rect[2] + 0.5,
            "title needs {width} pt, has {}",
            title.rect[2]
        );
    }

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
            ("offer", Kind::Offer(slack_huddle()), false, PANEL),
            ("offer-choices", Kind::Offer(slack_huddle()), true, MENU),
            ("offer-choices-site", Kind::Offer(google_meet), true, MENU),
            ("reminder", reminder(true, false), false, REMINDER),
            ("reminder-no-link", reminder(false, false), false, REMINDER),
            ("reminder-automatic", reminder(true, true), false, REMINDER),
            ("call-ended", call_ended(), false, CALL_ENDED),
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
            .chain::<[(&str, Kind, bool, (f32, f32), Setup); 9]>([
                (
                    "recording-rest",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| capsule.bars.show([0.9, 0.6, 0.3]),
                ),
                (
                    "recording-silent",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        if let Some(Kind::Recording(shown)) = &mut capsule.kind {
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
                        capsule.live.pending = meeting::PendingDelta {
                            turns: 3,
                            span_ms: 4 * 60_000 + 12_000,
                        };
                        capsule.bars.show([0.9, 0.6, 0.3]);
                        capsule.hovered = true;
                    },
                ),
                (
                    "recording-hover-bottom-left",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = meeting::PendingDelta {
                            turns: 3,
                            span_ms: 4 * 60_000 + 12_000,
                        };
                        capsule.corner = MeetingIndicatorCorner::BottomLeft;
                        capsule.hovered = true;
                    },
                ),
                (
                    "recording-hover-silent",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        if let Some(Kind::Recording(shown)) = &mut capsule.kind {
                            shown.warning = Some(crate::meeting::TrackWarning::DigitalSilence);
                        }
                        capsule.hovered = true;
                    },
                ),
                (
                    "recording-empty",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| capsule.hovered = true,
                ),
                (
                    "recording-transcribing",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = meeting::PendingDelta {
                            turns: 3,
                            span_ms: 8_000,
                        };
                        capsule.live.copying = true;
                        capsule.hovered = true;
                    },
                ),
                (
                    "recording-copied",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.copied = Some((LiveCopy::New, Instant::now()));
                        capsule.hovered = true;
                    },
                ),
                (
                    "ready-finished",
                    ready_summarized(),
                    false,
                    ready_size_of(&ready_summarized()),
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = meeting::PendingDelta {
                            turns: 3,
                            span_ms: 3 * 60_000 + 5_000,
                        };
                    },
                ),
            ]);
        for (name, kind, more_open, size, setup) in states {
            for dark in [false, true] {
                let (mut rt, _, _) = runtime(kind.clone(), more_open, dark, size);
                rt.update(move |capsule, _| setup(capsule));
                let (w, h, pixels) = render(&gpu, &mut rt, size);
                let theme = if dark { "dark" } else { "light" };
                let path = folder.join(format!("capsule-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }

    fn render(
        gpu: &rune::render::Gpu,
        rt: &mut Runtime<Capsule>,
        size: (f32, f32),
    ) -> (u32, u32, Vec<u8>) {
        rt.settle_all();
        rt.frame(0.0);
        let (w, h) = ((size.0 * 2.0) as u32, (size.1 * 2.0) as u32);
        let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
        let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
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
            ("call-ended", call_ended(), CALL_ENDED),
            ("ready", ready_transcript(), ready_size()),
            ("auto-started", auto_started(), PANEL),
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
            let (w, h, pixels) = render(&gpu, &mut rt, size);
            if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
                rune::render::write_png(&path, w, h, &pixels).unwrap();
                continue;
            }
            // Where CI's golden-actual artifact picks it up.
            let actual = std::env::temp_dir().join(format!("rune-settings-{file}-actual.png"));
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
