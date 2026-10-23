//! The macOS meeting capsules on Rune: the recording offer, "left the
//! call", the ready transcript, and the recording capsule. Each is an
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
//! Option-Control-V's cursor.
//!
//! The recording capsule names the current topic from the live notes
//! (crossfading when it changes) and opens a notes panel under it, or above
//! it in a bottom corner. The overlay grows to fit the panel, so outside the
//! pill and panel clicks still reach the app below.
//!
//! Not yet built: hover expansion and dragging the recording capsule.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{
    Align, Cx, El, OverlayHandle, OverlaySpec, ScreenEdge, ScrollSpec, Transition, Tx, col, row,
    text,
};
use rune_kit::{Button, ButtonSize, ButtonVariant, IconButton, IconButtonVariant};

use super::controls::{DESCRIPTION, Paint, status_dot};
use super::meeting_outline::{OpenNote, Outline, outline, toggle};
use crate::app_settings::{MeetingPolicy, MeetingProviderChoice};
use crate::desktop_i18n::{tr, tr_fill};
use crate::meeting::{self, MeetingManifest, TranscriptEntry};
use crate::meeting_detection::MeetingCandidate;
use crate::meeting_watcher::{ControllerCommand, ControllerEvent};

/// The offer, call ended, and ready capsules.
const MEETING_PANEL_WIDTH: f32 = 420.0;
/// The offer's "…" choices need room for three buttons beside the name.
const MEETING_MENU_WIDTH: f32 = 560.0;
const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// The recording capsule.
const RECORDING_PANEL_WIDTH: f32 = 640.0;
/// "Copy new", "Copy new (12)", and "Copied" keep one width, so the other
/// buttons don't shift as the count changes.
const COPY_NEW_WIDTH: f32 = 112.0;
/// How long a copy button says "Copied".
const COPIED_FEEDBACK: Duration = Duration::from_millis(1_500);
const RECORDING_PANEL_HEIGHT: f32 = 48.0;
/// The notes panel the recording capsule opens, and its gap to the pill.
const NOTES_PANEL_HEIGHT: f32 = 340.0;
/// The panel before the first notes: one line of text.
const NOTES_EMPTY_HEIGHT: f32 = 44.0;
const NOTES_PANEL_GAP: f32 = 6.0;
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
        /// In a top corner the panel opens below the pill, else above it.
        top: bool,
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
}

impl Live {
    fn copied(&self, kind: LiveCopy) -> bool {
        self.copied
            .is_some_and(|(copied, at)| copied == kind && at.elapsed() < COPIED_FEEDBACK)
    }
}

/// The recording capsule's live notes.
#[derive(Clone, Default)]
struct Notes {
    outline: Option<Outline>,
    /// Apple Intelligence can take notes, or notes already exist.
    available: bool,
    open: bool,
    note: OpenNote,
}

struct Capsule {
    kind: Option<Kind>,
    /// The offer shows its Never offer / Always record choices in place of
    /// its actions.
    more_open: bool,
    copied: bool,
    live: Live,
    notes: Notes,
    commands: SyncSender<ControllerCommand>,
    hide: Option<Arc<dyn Fn() + Send + Sync>>,
    resize: Option<Resize>,
    choose: Choose,
}

type Choose = Arc<dyn Fn(MeetingProviderChoice) + Send + Sync>;
type Resize = Arc<dyn Fn((f32, f32)) + Send + Sync>;

impl Capsule {
    fn send(&self, command: ControllerCommand) -> bool {
        self.commands.try_send(command).is_ok()
    }

    fn set_more_open(&mut self, open: bool) {
        self.more_open = open;
        if let Some(resize) = &self.resize {
            let width = if open {
                MEETING_MENU_WIDTH
            } else {
                MEETING_PANEL_WIDTH
            };
            resize((width, MEETING_PANEL_HEIGHT));
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
        self.set_notes_open(false);
        self.notes = Notes::default();
        if let Some(hide) = &self.hide {
            hide();
        }
    }

    /// Opens or closes the notes panel, growing the overlay to fit it.
    fn set_notes_open(&mut self, open: bool) {
        if self.notes.open == open {
            return;
        }
        self.notes.open = open;
        self.fit_notes();
    }

    /// Sizes the overlay to the pill and, when open, its panel.
    fn fit_notes(&self) {
        if let Some(resize) = &self.resize {
            resize(self.notes.size());
        }
    }
}

impl Notes {
    fn is_empty(&self) -> bool {
        self.outline.as_ref().is_none_or(Outline::is_empty)
    }

    fn size(&self) -> (f32, f32) {
        recording_size(self.open, self.is_empty())
    }
}

/// The recording overlay; an open panel stays short until notes arrive.
fn recording_size(notes_open: bool, notes_empty: bool) -> (f32, f32) {
    let extra = match (notes_open, notes_empty) {
        (false, _) => 0.0,
        (true, true) => NOTES_EMPTY_HEIGHT + NOTES_PANEL_GAP,
        (true, false) => NOTES_PANEL_HEIGHT + NOTES_PANEL_GAP,
    };
    (RECORDING_PANEL_WIDTH, RECORDING_PANEL_HEIGHT + extra)
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
    ) -> Self {
        let choose: Choose = Arc::new(choose);
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
                    notes: Notes::default(),
                    commands: commands.clone(),
                    hide: None,
                    resize: None,
                    choose: choose.clone(),
                },
            );
            let (hide, resize) = (handle.clone(), handle.clone());
            handle.update(move |capsule, _| {
                capsule.hide = Some(Arc::new(move || hide.hide()));
                capsule.resize = Some(Arc::new(move |size| resize.resize(size)));
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
            ControllerEvent::OpenMeeting(_) | ControllerEvent::NotesStarted => {}
        }
    }

    fn show_panel(&self, kind: Kind, timeout: Option<Duration>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.panel
            .resize((MEETING_PANEL_WIDTH, MEETING_PANEL_HEIGHT));
        self.panel.update(move |capsule, tx| {
            capsule.kind = Some(kind);
            capsule.more_open = false;
            capsule.copied = false;
            tx.instant();
        });
        self.panel.show();
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
        let (edge, margin) = if corner.is_top() {
            (ScreenEdge::Top, WINDOW_MARGIN + MENU_BAR)
        } else {
            (ScreenEdge::Bottom, WINDOW_MARGIN + DOCK)
        };
        self.recording.place(edge, margin);
        self.recording.set_align(
            if corner.is_right() {
                Align::End
            } else {
                Align::Start
            },
            WINDOW_MARGIN,
        );
        let started = Instant::now();
        let ticker_id = id.clone();
        let top = corner.is_top();
        self.recording.resize(recording_size(false, true));
        self.recording.update(move |capsule, tx| {
            capsule.kind = Some(Kind::Recording {
                id,
                title,
                started,
                automatic,
                top,
            });
            capsule.live = Live::default();
            capsule.notes = Notes::default();
            tx.instant();
        });
        self.recording.show();
        *self.ticking.lock().unwrap() = Some(ticker_id.clone());
        let (recording, ticking) = (self.recording.clone(), self.ticking.clone());
        // Each second: the elapsed time, the in-call transcript, and the live
        // notes (only when their file changes), read here rather than on the
        // main thread.
        std::thread::spawn(move || {
            let mut reader = meeting::TranscriptReader::default();
            let mut notes = meeting::LiveNotesReader::default();
            let mut available = false;
            while ticking.lock().unwrap().as_deref() == Some(ticker_id.as_str()) {
                std::thread::sleep(Duration::from_secs(1));
                let live = match reader.read(&ticker_id) {
                    Ok(entries) => {
                        let pending = meeting::pending_delta_turns(&ticker_id, &entries);
                        Some((entries, pending))
                    }
                    Err(error) => {
                        tracing::debug!(%error, "could not read the in-call transcript");
                        None
                    }
                };
                let outline = notes
                    .refresh(&ticker_id)
                    .then(|| notes.notes().map(Outline::from));
                available = available || meeting::notes_wanted() || notes.notes().is_some();
                let id = ticker_id.clone();
                recording.update(move |capsule, _| {
                    if !matches!(&capsule.kind, Some(Kind::Recording { id: shown, .. }) if *shown == id)
                    {
                        return;
                    }
                    if let Some((entries, pending)) = live {
                        capsule.live.entries = entries;
                        capsule.live.pending = pending;
                    }
                    capsule.notes.available = available;
                    if let Some(outline) = outline {
                        let was_empty = capsule.notes.is_empty();
                        capsule.notes.outline = outline;
                        if capsule.notes.open && was_empty != capsule.notes.is_empty() {
                            capsule.fit_notes();
                        }
                    }
                });
            }
        });
    }

    fn close_recording(&self) {
        *self.ticking.lock().unwrap() = None;
        self.recording.update(|capsule, _| {
            capsule.kind = None;
            capsule.notes = Notes::default();
        });
        self.recording.hide();
    }
}
/// Debug preview: replays `entries` as a recording meeting in the isolated
/// data folder and shows its recording capsule and the Meetings window,
/// with no audio, listener, or controller. Buttons other than the notes
/// toggle do nothing.
#[cfg(debug_assertions)]
pub(crate) fn replay(
    entries: Vec<crate::meeting::TranscriptEntry>,
    speed: f32,
    title: &str,
    canned: bool,
    notes_open: bool,
) -> color_eyre::Result<()> {
    let (id, title) = meeting::replay(entries, speed, title, canned)?;
    let mut app = super::app(Default::default())
        .args(Vec::new())
        .keep_running();
    let window = app.handle();
    let (commands, _ignored) = std::sync::mpsc::sync_channel(8);
    let mut capsules = MeetingCapsules::install(&mut app, commands, |_| {});
    capsules.event(&ControllerEvent::RecordingStarted {
        id: id.clone(),
        title,
        automatic: false,
    });
    if notes_open {
        capsules
            .recording
            .update(|capsule, _| capsule.set_notes_open(true));
    }
    window.update(move |ui, tx| {
        ui.settings.meetings.enabled = true;
        super::show(ui, super::Destination::Meetings(Some(id)), tx);
    });
    window.open();
    app.run();
    Ok(())
}

/// Debug lab: the recording capsule rendered headless over outline snapshots
/// from `meeting live-notes --snapshots`, at `speed` meeting seconds per
/// second. Scenes: `replay` (from the start), and `pill`, `notes`, and
/// `notes-excerpt` frozen at the end.
#[cfg(debug_assertions)]
pub(crate) fn lab(
    snapshots: &std::path::Path,
    speed: f32,
    args: Vec<String>,
) -> color_eyre::Result<()> {
    use std::io::BufRead as _;
    #[derive(serde::Deserialize)]
    struct Snapshot {
        at_ms: u64,
        notes: meeting::LiveNotes,
    }
    let mut timeline = Vec::new();
    for line in std::io::BufReader::new(std::fs::File::open(snapshots)?).lines() {
        let snapshot: Snapshot = serde_json::from_str(&line?)?;
        timeline.push((snapshot.at_ms, Outline::from(&snapshot.notes)));
    }
    let end_ms = timeline.last().map_or(0, |(at_ms, _)| at_ms + 10_000);
    let _ = LAB.set(Lab {
        timeline,
        speed,
        end_ms,
    });
    let (width, height) = recording_size(true, false);
    rune::App::new(rune::AppSpec {
        title: "Hex capsule lab",
        size: (width + 2.0 * LAB_MARGIN, height + 2.0 * LAB_MARGIN),
        min_size: None,
        init: lab_capsule,
        view: lab_view,
        scenes: LAB_SCENES,
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: None,
        poll: Some(lab_poll),
        on_key: None,
    })
    .args(args)
    .run();
    Ok(())
}

#[cfg(debug_assertions)]
struct Lab {
    timeline: Vec<(u64, Outline)>,
    speed: f32,
    end_ms: u64,
}

#[cfg(debug_assertions)]
static LAB: std::sync::OnceLock<Lab> = std::sync::OnceLock::new();
/// Set by the frozen scenes: the meeting time they show.
#[cfg(debug_assertions)]
static LAB_FROZEN: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(debug_assertions)]
const LAB_MARGIN: f32 = 24.0;

#[cfg(debug_assertions)]
type LabScene = (&'static str, fn(&mut Capsule));

#[cfg(debug_assertions)]
const LAB_SCENES: &[LabScene] = &[
    ("replay", |_| {}),
    ("pill", |_| freeze_lab()),
    ("notes", |capsule| {
        freeze_lab();
        capsule.notes.open = true;
    }),
    ("notes-excerpt", |capsule| {
        freeze_lab();
        capsule.notes.open = true;
        capsule.notes.note = LAB
            .get()
            .and_then(|lab| lab.timeline.last())
            .and_then(|(_, outline)| outline.sections.last())
            .and_then(|section| section.notes.first())
            .map(|note| note.id());
    }),
];

#[cfg(debug_assertions)]
fn freeze_lab() {
    let end_ms = LAB.get().map_or(0, |lab| lab.end_ms);
    LAB_FROZEN.store(end_ms, Ordering::Relaxed);
}

#[cfg(debug_assertions)]
fn lab_capsule() -> Capsule {
    let (commands, _) = std::sync::mpsc::sync_channel(1);
    Capsule {
        kind: Some(Kind::Recording {
            id: "lab".into(),
            title: "Design sync".into(),
            started: Instant::now(),
            automatic: false,
            top: true,
        }),
        more_open: false,
        copied: false,
        live: Live::default(),
        notes: Notes {
            available: true,
            ..Notes::default()
        },
        commands,
        hide: None,
        resize: None,
        choose: Arc::new(|_| {}),
    }
}

/// Moves the meeting clock and shows the outline as it stood then.
#[cfg(debug_assertions)]
fn lab_poll(capsule: &mut Capsule, seconds: f64, _: &mut Tx) -> bool {
    let Some(lab) = LAB.get() else {
        return false;
    };
    let at_ms = match LAB_FROZEN.load(Ordering::Relaxed) {
        u64::MAX => (seconds * f64::from(lab.speed) * 1_000.0) as u64,
        frozen => frozen,
    };
    if let Some(Kind::Recording { started, .. }) = &mut capsule.kind {
        *started = Instant::now()
            .checked_sub(Duration::from_millis(at_ms))
            .unwrap_or_else(Instant::now);
    }
    capsule.notes.outline = lab
        .timeline
        .iter()
        .rev()
        .find(|(snapshot_ms, _)| *snapshot_ms <= at_ms)
        .map(|(_, outline)| outline.clone());
    true
}

/// The capsule over a plain backdrop, sized as its overlay would be.
#[cfg(debug_assertions)]
fn lab_view(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let (width, height) = capsule.notes.size();
    col()
        .key("lab")
        .w_full()
        .h_full()
        .padding(LAB_MARGIN)
        .fill(p.k.c.page)
        .child(
            col()
                .key("overlay")
                .w(width)
                .h(height)
                .child(view(capsule, cx)),
        )
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
        .size(ButtonSize::Normal)
        .on_press(press)
        .build(p.k)
        .key(key)
}

fn view(capsule: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let Some(kind) = &capsule.kind else {
        return row().key("empty");
    };
    let (dot, title, subtitle, actions) = match kind {
        Kind::Recording {
            title,
            started,
            automatic,
            top,
            ..
        } => {
            return recording(
                p,
                title,
                *started,
                *automatic,
                *top,
                &capsule.live,
                &capsule.notes,
            );
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
        .size(ButtonSize::Normal)
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
        .build(p.k)
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

/// The recording pill names the current topic once the notes have one, with
/// the meeting's title beside the elapsed time.
fn recording(
    p: Paint,
    title: &str,
    started: Instant,
    automatic: bool,
    top: bool,
    live: &Live,
    notes: &Notes,
) -> El {
    let elapsed = meeting::format_duration(started.elapsed().as_millis() as u64);
    let elapsed = if automatic && started.elapsed() < AUTOMATIC_LABEL {
        tr_fill("Started automatically · {}", &elapsed)
    } else {
        elapsed
    };
    let topic = notes.outline.as_ref().and_then(Outline::topic);
    let (label, subtitle) = match topic {
        Some(topic) => (topic, format!("{elapsed} · {title}")),
        None => (title, elapsed),
    };
    let copied_new = live.copied(LiveCopy::New);
    let copy_new = Button::new(if copied_new {
        tr("Copied").to_owned()
    } else if live.pending > 0 {
        tr_fill("Copy new ({})", &live.pending.to_string())
    } else {
        tr("Copy new").to_owned()
    })
    .variant(ButtonVariant::Neutral)
    .size(ButtonSize::Normal)
    .disabled(live.pending == 0 && !copied_new)
    .on_press_tx(|c: &mut Capsule, tx: &mut Tx| {
        let Some(Kind::Recording { id, .. }) = &c.kind else {
            return;
        };
        // Rune's clipboard write has no acknowledgment, so the cursor commits
        // once it is queued (as GPUI's Copy new did).
        match meeting::try_consume_delta_from(id, &c.live.entries, |text| {
            tx.copy(text.to_owned());
            Ok(())
        }) {
            Ok(_) => {
                c.live.pending = 0;
                c.live.copied = Some((LiveCopy::New, Instant::now()));
            }
            Err(error) => tracing::warn!(%error, "could not copy new meeting transcript turns"),
        }
    })
    .build(p.k)
    .min_w(COPY_NEW_WIDTH)
    .key("copy-new");
    let copied_all = live.copied(LiveCopy::All);
    let copy_all = Button::new(tr(if copied_all { "Copied" } else { "Copy all" }))
        .variant(ButtonVariant::Neutral)
        .size(ButtonSize::Normal)
        .disabled(live.entries.is_empty() && !copied_all)
        .on_press_tx(|c: &mut Capsule, tx: &mut Tx| {
            if c.live.entries.is_empty() {
                return;
            }
            tx.copy(meeting::transcript_markdown(&c.live.entries));
            c.live.copied = Some((LiveCopy::All, Instant::now()));
        })
        .build(p.k)
        .key("copy-all");
    let mut actions = Vec::new();
    if notes.available {
        let open = notes.open;
        actions.push(
            // Points toward the panel: down to open it below, up to close it.
            IconButton::new(if open == top {
                p.k.glyph.chevron_up
            } else {
                p.k.glyph.chevron_down
            })
            .variant(IconButtonVariant::Neutral)
            .expanded(open)
            .label(tr(if open { "Hide notes" } else { "Show notes" }))
            .on_press(|c: &mut Capsule| c.set_notes_open(!c.notes.open))
            .build(p.k)
            .key("notes-toggle"),
        );
    }
    actions.extend([
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
    ]);
    let pill = pill_row(p, p.danger(), label, &subtitle, actions).h(RECORDING_PANEL_HEIGHT - 8.0);
    if !notes.open {
        return frame(pill);
    }
    let panel = notes_panel(p, notes);
    let capsule = col()
        .key("capsule")
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .gap(NOTES_PANEL_GAP);
    if top {
        capsule.child(pill).child(panel)
    } else {
        capsule.child(panel).child(pill)
    }
}
/// The notes panel: the outline, newest at the bottom and kept in view.
fn notes_panel(p: Paint, notes: &Notes) -> El {
    let c = &p.k.c;
    let content = match notes.outline.as_ref().filter(|outline| !outline.is_empty()) {
        Some(notes_outline) => outline(
            p,
            notes_outline,
            &notes.note,
            true,
            |c: &mut Capsule, note| toggle(&mut c.notes.note, note),
        ),
        None => text(tr("Notes appear as the call goes on."))
            .key("notes-empty")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap(),
    };
    col()
        .key("notes-panel")
        .w_full()
        .grow(1.0)
        .shrink()
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .transition(Transition::opacity())
        .child(
            col()
                .key("notes-scroll")
                .grow(1.0)
                .shrink()
                .clip_fade(0.0, 12.0, 0.0, 12.0)
                .scroll(ScrollSpec { stick_bottom: true })
                .child(col().key("inset").px(8.0).py(10.0).child(content)),
        )
}

/// The capsule's pill: status dot, two truncated lines, and its actions.
fn pill(p: Paint, dot: rune::Color, title: &str, subtitle: &str, actions: Vec<El>) -> El {
    frame(pill_row(p, dot, title, subtitle, actions).h_full())
}

/// The overlay's inset around a lone pill.
fn frame(pill: El) -> El {
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(4.0)
        .py(4.0)
        .child(pill)
}

/// The title crossfades when it changes (a new topic).
fn pill_row(p: Paint, dot: rune::Color, title: &str, subtitle: &str, actions: Vec<El>) -> El {
    let c = &p.k.c;
    row()
        .key("pill")
        .w_full()
        .gap(12.0)
        .pl(16.0)
        .pr(8.0)
        .center_y()
        .radius(999.0)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(status_dot(dot))
        .child(
            col()
                .key("lines")
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(title.to_owned())
                        .key(format!("title:{title}"))
                        .truncate()
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .transition(Transition::blur_replace()),
                )
                .children((!subtitle.is_empty()).then(|| {
                    text(subtitle.to_owned())
                        .key("subtitle")
                        .truncate()
                        .font(p.k.ty.small)
                        .color(p.faint())
                })),
        )
        .child(row().key("actions").gap(8.0).center_y().children(actions))
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
    const NOTES: (f32, f32) = (
        RECORDING_PANEL_WIDTH,
        RECORDING_PANEL_HEIGHT + NOTES_PANEL_HEIGHT + NOTES_PANEL_GAP,
    );

    fn design_sync(top: bool) -> Kind {
        Kind::Recording {
            id: "m".into(),
            title: "Design sync".into(),
            started: Instant::now() - Duration::from_secs(412),
            automatic: false,
            top,
        }
    }

    /// The fixture outline; `open_section` opens that section's first note.
    fn notes(open: bool, open_section: Option<usize>) -> Notes {
        let outline = Outline::fixture();
        let note = open_section
            .and_then(|section| outline.sections.get(section))
            .and_then(|section| section.notes.first())
            .map(|note| note.id());
        Notes {
            outline: Some(outline),
            available: true,
            open,
            note,
        }
    }

    fn empty_notes() -> Notes {
        Notes {
            available: true,
            open: true,
            ..Notes::default()
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
            notes: Notes::default(),
            commands,
            hide: None,
            resize: None,
            choose: Arc::new(move |choice| saved.lock().unwrap().push(choice)),
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
            top: true,
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

    fn live_recording(id: &str) -> Kind {
        Kind::Recording {
            id: id.into(),
            title: "Q4 launch sync".into(),
            started: Instant::now() - Duration::from_secs(258),
            automatic: false,
            top: true,
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
    fn the_recording_pill_names_the_topic_and_opens_its_notes() {
        let (mut rt, _, _) = runtime(design_sync(true), false, true, RECORDING);
        assert!(rt.find("notes-toggle").is_none());
        rt.update(|capsule, _| capsule.notes = notes(false, None));
        rt.frame(1.0 / 60.0);
        rt.settle_all();
        rt.frame(0.0);
        let shown = texts(&rt);
        assert!(
            shown
                .iter()
                .any(|text| text == "Launch checklist and owners")
        );
        assert!(shown.iter().any(|text| text.ends_with(" · Design sync")));
        tap(&mut rt, "notes-toggle");
        assert!(rt.state.notes.open);
        // Closing clears them, so the next recording starts empty.
        rt.state.close();
        assert!(!rt.state.notes.open && rt.state.notes.outline.is_none());
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
            (
                "recording-automatic",
                Kind::Recording {
                    id: "m".into(),
                    title: "Slack Huddle call — 2:30 PM".into(),
                    started: Instant::now() - Duration::from_secs(4),
                    automatic: true,
                    top: true,
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
                    "recording-copied",
                    live_recording("snap"),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| {
                        capsule.live.entries = call();
                        capsule.live.copied = Some((LiveCopy::New, Instant::now()));
                    },
                ),
                (
                    "recording-topic",
                    design_sync(true),
                    false,
                    RECORDING,
                    |capsule: &mut Capsule| capsule.notes = notes(false, None),
                ),
                (
                    "recording-notes",
                    design_sync(true),
                    false,
                    NOTES,
                    |capsule: &mut Capsule| capsule.notes = notes(true, None),
                ),
                (
                    "recording-notes-bottom",
                    design_sync(false),
                    false,
                    NOTES,
                    |capsule: &mut Capsule| capsule.notes = notes(true, None),
                ),
                (
                    "recording-notes-excerpt",
                    design_sync(true),
                    false,
                    NOTES,
                    |capsule: &mut Capsule| capsule.notes = notes(true, Some(1)),
                ),
                (
                    "recording-notes-empty",
                    design_sync(true),
                    false,
                    (
                        RECORDING_PANEL_WIDTH,
                        RECORDING_PANEL_HEIGHT + NOTES_EMPTY_HEIGHT + NOTES_PANEL_GAP,
                    ),
                    |capsule: &mut Capsule| capsule.notes = empty_notes(),
                ),
            ]);
        for (name, kind, more_open, size, setup) in states {
            for dark in [false, true] {
                let (mut rt, _, _) = runtime(kind.clone(), more_open, dark, size);
                rt.update(|capsule, _| setup(capsule));
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
