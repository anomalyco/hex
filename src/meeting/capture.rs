//! Independent, self-healing meeting audio sources.
//!
//! The microphone and the system audio each run on their own supervisor
//! thread. A source that fails is dropped and reopened with bounded backoff
//! for as long as the meeting records, so losing one never stops the other.
//! The system audio source is each OS's own (`super::os`): a Core
//! Audio process tap on macOS, a PipeWire monitor of the default output on
//! Linux, and WASAPI loopback on Windows.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use super::warning::{SilenceAction, SilencePolicy, SilenceRecovery, TrackWarnings};
use super::{AudioPacket, MeetingSource, SAMPLE_RATE};
use crate::dictation::StreamingResampler;

const RETRY_INITIAL: Duration = Duration::from_millis(500);
const RETRY_MAX: Duration = Duration::from_secs(5);
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Core Audio delivers silence continuously; this long without a buffer
/// means the device or tap stopped.
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Delivers one source's 16 kHz audio to the track writer, stamped on the
/// meeting's shared capture clock.
#[derive(Clone)]
pub(super) struct PacketSink {
    pub(super) source: MeetingSource,
    pub(super) sender: SyncSender<AudioPacket>,
    pub(super) drops: Arc<AtomicU64>,
    pub(super) clock: Instant,
}

impl PacketSink {
    pub(super) fn send(&self, sample_rate: u32, samples: Vec<f32>, pts_us: Option<i64>) {
        if samples.is_empty() {
            return;
        }
        let packet = AudioPacket {
            source: self.source,
            pts_us,
            arrival_us: self.clock.elapsed().as_micros() as u64,
            position: 0,
            sample_rate,
            samples,
        };
        if matches!(self.sender.try_send(packet), Err(TrySendError::Full(_))) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// An open source that the supervisor polls until it fails.
pub(super) trait LiveSource {
    /// Waits briefly for audio and forwards it; an error ends this instance.
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String>;
    fn describe(&self) -> &'static str;
}

/// Owns one source's supervisor thread. Dropping it stops the source.
pub(super) struct SourceSupervisor {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SourceSupervisor {
    /// Records the input the call app `application` (a canonical bundle
    /// identifier) records from where the OS reports it, else the
    /// configured microphone.
    pub(super) fn microphone(
        sink: PacketSink,
        application: Option<String>,
        warnings: TrackWarnings,
    ) -> Self {
        let recovery = Arc::new(SilenceRecovery::default());
        // The processes recording from the input, shared across reopens so a
        // release between a stream failure and its replacement is still seen.
        let recorders = Arc::new(InputRecorders::default());
        Self::spawn(sink, move || {
            MicrophoneSource::open(
                crate::app_settings::microphone_selection(),
                application.clone(),
                recovery.clone(),
                recorders.clone(),
                warnings.clone(),
            )
            .map(|source| Box::new(source) as Box<dyn LiveSource>)
        })
    }

    /// Records the system audio of the call app `application` (a canonical
    /// bundle identifier), or of every output for a manual recording.
    pub(super) fn system(sink: PacketSink, application: Option<String>) -> Self {
        let source_sink = sink.clone();
        Self::spawn(sink, move || {
            super::os::open_system_audio(&source_sink, application.as_deref())
        })
    }

    fn spawn(
        sink: PacketSink,
        mut open: impl FnMut() -> Result<Box<dyn LiveSource>> + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name(format!("meeting-{:?}", sink.source).to_ascii_lowercase())
            .spawn(move || supervise(&sink, &thread_stop, &mut open))
            .expect("could not spawn meeting capture thread");
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for SourceSupervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("meeting capture thread panicked");
        }
    }
}

fn supervise(
    sink: &PacketSink,
    stop: &AtomicBool,
    open: &mut dyn FnMut() -> Result<Box<dyn LiveSource>>,
) {
    let source = sink.source;
    let mut delay = RETRY_INITIAL;
    let mut interrupted = false;
    let mut last_open_error = None::<String>;
    while !stop.load(Ordering::Relaxed) {
        match open() {
            Ok(mut live) => {
                tracing::info!(
                    ?source,
                    backend = live.describe(),
                    resumed = interrupted,
                    "meeting capture source started"
                );
                delay = RETRY_INITIAL;
                last_open_error = None;
                let failure = loop {
                    if stop.load(Ordering::Relaxed) {
                        break None;
                    }
                    if let Err(error) = live.poll(sink) {
                        break Some(error);
                    }
                };
                drop(live);
                let Some(error) = failure else {
                    return;
                };
                interrupted = true;
                tracing::warn!(
                    ?source,
                    %error,
                    "meeting capture source stopped; keeping the other source and retrying"
                );
            }
            Err(error) => {
                let error = format!("{error:#}");
                if last_open_error.as_deref() != Some(error.as_str()) {
                    tracing::warn!(?source, %error, "could not open meeting capture source; retrying");
                    last_open_error = Some(error);
                }
            }
        }
        let resume_at = Instant::now() + delay;
        while Instant::now() < resume_at {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(POLL_INTERVAL);
        }
        delay = delay.saturating_mul(2).min(RETRY_MAX);
    }
}

/// Converts any device rate to the 16 kHz meeting tracks so a reopened
/// device with a different rate never changes a track's format.
pub(super) struct To16k {
    pub(super) rate: u32,
    resampler: StreamingResampler,
}

impl To16k {
    pub(super) fn new(rate: u32) -> Result<Self> {
        Ok(Self {
            rate,
            resampler: StreamingResampler::new(rate).map_err(|error| eyre!(error))?,
        })
    }

    pub(super) fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.resampler.push(samples)
    }
}

/// Looks again, a bounded number of times, for a saved microphone that is
/// listed but would not open, while the system default stands in for it.
#[derive(Debug, Default)]
struct UnopenableRetry {
    due: Option<Instant>,
    tries: u32,
}

impl UnopenableRetry {
    const TRIES: u32 = 8;
    const FIRST: Duration = Duration::from_secs(1);
    const MAX: Duration = Duration::from_secs(30);

    /// Schedules the next look after resolving to `selection` at `now`.
    fn resolved(&mut self, selection: crate::audio::Selection, now: Instant) {
        if selection != crate::audio::Selection::Unopenable {
            *self = Self::default();
            return;
        }
        if self.tries >= Self::TRIES {
            self.due = None;
            return;
        }
        let delay = Self::FIRST
            .saturating_mul(1 << self.tries.min(16))
            .min(Self::MAX);
        self.due = Some(now + delay);
        self.tries += 1;
    }

    fn is_due(&self, now: Instant) -> bool {
        self.due.is_some_and(|due| now >= due)
    }
}

/// How often a meeting looks at which input its call app records from.
const CALL_INPUT_INTERVAL: Duration = Duration::from_secs(1);

/// The input a meeting follows for its call app, by UID: the one it follows
/// while the app still reports it or reports none (as some do while muted),
/// else the first the app reports.
fn follow_call_input(current: Option<String>, reported: Vec<String>) -> Option<String> {
    match current {
        Some(current) if reported.is_empty() || reported.contains(&current) => Some(current),
        _ => reported.into_iter().next(),
    }
}

/// Whether `opened`, the input a meeting records from, is one the call app
/// reports recording from.
fn call_app_reports_input(opened: Option<&str>, reported: &[String]) -> bool {
    opened.is_some_and(|opened| reported.iter().any(|reported| reported == opened))
}

/// The UIDs of the inputs the call app records from; none where the OS
/// does not report them.
fn call_app_inputs(application: &str) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        crate::microphone_activity::application_input_devices(application).unwrap_or_else(|error| {
            tracing::debug!(%error, "could not read the inputs the call app uses");
            Vec::new()
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = application;
        Vec::new()
    }
}

/// The processes recording from the input, shared by every stream the
/// supervisor opens so an app that let go between a stream failure and its
/// reopen is still seen leaving.
#[derive(Default)]
struct InputRecorders {
    seen: Mutex<SeenInput>,
    /// When our own renegotiating open last changed the device's rate; a
    /// recorder that disappears just after it is the driver restarting
    /// streams, not another app letting go.
    renegotiated: Mutex<Option<Instant>>,
}

/// The last input whose recorders were seen, and those processes.
#[derive(Default)]
struct SeenInput {
    uid: Option<String>,
    pids: Vec<i32>,
}

impl InputRecorders {
    /// Whether the shared baseline is for `uid`.
    fn same_input(&self, uid: Option<&str>) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .uid
            .as_deref()
            == uid
    }

    /// Records `current` for `uid`, reporting whether a process known on that
    /// same input stopped recording. A different input starts over. The
    /// comparison made by our own renegotiating open, and by any reopen
    /// within [`RENEGOTIATION_SETTLE`] of it, ignores a disappearing pid:
    /// the driver is restarting its streams, not another app letting go.
    fn update(&self, uid: Option<&str>, current: Vec<i32>, now: Instant) -> bool {
        let mut renegotiated = self
            .renegotiated
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut seen = self.seen.lock().unwrap_or_else(|error| error.into_inner());
        let same_input = seen.uid.as_deref() == uid;
        if !same_input {
            // The old input's rate change means nothing to this one.
            *renegotiated = None;
        }
        let settled = renegotiated.is_none_or(|at| now.duration_since(at) >= RENEGOTIATION_SETTLE);
        let released = same_input && settled && released_processes(&seen.pids, &current);
        seen.uid = uid.map(str::to_owned);
        seen.pids = current;
        released
    }

    /// Starts over on `uid` without treating its difference as a release.
    fn reset(&self, uid: Option<&str>, current: Vec<i32>) {
        *self.seen.lock().unwrap_or_else(|error| error.into_inner()) = SeenInput {
            uid: uid.map(str::to_owned),
            pids: current,
        };
    }

    /// Notes that this open renegotiated the device's sample rate, which
    /// restarts its other streams.
    fn note_renegotiation(&self, now: Instant) {
        *self
            .renegotiated
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(now);
    }
}

/// How long after our own rate change a recorder that disappears is taken to
/// be the driver restarting streams rather than another app letting go.
const RENEGOTIATION_SETTLE: Duration = Duration::from_secs(1);

/// Whether any process that was recording (`known`) is gone from `current`.
fn released_processes(known: &[i32], current: &[i32]) -> bool {
    known.iter().any(|pid| !current.contains(pid))
}

/// The processes recording from the input `uid`, Hex's own excluded, from
/// Core Audio's process metadata: no audio is read. None when the OS does
/// not report them or any part of the read failed, so a failed read never
/// looks like a release.
fn recording_processes(uid: Option<&str>) -> Option<Vec<i32>> {
    #[cfg(target_os = "macos")]
    {
        let uid = uid?;
        let ours = i32::try_from(std::process::id()).ok();
        match crate::microphone_activity::input_recorders(uid) {
            Ok(pids) => Some(pids.into_iter().filter(|pid| Some(*pid) != ours).collect()),
            Err(error) => {
                tracing::debug!(%error, "could not read the processes recording from the meeting microphone");
                None
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = uid;
        None
    }
}

struct MicrophoneSource {
    input: crate::audio::AudioInput,
    resample: To16k,
    last_audio: Instant,
    /// The per-chunk digital silence decision, sharing this recording's
    /// reopen budget and retry count with every reopened stream.
    silence: SilencePolicy,
    /// The controller-visible warning, shared across reopened streams.
    warnings: TrackWarnings,
    selected: Option<String>,
    /// The [`crate::app_settings::microphone_selection`] revision of `selected`.
    selection_revision: u64,
    /// The [`crate::audio::input_route_generation`] the input was resolved at.
    route_generation: u64,
    retry: UnopenableRetry,
    /// The detected call app's canonical bundle identifier.
    application: Option<String>,
    /// The UID of the input the call app records from.
    call_input: Option<String>,
    /// The input UIDs the call app last reported recording from.
    call_inputs: Vec<String>,
    /// The UID of this stream's input.
    opened_uid: Option<String>,
    call_input_checked: Instant,
    /// The processes recording from the input, shared across reopens.
    recorders: Arc<InputRecorders>,
    recorders_checked: Instant,
}

impl MicrophoneSource {
    fn open(
        (selection_revision, selected): (u64, Option<String>),
        application: Option<String>,
        silence_recovery: Arc<SilenceRecovery>,
        recorders: Arc<InputRecorders>,
        warnings: TrackWarnings,
    ) -> Result<Self> {
        let route_generation = crate::audio::input_route_generation();
        let call_inputs = application
            .as_deref()
            .map(call_app_inputs)
            .unwrap_or_default();
        let call_input = follow_call_input(None, call_inputs.clone());
        let renegotiate =
            silence_recovery.take_renegotiation(crate::transcription::engine::dictation_busy());
        if !renegotiate && silence_recovery.renegotiation_pending() {
            // A rate change would cut the dictation. The flag stays set and
            // the still-silent stream reopens once dictation is idle.
            tracing::info!(
                "dictation is active; opening the meeting microphone without renegotiating its rate"
            );
        }
        let crate::audio::Opened::Input(input) = crate::audio::open_meeting_input(
            selected.as_deref(),
            call_input.as_deref(),
            None,
            renegotiate,
        )?
        else {
            unreachable!("nothing to keep");
        };
        // The renegotiation is taken when this open attempts it, so a failing
        // open cannot flip the rate on every supervisor retry; only a
        // deferral for dictation leaves it pending.
        if renegotiate {
            // The open already did what a retry would: a release noted before
            // it is spent, though one this open's own comparison sees is not.
            silence_recovery.forget_release();
        }
        let input = *input;
        let opened_uid = input.uid.clone();
        let now = Instant::now();
        // A release from the old input says nothing about the new one.
        if !recorders.same_input(opened_uid.as_deref()) {
            silence_recovery.forget_release();
        }
        if application.is_some() {
            if renegotiate {
                recorders.note_renegotiation(now);
            }
            // Compare this open against the set the last stream saw, so an
            // app that let go while the source was down still counts as a
            // release. The shared state keeps it if this stream fails too.
            if let Some(current) = recording_processes(opened_uid.as_deref())
                && recorders.update(opened_uid.as_deref(), current, now)
            {
                silence_recovery.note_release();
            }
        }
        let resample = To16k::new(input.sample_rate)?;
        tracing::info!(
            device = input.device_name,
            rate = resample.rate,
            call_input = call_input.is_some(),
            renegotiated = renegotiate,
            "meeting microphone opened"
        );
        let mut retry = UnopenableRetry::default();
        retry.resolved(input.selection, Instant::now());
        let warning = warnings.live().is_some();
        Ok(Self {
            input,
            silence: SilencePolicy::new(resample.rate, silence_recovery, warning),
            warnings,
            resample,
            last_audio: Instant::now(),
            selected,
            selection_revision,
            route_generation,
            retry,
            application,
            call_input,
            call_inputs,
            opened_uid,
            call_input_checked: Instant::now(),
            recorders,
            recorders_checked: Instant::now(),
        })
    }

    /// Whether the call app records from the input this stream opened, which
    /// is when another app's voice processing can leave it digitally silent.
    /// A manual recording has no call app and never holds the device.
    fn call_app_holds_device(&self) -> bool {
        call_app_reports_input(self.opened_uid.as_deref(), &self.call_inputs)
    }

    /// Notes an application other than Hex stopping recording from this
    /// stream's input since the last look, sampled at the call app's once a
    /// second cadence from Core Audio process metadata; no audio is read. The
    /// shared recovery keeps it, so a stream that fails before its next chunk
    /// cannot lose it.
    fn poll_released(&mut self, now: Instant) {
        if self.application.is_none()
            || now.duration_since(self.recorders_checked) < CALL_INPUT_INTERVAL
        {
            return;
        }
        self.recorders_checked = now;
        let Some(current) = recording_processes(self.opened_uid.as_deref()) else {
            return;
        };
        if self
            .recorders
            .update(self.opened_uid.as_deref(), current, now)
        {
            self.silence.note_release();
        }
    }

    /// Follows a change of the input the call app records from, of the
    /// system default or of the saved microphone in Settings, the saved
    /// microphone returning, or a due look at one that would not open, by
    /// opening the input it now resolves to before dropping the old one. A
    /// failed open keeps the current input.
    fn follow_route(&mut self) {
        let now = Instant::now();
        self.poll_released(now);
        let mut call_input_changed = false;
        if self.application.is_some()
            && now.duration_since(self.call_input_checked) >= CALL_INPUT_INTERVAL
        {
            self.call_input_checked = now;
            self.call_inputs = self
                .application
                .as_deref()
                .map(call_app_inputs)
                .unwrap_or_default();
            let call_input = follow_call_input(self.call_input.clone(), self.call_inputs.clone());
            call_input_changed = call_input != self.call_input;
            self.call_input = call_input;
        }
        let generation = crate::audio::input_route_generation();
        let (revision, selected) = crate::app_settings::microphone_selection();
        if !call_input_changed
            && generation == self.route_generation
            && revision == self.selection_revision
            && !self.retry.is_due(now)
        {
            return;
        }
        self.route_generation = generation;
        if revision != self.selection_revision {
            self.selection_revision = revision;
            self.selected = selected;
            self.retry = UnopenableRetry::default();
        }
        let opened = crate::audio::open_meeting_input(
            self.selected.as_deref(),
            self.call_input.as_deref(),
            Some(&self.input.device_name),
            false,
        );
        let input = match opened {
            Ok(crate::audio::Opened::Input(input)) => *input,
            Ok(crate::audio::Opened::Unchanged { selection }) => {
                self.input.selection = selection;
                self.retry.resolved(selection, now);
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "could not switch the meeting microphone; keeping it");
                self.retry.resolved(self.input.selection, now);
                return;
            }
        };
        if input.sample_rate != self.resample.rate {
            match To16k::new(input.sample_rate) {
                Ok(resample) => self.resample = resample,
                Err(error) => {
                    tracing::warn!(%error, "could not switch the meeting microphone; keeping it");
                    self.retry.resolved(self.input.selection, now);
                    return;
                }
            }
        }
        tracing::info!(
            from = %self.input.device_name,
            to = %input.device_name,
            "meeting microphone switched"
        );
        self.retry.resolved(input.selection, now);
        // A new device starts a fresh zero count, but a warning raised by the
        // old one stays until real audio clears it.
        self.silence.reset(input.sample_rate);
        self.opened_uid = input.uid.clone();
        if let Some(current) = recording_processes(self.opened_uid.as_deref()) {
            self.recorders.reset(self.opened_uid.as_deref(), current);
        }
        self.recorders_checked = now;
        // A release or renegotiation from the old input says nothing about
        // the new one.
        self.silence.forget_release();
        self.silence.clear_renegotiation();
        self.input = input;
        self.last_audio = Instant::now();
    }
}

impl LiveSource for MicrophoneSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        self.follow_route();
        match self.input.recv_timeout(POLL_INTERVAL) {
            crate::audio::AudioInputEvent::Chunk { samples, .. } => {
                self.last_audio = Instant::now();
                let action = self.silence.observe(
                    &samples,
                    self.call_app_holds_device(),
                    crate::transcription::engine::dictation_busy,
                );
                sink.send(SAMPLE_RATE, self.resample.push(&samples), None);
                match action {
                    SilenceAction::Reopen => {
                        self.silence.request_renegotiation();
                        if self.silence.retrying() {
                            tracing::warn!(
                                device = %self.input.device_name,
                                "meeting microphone is still digitally silent; retrying it after an app released it"
                            );
                        } else {
                            tracing::warn!(
                                device = %self.input.device_name,
                                "meeting microphone delivered digital silence; reopening it"
                            );
                        }
                        return Err("microphone delivered digital silence".into());
                    }
                    SilenceAction::Gated => {
                        tracing::info!(
                            device = %self.input.device_name,
                            device_uid = ?self.opened_uid,
                            call_app_inputs = ?self.call_inputs,
                            "meeting microphone is digitally silent but the call app does not report recording from it; not reopening or warning"
                        );
                    }
                    SilenceAction::Warn => {
                        if self.warnings.warn() {
                            self.silence.applied(SilenceAction::Warn);
                            tracing::warn!(
                                device = %self.input.device_name,
                                "meeting microphone is still digitally silent; keeping the stream open"
                            );
                        }
                    }
                    SilenceAction::Clear => {
                        if self.warnings.clear() {
                            self.silence.applied(SilenceAction::Clear);
                            tracing::info!(
                                device = %self.input.device_name,
                                "meeting microphone audio returned"
                            );
                        }
                    }
                    SilenceAction::Nothing => {}
                }
                Ok(())
            }
            crate::audio::AudioInputEvent::Timeout if self.last_audio.elapsed() > STALL_TIMEOUT => {
                Err("microphone stopped delivering audio".into())
            }
            crate::audio::AudioInputEvent::Timeout => Ok(()),
            crate::audio::AudioInputEvent::StreamFailed(error) => Err(error),
        }
    }

    fn describe(&self) -> &'static str {
        "cpal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Selection;

    #[test]
    fn a_meeting_follows_the_input_its_call_app_records_from() {
        let uids = |uids: &[&str]| uids.iter().map(|uid| (*uid).to_owned()).collect::<Vec<_>>();
        let current = |uid: &str| Some(uid.to_owned());
        // The first input the app reports, until it reports a different one.
        assert_eq!(follow_call_input(None, uids(&["Jabra"])), current("Jabra"));
        assert_eq!(
            follow_call_input(current("Jabra"), uids(&["BuiltIn", "Jabra"])),
            current("Jabra")
        );
        assert_eq!(
            follow_call_input(current("Jabra"), uids(&["BuiltIn"])),
            current("BuiltIn")
        );
        // An app that reports none, as some do while muted, keeps its input.
        assert_eq!(
            follow_call_input(current("Jabra"), Vec::new()),
            current("Jabra")
        );
        // With nothing reported yet, the configured microphone stays.
        assert_eq!(follow_call_input(None, Vec::new()), None);
    }

    #[test]
    fn a_microphone_that_would_not_open_is_tried_a_bounded_number_of_times() {
        let start = Instant::now();
        let mut retry = UnopenableRetry::default();
        let mut now = start;
        let mut delays = Vec::new();
        retry.resolved(Selection::Unopenable, now);
        while let Some(due) = retry.due {
            assert!(!retry.is_due(due - Duration::from_millis(1)));
            assert!(retry.is_due(due));
            delays.push(due - now);
            now = due;
            retry.resolved(Selection::Unopenable, now);
        }
        assert_eq!(
            delays,
            [1, 2, 4, 8, 16, 30, 30, 30].map(Duration::from_secs)
        );

        // Opening it, or a microphone that is merely unlisted, stops looking.
        let mut retry = UnopenableRetry::default();
        retry.resolved(Selection::Unopenable, start);
        retry.resolved(Selection::InUse, start);
        assert!(retry.due.is_none());
        retry.resolved(Selection::Unlisted, start);
        assert!(retry.due.is_none());
    }

    #[test]
    fn a_process_that_stops_recording_between_reopens_is_a_release() {
        let now = Instant::now();
        // The shared set from the old stream still names a process the new
        // stream no longer sees.
        let recorders = InputRecorders::default();
        assert!(!recorders.update(Some("BuiltIn"), vec![41, 42], now));
        assert!(recorders.update(Some("BuiltIn"), vec![42], now));
        // A different input starts over rather than reporting every old pid.
        assert!(!recorders.update(Some("Jabra"), vec![7], now));
        assert!(!recorders.update(Some("Jabra"), vec![7, 8], now));
        // The same input still catches a release after the new baseline.
        assert!(recorders.update(Some("Jabra"), vec![8], now));
        // An empty first baseline is not a release.
        let recorders = InputRecorders::default();
        assert!(!recorders.update(Some("BuiltIn"), Vec::new(), now));
        assert!(!recorders.update(Some("BuiltIn"), vec![1], now));
        assert!(!released_processes(&[], &[1, 2]));
        assert!(!released_processes(&[1, 2], &[1, 2]));
        assert!(released_processes(&[1, 2], &[2]));
    }

    #[test]
    fn a_release_just_after_our_own_rate_change_is_not_another_apps() {
        let now = Instant::now();
        let recorders = InputRecorders::default();
        assert!(!recorders.update(Some("BuiltIn"), vec![41, 42], now));
        recorders.note_renegotiation(now);
        // The driver restarting streams drops a pid; that is our change.
        assert!(!recorders.update(Some("BuiltIn"), vec![42], now));
        // The baseline still moved, so a later release is seen.
        assert!(recorders.update(Some("BuiltIn"), Vec::new(), now + RENEGOTIATION_SETTLE));
    }

    #[test]
    fn a_release_belongs_only_to_the_input_that_saw_it() {
        let now = Instant::now();
        let recorders = InputRecorders::default();
        assert!(!recorders.update(Some("BuiltIn"), vec![41], now));
        assert!(recorders.same_input(Some("BuiltIn")));
        assert!(!recorders.same_input(Some("Jabra")));
    }

    #[test]
    fn only_an_input_the_call_app_reports_counts_as_held() {
        let reported = ["BuiltIn".to_owned()];
        assert!(call_app_reports_input(Some("BuiltIn"), &reported));
        assert!(!call_app_reports_input(Some("Jabra"), &reported));
        // A manual recording, or an app macOS reports no inputs for.
        assert!(!call_app_reports_input(None, &reported));
        assert!(!call_app_reports_input(Some("BuiltIn"), &[]));
    }
}
