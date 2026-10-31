//! Meetings in the Linux and Windows apps: the controller thread that
//! detects calls and records them (`meeting_watcher`), the capsules its
//! events drive, and the recording state the trays read. The macOS app runs
//! the same controller and capsules from `macos_rune`.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use color_eyre::Result;

use crate::meeting::MeetingRequest;
use crate::meeting::watcher::{ControllerEvent, MeetingMenuState};
use crate::rune_ui::{Destination, Ui};
use crate::tray_menu::MeetingTray;

static REQUESTS: OnceLock<SyncSender<MeetingRequest>> = OnceLock::new();
/// The app, whose clipboard the tray's copy writes.
static APP: OnceLock<rune::AppHandle<Ui>> = OnceLock::new();
static STATE: Mutex<MeetingMenuState> = Mutex::new(MeetingMenuState::Idle);

/// The meeting controller and the thread that shows its events. Detection
/// stays idle until the user turns Meetings on.
pub(crate) struct Meetings {
    controller: JoinHandle<()>,
    events: JoinHandle<()>,
}

impl Meetings {
    /// Starts the controller and installs its capsules on `app`.
    pub(crate) fn install(app: &mut rune::App<Ui>, shutdown: &'static AtomicBool) -> Result<Self> {
        let (event_sender, events) = mpsc::sync_channel(32);
        let (commands, command_receiver) = mpsc::sync_channel(8);
        let (requests, request_receiver) = mpsc::sync_channel(8);
        let _ = REQUESTS.set(requests);
        let controller = thread::Builder::new()
            .name("hex-meetings".into())
            .spawn(move || {
                if let Err(error) = crate::meeting::watcher::controller_loop(
                    shutdown,
                    event_sender.clone(),
                    command_receiver,
                    request_receiver,
                ) {
                    tracing::error!(%error, "meeting controller stopped");
                    let _ = event_sender.send(ControllerEvent::Failed {
                        title: "Meeting detection stopped".into(),
                        error: error.to_string(),
                    });
                }
            })?;
        let window = app.handle();
        let _ = APP.set(window.clone());
        let mut capsules = {
            let chosen = window.clone();
            let moved = window.clone();
            crate::rune_ui::meeting_capsules::MeetingCapsules::install(
                app,
                commands,
                move |choice| {
                    crate::app_settings::save_meeting_provider(choice.clone());
                    chosen.update(move |ui, _| crate::rune_ui::meeting_provider_chosen(ui, choice));
                },
                move |corner| {
                    crate::app_settings::save_meeting_indicator_corner(corner);
                    moved.update(move |ui, _| crate::rune_ui::meeting_corner_moved(ui, corner));
                },
            )
        };
        let events = thread::Builder::new()
            .name("hex-meeting-events".into())
            .spawn(move || {
                for event in events {
                    capsules.event(&event);
                    follow(&event, &window);
                }
            })?;
        Ok(Self { controller, events })
    }

    /// Waits for the controller, which stops once `shutdown` is set: a
    /// recording keeps its audio and finishes its transcript at the next
    /// launch.
    pub(crate) fn join(self) {
        if self.controller.join().is_err() {
            tracing::error!("meeting controller panicked");
        }
        let _ = self.events.join();
    }
}

/// Asks the controller to start, stop, or summarize a meeting.
pub(crate) fn request(request: MeetingRequest) -> Result<(), String> {
    REQUESTS
        .get()
        .ok_or("meetings are not running")?
        .try_send(request)
        .map_err(|error| format!("could not reach the meeting recorder: {error}"))
}

/// The recording state the tray shows.
pub(crate) fn state() -> MeetingMenuState {
    STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The recorder as the tray shows it; `None` while Meetings is off.
pub(crate) fn tray_state() -> Option<MeetingTray> {
    if !crate::app_settings::meetings_enabled() || REQUESTS.get().is_none() {
        return None;
    }
    Some(match state() {
        MeetingMenuState::Idle => MeetingTray::Idle,
        MeetingMenuState::Starting => MeetingTray::Starting,
        MeetingMenuState::Recording { .. } => MeetingTray::Recording,
        MeetingMenuState::Transcribing => MeetingTray::Transcribing,
    })
}

/// The tray's Copy last meeting transcript.
pub(crate) fn copy_last_transcript() -> Result<(), String> {
    let markdown =
        crate::meeting::latest_transcript_markdown().map_err(|error| format!("{error:#}"))?;
    APP.get()
        .ok_or("the window is not running")?
        .update(move |_, tx| tx.copy(markdown));
    Ok(())
}

/// Mirrors an event into the recording state and the window.
fn follow(event: &ControllerEvent, window: &rune::AppHandle<Ui>) {
    let mut state = STATE.lock().unwrap_or_else(|error| error.into_inner());
    match event {
        ControllerEvent::Starting => *state = MeetingMenuState::Starting,
        ControllerEvent::RecordingStarted { id, title, .. } => {
            *state = MeetingMenuState::Recording {
                started: Instant::now(),
                id: id.clone(),
                title: title.clone(),
                chapters: Vec::new(),
            };
        }
        ControllerEvent::Retitled { title } => {
            if let MeetingMenuState::Recording { title: shown, .. } = &mut *state {
                shown.clone_from(title);
            }
        }
        ControllerEvent::Chapters(chapters) => {
            if let MeetingMenuState::Recording {
                chapters: shown, ..
            } = &mut *state
            {
                shown.clone_from(chapters);
            }
        }
        ControllerEvent::Transcribing => *state = MeetingMenuState::Transcribing,
        ControllerEvent::Finished(manifest) => {
            tracing::info!(id = %manifest.id, "meeting transcript ready");
            *state = MeetingMenuState::Idle;
        }
        ControllerEvent::OpenMeeting(id) => {
            let id = id.clone();
            window.update(move |ui, tx| {
                crate::rune_ui::show(ui, Destination::Meetings(Some(id)), tx)
            });
            window.open();
        }
        ControllerEvent::Failed { title, error } => {
            tracing::error!(%title, %error, "meeting failed");
            *state = MeetingMenuState::Idle;
            let message = format!("{title}: {error}");
            window.update(move |ui, tx| {
                crate::rune_ui::meeting_failed(ui, message);
                crate::rune_ui::show(ui, Destination::Meetings(None), tx);
            });
            window.open();
        }
        ControllerEvent::CallEnded { app } => {
            tracing::info!(%app, "the call ended; the recording continues until Stop");
        }
        ControllerEvent::Offer(_)
        | ControllerEvent::Reminder { .. }
        | ControllerEvent::ReminderWithdrawn { .. }
        | ControllerEvent::NotesStarted
        | ControllerEvent::NotesFinished(_) => {}
    }
}
