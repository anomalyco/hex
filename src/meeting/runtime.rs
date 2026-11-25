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

use crate::desktop::tray_menu::MeetingTray;
use crate::meeting::MeetingRequest;
use crate::meeting::watcher::{ControllerEvent, MeetingMenuState};
use crate::rune_ui::{Destination, Ui};

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
            let opened = window.clone();
            crate::rune_ui::meeting_capsules::MeetingCapsules::install(
                app,
                commands,
                move |choice| {
                    crate::app_settings::save_meeting_provider(choice);
                    chosen.update(move |ui, _| crate::rune_ui::meeting_settings_saved(ui));
                },
                move |corner, display| {
                    crate::app_settings::save_meeting_indicator_anchor(corner, display);
                    moved.update(move |ui, _| crate::rune_ui::meeting_settings_saved(ui));
                },
                move |id| {
                    opened.update(move |ui, tx| {
                        crate::rune_ui::show(ui, Destination::Meetings(Some(id)), tx);
                    });
                    opened.open();
                },
            )
        };
        let events = thread::Builder::new()
            .name("hex-meeting-events".into())
            .spawn(move || {
                for event in events {
                    capsules.event(&event);
                    #[cfg(target_os = "linux")]
                    if matches!(event, ControllerEvent::Offer(..)) {
                        let window = window.clone();
                        capsules.when_offer_unshown(move |reason| offer_unshown(&window, reason));
                    }
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
    if let Some(lock) = crate::labs::Feature::Meetings.managed() {
        return Err(lock.refusal().into());
    }
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
        // A stop under way takes no new recording, as Starting doesn't; the
        // row says why (see `MeetingTray::Stopping`).
        MeetingMenuState::Starting { stopping: true } => MeetingTray::Stopping,
        MeetingMenuState::Starting { .. } => MeetingTray::Starting,
        MeetingMenuState::Recording { stopping: true, .. } => MeetingTray::Stopping,
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

/// The offer capsule could not open (GNOME on Wayland, #1271): the
/// Meetings pane says so and its Record starts the recording instead.
#[cfg(target_os = "linux")]
fn offer_unshown(window: &rune::AppHandle<Ui>, reason: String) {
    tracing::warn!(%reason, "meeting recording offer could not show; opening the Meetings pane");
    let mut message = format!(
        "Hex detected a call but couldn't show its recording offer on this desktop ({reason}). Record it from here."
    );
    if !crate::labs::gnome_meeting_capsules() {
        message.push_str(
            " On GNOME, Labs › Call recording controls on GNOME can show it over your call.",
        );
    }
    window.update(move |ui, tx| {
        crate::rune_ui::meeting_failed(ui, message);
        crate::rune_ui::show(ui, Destination::Meetings(None), tx);
    });
    window.open();
}

/// Mirrors an event into the recording state and the window.
fn follow(event: &ControllerEvent, window: &rune::AppHandle<Ui>) {
    {
        let mut state = STATE.lock().unwrap_or_else(|error| error.into_inner());
        mirror(&mut state, event);
    }
    match event {
        ControllerEvent::Finished(manifest) => {
            tracing::info!(id = %manifest.id, "meeting transcript ready");
        }
        ControllerEvent::Discarded(id) => {
            tracing::info!(%id, "meeting had no speech; moved to the Trash");
        }
        ControllerEvent::Failed { title, error } => {
            tracing::error!(%title, %error, "meeting failed");
            let message = format!("{title}: {error}");
            window.update(move |ui, tx| {
                crate::rune_ui::meeting_failed(ui, message);
                crate::rune_ui::show(ui, Destination::Meetings(None), tx);
            });
            window.open();
        }
        ControllerEvent::ImportFailed { error } => {
            tracing::error!(%error, "meeting audio import failed");
            let message = error.clone();
            window.update(move |ui, tx| {
                crate::rune_ui::meeting_import_failed(ui, message);
                crate::rune_ui::show(ui, Destination::Meetings(None), tx);
            });
            window.open();
        }
        ControllerEvent::TranscriptEditFailed { error } => {
            tracing::error!(%error, "could not delete meeting transcript lines");
            let message = error.clone();
            window.update(move |ui, _| crate::rune_ui::meeting_edit_failed(ui, message));
        }
        ControllerEvent::CallEnded { app, stops_at, .. } => {
            if stops_at.is_some() {
                tracing::info!(
                    %app,
                    "the call ended; the recording stops in {} s unless kept",
                    crate::meeting::detection::AUTO_STOP_GRACE.as_secs()
                );
            } else {
                tracing::info!(%app, "the call ended; the recording continues until Stop");
            }
        }
        // These only move the mirror.
        ControllerEvent::Starting(_)
        | ControllerEvent::RecordingStarted { .. }
        | ControllerEvent::SourceWarning { .. }
        | ControllerEvent::Retitled { .. }
        | ControllerEvent::Chapters(_)
        | ControllerEvent::Stopping
        | ControllerEvent::Transcribing
        | ControllerEvent::Offer(..)
        | ControllerEvent::OfferWithdrawn(_)
        | ControllerEvent::Reminder { .. }
        | ControllerEvent::ReminderWithdrawn { .. }
        | ControllerEvent::CallEndedWithdrawn
        | ControllerEvent::NotesProgress(_)
        | ControllerEvent::NotesFinished(_) => {}
    }
}

/// The recorder state the tray, the menu bar, and `hex status` read. The
/// window's own notices go through [`follow`].
fn mirror(state: &mut MeetingMenuState, event: &ControllerEvent) {
    match event {
        ControllerEvent::Starting(_) => *state = MeetingMenuState::Starting { stopping: false },
        ControllerEvent::RecordingStarted { id, title, .. } => {
            *state = MeetingMenuState::Recording {
                started: Instant::now(),
                id: id.clone(),
                title: title.clone(),
                chapters: Vec::new(),
                warning: None,
                stopping: false,
            };
        }
        ControllerEvent::SourceWarning { warning } => state.source_warning(*warning),
        ControllerEvent::Retitled { title } => {
            if let MeetingMenuState::Recording { title: shown, .. } = state {
                shown.clone_from(title);
            }
        }
        ControllerEvent::Chapters(chapters) => {
            if let MeetingMenuState::Recording {
                chapters: shown, ..
            } = state
            {
                shown.clone_from(chapters);
            }
        }
        // The recorder holds this recording until it finishes transcribing;
        // a recording still starting has no title to keep yet.
        ControllerEvent::Stopping => state.stopping_requested(),
        ControllerEvent::Transcribing => *state = MeetingMenuState::Transcribing,
        ControllerEvent::Finished(_)
        | ControllerEvent::Discarded(_)
        | ControllerEvent::Failed { .. } => *state = MeetingMenuState::Idle,
        ControllerEvent::Offer(..)
        | ControllerEvent::OfferWithdrawn(_)
        | ControllerEvent::Reminder { .. }
        | ControllerEvent::ReminderWithdrawn { .. }
        | ControllerEvent::CallEnded { .. }
        | ControllerEvent::CallEndedWithdrawn
        | ControllerEvent::ImportFailed { .. }
        | ControllerEvent::TranscriptEditFailed { .. }
        | ControllerEvent::NotesProgress(_)
        | ControllerEvent::NotesFinished(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(stopping: bool) -> MeetingMenuState {
        MeetingMenuState::Recording {
            started: Instant::now(),
            id: "1700000000000".into(),
            title: "Design sync".into(),
            chapters: Vec::new(),
            warning: None,
            stopping,
        }
    }

    /// A manifest the recorder publishes as it finishes; `Finished` only
    /// reads its id.
    fn finished() -> Box<crate::meeting::MeetingManifest> {
        Box::new(
            serde_json::from_value(serde_json::json!({
                "schema_version": 1,
                "id": "1700000000000",
                "title": "Design sync",
                "status": "transcribing",
                "created_at_ms": 0,
                "ended_at_ms": null,
                "duration_ms": null,
                "system": {"file": "system.wav", "sample_rate": 16000,
                           "first_pts_us": null, "last_pts_us": null,
                           "first_arrival_us": null, "samples": 0, "dropped_packets": 0},
                "microphone": {"file": "microphone.wav", "sample_rate": 16000,
                               "first_pts_us": null, "last_pts_us": null,
                               "first_arrival_us": null, "samples": 0,
                               "dropped_packets": 0},
                "error": null,
            }))
            .expect("a valid manifest"),
        )
    }

    #[test]
    fn a_stop_moves_the_mirror_through_stopping_to_idle() {
        // A stop asked while the recorder is still starting: no title to
        // keep yet, but no new recording either.
        let mut state = MeetingMenuState::Starting { stopping: false };
        mirror(&mut state, &ControllerEvent::Stopping);
        assert_eq!(state, MeetingMenuState::Starting { stopping: true });
        mirror(&mut state, &ControllerEvent::Transcribing);
        assert_eq!(state, MeetingMenuState::Transcribing);
        mirror(&mut state, &ControllerEvent::Finished(finished()));
        assert_eq!(state, MeetingMenuState::Idle);

        // A recording keeps its title and elapsed time while it stops.
        let mut state = recorded(false);
        mirror(&mut state, &ControllerEvent::Stopping);
        assert!(matches!(
            state,
            MeetingMenuState::Recording {
                stopping: true,
                ref title,
                ..
            } if title == "Design sync"
        ));
        mirror(&mut state, &ControllerEvent::Transcribing);
        assert_eq!(state, MeetingMenuState::Transcribing);

        // Every way the recorder finishes ends at Idle, where a new
        // recording can start.
        let endings = [
            ControllerEvent::Finished(finished()),
            ControllerEvent::Discarded("1700000000000".into()),
            ControllerEvent::Failed {
                title: "Design sync".into(),
                error: "no model".into(),
            },
        ];
        for (index, event) in endings.into_iter().enumerate() {
            let mut state = recorded(true);
            mirror(&mut state, &event);
            assert_eq!(state, MeetingMenuState::Idle, "ending {index}");
        }
    }

    #[test]
    fn a_stop_with_nothing_to_stop_leaves_the_mirror_alone() {
        let mut state = MeetingMenuState::Idle;
        mirror(&mut state, &ControllerEvent::Stopping);
        assert_eq!(state, MeetingMenuState::Idle);
        let mut state = MeetingMenuState::Transcribing;
        mirror(&mut state, &ControllerEvent::Stopping);
        assert_eq!(state, MeetingMenuState::Transcribing);
    }
}
