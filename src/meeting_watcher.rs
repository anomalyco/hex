//! Meeting detection and recording control for the macOS app: watches which
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
use crate::context::ContextSnapshot;
use crate::meeting::{self, MeetingCalendar, MeetingRequest, MeetingStart};
use crate::meeting_detection::{
    BrowserPageMemory, CallPresence, Detection, MeetingCandidate, MeetingDetector,
    candidate_from_microphone,
};
use crate::microphone_activity::{ActiveMicrophoneApplication, active_microphone_applications};

pub(crate) enum ControllerCommand {
    Record(MeetingCandidate),
    Dismiss,
    Stop,
    Open(String),
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
    /// Not now, or Don't record: the meeting's call is neither offered nor
    /// recorded automatically.
    DismissReminder(CalendarMeeting),
}

pub(crate) enum ControllerEvent {
    Offer(MeetingCandidate),
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
    Starting,
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
    OpenMeeting(String),
    Transcribing,
    CallEnded {
        app: String,
    },
    Finished(Box<meeting::MeetingManifest>),
    Failed {
        title: String,
        error: String,
    },
    NotesStarted,
    NotesFinished(Box<meeting::MeetingManifest>),
}

struct ActiveRecording {
    title: String,
    application: Option<String>,
    app_label: String,
    automatic: bool,
    presence: CallPresence,
    stop: Arc<AtomicBool>,
    updates: Receiver<meeting::RecordingUpdate>,
    reported_started: bool,
    worker: JoinHandle<Result<meeting::MeetingManifest>>,
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
    },
    Transcribing,
}

pub struct ListenerConfig {
    pub event_path: PathBuf,
    pub device: Option<String>,
}

pub(crate) fn should_open_app_on_launch(
    show_dock_icon: bool,
    setup_ready: bool,
    onboarding_completed: bool,
    status_item_available: bool,
) -> bool {
    show_dock_icon || !setup_ready || !onboarding_completed || !status_item_available
}

pub(crate) fn controller_loop(
    shutdown: &AtomicBool,
    events: SyncSender<ControllerEvent>,
    commands: Receiver<ControllerCommand>,
    meeting_requests: Receiver<MeetingRequest>,
) -> Result<()> {
    let mut detector = MeetingDetector::default();
    let mut recording: Option<ActiveRecording> = None;
    let mut reminders = Reminders::default();
    let mut observed_microphone_apps = Vec::new();
    let mut browser = BrowserWatch::new(ContextSnapshot::capture_for_meeting);
    let mut pending_finals = PendingFinals::new();
    tracing::info!("meeting watcher started");

    while !shutdown.load(Ordering::Relaxed) {
        let enabled = crate::app_settings::meetings_enabled();
        let requested_commands = meeting_requests
            .try_iter()
            .take(8)
            .map(|request| match request {
                MeetingRequest::Start { title } => {
                    ControllerCommand::Record(MeetingCandidate::manual_titled(title))
                }
                MeetingRequest::Stop => ControllerCommand::Stop,
                MeetingRequest::Summarize(id) => ControllerCommand::Summarize(id),
            });
        for command in commands.try_iter().take(8).chain(requested_commands) {
            handle_command(
                &mut recording,
                &mut detector,
                &mut reminders,
                command,
                enabled,
                &events,
            );
        }

        poll_recording(&mut recording, &mut detector, &events)?;
        reminders.poll(enabled, recording.is_some(), &events);
        pending_finals.poll(enabled, recording.is_some(), &events);

        if recording.is_none() && enabled {
            let choices = crate::app_settings::meeting_provider_choices();
            match detect_meeting(
                &mut detector,
                &mut observed_microphone_apps,
                &mut browser,
                &choices,
            ) {
                Err(()) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
                Ok(Some(Detection::Offer(candidate))) => {
                    let _ = events.send(ControllerEvent::Offer(candidate));
                }
                Ok(Some(Detection::Record(candidate))) => {
                    let start = reminders.start_for(&candidate);
                    start_recording(&mut recording, &mut reminders, start, true, &events);
                }
                Ok(None) => {}
            }
        } else {
            // Nothing is watched now: no answer read before this may come
            // back as fresh when detection resumes.
            browser.reset();
        }
        thread::sleep(Duration::from_millis(500));
    }

    if let Some(active) = recording {
        active.stop.store(true, Ordering::Relaxed);
        active
            .worker
            .join()
            .map_err(|_| eyre!("meeting recorder panicked"))??;
    }
    pending_finals.join();
    Ok(())
}

/// How long finishing pending transcripts waits for the dictation engine
/// before loading its own model, as a recording without one does.
const ENGINE_GRACE: Duration = Duration::from_secs(30);

/// Finishes, once per launch, the transcripts of meetings Hex quit while
/// recording. It waits for the dictation engine so the work borrows its
/// model rather than loading a second copy.
struct PendingFinals {
    since: Instant,
    checked: bool,
    worker: Option<JoinHandle<()>>,
}

impl PendingFinals {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            checked: false,
            worker: None,
        }
    }

    fn poll(&mut self, enabled: bool, recording: bool, events: &SyncSender<ControllerEvent>) {
        if self.checked
            || !enabled
            || recording
            || (crate::transcription_engine::TranscriptionEngine::shared().is_none()
                && self.since.elapsed() < ENGINE_GRACE)
        {
            return;
        }
        self.checked = true;
        let ids = meeting::pending_final_transcripts();
        if ids.is_empty() {
            return;
        }
        let events = events.clone();
        self.worker = thread::Builder::new()
            .name("meeting-pending-finals".into())
            .spawn(move || {
                for id in ids {
                    if crate::process_exit::quitting() {
                        return;
                    }
                    match meeting::finish_pending(&id) {
                        Ok(Some(manifest)) => {
                            tracing::info!(meeting = id, "finished a pending meeting transcript");
                            if meeting::notes_wanted() {
                                start_notes(manifest.id, &events);
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error, meeting = id, "could not finish a pending meeting transcript");
                        }
                    }
                }
            })
            .map_err(|error| tracing::error!(%error, "could not start pending meeting transcripts"))
            .ok();
    }

    fn join(self) {
        if let Some(worker) = self.worker
            && worker.join().is_err()
        {
            tracing::error!("pending meeting transcripts panicked");
        }
    }
}

fn handle_command(
    recording: &mut Option<ActiveRecording>,
    detector: &mut MeetingDetector,
    reminders: &mut Reminders,
    command: ControllerCommand,
    enabled: bool,
    events: &SyncSender<ControllerEvent>,
) {
    match command {
        ControllerCommand::Record(_) | ControllerCommand::RecordMeeting { .. } if !enabled => {
            tracing::warn!("ignored a meeting recording request while meetings are off");
        }
        ControllerCommand::Record(candidate) if recording.is_none() => {
            let start = reminders.start_for(&candidate);
            start_recording(recording, reminders, start, false, events);
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
                events,
            );
        }
        ControllerCommand::JoinMeeting(meeting) => {
            reminders.scheduler.answered(&meeting.occurrence());
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
            if let Some(active) = recording.as_ref()
                && !active.stop.swap(true, Ordering::Relaxed)
            {
                let _ = events.send(ControllerEvent::Transcribing);
            }
        }
        ControllerCommand::Open(id) => {
            let _ = events.send(ControllerEvent::OpenMeeting(id));
        }
        ControllerCommand::Summarize(id) => start_notes(id, events),
        ControllerCommand::Dismiss | ControllerCommand::Record(_) => {}
    }
}

/// What a recording starts with, and what the controller watches while it
/// runs.
struct Start {
    meeting: MeetingStart,
    title: String,
    /// The call app whose leaving the microphone ends the call.
    application: Option<String>,
    app_label: String,
}

impl Start {
    fn from_candidate(candidate: &MeetingCandidate) -> Self {
        Self {
            meeting: candidate.recording_start(),
            title: candidate.title.clone(),
            application: candidate.application.clone(),
            app_label: candidate.source.label().into(),
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
            calendar: Some(calendar_context(meeting)),
            title_source: meeting::TitleSource::Calendar,
        },
        title: meeting.display_title(),
        application: provider
            .and_then(|provider| provider.application())
            .map(str::to_string),
        app_label: provider
            .map_or("Meeting", |provider| provider.label())
            .into(),
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
    read_failed: bool,
}

impl Reminders {
    fn poll(&mut self, enabled: bool, recording: bool, events: &SyncSender<ControllerEvent>) {
        let settings = crate::app_settings::calendar_settings();
        if !enabled || !settings.reminders_active() {
            self.meetings.clear();
            self.change_count = None;
            self.read_at_ms = None;
            if let Some(occurrence) = self.scheduler.clear() {
                let _ = events.send(ControllerEvent::ReminderWithdrawn { occurrence });
            }
            return;
        }
        let now_ms = crate::calendar::wall_clock_ms();
        let changes = crate::calendar::change_count();
        let stale = self
            .read_at_ms
            .is_none_or(|read_at| (now_ms - read_at).abs() >= CALENDAR_REREAD_MS);
        if stale || self.change_count != Some(changes) {
            self.change_count = Some(changes);
            self.read_at_ms = Some(now_ms);
            match crate::calendar::read_meetings(&settings, now_ms) {
                Ok(meetings) => {
                    self.meetings = meetings;
                    self.read_failed = false;
                }
                Err(error) => {
                    if !self.read_failed {
                        tracing::warn!(%error, "could not read calendar meetings");
                    }
                    self.read_failed = true;
                    self.meetings.clear();
                }
            }
        }
        let lead_ms = settings.lead().as_millis() as i64;
        match self.step(recording, now_ms, lead_ms) {
            Some(ReminderUpdate::Show(meeting)) => {
                let automatic = records_automatically(
                    &meeting,
                    &crate::app_settings::meeting_provider_choices(),
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
                && candidate.source == crate::meeting_detection::MeetingSource::Manual)
            {
                start.meeting.title = Some(meeting.display_title());
                start.meeting.title_source = meeting::TitleSource::Calendar;
                start.title = meeting.display_title();
            }
        }
        start
    }
}

/// Whether the meeting's call service is set to Always record, so its call
/// records once detected.
fn records_automatically(
    meeting: &CalendarMeeting,
    choices: &[crate::app_settings::MeetingProviderChoice],
) -> bool {
    meeting
        .link
        .as_ref()
        .and_then(|link| link.provider.choice_key())
        .is_some_and(|key| {
            choices.iter().any(|choice| {
                choice.key == key
                    && choice.policy == crate::app_settings::MeetingPolicy::AlwaysRecord
            })
        })
}

fn start_recording(
    recording: &mut Option<ActiveRecording>,
    reminders: &mut Reminders,
    start: Start,
    automatic: bool,
    events: &SyncSender<ControllerEvent>,
) {
    // `Starting` closes a showing reminder; no reminder shows over a
    // recording.
    reminders.scheduler.clear();
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
    let meeting_start = start.meeting;
    let worker =
        thread::spawn(move || meeting::record(meeting_start, &worker_stop, Some(update_sender)));
    let _ = events.send(ControllerEvent::Starting);
    *recording = Some(ActiveRecording {
        title: start.title,
        application: start.application,
        app_label: start.app_label,
        automatic,
        presence: CallPresence::default(),
        stop,
        updates,
        reported_started: false,
        worker,
    });
}

fn poll_recording(
    recording: &mut Option<ActiveRecording>,
    detector: &mut MeetingDetector,
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
                meeting::RecordingUpdate::Transcribing => {
                    if !active.stop.swap(true, Ordering::Relaxed) {
                        let _ = events.send(ControllerEvent::Transcribing);
                    }
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
        let event = match active.worker.join() {
            Ok(Ok(manifest)) => {
                if manifest.status == meeting::MeetingStatus::Complete && meeting::notes_wanted() {
                    start_notes(manifest.id.clone(), events);
                }
                ControllerEvent::Finished(Box::new(manifest))
            }
            Ok(Err(error)) => ControllerEvent::Failed {
                title: active.title,
                error: error.to_string(),
            },
            Err(_) => ControllerEvent::Failed {
                title: active.title,
                error: "meeting recorder panicked".into(),
            },
        };
        let _ = events.send(event);
    }

    if let Some(active) = recording.as_mut()
        && active.reported_started
        && !active.stop.load(Ordering::Relaxed)
        && let Some(application) = active.application.as_deref()
    {
        // A failed query must not end the controller while it owns a recording.
        let present = match active_microphone_applications() {
            Ok(applications) => applications
                .iter()
                .any(|candidate| candidate.canonical_bundle_id() == Some(application)),
            Err(error) => {
                tracing::warn!(%error, "could not inspect microphone activity during a meeting");
                true
            }
        };
        if present {
            detector.keep_snooze_alive();
        }
        if active.presence.observe(present) {
            tracing::info!(application, "meeting application released the microphone");
            let _ = events.send(ControllerEvent::CallEnded {
                app: active.app_label.clone(),
            });
        }
    }
    Ok(())
}

/// How often a browser's tabs are queried while it holds the microphone.
const TAB_QUERY_INTERVAL: Duration = Duration::from_secs(1);
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
    latest: Option<(String, ContextSnapshot, Instant)>,
}

impl TabProbe {
    fn new(query: TabQuery) -> Self {
        Self {
            query,
            running: None,
            generation: 0,
            started: None,
            latest: None,
        }
    }

    /// The browser's newest answered page, or nothing yet.
    fn page(&mut self, bundle_id: &str, now: Instant) -> ContextSnapshot {
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
        if self.running.is_none()
            && self
                .started
                .is_none_or(|started| now.saturating_duration_since(started) >= TAB_QUERY_INTERVAL)
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
    /// dropped.
    fn reset(&mut self) {
        self.latest = None;
        self.generation += 1;
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
        let page = self.probe.page(bundle_id, now);
        self.memory.recall(application, page, now)
    }
}

/// One detection pass; `Err` when microphone activity could not be read.
fn detect_meeting(
    detector: &mut MeetingDetector,
    observed_microphone_apps: &mut Vec<(i32, String)>,
    browser: &mut BrowserWatch,
    choices: &[crate::app_settings::MeetingProviderChoice],
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
    let detection = detector.detect(candidate, choices);
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
fn start_notes(id: String, events: &SyncSender<ControllerEvent>) {
    let finished = events.clone();
    if meeting::spawn_summary(id, move |result| {
        if let Ok(manifest) = result {
            let _ = finished.send(ControllerEvent::NotesFinished(Box::new(manifest)));
        }
    }) {
        let _ = events.send(ControllerEvent::NotesStarted);
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
    fn dockless_startup_stays_quiet_only_when_setup_and_menu_bar_are_ready() {
        for setup_ready in [false, true] {
            for onboarding_completed in [false, true] {
                for status_item_available in [false, true] {
                    assert_eq!(
                        should_open_app_on_launch(
                            false,
                            setup_ready,
                            onboarding_completed,
                            status_item_available,
                        ),
                        !(setup_ready && onboarding_completed && status_item_available),
                        "setup_ready={setup_ready}, onboarding_completed={onboarding_completed}, status_item_available={status_item_available}",
                    );
                }
            }
        }
    }

    #[test]
    fn dock_visible_startup_always_opens_the_app() {
        for setup_ready in [false, true] {
            for onboarding_completed in [false, true] {
                for status_item_available in [false, true] {
                    assert!(should_open_app_on_launch(
                        true,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    ));
                }
            }
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
            probe.page("com.brave.Browser", started),
            ContextSnapshot::default()
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        // The answer is picked up on a later pass.
        let deadline = Instant::now() + Duration::from_secs(10);
        while probe.page("com.brave.Browser", Instant::now()) == ContextSnapshot::default() {
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
        probe.page("com.brave.Browser", Instant::now());
        probe.reset();
        // The first query is still running: no second one starts beside it.
        thread::sleep(Duration::from_millis(100));
        probe.page("com.brave.Browser", Instant::now());
        assert_eq!(QUERIES.load(Ordering::SeqCst), 1);
        // Once it finishes, its answer belongs to the earlier call.
        thread::sleep(Duration::from_millis(500));
        assert_eq!(
            probe.page("com.brave.Browser", Instant::now()),
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
        probe.page("com.brave.Browser", started);
        thread::sleep(Duration::from_millis(300));
        // Polled well after the query finished: the answer is already old.
        let polled = started + TAB_QUERY_MAX_AGE + Duration::from_secs(1);
        assert_eq!(
            probe.page("com.brave.Browser", polled),
            ContextSnapshot::default()
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
            if let Some(detection) = detector.detect(candidate, &[]) {
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
