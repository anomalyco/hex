//! Meeting detection and recording control for the app: watches which
//! applications use the microphone, offers a recording for supported calls,
//! runs the recorder, reports when the call app leaves, and starts notes. The
//! menu bar and the meeting capsules show its events.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::calendar::{CalendarMeeting, ReminderScheduler, ReminderUpdate};
use crate::dictation::context::ContextSnapshot;
use crate::meeting::detection::{
    AutoStop, BrowserPageMemory, CallPresence, CallSwitch, Detection, MeetingCandidate,
    MeetingDetector, MeetingSource, OfferId, candidate_from_microphone,
};
use crate::meeting::recovery::PendingFinals;
use crate::meeting::{self, MeetingCalendar, MeetingRequest, MeetingStart, TrackWarning};
use crate::microphone_activity::{ActiveMicrophoneApplication, active_microphone_applications};

use call_end::{CallEndStop, CallWatch, stops_when_call_ends};
use call_switch::{
    disarm_call_stop_unless_watching, watching_call_switch, withdraw_call_switch_stop,
};
use import::{ImportWorker, poll_import, start_import};

mod call_end;
mod call_switch;
mod import;
mod publication;

pub(crate) enum ControllerCommand {
    Record(MeetingCandidate, Option<OfferId>),
    Dismiss,
    /// The offer's capsule closed on its timeout, unanswered: the call is
    /// offered again once its app releases and retakes the microphone.
    OfferTimedOut(OfferId),
    Stop,
    /// Import a local audio file as a meeting (macOS; Labs › Import audio).
    Import(PathBuf),
    /// Delete selected lines from a saved meeting's transcript
    /// (Labs › Delete transcript lines).
    DeleteTranscript {
        id: String,
        entries: Vec<meeting::TranscriptEntryKey>,
    },
    /// Leave Hex's microphone out of the recording under way, or put it
    /// back (Labs › Mute the microphone while recording, macOS).
    SetMicrophoneMuted(bool),
    Summarize(String),
    /// Record a calendar meeting from its reminder, first opening its call
    /// link when `join`.
    RecordMeeting {
        meeting: CalendarMeeting,
        join: bool,
    },
    /// Open the meeting's call link without recording (Always record
    /// starts once the call is detected).
    JoinMeeting(CalendarMeeting),
    /// Joined from the Labs menu: opens the call link without recording, and
    /// records the occurrence so no reminder pops (closing the showing one).
    JoinFromMenu(CalendarMeeting),
    /// Not now, or Don't record: the meeting's call is neither offered nor
    /// recorded automatically.
    DismissReminder(CalendarMeeting),
    /// Keep recording on the call-ended capsule: Stop when the call
    /// ends doesn't stop this absence (it arms again if the call app comes
    /// back and leaves again).
    KeepRecording,
}

pub(crate) enum ControllerEvent {
    Offer(OfferId, MeetingCandidate),
    OfferWithdrawn(OfferId),
    /// A calendar meeting is about to start. `automatic` when its call
    /// service is set to Always record.
    Reminder {
        meeting: CalendarMeeting,
        automatic: bool,
    },
    /// Close this occurrence's reminder: it was cancelled, moved, is long
    /// past, or reminders were turned off.
    ReminderWithdrawn {
        occurrence: String,
    },
    /// The accepted offer, or None for a start that did not come from one.
    Starting(Option<OfferId>),
    /// `automatic` when the provider is set to Always record.
    RecordingStarted {
        id: String,
        title: String,
        automatic: bool,
    },
    Retitled {
        title: String,
    },
    /// The recording's live chapters so far (Labs).
    Chapters(Vec<meeting::Chapter>),
    /// The microphone became digitally silent, or its audio returned.
    SourceWarning {
        warning: Option<TrackWarning>,
    },
    /// Stop requested; the capture worker may still be closing its sources.
    Stopping,
    /// Both sources have stopped. The worker is preparing the final transcript.
    Transcribing,
    /// `stops_at` when the recording stops on its own then, after
    /// [`crate::meeting::detection::AUTO_STOP_GRACE`] (Stop when the
    /// call ends or Labs › Stop every call recording when the call ends).
    CallEnded {
        app: String,
        /// The native call app's bundle ID (macOS) or executable name for
        /// its icon; `None` for web calls in a browser.
        application: Option<String>,
        stops_at: Option<Instant>,
    },
    /// A pending automatic stop was dropped because the call came back
    /// (Labs › Stop when the call ends): its notice goes, and the recording
    /// capsule returns.
    CallEndedWithdrawn,
    Finished(Box<meeting::MeetingManifest>),
    /// The final pass succeeded with no speech: its recording and Markdown
    /// went to the Trash, so nothing opens a finished meeting.
    Discarded(String),
    Failed {
        title: String,
        error: String,
    },
    /// An audio file could not be imported as a meeting (macOS).
    ImportFailed {
        error: String,
    },
    /// Transcript lines could not be deleted (Labs › Delete transcript lines).
    TranscriptEditFailed {
        error: String,
    },
    NotesProgress(String),
    NotesFinished(Box<meeting::MeetingManifest>),
}

struct ActiveRecording {
    title: String,
    application: Option<String>,
    app_label: String,
    automatic: bool,
    presence: CallPresence,
    /// The app this recording has seen on the microphone, so one whose app
    /// joins late counts no absence before it does.
    watch: CallWatch,
    /// Stop when the call ends, armed while the call is over.
    auto_stop: AutoStop,
    /// The call's own code, watching the browser's page for another call
    /// (Stop when the call ends, browsers that never release the
    /// microphone between calls).
    call_switch: CallSwitch,
    /// Stop when the call ends, armed while the page names a different
    /// call. Its own deadline: a browser still on the microphone keeps the
    /// call app present, so the absence one is dropped every pass.
    call_stop: AutoStop,
    stop: Arc<AtomicBool>,
    updates: Receiver<meeting::RecordingUpdate>,
    reported_started: bool,
    /// A browser's playback stopped counting while the Mac was locked, and
    /// that was logged once for this recording (#2685); cleared on unlock.
    playback_locked_noted: bool,
    worker: JoinHandle<Result<meeting::MeetingManifest>>,
}

impl ActiveRecording {
    /// Request a stop and end the snooze; the worker confirms when capture ends.
    fn end(&self, detector: &mut MeetingDetector, events: &SyncSender<ControllerEvent>) {
        if !self.stop.swap(true, Ordering::Relaxed) {
            detector.recording_stopped();
            let _ = events.send(ControllerEvent::Stopping);
        }
    }
}

fn flush_withdrawn_offers(detector: &mut MeetingDetector, events: &SyncSender<ControllerEvent>) {
    for id in detector.take_withdrawn_offers() {
        let _ = events.send(ControllerEvent::OfferWithdrawn(id));
    }
}

/// The meeting state mirrored into the menu bar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeetingMenuState {
    Idle,
    Starting,
    Recording {
        started: Instant,
        id: String,
        title: String,
        /// Live chapters, for Copy Section (Labs).
        chapters: Vec<meeting::Chapter>,
        /// The microphone is digitally silent, as a call app holding it with
        /// voice processing leaves it (#1518).
        warning: Option<TrackWarning>,
        /// Stop was requested and the capture worker is still closing its
        /// sources. The recorder holds this recording until it finishes
        /// transcribing, so nothing new can start yet.
        stopping: bool,
    },
    Transcribing,
}

impl MeetingMenuState {
    /// Sets the recording's microphone warning; other states have none.
    pub(crate) fn source_warning(&mut self, warning: Option<TrackWarning>) {
        if let Self::Recording { warning: shown, .. } = self {
            *shown = warning;
        }
    }
}

pub struct ListenerConfig {
    pub event_path: PathBuf,
    pub device: Option<String>,
}

pub(crate) fn controller_loop(
    shutdown: &AtomicBool,
    events: SyncSender<ControllerEvent>,
    commands: Receiver<ControllerCommand>,
    meeting_requests: Receiver<MeetingRequest>,
) -> Result<()> {
    let mut detector = MeetingDetector::default();
    let mut seen_calls = crate::meeting::seen_calls::SeenCalls::default();
    let mut recording: Option<ActiveRecording> = None;
    let mut importing: Option<ImportWorker> = None;
    let mut reminders = Reminders::default();
    let mut observed_microphone_apps = Vec::new();
    let mut browser = BrowserWatch::new(ContextSnapshot::capture_for_meeting);
    let mut pending_finals = PendingFinals::new();
    let mut expiry = MeetingExpiry::default();
    tracing::info!("meeting watcher started");

    while !shutdown.load(Ordering::Relaxed) {
        let enabled = crate::app_settings::meetings_enabled();
        // Labs › Mute the microphone while recording off: clear the switch on
        // the first pass that sees it, so turning the lab back on in the same
        // recording does not silently mute it again. The sink reads the same
        // gate with each packet, so audio is already back by then.
        crate::meeting::capture::reconcile_microphone_mute(crate::labs::meeting_microphone_mute());
        poll_import(&mut importing, &events);
        let requested_commands = meeting_requests
            .try_iter()
            .take(8)
            .map(|request| match request {
                MeetingRequest::Start { title } => {
                    ControllerCommand::Record(MeetingCandidate::manual_titled(title), None)
                }
                MeetingRequest::Stop => ControllerCommand::Stop,
                MeetingRequest::Summarize(id) => ControllerCommand::Summarize(id),
                MeetingRequest::Import(path) => ControllerCommand::Import(path),
                MeetingRequest::DeleteTranscript { id, entries } => {
                    ControllerCommand::DeleteTranscript { id, entries }
                }
                MeetingRequest::SetMicrophoneMuted(muted) => {
                    ControllerCommand::SetMicrophoneMuted(muted)
                }
            });
        for command in commands.try_iter().take(8).chain(requested_commands) {
            handle_command(
                &mut recording,
                &mut importing,
                &mut detector,
                &mut reminders,
                command,
                enabled,
                &events,
            );
        }

        poll_recording(&mut recording, &mut detector, &mut browser, &events)?;
        reminders.poll(enabled, recording.is_some(), &events);
        pending_finals.poll(enabled, recording.is_some(), &events);
        expiry.poll();

        // Meetings off: detection never runs, so a remembered switch must not
        // outlive the setting and start a call when it comes back.
        if recording.is_none() && !enabled {
            detector.forget_switched_call();
        }

        if recording.is_none() && enabled {
            let choices = crate::app_settings::meeting_provider_choices();
            match detect_meeting(
                &mut detector,
                &mut observed_microphone_apps,
                &mut browser,
                &mut seen_calls,
                &choices,
                crate::app_settings::meeting_default_policy(),
            ) {
                Err(()) => {
                    flush_withdrawn_offers(&mut detector, &events);
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
                Ok(Some(Detection::Offer(candidate))) => {
                    flush_withdrawn_offers(&mut detector, &events);
                    // Offers record by hand, which never watches a page: the
                    // call whose page named this one is not kept for it.
                    detector.take_switched_from();
                    if let Some(id) = detector.open_offer() {
                        let _ = events.send(ControllerEvent::Offer(id, candidate));
                    }
                }
                Ok(Some(Detection::Record(candidate))) => {
                    flush_withdrawn_offers(&mut detector, &events);
                    let mut start = reminders.start_for(&candidate);
                    // Detected while the page still named the stopped call:
                    // this recording keeps that code, so the page naming it
                    // never ends this one (#1896).
                    start.switched_from = detector.take_switched_from();
                    start_recording(&mut recording, &mut reminders, start, true, None, &events);
                }
                Ok(None) => {}
            }
        } else if !watching_call_switch(
            recording.as_ref(),
            crate::app_settings::meeting_stop_when_call_ends(),
        ) {
            // Nothing is watched now: no answer read before this may come
            // back as fresh when detection resumes. A recording that watches
            // its browser's page for another call keeps reading it.
            browser.reset();
        }
        if !enabled || recording.is_some() {
            detector.withdraw_offer();
        }
        flush_withdrawn_offers(&mut detector, &events);
        thread::sleep(Duration::from_millis(500));
    }

    // The app is quitting: a recording keeps its audio, and its transcript
    // finishes at the next launch. macOS marks this before it drains.
    #[cfg(not(target_os = "macos"))]
    crate::desktop::process_exit::begin_quitting();
    if let Some(active) = recording {
        active.stop.store(true, Ordering::Relaxed);
        let manifest = active
            .worker
            .join()
            .map_err(|_| eyre!("meeting recorder panicked"))??;
        // The drain publishes a kept final pass exactly as the running
        // controller would; it starts no notes.
        meeting::keep_or_discard(&manifest);
    }
    if let Some(importing) = importing {
        // A failed import during shutdown is reported, not propagated: it
        // must not skip joining the pending finals below.
        match importing.worker.join() {
            Ok(Ok(manifest)) => {
                if meeting::keep_or_discard(&manifest) {
                    tracing::info!(
                        meeting = manifest.id,
                        "audio import finished while shutting down"
                    );
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "audio import did not finish before shutdown");
            }
            Err(_) => tracing::error!("meeting import panicked"),
        }
    }
    pending_finals.join();
    Ok(())
}

/// How often meetings past their retention move to the Trash while Hex runs.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Moves meetings past their retention to the Trash: at launch, then
/// hourly, and soon after the setting changes. Meetings off still expires
/// what was recorded, since kept meetings are a liability. A separate audio
/// retention runs after the whole-meeting pass, and only while its Labs
/// feature is on.
#[derive(Default)]
struct MeetingExpiry {
    last: Option<(
        Instant,
        Option<crate::app_settings::MeetingRetention>,
        Option<crate::app_settings::MeetingRetention>,
    )>,
}

impl MeetingExpiry {
    fn poll(&mut self) {
        let retention = crate::app_settings::meeting_retention();
        let audio = crate::app_settings::meeting_audio_retention();
        if self.last.is_some_and(|(at, last, last_audio)| {
            last == retention && last_audio == audio && at.elapsed() < EXPIRY_INTERVAL
        }) {
            return;
        }
        self.last = Some((Instant::now(), retention, audio));
        if let Err(error) = meeting::expire(retention) {
            tracing::warn!(%error, "could not expire old meetings");
        }
        if let Err(error) = meeting::expire_audio(audio) {
            tracing::warn!(%error, "could not expire old meeting audio");
        }
    }
}

fn handle_command(
    recording: &mut Option<ActiveRecording>,
    importing: &mut Option<ImportWorker>,
    detector: &mut MeetingDetector,
    reminders: &mut Reminders,
    command: ControllerCommand,
    enabled: bool,
    events: &SyncSender<ControllerEvent>,
) {
    match command {
        ControllerCommand::Record(..) | ControllerCommand::RecordMeeting { .. } if !enabled => {
            tracing::warn!("ignored a meeting recording request while meetings are off");
        }
        ControllerCommand::Import(_) if !enabled => {
            tracing::warn!("ignored an audio import while meetings are off");
        }
        ControllerCommand::Import(_) if !crate::labs::meeting_import() => {
            tracing::warn!("ignored an audio import while Labs › Import audio is off");
            let _ = events.send(ControllerEvent::ImportFailed {
                error: "Labs › Import audio is off".into(),
            });
        }
        ControllerCommand::Import(path) => start_import(recording, importing, path, events),
        ControllerCommand::DeleteTranscript { .. } if !enabled => {
            tracing::warn!("ignored a transcript edit while meetings are off");
        }
        // The edit refuses on its own while the lab is off.
        ControllerCommand::DeleteTranscript { id, entries } => {
            if let Err(error) = meeting::edit::delete_entries(&id, &entries) {
                tracing::warn!(%error, meeting = id, "could not delete transcript lines");
                let _ = events.send(ControllerEvent::TranscriptEditFailed {
                    error: error.message(),
                });
            }
        }
        ControllerCommand::Record(candidate, offer)
            if recording.is_none() && (offer.is_none() || offer == detector.open_offer()) =>
        {
            tracing::info!(
                source = candidate.source.label(),
                application = candidate.application.as_deref(),
                "meeting recording started on request"
            );
            let start = reminders.start_for(&candidate);
            start_recording(recording, reminders, start, false, offer, events);
        }
        ControllerCommand::RecordMeeting { meeting, join } => {
            reminders.scheduler.answered(&meeting.occurrence());
            if join && let Some(link) = &meeting.link {
                crate::calendar::open_link(&link.url);
            }
            snooze_calendar_call(detector, &meeting);
            if recording.is_some() {
                tracing::warn!("ignored a calendar recording request while recording");
                return;
            }
            tracing::info!(
                provider = meeting.link.as_ref().map(|link| link.provider.label()),
                join,
                "calendar meeting recording started from its reminder"
            );
            start_recording(
                recording,
                reminders,
                calendar_start(&meeting),
                false,
                None,
                events,
            );
        }
        ControllerCommand::JoinMeeting(meeting) => {
            reminders.scheduler.answered(&meeting.occurrence());
            if let Some(link) = &meeting.link {
                crate::calendar::open_link(&link.url);
            }
        }
        ControllerCommand::JoinFromMenu(meeting) => {
            if let Some(occurrence) = reminders.scheduler.joined_elsewhere(&meeting) {
                let _ = events.send(ControllerEvent::ReminderWithdrawn { occurrence });
            }
            if let Some(link) = &meeting.link {
                crate::calendar::open_link(&link.url);
            }
        }
        ControllerCommand::DismissReminder(meeting) => {
            reminders.scheduler.answered(&meeting.occurrence());
            snooze_calendar_call(detector, &meeting);
            tracing::info!("calendar meeting reminder dismissed");
        }
        ControllerCommand::Stop => {
            if let Some(active) = recording.as_ref() {
                active.end(detector, events);
            }
        }
        ControllerCommand::KeepRecording => {
            if let Some(active) = recording.as_mut() {
                let changed = active.call_switch.changed();
                let page_stop = active.call_stop.disarm();
                active.call_switch.keep();
                // Kept through, the page's next call never starts on this
                // stop's behalf.
                detector.forget_switched_call();
                if active.auto_stop.disarm() || changed || page_stop {
                    tracing::info!("kept recording after the call ended");
                }
            }
        }
        // Labs › Mute the microphone while recording: it acts on the recording
        // under way, and only while the lab is on.
        ControllerCommand::SetMicrophoneMuted(muted) => {
            if crate::meeting::capture::request_microphone_mute(
                recording.is_some(),
                crate::labs::meeting_microphone_mute(),
                muted,
            ) {
                tracing::info!(muted, "meeting microphone mute set");
            }
        }
        ControllerCommand::Summarize(id) => start_notes(id, events),
        ControllerCommand::OfferTimedOut(id) => detector.offer_timed_out(id),
        ControllerCommand::Dismiss | ControllerCommand::Record(..) => {}
    }
}

/// What a recording starts with, and what the controller watches while it
/// runs.
struct Start {
    /// Its `application` is the call app whose leaving the microphone ends
    /// the call.
    meeting: MeetingStart,
    title: String,
    app_label: String,
    /// The call's own code, when its page named one: the recording watches
    /// that browser's page for another call under Stop when the call
    /// ends. Calendar and manual recordings have none.
    call: Option<String>,
    /// The code of the call whose recording stopped while its page kept
    /// naming it, when this recording was detected in its place: it is kept,
    /// so the page still naming it never ends this one.
    switched_from: Option<String>,
    /// The call app a recording started by hand watches, with its label
    /// (Labs › Stop every call recording when the call ends). Its own title,
    /// app, and capture stay the recording's.
    watched: Option<(String, String)>,
}

impl Start {
    fn from_candidate(candidate: &MeetingCandidate) -> Self {
        // A detected call already knows its app; a recording started by hand
        // watches the one call app on the microphone when it starts.
        let watched = (candidate.source == MeetingSource::Manual)
            .then(call_end::manual_call_app)
            .flatten();
        Self {
            meeting: candidate.recording_start(),
            title: candidate.title.clone(),
            app_label: candidate.source.label().into(),
            call: candidate.call.clone(),
            switched_from: None,
            watched,
        }
    }
}

/// A recording for a calendar meeting: its title, attendees, and call app.
fn calendar_start(meeting: &CalendarMeeting) -> Start {
    let provider = meeting.link.as_ref().map(|link| link.provider);
    Start {
        meeting: MeetingStart {
            title: Some(meeting.display_title()),
            app: provider.map(|provider| provider.label().to_string()),
            application: provider
                .and_then(|provider| provider.application())
                .map(str::to_string),
            calendar: Some(calendar_context(meeting)),
            title_source: meeting::TitleSource::Calendar,
        },
        title: meeting.display_title(),
        app_label: provider
            .map_or("Meeting", |provider| provider.label())
            .into(),
        call: None,
        switched_from: None,
        watched: None,
    }
}

fn calendar_context(meeting: &CalendarMeeting) -> MeetingCalendar {
    MeetingCalendar {
        title: meeting.display_title(),
        attendees: meeting.attendees.clone(),
    }
}

/// What a calendar meeting's answered reminder covers: its call service's
/// detected-call keys until the meeting ends. Without a call link, the next
/// call of any service, but only around the scheduled start, so one
/// dismissal never silences other calls for the rest of a long event.
fn calendar_snooze(meeting: &CalendarMeeting) -> (Vec<String>, i64) {
    match &meeting.link {
        Some(link) => (
            link.provider
                .detection_keys()
                .iter()
                .map(|key| key.to_string())
                .collect(),
            meeting.end_ms,
        ),
        None => (
            Vec::new(),
            (meeting.start_ms + crate::calendar::LATE_GRACE_MS).min(meeting.end_ms),
        ),
    }
}

fn snooze_calendar_call(detector: &mut MeetingDetector, meeting: &CalendarMeeting) {
    let (keys, until_ms) = calendar_snooze(meeting);
    detector.snooze_calendar_call(keys, until_ms);
}

/// How often the reminder poll rereads the calendar without a change
/// notification.
const CALENDAR_REREAD_MS: i64 = 60_000;

/// Calendar reminders on the controller thread: a bounded EventKit read on
/// each store change or once a minute, and a cheap scheduler pass on every
/// loop. No network; never on the audio or UI thread.
#[derive(Default)]
struct Reminders {
    scheduler: ReminderScheduler,
    meetings: Vec<CalendarMeeting>,
    change_count: Option<u64>,
    /// Wall-clock time of the last read, so a wake from sleep rereads.
    read_at_ms: Option<i64>,
    /// The last read ran only for the menu snapshot, not for reminders: when
    /// reminders come back they read at once instead of reusing it.
    menu_only_read: bool,
    /// The menu has read since the lab came on: its first poll reads at once
    /// instead of waiting out the minute.
    menu_read: bool,
    read_failed: bool,
}

/// Where a successful calendar read goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadSources {
    /// The list reminders schedule from and detected calls are titled by:
    /// only while reminders are active.
    meetings: bool,
    /// Labs › Upcoming meetings in menu bar's own snapshot.
    menu: bool,
}

impl ReadSources {
    fn new(reminders: bool, menu: bool) -> Self {
        Self {
            meetings: reminders,
            menu,
        }
    }
}

impl Reminders {
    /// Forces the extra reads a poll needs beyond a stale read or a store
    /// change: the menu's first read after the lab comes on, and reminders'
    /// first read after a menu-only one left their list empty.
    fn prepare_read(&mut self, reminders: bool, menu: bool) {
        if !menu {
            self.menu_read = false;
        } else if !self.menu_read {
            self.read_at_ms = None;
        }
        if reminders && self.menu_only_read {
            self.read_at_ms = None;
            self.menu_only_read = false;
        }
    }

    fn poll(&mut self, enabled: bool, recording: bool, events: &SyncSender<ControllerEvent>) {
        let settings = crate::app_settings::calendar_settings();
        // Labs › Upcoming meetings in menu bar lists the same read with
        // reminders off, but only while a calendar is connected; while it is
        // off nothing here changes at all.
        let menu_on = crate::calendar_menu::lab_on();
        let menu = crate::calendar_menu::wanted(settings.connected);
        let reminders = settings.reminders_active();
        let reads = ReadSources::new(reminders, menu);
        if !enabled || !(reminders || menu) {
            self.meetings.clear();
            self.change_count = None;
            self.read_at_ms = None;
            if menu_on {
                crate::calendar_menu::clear();
            }
            self.menu_only_read = false;
            self.menu_read = false;
            if let Some(occurrence) = self.scheduler.clear() {
                let _ = events.send(ControllerEvent::ReminderWithdrawn { occurrence });
            }
            return;
        }
        if !reminders {
            // The menu's list is the read's own: with reminders off the read
            // never feeds the list recordings are titled and reminded from.
            self.meetings.clear();
            if let Some(occurrence) = self.scheduler.clear() {
                let _ = events.send(ControllerEvent::ReminderWithdrawn { occurrence });
            }
        }
        let now_ms = crate::calendar::wall_clock_ms();
        let changes = crate::calendar::change_count();
        self.prepare_read(reminders, menu);
        let stale = self
            .read_at_ms
            .is_none_or(|read_at| (now_ms - read_at).abs() >= CALENDAR_REREAD_MS);
        if stale || self.change_count != Some(changes) {
            self.change_count = Some(changes);
            self.read_at_ms = Some(now_ms);
            match crate::calendar::read_meetings(&settings, now_ms) {
                Ok(meetings) => {
                    self.read_failed = false;
                    if reads.menu {
                        crate::calendar_menu::publish(Some(&meetings), now_ms);
                    }
                    if reads.meetings {
                        self.meetings = meetings;
                    }
                }
                Err(error) => {
                    if !self.read_failed {
                        tracing::warn!(%error, "could not read calendar meetings");
                    }
                    self.read_failed = true;
                    self.meetings.clear();
                    if reads.menu {
                        crate::calendar_menu::clear();
                    }
                }
            }
            self.menu_only_read = !reads.meetings;
        }
        if menu {
            self.menu_read = true;
        }
        if !reminders {
            return;
        }
        let lead_ms = settings.lead().as_millis() as i64;
        match self.step(recording, now_ms, lead_ms) {
            Some(ReminderUpdate::Show(meeting)) => {
                let automatic = records_automatically(
                    &meeting,
                    &crate::app_settings::meeting_provider_choices(),
                    crate::app_settings::meeting_default_policy(),
                );
                tracing::info!(
                    provider = meeting.link.as_ref().map(|link| link.provider.label()),
                    automatic,
                    "calendar meeting reminder shown"
                );
                let _ = events.send(ControllerEvent::Reminder { meeting, automatic });
            }
            Some(ReminderUpdate::Withdraw(occurrence)) => {
                let _ = events.send(ControllerEvent::ReminderWithdrawn { occurrence });
            }
            None => {}
        }
    }

    /// The scheduler pass over the meetings last read. A reminder waits
    /// while Hex records; it shows after, if still due.
    fn step(&mut self, recording: bool, now_ms: i64, lead_ms: i64) -> Option<ReminderUpdate> {
        if recording {
            return None;
        }
        self.scheduler.poll(&self.meetings, now_ms, lead_ms)
    }

    /// A detected call's recording, titled from the calendar meeting it
    /// belongs to when there is one.
    fn start_for(&self, candidate: &MeetingCandidate) -> Start {
        let mut start = Start::from_candidate(candidate);
        let keys = [candidate.site.as_deref(), candidate.application.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let lead_ms = crate::app_settings::calendar_settings().lead().as_millis() as i64;
        if let Some(meeting) = crate::calendar::current_for_call(
            &self.meetings,
            &keys,
            crate::calendar::wall_clock_ms(),
            lead_ms,
        ) {
            start.meeting.calendar = Some(calendar_context(meeting));
            // A title the user gave (`hex meetings record --title`) wins.
            if !(candidate.named
                && candidate.source == crate::meeting::detection::MeetingSource::Manual)
            {
                start.meeting.title = Some(meeting.display_title());
                start.meeting.title_source = meeting::TitleSource::Calendar;
                start.title = meeting.display_title();
            }
        }
        start
    }
}

/// Whether the meeting's call service records automatically, by its own
/// answer or `default`, so its call records once detected.
fn records_automatically(
    meeting: &CalendarMeeting,
    choices: &[crate::app_settings::MeetingProviderChoice],
    default: crate::app_settings::MeetingPolicy,
) -> bool {
    meeting
        .link
        .as_ref()
        .and_then(|link| link.provider.choice_key())
        .is_some_and(|key| {
            choices
                .iter()
                .find(|choice| choice.key == key)
                .map_or(default, |choice| choice.policy)
                == crate::app_settings::MeetingPolicy::AlwaysRecord
        })
}

fn start_recording(
    recording: &mut Option<ActiveRecording>,
    reminders: &mut Reminders,
    start: Start,
    automatic: bool,
    offer: Option<OfferId>,
    events: &SyncSender<ControllerEvent>,
) {
    // `Starting` closes a showing reminder; no reminder shows over a
    // recording. Every recording begins with the microphone in.
    reminders.scheduler.clear();
    crate::meeting::capture::set_microphone_muted(false);
    if automatic {
        tracing::info!(
            source = start.app_label,
            title = start.title,
            "meeting recording started automatically"
        );
    }
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    // Stop can send six updates before the controller next drains them.
    let (update_sender, updates) = mpsc::sync_channel(16);
    // A recording started by hand watches the call app it found on the
    // microphone instead of one it detected.
    let (application, app_label) = match start.watched {
        Some((application, label)) => (Some(application), label),
        None => (start.meeting.application.clone(), start.app_label.clone()),
    };
    let call_switch = CallSwitch::with_kept(start.call, start.switched_from);
    let meeting_start = start.meeting;
    let worker =
        thread::spawn(move || meeting::record(meeting_start, &worker_stop, Some(update_sender)));
    let _ = events.send(ControllerEvent::Starting(offer));
    *recording = Some(ActiveRecording {
        title: start.title,
        application,
        app_label,
        automatic,
        presence: CallPresence::default(),
        watch: CallWatch::default(),
        auto_stop: AutoStop::default(),
        call_switch,
        call_stop: AutoStop::default(),
        stop,
        updates,
        reported_started: false,
        playback_locked_noted: false,
        worker,
    });
}

fn poll_recording(
    recording: &mut Option<ActiveRecording>,
    detector: &mut MeetingDetector,
    browser: &mut BrowserWatch,
    events: &SyncSender<ControllerEvent>,
) -> Result<()> {
    if let Some(active) = recording.as_mut() {
        for update in active.updates.try_iter() {
            match update {
                meeting::RecordingUpdate::Started { id, title } => {
                    active.reported_started = true;
                    active.title = title.clone();
                    if !active.stop.load(Ordering::Relaxed) {
                        let _ = events.send(ControllerEvent::RecordingStarted {
                            id,
                            title,
                            automatic: active.automatic,
                        });
                    }
                }
                meeting::RecordingUpdate::Retitled { title } => {
                    active.title = title.clone();
                    let _ = events.send(ControllerEvent::Retitled { title });
                }
                meeting::RecordingUpdate::Chapters(chapters) => {
                    let _ = events.send(ControllerEvent::Chapters(chapters));
                }
                meeting::RecordingUpdate::SourceWarning { warning } => {
                    let _ = events.send(ControllerEvent::SourceWarning { warning });
                }
                meeting::RecordingUpdate::Transcribing => {
                    active.end(detector, events);
                    let _ = events.send(ControllerEvent::Transcribing);
                }
            }
        }
    }

    if recording
        .as_ref()
        .is_some_and(|active| active.worker.is_finished())
    {
        let active = recording
            .take()
            .ok_or_else(|| eyre!("recording disappeared"))?;
        // Labs › Mute the microphone while recording ends with its recording.
        crate::meeting::capture::set_microphone_muted(false);
        match active
            .worker
            .join()
            .unwrap_or_else(|_| Err(eyre!("meeting recorder panicked")))
        {
            Ok(manifest) => publication::finished(manifest, events),
            Err(error) => {
                let _ = events.send(ControllerEvent::Failed {
                    title: active.title,
                    error: error.to_string(),
                });
            }
        }
    }

    // Stopped and transcribing: detection waits, but whether the call app
    // left the microphone meanwhile still decides when its snooze ends.
    if let Some(active) = recording.as_ref()
        && active.stop.load(Ordering::Relaxed)
        && let Some(application) = active.application.as_deref()
    {
        match active_microphone_applications() {
            Ok(applications) => detector.observe_call_app(
                applications
                    .iter()
                    .any(|candidate| candidate.canonical_bundle_id() == Some(application)),
            ),
            Err(error) => {
                tracing::debug!(%error, "could not inspect microphone activity while transcribing");
            }
        }
    }

    if let Some(active) = recording.as_mut()
        && active.reported_started
        && !active.stop.load(Ordering::Relaxed)
        && let Some(application) = active.application.as_deref()
    {
        // A failed query must not end the controller while it owns a recording.
        let start = CallEndStop::of(active.automatic, active.application.as_deref());
        let auto_stop = stops_when_call_ends(
            start,
            crate::app_settings::meeting_stop_when_call_ends(),
            crate::labs::stop_any_call_recording(),
        );
        // A browser seems to keep its output open after its call while the
        // Mac is locked (#2685): playback stops counting there, and one note
        // per recording says so, so the next report shows whether that is
        // what happened.
        let locked = crate::audio::system_idle::session_locked();
        if !locked {
            active.playback_locked_noted = false;
        } else if auto_stop
            && !active.playback_locked_noted
            && crate::microphone_activity::is_browser_id(Some(application))
        {
            active.playback_locked_noted = true;
            tracing::info!(
                application,
                "the Mac is locked; the browser's playback no longer keeps its call present"
            );
        }
        let applications = active_microphone_applications();
        let pass = call_end::watched_pass(
            application,
            locked,
            auto_stop,
            &applications,
            call_app_playing,
        );
        if pass.present {
            detector.keep_snooze_alive();
        }
        // The call came back (or its setting went off mid-count): the stop it
        // armed is gone, and its notice goes with the last one counting.
        if (pass.present || !auto_stop)
            && withdraw_stop(&mut active.auto_stop, &active.call_stop, events)
        {
            tracing::info!(application, "the pending automatic stop is withdrawn");
        }
        // A recording started on request counts no absence before its app has
        // been on the microphone: a calendar reminder records before the call
        // is answered, and stopping it there would end a call it never saw.
        if active
            .watch
            .counts_absence(start, auto_stop, pass.on_microphone)
            && active.presence.observe(pass.present)
        {
            tracing::info!(
                application,
                auto_stop,
                "meeting application released the microphone"
            );
            let stops_at = auto_stop.then(|| active.auto_stop.arm());
            let _ = events.send(ControllerEvent::CallEnded {
                app: active.app_label.clone(),
                application: call_ended_icon_application(active.application.as_deref()),
                stops_at,
            });
        }
        if active.auto_stop.due() {
            tracing::info!(
                application,
                "meeting recording stopped automatically after its call ended"
            );
            active.end(detector, events);
        }

        // Stop when the call ends, in a browser that never releases
        // the microphone between calls: the page naming another call ends
        // this one (#1775). Only a positive read of a different code counts;
        // a tab naming no call never does.
        if watching_call_switch(
            Some(active),
            crate::app_settings::meeting_stop_when_call_ends(),
        ) {
            let microphone = applications.as_ref().ok().and_then(|applications| {
                applications
                    .iter()
                    .find(|candidate| candidate.canonical_bundle_id() == Some(application))
            });
            if let Some(microphone) = microphone {
                let context = browser.recording_context(microphone);
                let candidate = candidate_from_microphone(microphone, &context);
                let current = candidate
                    .as_ref()
                    .and_then(|candidate| candidate.call.clone());
                if active.call_switch.observe(current.as_deref()) {
                    tracing::info!(
                        application,
                        from = active.call_switch.recorded().unwrap_or_default(),
                        to = current.as_deref().unwrap_or_default(),
                        "the page named a different call; the recording stops after its grace unless kept"
                    );
                    // The browser stays on the microphone and its next reads
                    // can still name the ended call (or none): remember the
                    // call the page named, so detection starts that one.
                    if let Some(candidate) = candidate.filter(|candidate| candidate.call.is_some())
                    {
                        detector.call_switched(
                            active.call_switch.recorded().map(str::to_owned),
                            candidate,
                        );
                    }
                    let stops_at = active.call_stop.arm();
                    let _ = events.send(ControllerEvent::CallEnded {
                        app: active.app_label.clone(),
                        application: call_ended_icon_application(active.application.as_deref()),
                        stops_at: Some(stops_at),
                    });
                }
                // The page names the recorded call again (or one the user
                // kept): whatever change was counting is over. A page that
                // names no call neither starts nor ends a count.
                withdraw_call_switch_stop(
                    &active.call_switch,
                    &mut active.call_stop,
                    &active.auto_stop,
                    detector,
                    events,
                );
            }
            if active.call_stop.due() {
                tracing::info!(
                    application,
                    "meeting recording stopped automatically after its page named another call"
                );
                active.end(detector, events);
            }
        }
    }

    // Stop when the call ends turned off while a page change was counting:
    // the deadline goes rather than stopping the recording when it comes back.
    let stop_on_end = crate::app_settings::meeting_stop_when_call_ends();
    disarm_call_stop_unless_watching(recording, stop_on_end, detector, events);
    Ok(())
}

/// The native call app's ID for the call-ended capsule's icon; browsers
/// return `None` so web calls show their service mark instead of the browser's.
fn call_ended_icon_application(application: Option<&str>) -> Option<String> {
    application
        .filter(|id| !crate::microphone_activity::is_browser_id(Some(id)))
        .map(str::to_owned)
}

/// Drops an armed stop whose call came back or whose setting went off (both share one notice).
fn withdraw_stop(
    stop: &mut AutoStop,
    other: &AutoStop,
    events: &SyncSender<ControllerEvent>,
) -> bool {
    if !stop.disarm() {
        return false;
    }
    if !other.armed() {
        let _ = events.send(ControllerEvent::CallEndedWithdrawn);
    }
    true
}

/// Whether the call app still plays audio; a failed read counts as playing,
/// so it never stops a recording.
fn call_app_playing(application: &str) -> bool {
    crate::microphone_activity::application_running_output(application).unwrap_or_else(|error| {
        tracing::warn!(%error, "could not inspect the call app's audio output");
        true
    })
}

/// How often a browser's tabs are queried while it has just started holding
/// the microphone: enough to find a call's site within the 20 s unknown-page
/// threshold (`detection::BROWSER_STABLE_FOR`).
const TAB_QUERY_INTERVAL: Duration = Duration::from_secs(1);
/// How long that one-second burst lasts after a browser starts holding the
/// microphone.
const TAB_QUERY_BURST: Duration = Duration::from_secs(30);
/// How often they are queried after the burst; `BrowserPageMemory` (60 s)
/// carries the page between answers.
const TAB_QUERY_BACKOFF: Duration = Duration::from_secs(30);
/// How often they are queried while a recording watches its page for another
/// call (Stop when the call ends): a switched call must be named well
/// within `detection::CALL_CHANGED_AFTER`, and no answer older than
/// [`TAB_QUERY_MAX_AGE`] says what the browser shows.
const TAB_QUERY_WATCH: Duration = Duration::from_secs(2);
/// A tab query that finished longer ago than this no longer says what the
/// browser shows.
const TAB_QUERY_MAX_AGE: Duration = Duration::from_secs(5);

type TabQuery = fn(&str) -> Result<ContextSnapshot>;

/// A page and when its query finished.
type Answer = Option<(ContextSnapshot, Instant)>;

struct InFlight {
    bundle_id: String,
    /// The probe's generation when the query started; a reset since makes
    /// the answer stale.
    generation: u64,
    answer: Receiver<Answer>,
}

/// Reads a browser's tabs off the controller thread, one query at a time. A
/// query is an AppleScript round trip that can take seconds (it is killed
/// after its own timeout), so the loop only starts one and reads the newest
/// finished answer; it never waits for one.
struct TabProbe {
    query: TabQuery,
    running: Option<InFlight>,
    generation: u64,
    started: Option<Instant>,
    /// The browser this watch began for, and when: its first 30 s ask every
    /// second, then every 30 s.
    burst: Option<(String, Instant)>,
    latest: Option<(String, ContextSnapshot, Instant)>,
}

impl TabProbe {
    fn new(query: TabQuery) -> Self {
        Self {
            query,
            running: None,
            generation: 0,
            started: None,
            burst: None,
            latest: None,
        }
    }

    /// The browser's newest answered page, or nothing yet. When `watching` is
    /// true (a recording watches its page for another call under Stop when the
    /// call ends), queries run every [`TAB_QUERY_WATCH`] instead of backing off.
    fn page(&mut self, bundle_id: &str, now: Instant, watching: bool) -> ContextSnapshot {
        let received = self
            .running
            .as_ref()
            .map(|flight| (flight.generation, flight.answer.try_recv()));
        match received {
            Some((generation, Ok(answer))) => {
                let queried = self.running.take().map(|flight| flight.bundle_id);
                if generation == self.generation
                    && let Some(queried) = queried
                {
                    self.latest = answer.map(|(page, at)| (queried, page, at));
                }
            }
            Some((_, Err(mpsc::TryRecvError::Disconnected))) => self.running = None,
            Some((_, Err(mpsc::TryRecvError::Empty))) | None => {}
        }
        let interval = self.interval(bundle_id, now, watching);
        if self.running.is_none()
            && self
                .started
                .is_none_or(|started| now.saturating_duration_since(started) >= interval)
        {
            self.started = Some(now);
            let (sender, receiver) = mpsc::sync_channel(1);
            let query = self.query;
            let queried = bundle_id.to_string();
            let spawned = thread::Builder::new()
                .name("meeting-browser-tabs".into())
                .spawn(move || {
                    let answer = query(&queried)
                        .map_err(
                            |error| tracing::debug!(%error, "could not read the browser's tabs"),
                        )
                        .ok()
                        .map(|page| (page, Instant::now()));
                    let _ = sender.send(answer);
                });
            if spawned.is_ok() {
                self.running = Some(InFlight {
                    bundle_id: bundle_id.to_string(),
                    generation: self.generation,
                    answer: receiver,
                });
            }
        }
        match &self.latest {
            Some((bundle, page, at))
                if bundle == bundle_id
                    && now.saturating_duration_since(*at) <= TAB_QUERY_MAX_AGE =>
            {
                page.clone()
            }
            _ => ContextSnapshot::default(),
        }
    }

    /// Nothing is watched: an answer, finished or still being read, must not
    /// outlive the call. A query in flight is not abandoned (it ends by its
    /// own timeout, and no second one starts beside it); its answer is
    /// dropped. The next call starts with a fresh one-second burst.
    fn reset(&mut self) {
        self.latest = None;
        self.burst = None;
        self.generation += 1;
    }

    /// How far apart this browser's tab queries are: every two seconds while
    /// a recording watches its page for another call, else one second through
    /// the first 30 s it holds the microphone, then every 30 s.
    fn interval(&mut self, bundle_id: &str, now: Instant, watching: bool) -> Duration {
        if watching {
            return TAB_QUERY_WATCH;
        }
        let bursting = match &self.burst {
            Some((watching, since)) if watching.as_str() == bundle_id => {
                now.saturating_duration_since(*since) < TAB_QUERY_BURST
            }
            _ => true,
        };
        if self
            .burst
            .as_ref()
            .is_none_or(|(watching, _)| watching.as_str() != bundle_id)
        {
            self.burst = Some((bundle_id.to_string(), now));
        }
        if bursting {
            TAB_QUERY_INTERVAL
        } else {
            TAB_QUERY_BACKOFF
        }
    }
}

/// What the browser holding the microphone shows: the newest tab query, else
/// a page it showed a moment ago, so a call's site survives a slow or failed
/// query and a window in front of the browser.
struct BrowserWatch {
    probe: TabProbe,
    memory: BrowserPageMemory,
}

impl BrowserWatch {
    fn new(query: TabQuery) -> Self {
        Self {
            probe: TabProbe::new(query),
            memory: BrowserPageMemory::default(),
        }
    }

    fn reset(&mut self) {
        self.probe.reset();
        self.memory.forget();
    }

    fn context(
        &mut self,
        application: Option<&ActiveMicrophoneApplication>,
        snoozed: impl Fn(&str) -> bool,
    ) -> ContextSnapshot {
        let browser = application.filter(|application| application.is_browser());
        let Some((application, bundle_id)) =
            browser.and_then(|application| Some((application, application.canonical_bundle_id()?)))
        else {
            self.reset();
            return ContextSnapshot::default();
        };
        if snoozed(bundle_id) {
            self.reset();
            return ContextSnapshot::default();
        }
        let now = Instant::now();
        let page = self.probe.page(bundle_id, now, false);
        self.memory.recall(application, page, now)
    }

    /// The page while a recording watches it for another call (Stop
    /// when the call ends): read every couple of seconds rather than backing
    /// off, so a call switched to in the same browser is named. It is the
    /// probe's newest answer, never `BrowserPageMemory`'s: a page seen once
    /// must not name the call for 60 s while the reads fail.
    fn recording_context(&mut self, application: &ActiveMicrophoneApplication) -> ContextSnapshot {
        if !application.is_browser() {
            return ContextSnapshot::default();
        }
        let Some(bundle_id) = application.canonical_bundle_id() else {
            return ContextSnapshot::default();
        };
        self.probe.page(bundle_id, Instant::now(), true)
    }
}

/// One detection pass; `Err` when microphone activity could not be read.
fn detect_meeting(
    detector: &mut MeetingDetector,
    observed_microphone_apps: &mut Vec<(i32, String)>,
    browser: &mut BrowserWatch,
    seen_calls: &mut crate::meeting::seen_calls::SeenCalls,
    choices: &[crate::app_settings::MeetingProviderChoice],
    default: crate::app_settings::MeetingPolicy,
) -> Result<Option<Detection>, ()> {
    let microphone_apps = match active_microphone_applications() {
        Ok(applications) => applications,
        Err(error) => {
            tracing::warn!(%error, "could not inspect microphone activity");
            return Err(());
        }
    };
    let observed = microphone_apps
        .iter()
        .map(|application| (application.pid, application.bundle_id.clone()))
        .collect::<Vec<_>>();
    if observed != *observed_microphone_apps {
        let applications = microphone_apps
            .iter()
            .map(|application| {
                format!(
                    "{} ({}, pid {})",
                    application.name, application.bundle_id, application.pid
                )
            })
            .collect::<Vec<_>>();
        tracing::info!(?applications, "microphone application activity changed");
        *observed_microphone_apps = observed;
    }
    let application = microphone_apps
        .into_iter()
        .filter(|application| application.is_supported_meeting_app())
        .min_by_key(|application| application.detection_priority());
    let context = browser.context(application.as_ref(), |bundle_id| {
        detector.is_application_snoozed(bundle_id)
    });
    let candidate =
        application.and_then(|application| candidate_from_microphone(&application, &context));
    // A Don't offer call shows nothing, but Settings still lists where it was.
    let unoffered = candidate
        .as_ref()
        .filter(|candidate| {
            candidate.policy(choices, default) == crate::app_settings::MeetingPolicy::NeverOffer
        })
        .cloned();
    let detection = detector.detect(candidate, choices, default);
    if let Some(Detection::Offer(seen) | Detection::Record(seen)) = &detection {
        seen_calls.note(seen, crate::calendar::wall_clock_ms());
    } else if let Some(seen) = &unoffered {
        seen_calls.note(seen, crate::calendar::wall_clock_ms());
    }
    if let Some(Detection::Offer(candidate)) = &detection {
        tracing::info!(
            source = candidate.source.label(),
            title = candidate.title,
            "meeting recording offer triggered"
        );
    }
    Ok(detection)
}

/// Writes notes on a background thread; the UI refreshes from the manifest
/// whether generation succeeds or fails.
pub(super) fn start_notes(id: String, events: &SyncSender<ControllerEvent>) {
    let finished = events.clone();
    let pending = id.clone();
    if meeting::spawn_summary(id.clone(), move |result| {
        let _ = finished.send(match result {
            Ok(manifest) => ControllerEvent::NotesFinished(Box::new(manifest)),
            Err(_) => ControllerEvent::NotesProgress(pending),
        });
    }) {
        let _ = events.send(ControllerEvent::NotesProgress(id));
    }
}

#[cfg(debug_assertions)]
pub fn probe() -> Result<()> {
    for application in active_microphone_applications()? {
        println!(
            "{}\t{}\t{}\t{}",
            application.pid,
            if application.is_supported_meeting_app() {
                "meeting"
            } else {
                "other"
            },
            application.bundle_id,
            application.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reminders_keep_the_titling_and_reminder_list() {
        let menu_only = ReadSources::new(false, true);
        assert!(!menu_only.meetings);
        assert!(menu_only.menu);
        // Reminders keep the read they always had.
        let reminders = ReadSources::new(true, false);
        assert!(reminders.meetings);
        assert!(!reminders.menu);
        let both = ReadSources::new(true, true);
        assert!(both.meetings);
        assert!(both.menu);
    }

    #[test]
    fn a_menu_only_read_and_a_new_lab_both_force_an_immediate_read() {
        // Reminders coming back after a menu-only read must not reuse it:
        // their titling and reminder list is empty.
        let mut reminders = Reminders {
            read_at_ms: Some(1_000),
            menu_only_read: true,
            ..Reminders::default()
        };
        reminders.prepare_read(true, true);
        assert_eq!(reminders.read_at_ms, None);
        assert!(!reminders.menu_only_read);

        // The menu's first poll after the lab comes on reads at once too.
        let mut reminders = Reminders {
            read_at_ms: Some(1_000),
            ..Reminders::default()
        };
        reminders.prepare_read(true, true);
        assert_eq!(reminders.read_at_ms, None);

        // Once the menu has read, a fresh read is kept.
        reminders.menu_read = true;
        reminders.read_at_ms = Some(2_000);
        reminders.prepare_read(true, true);
        assert_eq!(reminders.read_at_ms, Some(2_000));

        // With the menu off nothing is forced, and its next turn reads.
        reminders.prepare_read(true, false);
        assert_eq!(reminders.read_at_ms, Some(2_000));
        assert!(!reminders.menu_read);
        reminders.prepare_read(true, true);
        assert_eq!(reminders.read_at_ms, None);
    }

    #[test]
    fn joining_from_the_menu_withdraws_its_reminder_and_starts_nothing() {
        let (events, received) = mpsc::sync_channel(4);
        let mut reminders = Reminders::default();
        let meeting = meeting("sync", 100, 30);
        assert!(matches!(
            reminders
                .scheduler
                .poll(std::slice::from_ref(&meeting), 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(_))
        ));
        let mut recording = None;
        handle_command(
            &mut recording,
            &mut None,
            &mut MeetingDetector::default(),
            &mut reminders,
            ControllerCommand::JoinFromMenu(meeting.clone()),
            true,
            &events,
        );
        assert!(recording.is_none(), "joining from the menu never records");
        assert!(matches!(
            received.try_recv(),
            Ok(ControllerEvent::ReminderWithdrawn { occurrence })
                if occurrence == meeting.occurrence()
        ));
        // It was recorded as reminded, so it never pops later.
        assert_eq!(
            reminders
                .scheduler
                .poll(std::slice::from_ref(&meeting), 100 * MINUTE, MINUTE),
            None
        );
    }

    #[test]
    fn only_automatic_recordings_stop_when_the_call_ends_and_only_with_the_lab_on() {
        use CallEndStop::*;
        assert!(stops_when_call_ends(Automatic, true, false));
        // Manual, offered, and calendar recordings never stop on their own...
        assert!(!stops_when_call_ends(OnRequest, true, false));
        // ...until Labs › Stop every call recording when the call ends, which
        // leaves Always record to its own setting.
        assert!(stops_when_call_ends(OnRequest, true, true));
        assert!(stops_when_call_ends(OnRequest, false, true));
        assert!(!stops_when_call_ends(Automatic, false, true));
        // A by-hand recording of no call has nothing to watch, ever.
        assert!(!stops_when_call_ends(Unwatched, false, true));
        // The toggle off leaves every recording running until Stop.
        assert!(!stops_when_call_ends(Automatic, false, false));
        assert!(!stops_when_call_ends(OnRequest, false, false));
    }

    #[test]
    fn keep_recording_disarms_a_pending_automatic_stop() {
        let (events, _received) = mpsc::sync_channel(4);
        let (_update_sender, updates) = mpsc::sync_channel(1);
        let mut recording = Some(ActiveRecording {
            title: "WhatsApp call".into(),
            application: Some("net.whatsapp.WhatsApp".into()),
            app_label: "WhatsApp".into(),
            automatic: true,
            presence: CallPresence::default(),
            watch: CallWatch::default(),
            auto_stop: AutoStop::default(),
            call_switch: CallSwitch::new(None),
            call_stop: AutoStop::default(),
            stop: Arc::new(AtomicBool::new(false)),
            updates,
            reported_started: true,
            playback_locked_noted: false,
            worker: thread::spawn(|| Err(eyre!("not recording"))),
        });
        if let Some(active) = recording.as_mut() {
            active.auto_stop.arm();
        }
        handle_command(
            &mut recording,
            &mut None,
            &mut MeetingDetector::default(),
            &mut Reminders::default(),
            ControllerCommand::KeepRecording,
            true,
            &events,
        );
        let active = recording.as_mut().unwrap();
        assert!(!active.auto_stop.disarm(), "nothing is left to stop");
        assert!(!active.stop.load(Ordering::Relaxed));
    }

    /// A recording as `start_recording` builds one, for the checks that only
    /// read its state.
    fn active_recording(
        application: Option<&str>,
        automatic: bool,
        call: Option<&str>,
    ) -> ActiveRecording {
        let (_update_sender, updates) = mpsc::sync_channel(1);
        ActiveRecording {
            title: "Meet - Design sync".into(),
            application: application.map(str::to_string),
            app_label: "Google Meet".into(),
            automatic,
            presence: CallPresence::default(),
            watch: CallWatch::default(),
            auto_stop: AutoStop::default(),
            call_switch: CallSwitch::new(call.map(str::to_string)),
            call_stop: AutoStop::default(),
            stop: Arc::new(AtomicBool::new(false)),
            updates,
            reported_started: true,
            playback_locked_noted: false,
            worker: thread::spawn(|| Err(eyre!("not recording"))),
        }
    }

    #[test]
    fn stop_request_is_not_a_capture_stopped_acknowledgment() {
        let (events, read) = mpsc::sync_channel(4);
        let mut active = active_recording(None, false, None);
        let (updates, worker) = mpsc::sync_channel(1);
        let (finish, wait) = mpsc::channel();
        active.worker = thread::spawn(move || {
            wait.recv().unwrap();
            Err(eyre!("fixture"))
        });
        active.updates = worker;
        let mut detector = MeetingDetector::default();
        active.end(&mut detector, &events);
        active.end(&mut detector, &events);
        assert!(active.stop.load(Ordering::Relaxed));
        assert!(matches!(read.try_recv(), Ok(ControllerEvent::Stopping)));
        assert!(read.try_recv().is_err());
        updates
            .send(meeting::RecordingUpdate::Transcribing)
            .unwrap();
        let mut active = Some(active);
        let mut browser = BrowserWatch::new(ContextSnapshot::capture_for_meeting);
        poll_recording(&mut active, &mut detector, &mut browser, &events).unwrap();
        assert!(matches!(read.try_recv(), Ok(ControllerEvent::Transcribing)));
        assert!(read.try_recv().is_err());
        finish.send(()).unwrap();
        assert!(active.unwrap().worker.join().unwrap().is_err());
    }

    #[test]
    fn keep_recording_disarms_the_call_switch_stop_too() {
        let (events, _received) = mpsc::sync_channel(4);
        let mut active = active_recording(
            Some("company.thebrowser.Browser"),
            true,
            Some("abc-defg-hij"),
        );
        active.auto_stop.arm();
        active.call_stop.arm();
        let mut recording = Some(active);
        handle_command(
            &mut recording,
            &mut None,
            &mut MeetingDetector::default(),
            &mut Reminders::default(),
            ControllerCommand::KeepRecording,
            true,
            &events,
        );
        let active = recording.as_mut().unwrap();
        assert!(!active.auto_stop.disarm(), "the absence stop was dropped");
        assert!(
            !active.call_stop.disarm(),
            "the call switch stop was dropped"
        );
        assert!(!active.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn keep_recording_keeps_the_code_that_was_counting() {
        use crate::meeting::detection::CALL_CHANGED_AFTER;
        let (events, _received) = mpsc::sync_channel(4);
        let mut active = active_recording(
            Some("company.thebrowser.Browser"),
            true,
            Some("abc-defg-hij"),
        );
        // Another code has shown long enough to be a different call.
        let base = Instant::now();
        assert!(!active.call_switch.observe_at(Some("xyz-wxyz-xyz"), base));
        assert!(
            active
                .call_switch
                .observe_at(Some("xyz-wxyz-xyz"), base + CALL_CHANGED_AFTER)
        );
        assert!(active.call_switch.changed());
        active.call_stop.arm();
        let mut recording = Some(active);
        handle_command(
            &mut recording,
            &mut None,
            &mut MeetingDetector::default(),
            &mut Reminders::default(),
            ControllerCommand::KeepRecording,
            true,
            &events,
        );
        let active = recording.as_mut().unwrap();
        assert!(!active.call_switch.changed());
        assert!(!active.call_stop.disarm());
        // Keeping again with nothing counting never forgets the kept code.
        handle_command(
            &mut recording,
            &mut None,
            &mut MeetingDetector::default(),
            &mut Reminders::default(),
            ControllerCommand::KeepRecording,
            true,
            &events,
        );
        let active = recording.as_mut().unwrap();
        let later = base + Duration::from_secs(120);
        assert!(!active.call_switch.observe_at(Some("xyz-wxyz-xyz"), later));
        assert!(
            !active
                .call_switch
                .observe_at(Some("xyz-wxyz-xyz"), later + CALL_CHANGED_AFTER),
            "the kept code ended the recording anyway"
        );
        assert!(!active.call_switch.changed());
        assert!(!active.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn only_an_automatic_browser_call_with_a_code_keeps_reading_its_page() {
        let arc = Some("company.thebrowser.Browser");
        assert!(watching_call_switch(
            Some(&active_recording(arc, true, Some("abc-defg-hij"))),
            true
        ));
        // Stop when the call ends off: the page is dropped as before.
        assert!(!watching_call_switch(
            Some(&active_recording(arc, true, Some("abc-defg-hij"))),
            false
        ));
        // Manual/offered recordings never watch.
        assert!(!watching_call_switch(
            Some(&active_recording(arc, false, Some("abc-defg-hij"))),
            true
        ));
        // A page that named no code has nothing to compare.
        assert!(!watching_call_switch(
            Some(&active_recording(arc, true, None)),
            true
        ));
        // A call app is ended by its microphone, not by a page.
        assert!(!watching_call_switch(
            Some(&active_recording(
                Some("us.zoom.xos"),
                true,
                Some("abc-defg-hij")
            )),
            true
        ));
        // Calendar and manual recordings have no application.
        assert!(!watching_call_switch(
            Some(&active_recording(None, true, Some("abc-defg-hij"))),
            true
        ));
        assert!(!watching_call_switch(None, true));
        // Once told to stop (transcribing), the page is dropped and detection
        // resumes with a fresh burst.
        let stopped = active_recording(arc, true, Some("abc-defg-hij"));
        stopped.stop.store(true, Ordering::Relaxed);
        assert!(!watching_call_switch(Some(&stopped), true));
    }

    #[test]
    fn turning_the_lab_off_drops_an_armed_call_switch_stop() {
        let (events, received) = mpsc::sync_channel(4);
        let arc = Some("company.thebrowser.Browser");
        let mut active = active_recording(arc, true, Some("abc-defg-hij"));
        active.call_stop.arm();
        let mut recording = Some(active);
        disarm_call_stop_unless_watching(
            &mut recording,
            false,
            &mut MeetingDetector::default(),
            &events,
        );
        let active = recording.as_mut().unwrap();
        assert!(!active.call_stop.disarm(), "the armed stop was kept");
        assert!(!active.stop.load(Ordering::Relaxed));
        // Its notice would otherwise claim a stop nothing is counting.
        assert!(matches!(
            received.try_recv(),
            Ok(ControllerEvent::CallEndedWithdrawn)
        ));
        assert!(received.try_recv().is_err(), "one withdrawal only");

        // While its setting is on and the page watched, the deadline stays.
        let mut active = active_recording(arc, true, Some("abc-defg-hij"));
        active.call_stop.arm();
        let mut recording = Some(active);
        disarm_call_stop_unless_watching(
            &mut recording,
            true,
            &mut MeetingDetector::default(),
            &events,
        );
        assert!(recording.as_mut().unwrap().call_stop.disarm());
        assert!(
            received.try_recv().is_err(),
            "a watched page keeps its stop"
        );
    }

    #[test]
    fn the_page_returning_during_the_grace_forgets_the_switched_call() {
        use crate::meeting::detection::CALL_CHANGED_AFTER;
        let (events, _received) = mpsc::sync_channel(4);
        let mut detector = MeetingDetector::default();
        detector.call_switched(Some("abc-defg-hij".into()), meet_candidate());
        let mut active = active_recording(
            Some("company.thebrowser.Browser"),
            true,
            Some("abc-defg-hij"),
        );
        // The page named the next call long enough to arm the stop, then came
        // back to the recorded one before the grace ran out.
        let base = Instant::now();
        assert!(!active.call_switch.observe_at(Some("xyz-wxyz-xyz"), base));
        assert!(
            active
                .call_switch
                .observe_at(Some("xyz-wxyz-xyz"), base + CALL_CHANGED_AFTER)
        );
        active.call_stop.arm();
        assert!(!active.call_switch.observe_at(
            Some("abc-defg-hij"),
            base + CALL_CHANGED_AFTER + Duration::from_secs(1)
        ));
        withdraw_call_switch_stop(
            &active.call_switch,
            &mut active.call_stop,
            &active.auto_stop,
            &mut detector,
            &events,
        );
        assert!(!active.call_stop.disarm(), "the stop was left pending");
        assert!(
            detector.switched_call().is_none(),
            "the withdrawn stop kept the call its page named"
        );
    }

    #[test]
    fn a_hand_stop_keeps_the_switched_call() {
        let (events, _received) = mpsc::sync_channel(4);
        let mut detector = MeetingDetector::default();
        detector.call_switched(Some("abc-defg-hij".into()), meet_candidate());
        let mut active = active_recording(
            Some("company.thebrowser.Browser"),
            true,
            Some("abc-defg-hij"),
        );
        active.call_stop.arm();
        // Stop pressed during the grace: the recording is already stopping,
        // so its armed page stop goes without forgetting the call its page
        // named, and the next recording starts that call.
        active.stop.store(true, Ordering::Relaxed);
        let mut recording = Some(active);
        disarm_call_stop_unless_watching(&mut recording, false, &mut detector, &events);
        assert!(
            !recording.as_mut().unwrap().call_stop.disarm(),
            "the armed stop was kept"
        );
        assert!(
            detector.switched_call().is_some(),
            "the hand stop forgot the call its page named"
        );
    }

    #[test]
    fn the_page_naming_the_recorded_call_again_withdraws_a_pending_stop() {
        use crate::meeting::detection::CALL_CHANGED_AFTER;
        let (events, received) = mpsc::sync_channel(4);
        let mut active = active_recording(
            Some("company.thebrowser.Browser"),
            true,
            Some("abc-defg-hij"),
        );
        // Another code counted long enough to arm the stop, then the
        // recorded call's page came back before the deadline.
        let base = Instant::now();
        assert!(!active.call_switch.observe_at(Some("xyz-wxyz-xyz"), base));
        assert!(
            active
                .call_switch
                .observe_at(Some("xyz-wxyz-xyz"), base + CALL_CHANGED_AFTER)
        );
        active.call_stop.arm();
        assert!(!active.call_switch.observe_at(
            Some("abc-defg-hij"),
            base + CALL_CHANGED_AFTER + Duration::from_secs(1)
        ));
        withdraw_call_switch_stop(
            &active.call_switch,
            &mut active.call_stop,
            &active.auto_stop,
            &mut MeetingDetector::default(),
            &events,
        );
        assert!(matches!(
            received.try_recv(),
            Ok(ControllerEvent::CallEndedWithdrawn)
        ));
        assert!(received.try_recv().is_err(), "one withdrawal only");
        assert!(!active.call_stop.disarm(), "the stop was left pending");
        assert!(!active.stop.load(Ordering::Relaxed));

        // Nothing pending: nothing is withdrawn.
        withdraw_call_switch_stop(
            &active.call_switch,
            &mut active.call_stop,
            &active.auto_stop,
            &mut MeetingDetector::default(),
            &events,
        );
        assert!(received.try_recv().is_err());

        // A page still naming another call keeps its stop and says nothing.
        assert!(
            !active
                .call_switch
                .observe_at(Some("xyz-wxyz-xyz"), base + Duration::from_secs(120))
        );
        active.call_stop.arm();
        withdraw_call_switch_stop(
            &active.call_switch,
            &mut active.call_stop,
            &active.auto_stop,
            &mut MeetingDetector::default(),
            &events,
        );
        assert!(active.call_stop.disarm(), "the stop was dropped mid-count");
        assert!(received.try_recv().is_err());

        // The recorded code back while the absence count runs: the page stop
        // goes, but the shared notice stays for the other.
        assert!(
            !active
                .call_switch
                .observe_at(Some("abc-defg-hij"), base + Duration::from_secs(180))
        );
        active.call_stop.arm();
        active.auto_stop.arm();
        withdraw_call_switch_stop(
            &active.call_switch,
            &mut active.call_stop,
            &active.auto_stop,
            &mut MeetingDetector::default(),
            &events,
        );
        assert!(!active.call_stop.disarm(), "the page stop was not dropped");
        assert!(active.auto_stop.armed(), "the absence stop still counts");
        assert!(received.try_recv().is_err(), "the notice stays");
    }

    #[test]
    fn the_call_coming_back_withdraws_a_pending_absence_stop_once() {
        let (events, received) = mpsc::sync_channel(4);
        let mut absence = AutoStop::default();
        let mut page = AutoStop::default();
        assert!(
            !withdraw_stop(&mut absence, &page, &events),
            "nothing was counting"
        );
        assert!(received.try_recv().is_err());

        // Both deadlines count one notice: the first to go leaves it up.
        absence.arm();
        page.arm();
        assert!(withdraw_stop(&mut absence, &page, &events));
        assert!(page.armed(), "the page stop still counts");
        assert!(received.try_recv().is_err(), "the notice stays");

        // The last one goes: the notice goes with it, once.
        assert!(withdraw_stop(&mut page, &absence, &events));
        assert!(matches!(
            received.try_recv(),
            Ok(ControllerEvent::CallEndedWithdrawn)
        ));
        assert!(received.try_recv().is_err(), "one withdrawal only");

        // The controller disarms every pass while the call is present, and
        // only the pass that dropped a stop says anything.
        assert!(!withdraw_stop(&mut absence, &page, &events));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn an_import_is_refused_while_a_recording_runs_or_another_import_does() {
        let (events, received) = mpsc::sync_channel(4);
        let (_update_sender, updates) = mpsc::sync_channel(1);
        let mut recording = Some(ActiveRecording {
            title: "Zoom call".into(),
            application: None,
            app_label: "Zoom".into(),
            automatic: false,
            presence: CallPresence::default(),
            watch: CallWatch::default(),
            auto_stop: AutoStop::default(),
            call_switch: CallSwitch::new(None),
            call_stop: AutoStop::default(),
            stop: Arc::new(AtomicBool::new(false)),
            updates,
            reported_started: true,
            playback_locked_noted: false,
            worker: thread::spawn(|| Err(eyre!("not recording"))),
        });
        start_import(
            &mut recording,
            &mut None,
            PathBuf::from("/tmp/call.m4a"),
            &events,
        );
        match received.try_recv() {
            Ok(ControllerEvent::ImportFailed { error }) => {
                assert!(error.contains("Stop the recording"), "{error}");
            }
            _ => panic!("a recording did not refuse the import"),
        }

        let mut recording = None;
        let mut importing = Some(ImportWorker {
            worker: thread::spawn(|| Err(eyre!("not imported"))),
            file: "call.m4a".into(),
        });
        start_import(
            &mut recording,
            &mut importing,
            PathBuf::from("/tmp/call.m4a"),
            &events,
        );
        match received.try_recv() {
            Ok(ControllerEvent::ImportFailed { error }) => {
                assert!(error.contains("already running"), "{error}");
            }
            _ => panic!("a running import did not refuse another"),
        }
    }

    fn banshee_tab() -> ContextSnapshot {
        ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://banshee.app/standup").unwrap()),
            window_title: Some("Standup".into()),
            ..ContextSnapshot::default()
        }
    }

    /// A Google Meet call's page in the test browser, for the call-switch
    /// checks.
    fn meet_candidate() -> MeetingCandidate {
        let tab = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://meet.google.com/xyz-wxyz-xyz").unwrap()),
            window_title: Some("Meet - Standup".into()),
            ..ContextSnapshot::default()
        };
        candidate_from_microphone(&brave(), &tab).unwrap()
    }

    fn brave() -> ActiveMicrophoneApplication {
        ActiveMicrophoneApplication {
            pid: 1,
            bundle_id: "com.brave.Browser.helper".into(),
            name: "Brave Browser Helper".into(),
        }
    }

    #[test]
    fn a_slow_tab_query_never_holds_up_the_loop() {
        fn slow(_: &str) -> Result<ContextSnapshot> {
            thread::sleep(Duration::from_millis(400));
            Ok(banshee_tab())
        }
        let mut probe = TabProbe::new(slow);
        let started = Instant::now();
        assert_eq!(
            probe.page("com.brave.Browser", started, false),
            ContextSnapshot::default()
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        // The answer is picked up on a later pass.
        let deadline = Instant::now() + Duration::from_secs(10);
        while probe.page("com.brave.Browser", Instant::now(), false) == ContextSnapshot::default() {
            assert!(Instant::now() < deadline, "the tab query never answered");
            thread::sleep(Duration::from_millis(20));
        }
        // It answered for that browser only, and is dropped with the call.
        assert_eq!(
            probe.latest.as_ref().map(|(bundle, ..)| bundle.as_str()),
            Some("com.brave.Browser")
        );
        probe.reset();
        assert!(probe.latest.is_none());
    }

    #[test]
    fn a_snoozed_or_non_browser_application_is_not_queried() {
        use std::sync::atomic::AtomicUsize;
        static QUERIES: AtomicUsize = AtomicUsize::new(0);
        fn counted(_: &str) -> Result<ContextSnapshot> {
            QUERIES.fetch_add(1, Ordering::SeqCst);
            Ok(banshee_tab())
        }
        let mut watch = BrowserWatch::new(counted);
        assert_eq!(
            watch.context(Some(&brave()), |_| true),
            ContextSnapshot::default()
        );
        assert_eq!(watch.context(None, |_| false), ContextSnapshot::default());
        let slack = ActiveMicrophoneApplication {
            pid: 2,
            bundle_id: "com.tinyspeck.slackmacgap.helper".into(),
            name: "Slack Helper".into(),
        };
        assert_eq!(
            watch.context(Some(&slack), |_| false),
            ContextSnapshot::default()
        );
        thread::sleep(Duration::from_millis(100));
        assert_eq!(QUERIES.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_reset_drops_the_answer_in_flight_without_starting_a_second_query() {
        use std::sync::atomic::AtomicUsize;
        static QUERIES: AtomicUsize = AtomicUsize::new(0);
        fn slow(_: &str) -> Result<ContextSnapshot> {
            QUERIES.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(400));
            Ok(banshee_tab())
        }
        let mut probe = TabProbe::new(slow);
        probe.page("com.brave.Browser", Instant::now(), false);
        probe.reset();
        // The first query is still running: no second one starts beside it.
        thread::sleep(Duration::from_millis(100));
        probe.page("com.brave.Browser", Instant::now(), false);
        assert_eq!(QUERIES.load(Ordering::SeqCst), 1);
        // Once it finishes, its answer belongs to the earlier call.
        thread::sleep(Duration::from_millis(500));
        assert_eq!(
            probe.page("com.brave.Browser", Instant::now(), false),
            ContextSnapshot::default()
        );
        assert!(probe.latest.is_none());
    }

    #[test]
    fn an_answer_is_dated_by_when_its_query_finished() {
        fn quick(_: &str) -> Result<ContextSnapshot> {
            Ok(banshee_tab())
        }
        let mut probe = TabProbe::new(quick);
        let started = Instant::now();
        probe.page("com.brave.Browser", started, false);
        thread::sleep(Duration::from_millis(300));
        // Polled well after the query finished: the answer is already old.
        let polled = started + TAB_QUERY_MAX_AGE + Duration::from_secs(1);
        assert_eq!(
            probe.page("com.brave.Browser", polled, false),
            ContextSnapshot::default()
        );
    }

    #[test]
    fn tab_queries_back_off_after_the_first_thirty_seconds_and_reset() {
        use std::sync::atomic::AtomicUsize;
        static QUERIES: AtomicUsize = AtomicUsize::new(0);
        fn counted(_: &str) -> Result<ContextSnapshot> {
            QUERIES.fetch_add(1, Ordering::SeqCst);
            Ok(banshee_tab())
        }
        // A query runs on its own thread, so wait with a deadline until its
        // answer is received: polling at the query's own start time can never
        // start another one, so this only drains the answer.
        let settle = |probe: &mut TabProbe, at: Instant, expected: usize| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while QUERIES.load(Ordering::SeqCst) < expected || probe.running.is_some() {
                assert!(Instant::now() < deadline, "the tab query never finished");
                probe.page("com.brave.Browser", at, false);
                thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(QUERIES.load(Ordering::SeqCst), expected);
        };
        let mut probe = TabProbe::new(counted);
        let started = Instant::now();
        // The first 30 s ask every second; `started` records the query
        // synchronously, so a backed-off call is proven by it not moving.
        probe.page("com.brave.Browser", started, false);
        assert_eq!(probe.started, Some(started));
        settle(&mut probe, started, 1);
        let at_1 = started + Duration::from_secs(1);
        probe.page("com.brave.Browser", at_1, false);
        assert_eq!(probe.started, Some(at_1));
        settle(&mut probe, at_1, 2);

        // From then on they are 30 s apart; an answer waits that long.
        let at_29 = started + Duration::from_secs(29);
        probe.page("com.brave.Browser", at_29, false);
        assert_eq!(probe.started, Some(at_29));
        settle(&mut probe, at_29, 3);
        probe.page(
            "com.brave.Browser",
            started + Duration::from_secs(31),
            false,
        );
        assert_eq!(
            probe.started,
            Some(at_29),
            "a backed-off query started early"
        );
        let at_59 = started + Duration::from_secs(59);
        probe.page("com.brave.Browser", at_59, false);
        assert_eq!(probe.started, Some(at_59));
        settle(&mut probe, at_59, 4);

        // A reset starts a fresh burst at once.
        probe.reset();
        let at_61 = started + Duration::from_secs(61);
        probe.page("com.brave.Browser", at_61, false);
        assert_eq!(probe.started, Some(at_61));
        settle(&mut probe, at_61, 5);
    }

    #[test]
    fn a_watched_page_is_queried_every_two_seconds_instead_of_backing_off() {
        fn counted(_: &str) -> Result<ContextSnapshot> {
            Ok(banshee_tab())
        }
        let mut probe = TabProbe::new(counted);
        let started = Instant::now();
        // A recording watches its page: two seconds, however long the browser
        // has held the microphone.
        assert_eq!(
            probe.interval("com.brave.Browser", started, true),
            TAB_QUERY_WATCH
        );
        assert_eq!(
            probe.interval("com.brave.Browser", started + Duration::from_secs(31), true),
            TAB_QUERY_WATCH
        );
        assert_eq!(
            probe.interval(
                "com.brave.Browser",
                started + Duration::from_secs(120),
                true
            ),
            TAB_QUERY_WATCH
        );
        // Once nothing watches, detection resumes with a fresh burst, then
        // backs off as before.
        assert_eq!(
            probe.interval(
                "com.brave.Browser",
                started + Duration::from_secs(120),
                false
            ),
            TAB_QUERY_INTERVAL
        );
        assert_eq!(
            probe.interval(
                "com.brave.Browser",
                started + Duration::from_secs(150),
                false
            ),
            TAB_QUERY_BACKOFF
        );
    }

    #[test]
    fn a_cold_tab_query_then_its_answer_leads_to_a_banshee_offer() {
        fn banshee(_: &str) -> Result<ContextSnapshot> {
            thread::sleep(Duration::from_millis(300));
            Ok(banshee_tab())
        }
        let mut browser = BrowserWatch::new(banshee);
        let mut detector = MeetingDetector::default();
        let started = Instant::now();
        let mut first = true;
        let offer = loop {
            assert!(started.elapsed() < Duration::from_secs(10), "no offer");
            let context = browser.context(Some(&brave()), |_| false);
            if first {
                // The cold miss: the page is not known yet.
                assert_eq!(context, ContextSnapshot::default());
                first = false;
            }
            let candidate = candidate_from_microphone(&brave(), &context);
            if let Some(detection) =
                detector.detect(candidate, &[], crate::app_settings::MeetingPolicy::Ask)
            {
                break detection;
            }
            thread::sleep(Duration::from_millis(50));
        };
        let Detection::Offer(candidate) = offer else {
            panic!("expected an offer, not {offer:?}");
        };
        assert_eq!(candidate.site.as_deref(), Some("banshee.app"));
        // Offered at the known service's 2 s, not the 20 s of an unknown page.
        // (That it counts from the first pass is checked on the detector.)
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    const MINUTE: i64 = 60_000;

    fn meeting(id: &str, start_minute: i64, minutes: i64) -> CalendarMeeting {
        CalendarMeeting {
            id: id.into(),
            title: id.into(),
            start_ms: start_minute * MINUTE,
            end_ms: (start_minute + minutes) * MINUTE,
            attendees: vec!["Ana Ruiz".into()],
            link: None,
        }
    }

    #[test]
    fn no_reminder_shows_over_a_recording() {
        let mut reminders = Reminders {
            meetings: vec![meeting("sync", 100, 30)],
            ..Reminders::default()
        };
        assert_eq!(reminders.step(true, 99 * MINUTE, MINUTE), None);
        // After the recording, it shows if still due.
        assert!(matches!(
            reminders.step(false, 100 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(_))
        ));

        // A recording that starts while one shows closes it for good.
        let mut reminders = Reminders {
            meetings: vec![meeting("sync", 100, 30)],
            ..Reminders::default()
        };
        assert!(reminders.step(false, 99 * MINUTE, MINUTE).is_some());
        reminders.scheduler.clear();
        assert_eq!(reminders.step(true, 99 * MINUTE + 1_000, MINUTE), None);
        assert_eq!(reminders.step(false, 102 * MINUTE, MINUTE), None);
    }

    #[test]
    fn a_dismissed_reminder_without_a_link_snoozes_only_around_its_start() {
        let all_day_workshop = meeting("workshop", 100, 240);
        assert_eq!(
            calendar_snooze(&all_day_workshop),
            (Vec::new(), 100 * MINUTE + crate::calendar::LATE_GRACE_MS)
        );
        let short = meeting("standup", 100, 3);
        assert_eq!(calendar_snooze(&short), (Vec::new(), 103 * MINUTE));

        let mut zoom = meeting("zoom", 100, 240);
        zoom.link = Some(crate::calendar::CallLink {
            provider: crate::calendar::CallProvider::Zoom,
            url: "https://zoom.us/j/1".into(),
        });
        let (keys, until_ms) = calendar_snooze(&zoom);
        assert!(keys.iter().any(|key| key == "us.zoom.xos"), "{keys:?}");
        assert_eq!(until_ms, zoom.end_ms);
    }
}
