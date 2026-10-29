//! The macOS meeting capsules on Rune: the calendar meeting reminder, the
//! recording offer, "left the call", the ready transcript, and the recording
//! capsule. Each is an
//! interactive Rune overlay, which on macOS is a non-activating `NSPanel`:
//! its buttons take clicks while the call app keeps focus and Hex never
//! activates. An offer that times out only closes; nothing starts or stops
//! on a timeout.
//!
//! The pump in `macos_rune` forwards controller events on the main thread;
//! buttons send `ControllerCommand`s back to the controller.
//!
//! The recording capsule's "Copy new" and "Copy all" copy the in-call
//! transcript (Parakeet, `meeting::TranscriptReader`); "Copy new" shares
//! Option-Control-V's cursor and first transcribes the call up to the click
//! (`meeting::entries_for_copy`), saying "Transcribing…" meanwhile.
//!
//! The recording capsule rests as a compact pill in its corner and springs
//! open to show its actions while the pointer is over it. Dragging it moves
//! the window; on release it snaps to the nearest corner, which is saved.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{
    Align, Anim, Cursor, Cx, Drag, El, OverlayHandle, OverlaySpec, Phase, ScreenEdge, Spring, Tx,
    col, row, text,
};
use rune_kit::{Badge, BadgeVariant, Button, ButtonVariant};

use super::controls::{Paint, PushSize, pulsing_dot, status_dot};
use crate::app_settings::{MeetingIndicatorCorner, MeetingPolicy, MeetingProviderChoice};
use crate::calendar::CalendarMeeting;
use crate::desktop_i18n::{tr, tr_fill};
use crate::meeting::{self, MeetingManifest, TranscriptEntry};
use crate::meeting_detection::MeetingCandidate;
use crate::meeting_watcher::{ControllerCommand, ControllerEvent};

/// The offer, call ended, and ready capsules.
const MEETING_PANEL_WIDTH: f32 = 420.0;
/// The offer's "…" choices need room for three buttons beside the name.
const MEETING_MENU_WIDTH: f32 = 560.0;
const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// Between a capsule's buttons and its right edge.
const ACTIONS_INSET: f32 = 8.0;
/// The recording capsule stays a full pill.
const PILL_RADIUS: f32 = 999.0;
/// The reminder: the meeting's title beside up to three buttons.
const REMINDER_PANEL_WIDTH: f32 = 520.0;
/// How often an open reminder recounts "Starts in N min".
const REMINDER_TICK: Duration = Duration::from_secs(5);
/// The recording capsule's window, as wide as the open pill.
const RECORDING_PANEL_WIDTH: f32 = 640.0;
/// The resting pill: the dot, the title and elapsed time, and a `+N` tag.
const RECORDING_COMPACT_WIDTH: f32 = 244.0;
/// Opening under the pointer, and closing after it leaves (GPUI's snap
/// springs), with no overshoot.
const RECORDING_OPEN: Spring = Spring::new(0.25, 0.0);
const RECORDING_CLOSE: Spring = Spring::new(0.2, 0.0);
/// "Copy new", "Copy new (12)", and "Copied" keep one width, so the other
/// buttons don't shift as the count changes.
const COPY_NEW_WIDTH: f32 = 112.0;
/// How long a copy button says "Copied".
const COPIED_FEEDBACK: Duration = Duration::from_millis(1_500);
const RECORDING_PANEL_HEIGHT: f32 = 48.0;
const OFFER_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT: Duration = Duration::from_secs(15);
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
    CallEnded {
        app: String,
    },
    Ready(Box<MeetingManifest>),
    /// An automatic recording while the recording indicator is off.
    AutoStarted {
        title: String,
    },
    Recording {
        id: String,
        title: String,
        started: Instant,
        automatic: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LiveCopy {
    New,
    All,
}

/// The recording's in-call transcript as the ticker last read it.
#[derive(Default)]
struct Live {
    entries: Vec<TranscriptEntry>,
    /// Speaker turns not yet copied or pasted.
    pending: usize,
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
    copied: bool,
    live: Live,
    commands: SyncSender<ControllerCommand>,
    hide: Option<Arc<dyn Fn() + Send + Sync>>,
    resize: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    choose: Choose,
    /// The recording capsule: the pointer is over the pill (or holding it).
    hovered: bool,
    /// Where the recording capsule rests.
    corner: MeetingIndicatorCorner,
    /// Moves the recording capsule's window by a drag's travel from `corner`.
    shift: Option<Shift>,
    save_corner: SaveCorner,
    /// Reads a recording's transcript through now off the main thread, then
    /// calls [`Capsule::finish_copy_new`]. Without it, Copy new copies what
    /// the ticker last read.
    copy_new: Option<CopyNew>,
}

type Choose = Arc<dyn Fn(MeetingProviderChoice) + Send + Sync>;
type CopyNew = Arc<dyn Fn(String) + Send + Sync>;
type Shift = Arc<dyn Fn(MeetingIndicatorCorner, f32, f32) + Send + Sync>;
type SaveCorner = Arc<dyn Fn(MeetingIndicatorCorner) + Send + Sync>;

impl Capsule {
    fn send(&self, command: ControllerCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    fn set_more_open(&mut self, open: bool) {
        self.more_open = open;
        if let Some(resize) = &self.resize {
            resize(if open {
                MEETING_MENU_WIDTH
            } else {
                MEETING_PANEL_WIDTH
            });
        }
    }

    /// Saves the offered provider's choice; returns whether there was one.
    fn choose(&self, policy: MeetingPolicy) -> bool {
        let Some(Kind::Offer(candidate)) = &self.kind else {
            return false;
        };
        let Some(provider) = candidate.provider() else {
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
        let Some(Kind::Recording { id, .. }) = &self.kind else {
            return;
        };
        if self.live.copying {
            return;
        }
        match self.copy_new.clone() {
            Some(copy_new) => {
                self.live.copying = true;
                copy_new(id.clone());
            }
            None => {
                let (id, entries) = (id.clone(), self.live.entries.clone());
                self.copy_new_from(&id, &entries, tx);
            }
        }
    }

    /// Copies the new turns of `entries`, read through the click for the
    /// recording `id`, if it still shows.
    fn finish_copy_new(
        &mut self,
        id: &str,
        entries: color_eyre::Result<Vec<TranscriptEntry>>,
        tx: &mut Tx,
    ) {
        // A reply for an earlier recording leaves the shown one's copy alone.
        if !matches!(&self.kind, Some(Kind::Recording { id: shown, .. }) if shown == id) {
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
                self.live.pending = 0;
                self.live.copied = Some((LiveCopy::New, Instant::now()));
            }
            Err(error) => tracing::warn!(%error, "could not copy new meeting transcript turns"),
        }
    }

    /// The recording pill follows the pointer and snaps to the nearest
    /// corner on release. A cancelled drag (Escape, window blur) returns to
    /// the corner it started from and saves nothing.
    fn drag(&mut self, drag: &Drag, tx: &mut Tx) {
        let Some(shift) = self.shift.clone() else {
            return;
        };
        match drag.phase {
            Phase::Down => {}
            Phase::Move => shift(self.corner, drag.dx, drag.dy),
            Phase::Up if drag.cancelled() => shift(self.corner, 0.0, 0.0),
            Phase::Up => {
                let corner = primary_screen_size().map_or(self.corner, |screen| {
                    nearest_corner(self.corner, drag.dx, drag.dy, screen)
                });
                shift(corner, 0.0, 0.0);
                if corner != self.corner {
                    tracing::info!(?corner, "recording capsule moved");
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

/// The corner nearest the capsule window's centre after a drag of
/// (dx, dy) points from `from`, on a screen of `screen` points.
fn nearest_corner(
    from: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
    screen: (f32, f32),
) -> MeetingIndicatorCorner {
    let (w, h) = (RECORDING_PANEL_WIDTH, RECORDING_PANEL_HEIGHT);
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
fn primary_screen_size() -> Option<(f32, f32)> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::screens(mtm).iter().next()?;
    let size = screen.frame().size;
    Some((size.width as f32, size.height as f32))
}

fn place_recording(
    handle: &OverlayHandle<Capsule>,
    corner: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
) {
    let (edge, margin, align, inset) = corner_placement(corner, dx, dy);
    handle.place(edge, margin);
    handle.set_align(align, inset);
}

/// The two capsules: the timed panel (offer, call ended, ready) and the
/// recording capsule in the configured corner.
pub(crate) struct MeetingCapsules {
    panel: OverlayHandle<Capsule>,
    recording: OverlayHandle<Capsule>,
    /// Bumped whenever the panel shows something new, so a stale timeout
    /// doesn't close it.
    generation: Arc<AtomicU64>,
    /// The recording capsule's elapsed-time ticker runs while this is set.
    ticking: Arc<Mutex<Option<String>>>,
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
        let overlay = |app: &mut rune::App<S>, name, size| {
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
                    copied: false,
                    live: Live::default(),
                    commands: commands.clone(),
                    hide: None,
                    resize: None,
                    choose: choose.clone(),
                    hovered: false,
                    corner: MeetingIndicatorCorner::default(),
                    shift: None,
                    save_corner: save_corner.clone(),
                    copy_new: None,
                },
            );
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
                capsule.resize = Some(Arc::new(move |width| {
                    resize.resize((width, MEETING_PANEL_HEIGHT));
                }));
                capsule.shift = Some(Arc::new(move |corner, dx, dy| {
                    place_recording(&shift, corner, dx, dy);
                }));
            });
            handle
        };
        let panel = overlay(
            app,
            "Hex meeting",
            (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT),
        );
        let recording = overlay(
            app,
            "Hex meeting recording",
            (RECORDING_PANEL_WIDTH, RECORDING_PANEL_HEIGHT),
        );
        Self {
            panel,
            recording,
            generation: Arc::default(),
            ticking: Arc::default(),
        }
    }

    /// Shows, updates, or closes a capsule for each controller event.
    pub(crate) fn event(&mut self, event: &ControllerEvent) {
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
            ControllerEvent::Starting | ControllerEvent::Transcribing => {
                self.close_panel();
                self.close_recording();
            }
            ControllerEvent::RecordingStarted {
                id,
                title,
                automatic,
            } => {
                self.close_recording();
                if crate::app_settings::meeting_indicator_enabled() {
                    self.show_recording(id.clone(), title.clone(), *automatic);
                } else if *automatic {
                    // Nothing else on screen would say a recording began.
                    self.show_panel(
                        Kind::AutoStarted {
                            title: title.clone(),
                        },
                        Some(AUTOMATIC_LABEL),
                    );
                }
            }
            ControllerEvent::Retitled { title } => {
                let title = title.clone();
                self.recording.update(move |capsule, _| {
                    if let Some(Kind::Recording { title: shown, .. }) = &mut capsule.kind {
                        *shown = title;
                    }
                });
            }
            ControllerEvent::CallEnded { app } => {
                self.close_recording();
                self.show_panel(Kind::CallEnded { app: app.clone() }, None);
            }
            ControllerEvent::Finished(manifest) => {
                self.close_recording();
                self.show_panel(Kind::Ready(manifest.clone()), Some(READY_TIMEOUT));
            }
            ControllerEvent::NotesFinished(manifest) => {
                let manifest = manifest.clone();
                self.panel.update(move |capsule, _| {
                    if let Some(Kind::Ready(ready)) = &mut capsule.kind
                        && ready.id == manifest.id
                    {
                        *ready = manifest;
                    }
                });
            }
            ControllerEvent::Failed { .. } => self.close_recording(),
            ControllerEvent::OpenMeeting(_)
            | ControllerEvent::Chapters(_)
            | ControllerEvent::NotesStarted => {}
        }
    }

    fn show_panel(&self, kind: Kind, timeout: Option<Duration>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let reminder = matches!(kind, Kind::Reminder { .. });
        self.panel.resize((
            if reminder {
                REMINDER_PANEL_WIDTH
            } else {
                MEETING_PANEL_WIDTH
            },
            MEETING_PANEL_HEIGHT,
        ));
        self.panel.update(move |capsule, tx| {
            capsule.kind = Some(kind);
            capsule.more_open = false;
            capsule.copied = false;
            tx.instant();
        });
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
                        }
                        capsule.close();
                    });
                }
            });
        }
    }

    // Hide through the handle, not `Capsule::close`: a hide requested inside
    // an update runs on the event loop after a show queued in the same batch,
    // so closing then showing a capsule would leave it hidden.
    fn close_panel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.panel.update(|capsule, _| capsule.kind = None);
        self.panel.hide();
    }

    fn show_recording(&self, id: String, title: String, automatic: bool) {
        let corner = crate::app_settings::meeting_indicator_corner();
        place_recording(&self.recording, corner, 0.0, 0.0);
        let started = Instant::now();
        let ticker_id = id.clone();
        self.recording.update(move |capsule, tx| {
            capsule.kind = Some(Kind::Recording {
                id,
                title,
                started,
                automatic,
            });
            capsule.live = Live::default();
            capsule.hovered = false;
            capsule.corner = corner;
            tx.instant();
        });
        self.recording.show();
        *self.ticking.lock().unwrap() = Some(ticker_id.clone());
        let (recording, ticking) = (self.recording.clone(), self.ticking.clone());
        // Each second: the elapsed time, and the in-call transcript, read here
        // rather than on the main thread.
        std::thread::spawn(move || {
            let mut reader = meeting::TranscriptReader::default();
            while ticking.lock().unwrap().as_deref() == Some(ticker_id.as_str()) {
                std::thread::sleep(Duration::from_secs(1));
                let entries = match reader.read(&ticker_id) {
                    Ok(entries) => entries,
                    Err(error) => {
                        tracing::debug!(%error, "could not read the in-call transcript");
                        recording.update(|_, _| {});
                        continue;
                    }
                };
                let pending = meeting::pending_delta_turns(&ticker_id, &entries);
                let id = ticker_id.clone();
                recording.update(move |capsule, _| {
                    if matches!(&capsule.kind, Some(Kind::Recording { id: shown, .. }) if *shown == id)
                    {
                        capsule.live.entries = entries;
                        capsule.live.pending = pending;
                    }
                });
            }
        });
    }

    fn close_recording(&self) {
        *self.ticking.lock().unwrap() = None;
        self.recording.update(|capsule, _| capsule.kind = None);
        self.recording.hide();
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

fn view(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    view_body(capsule, cx).hover_motion_within(Anim::INSTANT, Anim::INSTANT)
}

fn view_body(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let Some(kind) = &capsule.kind else {
        return row().key("empty");
    };
    let (dot, title, subtitle, actions) = match kind {
        Kind::Recording {
            title,
            started,
            automatic,
            ..
        } => {
            return recording(p, capsule, title, *started, *automatic);
        }
        Kind::AutoStarted { title } => (
            p.danger(),
            tr("Recording automatically").to_owned(),
            title.clone(),
            vec![button(p, "stop", tr("Stop"), true, |c| {
                if c.send(ControllerCommand::Stop) {
                    c.close();
                }
            })],
        ),
        Kind::Reminder { meeting, automatic } => {
            reminder(p, meeting, *automatic, crate::calendar::wall_clock_ms())
        }
        Kind::Offer(candidate) if capsule.more_open => offer_choices(p, candidate),
        Kind::Offer(candidate) => {
            let mut actions = Vec::new();
            if candidate.provider().is_some() {
                actions.push(button(p, "offer-more", "…", false, |c| {
                    c.set_more_open(true);
                }));
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
        Kind::CallEnded { app } => (
            p.danger(),
            tr_fill("{} left the call", app),
            tr("Hex is still recording.").to_owned(),
            vec![
                button(p, "keep-recording", tr("Keep recording"), false, |c| {
                    c.close();
                }),
                button(p, "stop-meeting", tr("Stop & transcribe"), true, |c| {
                    if c.send(ControllerCommand::Stop) {
                        c.close();
                    }
                }),
            ],
        ),
        Kind::Ready(manifest) => ready(p, manifest, capsule.copied),
    };
    pill(p, dot, &title, &subtitle, actions)
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
    let label = candidate
        .provider()
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
    if candidate.can_always_record() {
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

fn ready(
    p: Paint,
    manifest: &MeetingManifest,
    copied: bool,
) -> (rune::Color, String, String, Vec<El>) {
    let summary = manifest
        .notes
        .as_ref()
        .map(|notes| notes.summary.trim().to_string())
        .filter(|summary| !summary.is_empty());
    let subtitle = if meeting::notes_in_progress(&manifest.id) {
        tr("Writing a title and summary with Apple Intelligence…").to_owned()
    } else if let Some(summary) = summary {
        summary
    } else if let Some(path) = &manifest.export_path {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
    } else if let Some(error) = &manifest.export_error {
        tr_fill("Markdown not saved: {}", error)
    } else {
        tr("Saved in Meetings").to_owned()
    };
    let title = meeting::display_title(manifest);
    let title = if title == manifest.title && manifest.notes.is_none() {
        tr("Meeting transcript ready").to_owned()
    } else {
        tr_fill("Ready · {}", &title)
    };
    let copy = Button::new(tr(if copied { "Copied" } else { "Copy" }))
        .variant(ButtonVariant::Neutral)
        .on_press_tx(|c: &mut Capsule, tx: &mut Tx| {
            let Some(Kind::Ready(manifest)) = &c.kind else {
                return;
            };
            match meeting::transcript_markdown_for_manifest(manifest) {
                Ok(markdown) => {
                    tx.copy(markdown);
                    c.copied = true;
                }
                Err(error) => tracing::warn!(%error, "could not copy meeting transcript"),
            }
        })
        .build_push(p.k)
        .key("copy-transcript");
    let mut actions = vec![copy];
    if manifest.export_path.is_some() {
        actions.push(button(p, "reveal-transcript", tr("Reveal"), true, |c| {
            if let Some(Kind::Ready(manifest)) = &c.kind
                && let Some(path) = &manifest.export_path
            {
                meeting::reveal(path);
            }
            c.close();
        }));
    }
    (p.success(), title, subtitle, actions)
}

fn recording(p: Paint, capsule: &Capsule, title: &str, started: Instant, automatic: bool) -> El {
    let live = &capsule.live;
    let elapsed = meeting::format_duration(started.elapsed().as_millis() as u64);
    let subtitle = if automatic && started.elapsed() < AUTOMATIC_LABEL {
        tr_fill("Started automatically · {}", &elapsed)
    } else {
        elapsed
    };
    let copied_new = live.copied(LiveCopy::New);
    let copy_new = Button::new(if live.copying {
        tr("Transcribing…").to_owned()
    } else if copied_new {
        tr("Copied").to_owned()
    } else if live.pending > 0 {
        tr_fill("Copy new ({})", &live.pending.to_string())
    } else {
        tr("Copy new").to_owned()
    })
    .variant(ButtonVariant::Neutral)
    // Reading through the click can find speech the ticker hasn't shown.
    .disabled(live.copying || (live.pending == 0 && !copied_new && capsule.copy_new.is_none()))
    .on_press_tx(|c: &mut Capsule, tx: &mut Tx| c.press_copy_new(tx))
    .build_push(p.k)
    .min_w(COPY_NEW_WIDTH)
    .key("copy-new");
    let copied_all = live.copied(LiveCopy::All);
    let copy_all = Button::new(tr(if copied_all { "Copied" } else { "Copy all" }))
        .variant(ButtonVariant::Neutral)
        .disabled(live.entries.is_empty() && !copied_all)
        .on_press_tx(|c: &mut Capsule, tx: &mut Tx| {
            if c.live.entries.is_empty() {
                return;
            }
            tx.copy(meeting::transcript_markdown(&c.live.entries));
            c.live.copied = Some((LiveCopy::All, Instant::now()));
        })
        .build_push(p.k)
        .key("copy-all");
    let actions = vec![
        copy_new,
        copy_all,
        button(p, "open", tr("Open"), false, |c| {
            if let Some(Kind::Recording { id, .. }) = &c.kind {
                c.send(ControllerCommand::Open(id.clone()));
            }
        }),
        button(p, "stop", tr("Stop"), true, |c| {
            c.send(ControllerCommand::Stop);
        }),
    ];
    let open = capsule.hovered;
    let pending = (!open && live.pending > 0).then(|| {
        Badge::new(format!("+{}", live.pending))
            .variant(BadgeVariant::Accent)
            .build(p.k)
            .key("pending")
    });
    let pill = pill_body(p, pulsing_dot(p.danger()), title, &subtitle, PILL_RADIUS)
        .pr(if pending.is_some() { 12.0 } else { 8.0 })
        .children(pending)
        .children(open.then(|| row().key("actions").gap(8.0).children(actions)))
        .clip_rounded(PILL_RADIUS)
        .cursor(Cursor::Grab)
        .motion(if open {
            RECORDING_OPEN
        } else {
            RECORDING_CLOSE
        })
        .on_hover_change(|c: &mut Capsule, inside, _| c.hovered = inside)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    let pill = if open {
        pill.w_full()
    } else {
        pill.w(RECORDING_COMPACT_WIDTH - 8.0)
    };
    let capsule_row = row().key("capsule").w_full().h_full().px(4.0).py(4.0);
    if capsule.corner.is_right() {
        capsule_row.justify_end()
    } else {
        capsule_row.justify_start()
    }
    .child(pill)
}

/// The panel capsule (reminder, offer, call ended, ready): status dot, two
/// truncated lines, and its actions, in a rounded rectangle.
fn pill(p: Paint, dot: rune::Color, title: &str, subtitle: &str, actions: Vec<El>) -> El {
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .child(
            pill_body(p, status_dot(dot), title, subtitle, panel_radius(p))
                .w_full()
                .child(row().key("actions").gap(8.0).children(actions)),
        )
}

/// The panel capsule's corners, concentric with its buttons' (`radius.md`)
/// across the [`ACTIONS_INSET`] between them and the edge.
fn panel_radius(p: Paint) -> f32 {
    p.k.radius.md + ACTIONS_INSET
}

/// The capsule body with its dot and two truncated lines: a rounded
/// rectangle for the panel capsules, a full pill for the recording capsule.
fn pill_body(p: Paint, dot: El, title: &str, subtitle: &str, radius: f32) -> El {
    let c = &p.k.c;
    row()
        .key("pill")
        .h_full()
        .gap(12.0)
        .pl(16.0)
        .pr(ACTIONS_INSET)
        .center_y()
        .radius(radius)
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
                        .color(p.text()),
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

    use super::*;
    use crate::meeting_detection::MeetingSource;

    type Chosen = Arc<Mutex<Vec<MeetingProviderChoice>>>;

    const PANEL: (f32, f32) = (MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT);
    const MENU: (f32, f32) = (MEETING_MENU_WIDTH, MEETING_PANEL_HEIGHT);
    const RECORDING: (f32, f32) = (RECORDING_PANEL_WIDTH, RECORDING_PANEL_HEIGHT);
    const REMINDER: (f32, f32) = (REMINDER_PANEL_WIDTH, MEETING_PANEL_HEIGHT);

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
            copied: false,
            live: Live::default(),
            commands,
            hide: None,
            resize: None,
            choose: Arc::new(move |choice| saved.lock().unwrap().push(choice)),
            hovered: false,
            corner: MeetingIndicatorCorner::TopRight,
            shift: None,
            save_corner: Arc::new(|_| {}),
            copy_new: None,
        };
        let env = Env {
            system_dark: dark,
            scale: 2.0,
        };
        let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let mut rt = Runtime::with_text(capsule, view, size, env, text);
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
        let recording = |started| Kind::Recording {
            id: "m".into(),
            title: "Slack Huddle call — 2:30 PM".into(),
            started,
            automatic: true,
        };
        let (rt, _, _) = runtime(recording(Instant::now()), false, true, RECORDING);
        assert!(
            texts(&rt)
                .iter()
                .any(|text| text.starts_with("Started automatically · "))
        );
        let earlier = Instant::now() - AUTOMATIC_LABEL - Duration::from_secs(1);
        let (rt, _, _) = runtime(recording(earlier), false, true, RECORDING);
        assert!(!texts(&rt).iter().any(|text| text.contains("automatically")));
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
        Kind::Recording {
            id: id.into(),
            title: "Q4 launch sync".into(),
            started: Instant::now() - Duration::from_secs(258),
            automatic: false,
        }
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
        // Nothing transcribed yet: both copies are unavailable.
        assert!(texts(&rt).contains(&"Copy new".to_string()));
        tap(&mut rt, "copy-new");
        tap(&mut rt, "copy-all");
        assert!(rt.copy_request.is_none());

        rt.update(|capsule, _| {
            capsule.live.entries = call();
            capsule.live.pending = meeting::pending_delta_turns(id, &call());
        });
        rt.frame(1.0 / 60.0);
        assert!(texts(&rt).contains(&"Copy new (3)".to_string()));
        tap(&mut rt, "copy-new");
        assert_eq!(
            rt.copy_request.take().as_deref(),
            Some(
                "Them: Kickoff is at two thirty.\n\nYou: Two thirty works.\n\nThem: Then let's ship it."
            )
        );
        assert!(texts(&rt).contains(&"Copied".to_string()));
        // What was copied is not new again, here or for Option-Control-V.
        assert_eq!(meeting::pending_delta_turns(id, &call()), 0);

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
        assert_eq!(meeting::pending_delta_turns(id, &call()), 0);

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
        rt.update(|capsule, _| capsule.live.pending = 2);
        rt.settle_all();
        rt.frame(0.0);
        assert!(rt.find("copy-new").is_none());
        assert!(texts(&rt).contains(&"+2".to_string()));
        let (x, _) = rt.find("pill").unwrap();
        assert!(
            x > RECORDING_PANEL_WIDTH / 2.0,
            "a right corner's pill rests on the right"
        );

        open_recording(&mut rt);
        assert!(rt.state.hovered);
        assert!(rt.find("copy-new").is_some());
        assert!(!texts(&rt).contains(&"+2".to_string()));
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
        assert_eq!(nearest_corner(TopRight, -100.0, 40.0, screen), TopRight);
        assert_eq!(nearest_corner(TopRight, -600.0, 0.0, screen), TopLeft);
        assert_eq!(nearest_corner(TopRight, 0.0, 700.0, screen), BottomRight);
        assert_eq!(nearest_corner(BottomLeft, 500.0, -600.0, screen), TopRight);
        assert_eq!(nearest_corner(BottomLeft, 10.0, -10.0, screen), BottomLeft);
        let (edge, margin, align, inset) = corner_placement(BottomLeft, 10.0, -10.0);
        assert!(matches!((edge, align), (ScreenEdge::Bottom, Align::Start)));
        assert_eq!(
            (margin, inset),
            (WINDOW_MARGIN + DOCK + 10.0, WINDOW_MARGIN + 10.0)
        );
    }

    /// No `--snap` scene draws an overlay, so this renders the capsules to
    /// PNGs in `HEX_CAPSULE_SNAPS` for review; without it, or a GPU, it skips.
    #[test]
    fn capsule_snapshots() {
        use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
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
            (
                "recording-automatic",
                Kind::Recording {
                    id: "m".into(),
                    title: "Slack Huddle call — 2:30 PM".into(),
                    started: Instant::now() - Duration::from_secs(4),
                    automatic: true,
                },
                false,
                RECORDING,
            ),
            (
                "auto-started",
                Kind::AutoStarted {
                    title: "Slack Huddle call — 2:30 PM".into(),
                },
                false,
                PANEL,
            ),
        ];
        type Setup = fn(&mut Capsule);
        let nothing: Setup = |_| {};
        let states = states
            .into_iter()
            .map(|(name, kind, more_open, size)| (name, kind, more_open, size, nothing))
            .chain([
                (
                    "recording-empty",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    nothing,
                ),
                (
                    "recording-live",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = 3;
                    },
                ),
                (
                    "recording-open",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = 3;
                        capsule.hovered = true;
                    },
                ),
                (
                    "recording-left",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| capsule.corner = MeetingIndicatorCorner::BottomLeft,
                ),
                (
                    "recording-transcribing",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.pending = 3;
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
            ]);
        for (name, kind, more_open, size, setup) in states {
            for dark in [false, true] {
                let (mut rt, _, _) = runtime(kind.clone(), more_open, dark, size);
                rt.update(move |capsule, _| setup(capsule));
                rt.settle_all();
                rt.frame(0.0);
                let (w, h) = ((size.0 * 2.0) as u32, (size.1 * 2.0) as u32);
                let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
                let target = Offscreen::new(&gpu.device, w, h);
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
                let pixels = target.read(&gpu.device);
                let theme = if dark { "dark" } else { "light" };
                let path = folder.join(format!("capsule-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }
}
