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
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use super::{AudioPacket, MeetingSource, RecordingUpdate, SAMPLE_RATE, TrackWarning};
use crate::dictation::StreamingResampler;

const RETRY_INITIAL: Duration = Duration::from_millis(500);
const RETRY_MAX: Duration = Duration::from_secs(5);
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Core Audio delivers silence continuously; this long without a buffer
/// means the device or tap stopped.
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(3);
/// Exact-zero samples that mean the microphone stream is digitally silent
/// rather than a quiet room.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(3);
/// How many times one recording reopens a digitally silent microphone before
/// keeping the stream open and warning instead.
const SILENCE_REOPENS: u8 = 2;

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

/// The microphone's user-facing warning, shared by each reopened source with
/// the recorder that persists the final one.
#[derive(Clone)]
pub(super) struct TrackWarnings {
    current: Arc<Mutex<Option<TrackWarning>>>,
    updates: Option<SyncSender<RecordingUpdate>>,
}

impl TrackWarnings {
    pub(super) fn new(updates: Option<SyncSender<RecordingUpdate>>) -> Self {
        Self {
            current: Arc::default(),
            updates,
        }
    }

    /// Sets the warning, reporting a change to the controller.
    fn set(&self, warning: Option<TrackWarning>) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *current == warning {
            return;
        }
        *current = warning;
        drop(current);
        if let Some(updates) = &self.updates {
            let _ = updates.try_send(RecordingUpdate::SourceWarning { warning });
        }
    }

    /// The warning as of the last source change, for the final manifest.
    pub(super) fn current(&self) -> Option<TrackWarning> {
        *self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// What a microphone chunk's silence count crossed into.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SilenceEdge {
    /// The stream reached [`SILENCE_TIMEOUT`] of exact zeros.
    Silent,
    /// Nonzero audio ended a digitally silent episode.
    Audible,
    /// Neither the start nor the end of an episode.
    Unchanged,
}

/// Counts consecutive exact-zero samples from the microphone so a stream
/// that keeps delivering digital silence while its chunks keep arriving, as
/// a call app holding the device with voice processing does, is noticed.
#[derive(Clone, Debug)]
struct SilenceWatch {
    rate: u32,
    zeros: u64,
}

impl SilenceWatch {
    fn new(rate: u32) -> Self {
        Self { rate, zeros: 0 }
    }

    /// Starts a fresh count for a newly opened input device or sample rate.
    fn reset(&mut self, rate: u32) {
        self.rate = rate;
        self.zeros = 0;
    }

    /// Counts `samples`, reporting only the edges of an episode.
    fn observe(&mut self, samples: &[f32]) -> SilenceEdge {
        let was_silent = self.is_silent();
        for &sample in samples {
            if sample == 0.0 {
                self.zeros += 1;
            } else {
                self.zeros = 0;
            }
        }
        match (was_silent, self.is_silent()) {
            (false, true) => SilenceEdge::Silent,
            (true, false) => SilenceEdge::Audible,
            _ => SilenceEdge::Unchanged,
        }
    }

    fn is_silent(&self) -> bool {
        self.rate > 0
            && self.zeros >= u64::from(self.rate).saturating_mul(SILENCE_TIMEOUT.as_secs())
    }
}

/// Claims one of the recording's silence reopens; false once they are spent.
fn claim_silence_reopen(reopens: &AtomicU8) -> bool {
    let used = reopens.load(Ordering::Relaxed);
    if used >= SILENCE_REOPENS {
        return false;
    }
    reopens.store(used + 1, Ordering::Relaxed);
    true
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
        let reopens = Arc::new(AtomicU8::new(0));
        Self::spawn(sink, move || {
            MicrophoneSource::open(
                crate::app_settings::microphone_selection(),
                application.clone(),
                reopens.clone(),
                warnings.clone(),
            )
            .map(|source| Box::new(source) as Box<dyn LiveSource>)
        })
    }

    pub(super) fn system(sink: PacketSink) -> Self {
        let source_sink = sink.clone();
        Self::spawn(sink, move || super::os::open_system_audio(&source_sink))
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

/// The input the call app `application` records from, following `current`.
fn current_call_input(application: Option<&str>, current: Option<String>) -> Option<String> {
    application.and_then(|application| follow_call_input(current, call_app_inputs(application)))
}

struct MicrophoneSource {
    input: crate::audio::AudioInput,
    resample: To16k,
    last_audio: Instant,
    /// Exact-zero samples from the current input.
    silence: SilenceWatch,
    /// Silence reopens this recording has already spent.
    silence_reopens: Arc<AtomicU8>,
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
    call_input_checked: Instant,
}

impl MicrophoneSource {
    fn open(
        (selection_revision, selected): (u64, Option<String>),
        application: Option<String>,
        silence_reopens: Arc<AtomicU8>,
        warnings: TrackWarnings,
    ) -> Result<Self> {
        let route_generation = crate::audio::input_route_generation();
        let call_input = current_call_input(application.as_deref(), None);
        let crate::audio::Opened::Input(input) =
            crate::audio::open_meeting_input(selected.as_deref(), call_input.as_deref(), None)?
        else {
            unreachable!("nothing to keep");
        };
        let input = *input;
        let resample = To16k::new(input.sample_rate)?;
        tracing::info!(
            device = input.device_name,
            rate = resample.rate,
            call_input = call_input.is_some(),
            "meeting microphone opened"
        );
        let mut retry = UnopenableRetry::default();
        retry.resolved(input.selection, Instant::now());
        Ok(Self {
            input,
            silence: SilenceWatch::new(resample.rate),
            silence_reopens,
            warnings,
            resample,
            last_audio: Instant::now(),
            selected,
            selection_revision,
            route_generation,
            retry,
            application,
            call_input,
            call_input_checked: Instant::now(),
        })
    }

    /// Follows a change of the input the call app records from, of the
    /// system default or of the saved microphone in Settings, the saved
    /// microphone returning, or a due look at one that would not open, by
    /// opening the input it now resolves to before dropping the old one. A
    /// failed open keeps the current input.
    fn follow_route(&mut self) {
        let now = Instant::now();
        let mut call_input_changed = false;
        if self.application.is_some()
            && now.duration_since(self.call_input_checked) >= CALL_INPUT_INTERVAL
        {
            self.call_input_checked = now;
            let call_input =
                current_call_input(self.application.as_deref(), self.call_input.clone());
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
        self.silence.reset(input.sample_rate);
        self.warnings.set(None);
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
                let edge = self.silence.observe(&samples);
                sink.send(SAMPLE_RATE, self.resample.push(&samples), None);
                match edge {
                    SilenceEdge::Silent if claim_silence_reopen(&self.silence_reopens) => {
                        tracing::warn!(
                            device = %self.input.device_name,
                            "meeting microphone delivered digital silence; reopening it"
                        );
                        return Err("microphone delivered digital silence".into());
                    }
                    SilenceEdge::Silent => {
                        tracing::warn!(
                            device = %self.input.device_name,
                            "meeting microphone is still digitally silent; keeping the stream open"
                        );
                        self.warnings.set(Some(TrackWarning::DigitalSilence));
                    }
                    SilenceEdge::Audible => {
                        tracing::info!(
                            device = %self.input.device_name,
                            "meeting microphone audio returned"
                        );
                        self.warnings.set(None);
                    }
                    SilenceEdge::Unchanged => {}
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
    fn a_digital_silence_watch_needs_three_seconds_of_exact_zeros() {
        let mut watch = SilenceWatch::new(1_000);
        assert_eq!(watch.observe(&[0.0; 2_999]), SilenceEdge::Unchanged);
        assert_eq!(watch.observe(&[0.0]), SilenceEdge::Silent);
        assert_eq!(watch.observe(&[0.0; 500]), SilenceEdge::Unchanged);
    }

    #[test]
    fn a_nonzero_sample_ends_a_digital_silence_episode() {
        let mut watch = SilenceWatch::new(1_000);
        assert_eq!(watch.observe(&[0.0; 3_000]), SilenceEdge::Silent);
        assert_eq!(watch.observe(&[0.0, 0.25]), SilenceEdge::Audible);
        assert_eq!(watch.observe(&[0.0; 2_999]), SilenceEdge::Unchanged);
        assert_eq!(watch.observe(&[0.0]), SilenceEdge::Silent);
    }

    #[test]
    fn a_route_change_restarts_the_digital_silence_watch() {
        let mut watch = SilenceWatch::new(1_000);
        assert_eq!(watch.observe(&[0.0; 3_000]), SilenceEdge::Silent);
        watch.reset(2_000);
        assert_eq!(watch.observe(&[0.0; 1_000]), SilenceEdge::Unchanged);
        assert_eq!(watch.observe(&[0.0; 5_000]), SilenceEdge::Silent);
    }

    #[test]
    fn a_digitally_silent_microphone_is_reopened_twice_before_warning() {
        let reopens = AtomicU8::new(0);
        assert!(claim_silence_reopen(&reopens));
        assert!(claim_silence_reopen(&reopens));
        assert!(!claim_silence_reopen(&reopens));
        assert_eq!(reopens.load(Ordering::Relaxed), SILENCE_REOPENS);
    }
}
