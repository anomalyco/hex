//! Independent, self-healing meeting audio sources.
//!
//! The microphone and the system audio each run on their own supervisor
//! thread. A source that fails is dropped and reopened with bounded backoff
//! for as long as the meeting records, so losing one never stops the other.
//! The system audio source is each OS's own (`super::os`): a Core
//! Audio process tap on macOS, a PipeWire monitor of the default output on
//! Linux, and WASAPI loopback on Windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

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
    pub(super) fn microphone(sink: PacketSink) -> Self {
        Self::spawn(sink, || {
            MicrophoneSource::open(crate::app_settings::microphone_selection())
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

struct MicrophoneSource {
    input: crate::audio::AudioInput,
    resample: To16k,
    last_audio: Instant,
    selected: Option<String>,
    /// The [`crate::app_settings::microphone_selection`] revision of `selected`.
    selection_revision: u64,
    /// The [`crate::audio::input_route_generation`] the input was resolved at.
    route_generation: u64,
    retry: UnopenableRetry,
}

impl MicrophoneSource {
    fn open((selection_revision, selected): (u64, Option<String>)) -> Result<Self> {
        let route_generation = crate::audio::input_route_generation();
        let crate::audio::Opened::Input(input) =
            crate::audio::open_meeting_input(selected.as_deref(), None)?
        else {
            unreachable!("nothing to keep");
        };
        let input = *input;
        let resample = To16k::new(input.sample_rate)?;
        tracing::info!(
            device = input.device_name,
            rate = resample.rate,
            "meeting microphone opened"
        );
        let mut retry = UnopenableRetry::default();
        retry.resolved(input.selection, Instant::now());
        Ok(Self {
            input,
            resample,
            last_audio: Instant::now(),
            selected,
            selection_revision,
            route_generation,
            retry,
        })
    }

    /// Follows a change of the system default or of the saved microphone in
    /// Settings, the saved microphone returning, or a due look at one that
    /// would not open, by opening the input it now resolves to before
    /// dropping the old one. A failed open keeps the current input.
    fn follow_route(&mut self) {
        let now = Instant::now();
        let generation = crate::audio::input_route_generation();
        let (revision, selected) = crate::app_settings::microphone_selection();
        if generation == self.route_generation
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
                sink.send(SAMPLE_RATE, self.resample.push(&samples), None);
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
}
