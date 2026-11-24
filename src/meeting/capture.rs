//! Independent, self-healing meeting audio sources.
//!
//! The microphone and the system audio each run on their own supervisor
//! thread. A source that fails is dropped and reopened with bounded backoff
//! for as long as the meeting records, so losing one never stops the other.
//! The system audio source is each OS's own (`super::os`): a Core
//! Audio process tap on macOS, a PipeWire monitor of the default output on
//! Linux, and WASAPI loopback on Windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use super::warning::{SilenceAction, SilencePolicy, SilenceRecovery, TrackWarnings};
use super::{AudioPacket, MeetingSource, SAMPLE_RATE};
use crate::dictation::StreamingResampler;

pub(super) const RETRY_INITIAL: Duration = Duration::from_millis(500);
pub(super) const RETRY_MAX: Duration = Duration::from_secs(5);
/// A source that ran this long before failing started; one that dies sooner
/// keeps backing off instead of reopening at the initial delay.
const STARTED_AFTER: Duration = Duration::from_secs(1);
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Core Audio delivers silence continuously; this long without a buffer
/// means the device or tap stopped. A source that keeps delivering buffers
/// of exact zeros is a different failure, [`SYSTEM_SILENCE_TIMEOUT`].
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(3);
/// A system source that carried sound and then delivers only exact zeros
/// this long is treated as stalled: a tap whose default output device
/// changed keeps its IO callbacks coming, so the callbacks alone never show
/// the loss. Longer than the microphone's silence because an idle output is
/// legitimately silent.
pub(super) const SYSTEM_SILENCE_TIMEOUT: Duration = Duration::from_secs(15);

/// Delivers one source's 16 kHz audio to the track writer, stamped on the
/// meeting's shared capture clock.
#[derive(Clone)]
pub(super) struct PacketSink {
    pub(super) source: MeetingSource,
    pub(super) sender: SyncSender<AudioPacket>,
    pub(super) drops: Arc<AtomicU64>,
    pub(super) clock: Instant,
    /// The system track's digital silence, shared with its supervisor and
    /// every stream it reopens; `None` for the microphone. `send` is also
    /// called from another thread (ScreenCaptureKit's handler), so the state
    /// sits behind a mutex.
    pub(super) system_silence: Option<Arc<Mutex<SystemSilence>>>,
}

impl PacketSink {
    pub(super) fn send(&self, sample_rate: u32, samples: Vec<f32>, pts_us: Option<i64>) {
        self.send_samples(sample_rate, samples, pts_us, microphone_muted());
    }

    /// [`Self::send`] with the microphone's mute given, so a test holds it
    /// without the macOS-only Labs switch.
    fn send_samples(
        &self,
        sample_rate: u32,
        mut samples: Vec<f32>,
        pts_us: Option<i64>,
        muted: bool,
    ) {
        if samples.is_empty() {
            return;
        }
        if let Some(silence) = &self.system_silence {
            silence
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .observe(sample_rate, &samples);
        }
        // A muted microphone keeps its packets' length, and with it the WAV
        // timeline the two tracks share, but carries exact zeros. The
        // switches are read here, past the source's own silence watch, which
        // must keep hearing what the microphone really carries.
        if muted && self.source == MeetingSource::Microphone {
            samples.fill(0.0);
        }
        record_level(self.source, &samples);
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

/// Whether the recording leaves the microphone out of its track right now
/// (Labs › Mute the microphone while recording): set by the controller while
/// a meeting records and cleared when one starts, ends, or the lab goes off,
/// so every recording begins with the microphone in. Read as each microphone
/// packet arrives.
static MICROPHONE_MUTED: AtomicBool = AtomicBool::new(false);

/// Whether the microphone's audio is left out of the recording now: the
/// controller's switch, and Labs › Mute the microphone while recording still
/// on. Turning the lab off therefore restores audio without stopping.
pub fn microphone_muted() -> bool {
    MICROPHONE_MUTED.load(Ordering::Relaxed) && crate::labs::meeting_microphone_mute()
}

/// Sets the microphone's switch for the recording under way; the controller
/// owns it and clears it when a recording starts, ends, or the lab goes off.
pub fn set_microphone_muted(muted: bool) {
    MICROPHONE_MUTED.store(muted, Ordering::Relaxed);
}

/// Applies a mute request from the Meetings pane or the menu bar, and
/// reports whether it landed. It acts only while a recording runs with
/// Labs › Mute the microphone while recording on; anything else leaves the
/// switch as it is, so an idle request cannot mute the next recording.
pub(super) fn request_microphone_mute(recording: bool, lab: bool, muted: bool) -> bool {
    if !recording || !lab {
        return false;
    }
    set_microphone_muted(muted);
    true
}

/// Keeps the switch in step with the lab, once per controller pass: while the
/// lab is off the switch clears, so a lab turned back on later in the same
/// recording cannot silently mute it again. The sink already restores audio
/// by reading the same gate with each packet.
pub(super) fn reconcile_microphone_mute(lab: bool) {
    if !lab {
        set_microphone_muted(false);
    }
}

/// The controller's switch alone, whatever the lab says; a check of what a
/// request did on an OS whose lab never turns on.
#[cfg(test)]
pub(super) fn microphone_mute_set() -> bool {
    MICROPHONE_MUTED.load(Ordering::Relaxed)
}

/// The newest buffer's loudness per source (f32 bits, 0 to 1) and when it
/// arrived (milliseconds since [`level_epoch`]), for the recording pill's
/// level histories. One meeting records at a time.
static LEVELS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static LEVEL_AT_MS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// A reading older than this is silence: the source stopped delivering.
const LEVEL_STALE: Duration = Duration::from_millis(300);

fn level_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn level_slot(source: MeetingSource) -> usize {
    match source {
        MeetingSource::Microphone => 0,
        MeetingSource::System => 1,
    }
}

fn record_level(source: MeetingSource, samples: &[f32]) {
    let slot = level_slot(source);
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    LEVELS[slot].store(rms.to_bits(), Ordering::Relaxed);
    let now = level_epoch().elapsed().as_millis() as u64;
    LEVEL_AT_MS[slot].store(now.max(1), Ordering::Relaxed);
}

/// Latest microphone and system RMS, in that order, in the same units Dot scope takes.
pub fn track_levels() -> [f32; 2] {
    let now = level_epoch().elapsed().as_millis() as u64;
    std::array::from_fn(|slot| {
        fresh_level(
            now,
            LEVEL_AT_MS[slot].load(Ordering::Relaxed),
            f32::from_bits(LEVELS[slot].load(Ordering::Relaxed)),
        )
    })
}

fn fresh_level(now: u64, at: u64, level: f32) -> f32 {
    if at != 0 && now.saturating_sub(at) <= LEVEL_STALE.as_millis() as u64 {
        level
    } else {
        0.0
    }
}

#[test]
fn a_missing_or_stale_track_reads_as_silence() {
    assert_eq!(fresh_level(500, 0, 0.8), 0.0);
    assert_eq!(fresh_level(500, 199, 0.8), 0.0);
    assert_eq!(fresh_level(500, 200, 0.8), 0.8);
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
    pub(super) fn system(mut sink: PacketSink, application: Option<String>) -> Self {
        // Shared with every stream this supervisor reopens, so a reconnect
        // starts unarmed after a digital-silence restart.
        sink.system_silence = Some(Arc::new(Mutex::new(SystemSilence::default())));
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
                last_open_error = None;
                // Every stream starts with a fresh watch: it must carry
                // sound before its exact zeros can mean a stall, whatever
                // ended the last one.
                reset_system_silence(sink);
                let started = Instant::now();
                let failure = loop {
                    if stop.load(Ordering::Relaxed) {
                        break None;
                    }
                    if let Err(error) = live.poll(sink) {
                        break Some(error);
                    }
                    if let Some(error) = system_silence_stalled(sink) {
                        break Some(error);
                    }
                };
                drop(live);
                let Some(error) = failure else {
                    return;
                };
                interrupted = true;
                delay = retry_delay(delay, started.elapsed());
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

/// The wait before reopening a source that ran for `ran` before it failed:
/// the initial delay once it genuinely started, else the delay it already
/// reached, so a source that dies right after every open keeps backing off
/// instead of spawning and logging twice a second forever.
pub(super) fn retry_delay(delay: Duration, ran: Duration) -> Duration {
    if ran >= STARTED_AFTER {
        RETRY_INITIAL
    } else {
        delay
    }
}

/// Whether a system source opened on the default output `created` must
/// reopen for `current`: only a change to a different, readable output
/// counts, so an unreadable one never restarts the source.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn output_changed(created: Option<&str>, current: Option<&str>) -> bool {
    matches!((created, current), (Some(created), Some(current)) if created != current)
}

/// Consecutive exact zeros from the system track, counted only once it
/// carried sound so an idle output, or a manual recording of nothing, is not
/// mistaken for a stalled stream.
#[derive(Debug, Default)]
pub(super) struct SystemSilence {
    /// The sample rate of the last packet, for the threshold.
    rate: u32,
    /// The track carried sound at some point since the stream opened.
    armed: bool,
    /// Consecutive exact-zero samples since the last audible one.
    zeros: u64,
}

impl SystemSilence {
    /// Counts one packet's samples, delivered at `rate`.
    pub(super) fn observe(&mut self, rate: u32, samples: &[f32]) {
        self.rate = rate;
        for &sample in samples {
            if sample == 0.0 {
                self.zeros += 1;
            } else {
                self.zeros = 0;
                self.armed = true;
            }
        }
    }

    /// Whether the track carried sound and has since been exactly zero for
    /// [`SYSTEM_SILENCE_TIMEOUT`].
    pub(super) fn stalled(&self) -> bool {
        self.armed
            && self.rate > 0
            && self.zeros >= u64::from(self.rate).saturating_mul(SYSTEM_SILENCE_TIMEOUT.as_secs())
    }

    /// Starts a freshly opened stream unarmed, so its silence can only mean
    /// a stall again once it carries sound.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Ends a system source that carried sound and then went digitally silent:
/// `Some` with the reason. The stream the supervisor reopens starts unarmed
/// ([reset_system_silence]), so the same silence cannot reopen it again.
fn system_silence_stalled(sink: &PacketSink) -> Option<String> {
    let silence = sink.system_silence.as_ref()?;
    let silence = silence.lock().unwrap_or_else(|error| error.into_inner());
    silence
        .stalled()
        .then(|| "system audio went digitally silent after carrying sound".into())
}

/// Starts a freshly opened system stream unarmed.
fn reset_system_silence(sink: &PacketSink) {
    if let Some(silence) = &sink.system_silence {
        silence
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reset();
    }
}

/// 16 kHz samples each output's audio is held back before mixing, so two
/// outputs playing at once line up instead of following each other. Linux and
/// Windows mix their outputs; macOS hears one tap and compiles this only for
/// the mixer's test.
#[cfg(any(not(target_os = "macos"), test))]
pub(super) const MIX_HOLD: usize = SAMPLE_RATE as usize / 20;

/// Mixes each output's pending audio up to the latest output's position less
/// `hold`, leaving the rest for the next call. An output that delivered
/// nothing (nothing plays on it) adds nothing. Linux and Windows mix their
/// outputs; macOS hears one tap and compiles this only for the mixer's test.
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn mix(pending: &mut [&mut Vec<f32>], hold: usize) -> Vec<f32> {
    let length = pending
        .iter()
        .map(|samples| samples.len())
        .max()
        .unwrap_or(0)
        .saturating_sub(hold);
    let mut mixed = vec![0.0f32; length];
    for samples in pending.iter_mut() {
        let take = samples.len().min(length);
        for (out, sample) in mixed.iter_mut().zip(samples.drain(..take)) {
            *out += sample;
        }
    }
    for sample in &mut mixed {
        *sample = sample.clamp(-1.0, 1.0);
    }
    mixed
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

/// The UIDs of the reported devices that can record, in the app's order: an
/// output-only device a call app lists as its input, such as the MacBook Pro
/// Speakers, offers no input channels and is dropped.
fn recordable_inputs(reported: &[crate::microphone_activity::ReportedInput]) -> Vec<String> {
    reported
        .iter()
        .filter(|input| input.input_channels > 0)
        .map(|input| input.uid.clone())
        .collect()
}

/// The input a meeting follows for its call app, by UID: the one it follows
/// while the app still reports it or reports none (as some do while muted),
/// else the first input the app reports that can record.
fn follow_call_input(
    current: Option<String>,
    reported: Vec<crate::microphone_activity::ReportedInput>,
) -> Option<String> {
    let reported = recordable_inputs(&reported);
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

/// The devices the call app reports recording from, with their input
/// channel counts; none where the OS does not report them.
fn call_app_inputs(application: &str) -> Vec<crate::microphone_activity::ReportedInput> {
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
}

/// The last input whose recorders were seen, those processes, and when our
/// own renegotiating open last changed that device's rate.
#[derive(Default)]
struct SeenInput {
    uid: Option<String>,
    pids: Vec<i32>,
    /// When our own renegotiating open last changed the device's rate; a
    /// recorder that disappears just after it is the driver restarting
    /// streams, not another app letting go.
    renegotiated: Option<Instant>,
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
        let mut seen = self.seen.lock().unwrap_or_else(|error| error.into_inner());
        let same_input = seen.uid.as_deref() == uid;
        if !same_input {
            // The old input's rate change means nothing to this one.
            seen.renegotiated = None;
        }
        let settled = seen
            .renegotiated
            .is_none_or(|at| now.duration_since(at) >= RENEGOTIATION_SETTLE);
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
            renegotiated: None,
        };
    }

    /// Notes that this open renegotiated the device's sample rate, which
    /// restarts its other streams.
    fn note_renegotiation(&self, now: Instant) {
        self.seen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .renegotiated = Some(now);
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
        let reported = application
            .as_deref()
            .map(call_app_inputs)
            .unwrap_or_default();
        let call_input = follow_call_input(None, reported.clone());
        let call_inputs = recordable_inputs(&reported);
        if application.is_some() {
            tracing::debug!(
                application = application.as_deref(),
                reported = ?reported,
                followed = ?call_input,
                "meeting call app reports the inputs it records from"
            );
        }
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
        let now = Instant::now();
        // A release from the old input says nothing about the new one.
        if !recorders.same_input(input.uid.as_deref()) {
            silence_recovery.forget_release();
        }
        if application.is_some() {
            if renegotiate {
                recorders.note_renegotiation(now);
            }
            // Compare this open against the set the last stream saw, so an
            // app that let go while the source was down still counts as a
            // release. The shared state keeps it if this stream fails too.
            if let Some(current) = recording_processes(input.uid.as_deref())
                && recorders.update(input.uid.as_deref(), current, now)
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
            call_input_checked: Instant::now(),
            recorders,
            recorders_checked: Instant::now(),
        })
    }

    /// Whether the call app records from the input this stream opened, which
    /// is when another app's voice processing can leave it digitally silent.
    /// A manual recording has no call app and never holds the device.
    fn call_app_holds_device(&self) -> bool {
        call_app_reports_input(self.input.uid.as_deref(), &self.call_inputs)
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
        let Some(current) = recording_processes(self.input.uid.as_deref()) else {
            return;
        };
        if self
            .recorders
            .update(self.input.uid.as_deref(), current, now)
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
            let reported = self
                .application
                .as_deref()
                .map(call_app_inputs)
                .unwrap_or_default();
            self.call_inputs = recordable_inputs(&reported);
            let call_input = follow_call_input(self.call_input.clone(), reported.clone());
            call_input_changed = call_input != self.call_input;
            if call_input_changed {
                tracing::debug!(
                    reported = ?reported,
                    from = ?self.call_input,
                    to = ?call_input,
                    "meeting microphone follows the input its call app records from"
                );
            }
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
        if let Some(current) = recording_processes(input.uid.as_deref()) {
            self.recorders.reset(input.uid.as_deref(), current);
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
                            device_uid = ?self.input.uid,
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
            crate::audio::AudioInputEvent::StreamReconfigured => {
                Err("microphone reconfigured".into())
            }
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

    fn reported(uid: &str, input_channels: u32) -> crate::microphone_activity::ReportedInput {
        crate::microphone_activity::ReportedInput {
            uid: uid.to_owned(),
            input_channels,
        }
    }

    #[test]
    fn a_meeting_follows_the_input_its_call_app_records_from() {
        let inputs = |uids: &[&str]| uids.iter().map(|uid| reported(uid, 2)).collect::<Vec<_>>();
        let current = |uid: &str| Some(uid.to_owned());
        // The first input the app reports, until it reports a different one.
        assert_eq!(
            follow_call_input(None, inputs(&["Jabra"])),
            current("Jabra")
        );
        assert_eq!(
            follow_call_input(current("Jabra"), inputs(&["BuiltIn", "Jabra"])),
            current("Jabra")
        );
        assert_eq!(
            follow_call_input(current("Jabra"), inputs(&["BuiltIn"])),
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
    fn a_meeting_never_follows_a_device_without_input_channels() {
        // WhatsApp reported the MacBook Pro Speakers as its input device, and
        // Hex recorded speaker bleed and silence from it.
        let speakers = reported("MacBookProSpeakers", 0);
        assert_eq!(follow_call_input(None, vec![speakers.clone()]), None);
        // An input the app really records from is followed instead.
        assert_eq!(
            follow_call_input(None, vec![speakers.clone(), reported("MacBookProMic", 1)]),
            Some("MacBookProMic".to_owned())
        );
        // An output-only device never displaces the input already followed.
        assert_eq!(
            follow_call_input(Some("Jabra".to_owned()), vec![speakers.clone()]),
            Some("Jabra".to_owned())
        );
        assert_eq!(
            follow_call_input(
                Some("Jabra".to_owned()),
                vec![speakers.clone(), reported("BuiltIn", 2)]
            ),
            Some("BuiltIn".to_owned())
        );
        // A device that loses its input channels is no longer recordable.
        assert!(recordable_inputs(&[speakers, reported("BuiltIn", 0)]).is_empty());
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
    fn a_source_that_dies_right_after_each_open_keeps_backing_off() {
        let instantly = Duration::from_millis(20);
        // A source that really started reopens at the initial delay.
        assert_eq!(retry_delay(RETRY_MAX, STARTED_AFTER), RETRY_INITIAL);
        // One that failed instantly keeps the delay it already reached.
        assert_eq!(retry_delay(RETRY_MAX, instantly), RETRY_MAX);
        // So each instant failure waits longer, instead of spawning forever
        // twice a second.
        let mut delay = RETRY_INITIAL;
        let mut waits = Vec::new();
        for _ in 0..4 {
            delay = retry_delay(delay, instantly);
            waits.push(delay);
            delay = delay.saturating_mul(2).min(RETRY_MAX);
        }
        assert_eq!(waits, [500, 1000, 2000, 4000].map(Duration::from_millis));
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

    #[test]
    fn audio_on_any_output_reaches_the_system_track() {
        // Only the headset plays (#1221): its audio is kept, less the hold.
        let mut speakers = Vec::new();
        let mut headset = vec![0.25; 10];
        let mixed = mix(&mut [&mut speakers, &mut headset], 4);
        assert_eq!(mixed, vec![0.25; 6]);
        assert_eq!(headset.len(), 4);

        // Both play: their audio adds up sample by sample, clamped.
        let mut speakers = vec![0.5; 8];
        let mut headset = vec![0.75; 6];
        let mixed = mix(&mut [&mut speakers, &mut headset], 2);
        assert_eq!(mixed, vec![1.0; 6]);
        assert_eq!((speakers.len(), headset.len()), (2, 0));

        // Nothing plays: nothing is sent.
        let mut idle = Vec::new();
        assert!(mix(&mut [&mut idle], 2).is_empty());

        // The shared hold is 50 ms at 16 kHz, so a 1,000-sample buffer is
        // mixed only up to 200.
        assert_eq!(MIX_HOLD, 800);
        let mut left = vec![0.5; 1_000];
        let mut right = Vec::new();
        assert_eq!(mix(&mut [&mut left, &mut right], MIX_HOLD), vec![0.5; 200]);
    }

    #[test]
    fn a_system_source_reopens_only_for_a_different_readable_output() {
        let speakers = Some("BuiltInSpeakerDevice");
        assert!(!output_changed(speakers, speakers));
        // Bluetooth headphones went away and the speakers took over.
        assert!(output_changed(Some("AirPods Pro"), speakers));
        // An output Hex could not read says nothing about a change.
        assert!(!output_changed(Some("AirPods Pro"), None));
        assert!(!output_changed(None, speakers));
        assert!(!output_changed(None, None));
    }

    #[test]
    fn system_silence_never_fires_before_the_track_carried_sound() {
        // A manual recording with nothing playing, or a call before its
        // first sound, is exactly zero and is not a stall.
        let mut silence = SystemSilence::default();
        silence.observe(1_000, &[0.0; 20_000]);
        assert!(!silence.stalled());
    }

    #[test]
    fn system_silence_fires_after_fifteen_seconds_of_exact_zeros() {
        // Counted in samples at the packet's rate, so nothing sleeps.
        let mut silence = SystemSilence::default();
        silence.observe(1_000, &[0.25; 1_000]);
        silence.observe(1_000, &[0.0; 14_999]);
        assert!(!silence.stalled(), "14.999 s of zeros is not a stall");
        silence.observe(1_000, &[0.0]);
        assert!(silence.stalled());
    }

    #[test]
    fn an_audible_sample_restarts_the_system_zero_count() {
        let mut silence = SystemSilence::default();
        silence.observe(1_000, &[0.25]);
        silence.observe(1_000, &[0.0; 14_000]);
        silence.observe(1_000, &[0.25]);
        silence.observe(1_000, &[0.0; 14_000]);
        assert!(!silence.stalled());
        silence.observe(1_000, &[0.0; 1_000]);
        assert!(silence.stalled());
    }

    #[test]
    fn a_reopened_system_source_starts_unarmed() {
        let mut silence = SystemSilence::default();
        silence.observe(1_000, &[0.25]);
        silence.observe(1_000, &[0.0; 15_000]);
        assert!(silence.stalled());
        // The supervisor resets the shared watch before it reopens.
        silence.reset();
        silence.observe(1_000, &[0.0; 20_000]);
        assert!(!silence.stalled(), "the new stream must carry sound again");
    }

    /// Labs › Mute the microphone while recording: a muted microphone's
    /// packets carry exact zeros at the length they arrived at, so the WAV
    /// timeline and every position they share with the call's audio stay put;
    /// the system track and an unmuted microphone are untouched.
    #[test]
    fn a_muted_microphone_goes_silent_in_place_and_the_call_does_not() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let sink = |source| PacketSink {
            source,
            sender: sender.clone(),
            drops: Arc::new(AtomicU64::new(0)),
            clock: Instant::now(),
            system_silence: None,
        };
        let microphone = sink(MeetingSource::Microphone);
        microphone.send_samples(1_000, vec![0.5, -0.25, 0.125], None, true);
        microphone.send_samples(1_000, vec![0.5, -0.25, 0.125], None, false);
        let system = sink(MeetingSource::System);
        system.send_samples(1_000, vec![0.25, 0.5], None, true);

        let muted = receiver.try_recv().expect("the muted microphone sent");
        assert_eq!(
            muted.samples,
            vec![0.0; 3],
            "muted means silence, not a gap"
        );
        assert_eq!(muted.samples.len(), 3, "the packet keeps its length");
        let unmuted = receiver.try_recv().expect("the unmuted microphone sent");
        assert_eq!(unmuted.samples, vec![0.5, -0.25, 0.125]);
        let call = receiver.try_recv().expect("the system track sent");
        assert_eq!(call.samples, vec![0.25, 0.5], "the call keeps recording");
    }

    /// Serializes the tests that move the process-wide microphone switch, so
    /// one can't observe another's.
    static MICROPHONE_MUTE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn mute_test_lock() -> std::sync::MutexGuard<'static, ()> {
        MICROPHONE_MUTE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Labs › Mute the microphone while recording: a request lands only on a
    /// recording under way with the lab on; anything else leaves the switch
    /// alone, so an idle request cannot mute the next recording.
    #[test]
    fn a_microphone_mute_needs_a_recording_and_its_lab() {
        let _lock = mute_test_lock();
        set_microphone_muted(false);
        assert!(
            request_microphone_mute(true, true, true),
            "a recording with the lab on takes the request"
        );
        assert!(microphone_mute_set(), "the switch is on");
        assert!(
            request_microphone_mute(true, true, false),
            "unmuting in the same recording takes too"
        );
        assert!(!microphone_mute_set(), "the switch is off again");
        // No lab: nothing lands, and a request to unmute leaves it as it was.
        set_microphone_muted(true);
        assert!(!request_microphone_mute(true, false, false));
        assert!(
            microphone_mute_set(),
            "a lab that is off ignores the request"
        );
        // No recording: the switch a finished recording left is untouched.
        assert!(!request_microphone_mute(false, true, false));
        assert!(microphone_mute_set(), "an idle request is ignored");
        set_microphone_muted(false);
    }

    /// Labs › Mute the microphone while recording's gate also holds the
    /// switch: turning the lab off clears it on the next pass, so turning it
    /// back on later in the same recording does not silently mute again.
    #[test]
    fn a_lab_turned_off_clears_the_microphone_mute() {
        let _lock = mute_test_lock();
        set_microphone_muted(true);
        reconcile_microphone_mute(true);
        assert!(microphone_mute_set(), "a lab that is on leaves the switch");
        reconcile_microphone_mute(false);
        assert!(!microphone_mute_set(), "the lab off clears the switch");
        reconcile_microphone_mute(true);
        assert!(
            !microphone_mute_set(),
            "a lab turned back on does not re-mute"
        );
    }

    #[test]
    fn only_the_system_sink_watches_digital_silence() {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(8);
        let sink = |source, system_silence| PacketSink {
            source,
            sender: sender.clone(),
            drops: Arc::new(AtomicU64::new(0)),
            clock: Instant::now(),
            system_silence,
        };
        let system = sink(
            MeetingSource::System,
            Some(Arc::new(Mutex::new(SystemSilence::default()))),
        );
        system.send(1_000, vec![0.25], None);
        system.send(1_000, vec![0.0; 15_000], None);
        assert_eq!(
            system_silence_stalled(&system).as_deref(),
            Some("system audio went digitally silent after carrying sound")
        );
        // The supervisor resets the watch before it polls the reopened stream.
        reset_system_silence(&system);
        assert_eq!(system_silence_stalled(&system), None);

        // The microphone keeps its own SilencePolicy and has no system watch.
        let microphone = sink(MeetingSource::Microphone, None);
        microphone.send(1_000, vec![0.0; 20_000], None);
        assert_eq!(system_silence_stalled(&microphone), None);
        reset_system_silence(&microphone);
    }
}
