//! Microphone audio the host owns ([`Mic::External`](crate::Mic::External)).
//!
//! The host pushes mono PCM on the capture clock through an [`ExternalAudioSink`]. The sink never
//! blocks: when its bounded queue is full it drops the audio and records the span as a gap, sent
//! as soon as there is room. A writer thread places every chunk at its position on the clock,
//! pads missing spans with silence, and streams 16-bit PCM to a WAV whose first sample is the
//! clock anchor.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::manifest::AudioGap;
use crate::plan::ExternalMic;
use crate::time::HostTime;

/// A gap shorter than this is jitter between chunks, not missing audio.
pub(crate) const GAP_TOLERANCE: Duration = Duration::from_millis(50);
/// Audio younger than this stays in memory, so a chunk delivered just after stop can still be
/// cut at the stop instant.
const HOLDBACK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(20);
const NO_STOP: u64 = 0;

enum Chunk {
    Samples {
        samples: Vec<f32>,
        captured_through: HostTime,
    },
    Gap {
        from: HostTime,
        through: HostTime,
    },
}

#[derive(Debug, Default)]
struct Ledger {
    /// The end of the audio the sink accounted for, delivered or as a gap.
    accounted_through: HostTime,
    /// A gap still waiting for room in the queue.
    pending_gap: Option<(HostTime, HostTime)>,
}

/// The host's end of an external microphone. Offering never blocks.
pub struct ExternalAudioSink {
    sender: SyncSender<Chunk>,
    sample_rate: u32,
    since: HostTime,
    ledger: Arc<Mutex<Ledger>>,
}

impl ExternalAudioSink {
    /// The sample rate every chunk must have.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Offers mono samples whose last frame was captured at `captured_through`. Audio before the
    /// clock anchor is trimmed. Returns false once the recording no longer takes audio.
    pub fn offer(&mut self, samples: &[f32], captured_through: HostTime) -> bool {
        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        if captured_through <= self.since {
            return true;
        }
        let mut from = sub_frames(captured_through, samples.len(), self.sample_rate);
        let mut samples = samples;
        if from < self.since {
            // The first chunk starts before the capture: keep only its part.
            let frames = frames_between(from, self.since, self.sample_rate);
            samples = &samples[usize::try_from(frames)
                .unwrap_or(usize::MAX)
                .min(samples.len())..];
            from = self.since;
        } else if from > ledger.accounted_through
            && nanos_between(ledger.accounted_through, from) > GAP_TOLERANCE.as_nanos()
        {
            let accounted = ledger.accounted_through;
            extend_gap(&mut ledger, accounted, from);
        }
        // Timestamp jitter can make a chunk overlap the last one; every frame is still real
        // audio, so none is dropped.
        from = from.max(ledger.accounted_through);
        ledger.accounted_through = ledger.accounted_through.max(captured_through);
        if !self.send_pending_gap(&mut ledger) {
            return false;
        }
        if ledger.pending_gap.is_some() {
            extend_gap(&mut ledger, from, captured_through);
            return true;
        }
        if samples.is_empty() {
            return true;
        }
        match self.sender.try_send(Chunk::Samples {
            samples: samples.to_vec(),
            captured_through,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                tracing::warn!("external audio queue is full; recording its audio as a gap");
                extend_gap(&mut ledger, from, captured_through);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Records audio the host knows is missing, such as while its microphone reopens.
    pub fn gap(&mut self, from: HostTime, through: HostTime) -> bool {
        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        let from = from.max(self.since);
        if through <= from {
            return true;
        }
        extend_gap(&mut ledger, from, through);
        ledger.accounted_through = ledger.accounted_through.max(through);
        self.send_pending_gap(&mut ledger)
    }

    fn send_pending_gap(&self, ledger: &mut Ledger) -> bool {
        let Some((from, through)) = ledger.pending_gap else {
            return true;
        };
        match self.sender.try_send(Chunk::Gap { from, through }) {
            Ok(()) => {
                ledger.pending_gap = None;
                true
            }
            Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

fn extend_gap(ledger: &mut Ledger, from: HostTime, through: HostTime) {
    ledger.pending_gap = Some(match ledger.pending_gap {
        Some((start, end)) => (start, end.max(through)),
        None => (from, through),
    });
}

/// What the WAV holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExternalAudioSummary {
    pub frames: u64,
    pub sample_rate: u32,
    pub gaps: Vec<AudioGap>,
    pub error: Option<String>,
}

/// The writer thread and the stop signal it watches.
pub(crate) struct ExternalAudioWriter {
    stop_at: Arc<AtomicU64>,
    thread: Option<JoinHandle<ExternalAudioSummary>>,
}

impl ExternalAudioWriter {
    /// Creates the WAV and its writer, and the host's sink.
    pub(crate) fn start(
        path: &Path,
        anchor: HostTime,
        config: ExternalMic,
    ) -> std::io::Result<(Self, ExternalAudioSink)> {
        if config.sample_rate == 0 {
            return Err(std::io::Error::other(
                "sample rate must be greater than zero",
            ));
        }
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: config.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let file = File::create_new(path)?;
        let wav = hound::WavWriter::new(BufWriter::new(file), spec).map_err(io_error)?;
        let (sender, receiver) = mpsc::sync_channel(config.capacity.max(1));
        let ledger = Arc::new(Mutex::new(Ledger {
            accounted_through: anchor,
            pending_gap: None,
        }));
        let stop_at = Arc::new(AtomicU64::new(NO_STOP));
        let timeline = WavTimeline::new(wav, anchor, config.sample_rate);
        let thread_ledger = Arc::clone(&ledger);
        let thread_stop = Arc::clone(&stop_at);
        let tail_grace = config.tail_grace;
        let thread = thread::Builder::new()
            .name("capture-external-audio".into())
            .spawn(move || {
                run_writer(
                    timeline,
                    &receiver,
                    &thread_ledger,
                    &thread_stop,
                    tail_grace,
                    HostTime::now,
                )
            })?;
        Ok((
            Self {
                stop_at,
                thread: Some(thread),
            },
            ExternalAudioSink {
                sender,
                sample_rate: config.sample_rate,
                since: anchor,
                ledger,
            },
        ))
    }

    /// Tells the writer where the recording ends. Returns at once.
    pub(crate) fn signal_stop(&self, at: HostTime) {
        let _ = self.stop_at.compare_exchange(
            NO_STOP,
            at.ticks().max(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Waits for the writer to collect the tail and close the WAV.
    pub(crate) fn finish(mut self, at: HostTime) -> ExternalAudioSummary {
        self.signal_stop(at);
        self.join()
    }

    fn join(&mut self) -> ExternalAudioSummary {
        self.thread.take().map_or_else(
            || ExternalAudioSummary {
                frames: 0,
                sample_rate: 0,
                gaps: Vec::new(),
                error: Some("the external audio writer already finished".into()),
            },
            |thread| {
                thread.join().unwrap_or_else(|_| ExternalAudioSummary {
                    frames: 0,
                    sample_rate: 0,
                    gaps: Vec::new(),
                    error: Some("the external audio writer panicked".into()),
                })
            },
        )
    }
}

impl Drop for ExternalAudioWriter {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.signal_stop(HostTime::now());
            let _ = self.join();
        }
    }
}

fn run_writer(
    mut timeline: WavTimeline<BufWriter<File>>,
    receiver: &Receiver<Chunk>,
    ledger: &Mutex<Ledger>,
    stop_at: &AtomicU64,
    tail_grace: Duration,
    now: fn() -> HostTime,
) -> ExternalAudioSummary {
    let mut deadline = None;
    let stop = loop {
        let stop = match stop_at.load(Ordering::Acquire) {
            NO_STOP => None,
            ticks => Some(HostTime::from_ticks(ticks)),
        };
        if let Some(stop) = stop {
            let deadline = *deadline.get_or_insert_with(|| Instant::now() + tail_grace);
            let accounted = ledger
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .accounted_through;
            if accounted >= stop || Instant::now() >= deadline {
                // Everything already queued still belongs to the recording.
                while let Ok(chunk) = receiver.try_recv() {
                    timeline.apply(chunk);
                }
                break stop;
            }
        }
        match receiver.recv_timeout(POLL) {
            Ok(chunk) => timeline.apply(chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // The host dropped its sink; the recording ends where it says, or now.
                break stop.unwrap_or_else(now);
            }
        }
        if let Err(error) =
            timeline.flush_before(stop.unwrap_or_else(|| sub_duration(now(), HOLDBACK)))
        {
            return timeline.fail(error);
        }
    };
    let pending = ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending_gap
        .take();
    if let Some((from, through)) = pending {
        timeline.gap(from, through);
    }
    timeline.finish(stop)
}

/// A WAV on the capture clock: sample 0 is the anchor, and missing spans are silence.
struct WavTimeline<W: std::io::Write + std::io::Seek> {
    wav: Option<hound::WavWriter<W>>,
    anchor: HostTime,
    sample_rate: u32,
    /// Frames already in the file.
    written: u64,
    /// Frames after `written`, not yet in the file.
    held: Vec<f32>,
    gaps: Vec<AudioGap>,
    error: Option<String>,
}

impl<W: std::io::Write + std::io::Seek> WavTimeline<W> {
    fn new(wav: hound::WavWriter<W>, anchor: HostTime, sample_rate: u32) -> Self {
        Self {
            wav: Some(wav),
            anchor,
            sample_rate,
            written: 0,
            held: Vec::new(),
            gaps: Vec::new(),
            error: None,
        }
    }

    fn end(&self) -> u64 {
        self.written + self.held.len() as u64
    }

    fn position(&self, at: HostTime) -> u64 {
        frames_between(self.anchor, at, self.sample_rate)
    }

    fn apply(&mut self, chunk: Chunk) {
        match chunk {
            Chunk::Samples {
                samples,
                captured_through,
            } => self.push(&samples, captured_through),
            Chunk::Gap { from, through } => self.gap(from, through),
        }
    }

    fn push(&mut self, samples: &[f32], through: HostTime) {
        let from = sub_frames(through, samples.len(), self.sample_rate).max(self.anchor);
        let start = self.position(from);
        let tolerance = frames_for(GAP_TOLERANCE, self.sample_rate);
        if start > self.end() + tolerance {
            self.pad_gap(start);
        }
        self.held.extend_from_slice(samples);
    }

    fn gap(&mut self, from: HostTime, through: HostTime) {
        if nanos_between(from, through) <= GAP_TOLERANCE.as_nanos() {
            return;
        }
        let end = self.position(through);
        if end > self.end() {
            self.pad_gap(end);
        }
    }

    /// Pads silence from the current end to `end` and records the span.
    fn pad_gap(&mut self, end: u64) {
        let start = self.end();
        let start_ns = self.host_ns(start);
        let end_ns = self.host_ns(end);
        match self.gaps.last_mut() {
            Some(last) if last.end_ns >= start_ns => last.end_ns = last.end_ns.max(end_ns),
            _ => self.gaps.push(AudioGap { start_ns, end_ns }),
        }
        let padding = usize::try_from(end - start).unwrap_or(0);
        self.held.extend(std::iter::repeat_n(0.0, padding));
    }

    /// Where frame `frames` of the file sits on the host clock, in nanoseconds.
    fn host_ns(&self, frames: u64) -> u64 {
        let offset = u128::from(frames) * 1_000_000_000 / u128::from(self.sample_rate.max(1));
        self.anchor
            .as_nanos()
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }

    /// Writes held frames before `at`.
    fn flush_before(&mut self, at: HostTime) -> Result<(), String> {
        let through = self.position(at).max(self.written);
        let count = usize::try_from(through - self.written)
            .unwrap_or(usize::MAX)
            .min(self.held.len());
        if count == 0 {
            return Ok(());
        }
        let wav = self.wav.as_mut().ok_or("the WAV is closed")?;
        for sample in self.held.drain(..count) {
            wav.write_sample(to_i16(sample))
                .map_err(|error| error.to_string())?;
        }
        self.written += count as u64;
        Ok(())
    }

    /// Ends the WAV exactly at `stop`, padding any missing tail as a gap.
    fn finish(mut self, stop: HostTime) -> ExternalAudioSummary {
        let stop_frame = self.position(stop);
        if self.end() + frames_for(GAP_TOLERANCE, self.sample_rate) < stop_frame {
            self.pad_gap(stop_frame);
        } else if self.end() < stop_frame {
            let padding = usize::try_from(stop_frame - self.end()).unwrap_or(0);
            self.held.extend(std::iter::repeat_n(0.0, padding));
        }
        // Audio delivered after the stop belongs to no recording.
        let keep = usize::try_from(stop_frame.saturating_sub(self.written)).unwrap_or(usize::MAX);
        self.held.truncate(keep);
        let stop_ns = self.host_ns(stop_frame);
        self.gaps.retain_mut(|gap| {
            gap.end_ns = gap.end_ns.min(stop_ns);
            gap.start_ns < gap.end_ns
        });
        if let Err(error) = self.flush_before(stop) {
            return self.fail(error);
        }
        if let Some(wav) = self.wav.take()
            && let Err(error) = wav.finalize()
        {
            return self.fail(error.to_string());
        }
        ExternalAudioSummary {
            frames: self.written,
            sample_rate: self.sample_rate,
            gaps: std::mem::take(&mut self.gaps),
            error: self.error.take(),
        }
    }

    fn fail(mut self, error: String) -> ExternalAudioSummary {
        tracing::error!(error, "could not write the external audio");
        if let Some(wav) = self.wav.take() {
            let _ = wav.finalize();
        }
        ExternalAudioSummary {
            frames: self.written,
            sample_rate: self.sample_rate,
            gaps: std::mem::take(&mut self.gaps),
            error: Some(error),
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

fn nanos_between(from: HostTime, through: HostTime) -> u128 {
    u128::from(HostTime::from_ticks(through.ticks().saturating_sub(from.ticks())).as_nanos())
}

fn frames_between(from: HostTime, through: HostTime, sample_rate: u32) -> u64 {
    let frames = nanos_between(from, through) * u128::from(sample_rate) / 1_000_000_000;
    u64::try_from(frames).unwrap_or(u64::MAX)
}

fn frames_for(duration: Duration, sample_rate: u32) -> u64 {
    u64::try_from(duration.as_nanos() * u128::from(sample_rate) / 1_000_000_000).unwrap_or(u64::MAX)
}

fn sub_frames(at: HostTime, frames: usize, sample_rate: u32) -> HostTime {
    let nanos = (frames as u128 * 1_000_000_000) / u128::from(sample_rate.max(1));
    sub_duration(
        at,
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX)),
    )
}

fn sub_duration(at: HostTime, duration: Duration) -> HostTime {
    let ticks =
        HostTime::from_nanos(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)).ticks();
    HostTime::from_ticks(at.ticks().saturating_sub(ticks))
}

fn io_error(error: hound::Error) -> std::io::Error {
    match error {
        hound::Error::IoError(error) => error,
        error => std::io::Error::other(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::host_delta_millis;

    const RATE: u32 = 16_000;

    fn at_ms(anchor: HostTime, ms: u64) -> HostTime {
        HostTime::from_ticks(anchor.ticks() + HostTime::from_nanos(ms * 1_000_000).ticks())
    }

    fn sink(capacity: usize, anchor: HostTime) -> (ExternalAudioSink, Receiver<Chunk>) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        (
            ExternalAudioSink {
                sender,
                sample_rate: RATE,
                since: anchor,
                ledger: Arc::new(Mutex::new(Ledger {
                    accounted_through: anchor,
                    pending_gap: None,
                })),
            },
            receiver,
        )
    }

    /// A gap `start_ms`..`end_ms` from the anchor, on the host clock.
    fn gap(anchor: HostTime, start_ms: u64, end_ms: u64) -> AudioGap {
        AudioGap {
            start_ns: anchor.as_nanos() + start_ms * 1_000_000,
            end_ns: anchor.as_nanos() + end_ms * 1_000_000,
        }
    }

    fn ms_of(anchor: HostTime, at: HostTime) -> u64 {
        host_delta_millis(anchor, at).unwrap()
    }

    #[test]
    fn a_full_sink_records_a_gap_instead_of_blocking() {
        let anchor = HostTime::from_ticks(HostTime::from_nanos(10_000_000_000).ticks());
        let (mut sink, receiver) = sink(1, anchor);
        let chunk = vec![0.25; 1_600];
        let started = Instant::now();
        // The first 100 ms fills the queue; nine more chunks find it full.
        for index in 1..=10 {
            assert!(sink.offer(&chunk, at_ms(anchor, index * 100)));
        }
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "offer never blocks"
        );
        let (from, through) = sink.ledger.lock().unwrap().pending_gap.unwrap();
        assert_eq!((ms_of(anchor, from), ms_of(anchor, through)), (100, 1_000));

        assert!(matches!(receiver.try_recv(), Ok(Chunk::Samples { .. })));
        // With room again, the gap goes first and fresh audio waits behind it as more gap.
        assert!(sink.offer(&chunk, at_ms(anchor, 1_100)));
        let Ok(Chunk::Gap { from, through }) = receiver.try_recv() else {
            panic!("the gap is sent once there is room");
        };
        assert_eq!((ms_of(anchor, from), ms_of(anchor, through)), (100, 1_000));
        let (from, through) = sink.ledger.lock().unwrap().pending_gap.unwrap();
        assert_eq!(
            (ms_of(anchor, from), ms_of(anchor, through)),
            (1_000, 1_100)
        );
    }

    #[test]
    fn audio_before_the_anchor_is_trimmed() {
        let anchor = HostTime::from_ticks(HostTime::from_nanos(10_000_000_000).ticks());
        let (mut sink, receiver) = sink(4, anchor);
        let mut chunk = vec![0.0; 800];
        chunk.extend(std::iter::repeat_n(1.0, 800));
        assert!(sink.offer(&chunk, at_ms(anchor, 50)));
        let Ok(Chunk::Samples { samples, .. }) = receiver.try_recv() else {
            panic!("the part after the anchor is kept");
        };
        assert_eq!(samples.len(), 800);
        assert!(samples.iter().all(|sample| *sample == 1.0));
        assert!(sink.offer(&chunk, anchor));
        assert!(
            receiver.try_recv().is_err(),
            "audio wholly before the anchor is dropped"
        );
    }

    #[test]
    fn offer_reports_a_closed_recording() {
        let anchor = HostTime::from_ticks(1);
        let (mut sink, receiver) = sink(1, anchor);
        drop(receiver);
        assert!(!sink.offer(&[0.0; 16], at_ms(anchor, 10)));
    }

    #[test]
    fn writer_pads_gaps_with_silence_and_ends_at_stop() {
        let dir = crate::test_dir::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        // A real anchor: the writer holds back audio younger than a second, so none of these
        // future-stamped chunks reach the file before the stop is known.
        let anchor = HostTime::now();
        let config = ExternalMic {
            sample_rate: RATE,
            capacity: 16,
            tail_grace: Duration::from_millis(100),
        };
        let (writer, mut sink) = ExternalAudioWriter::start(&path, anchor, config).unwrap();
        // 0–100 ms of audio, a 200 ms hole, then 300–400 ms, then a host-reported gap to 600 ms
        // and audio to 700 ms that runs past the stop at 650 ms.
        assert!(sink.offer(&[0.5; 1_600], at_ms(anchor, 100)));
        assert!(sink.offer(&[0.5; 1_600], at_ms(anchor, 400)));
        assert!(sink.gap(at_ms(anchor, 400), at_ms(anchor, 600)));
        assert!(sink.offer(&[0.5; 1_600], at_ms(anchor, 700)));
        let summary = writer.finish(at_ms(anchor, 650));

        assert_eq!(summary.error, None);
        assert_eq!(summary.frames, 650 * 16);
        assert_eq!(
            summary.gaps,
            vec![gap(anchor, 100, 300), gap(anchor, 400, 600)]
        );
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, RATE);
        assert_eq!(reader.spec().channels, 1);
        let samples = reader
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples.len(), 650 * 16);
        assert!(samples[..1_600].iter().all(|sample| *sample > 0));
        assert!(samples[1_600..4_800].iter().all(|sample| *sample == 0));
        assert!(samples[4_800..6_400].iter().all(|sample| *sample > 0));
        assert!(samples[6_400..9_600].iter().all(|sample| *sample == 0));
        assert!(samples[9_600..].iter().all(|sample| *sample > 0));
        assert!(
            !sink.offer(&[0.5; 16], at_ms(anchor, 710)),
            "the writer is closed"
        );
    }

    #[test]
    fn missing_tail_is_a_gap_after_the_grace() {
        let dir = crate::test_dir::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        // A real anchor: the writer holds back audio younger than a second, so none of these
        // future-stamped chunks reach the file before the stop is known.
        let anchor = HostTime::now();
        let config = ExternalMic {
            sample_rate: RATE,
            capacity: 4,
            tail_grace: Duration::from_millis(30),
        };
        let (writer, mut sink) = ExternalAudioWriter::start(&path, anchor, config).unwrap();
        assert!(sink.offer(&[0.5; 1_600], at_ms(anchor, 100)));
        let started = Instant::now();
        let summary = writer.finish(at_ms(anchor, 500));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(summary.frames, 500 * 16);
        assert_eq!(summary.gaps, vec![gap(anchor, 100, 500)]);
    }
}
