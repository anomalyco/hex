//! The capture's copy of the microphone timeline: a bounded sink that never
//! blocks the timeline. The macOS audio owner (`dictation_audio`) and the
//! Linux and Windows listeners (`linux_dictation`, `windows_dictation`) feed
//! it the same way.

use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

use crate::audio::CaptureInstant;

/// Audio for the `capture` experiment's sink, on the dictation timeline.
#[derive(Debug)]
pub enum CaptureAudio {
    Samples {
        samples: Vec<f32>,
        sample_rate: u32,
        captured_through: CaptureInstant,
    },
    /// Audio the sink never received: dropped because the sink was full, or
    /// missing from the microphone stream (a gap, failure, or reopen).
    Gap {
        from: CaptureInstant,
        through: CaptureInstant,
    },
}

/// What a closed capture sink could not deliver.
#[derive(Debug, PartialEq, Eq)]
pub struct CaptureSinkClosed {
    /// A gap still waiting for room in the sink when it closed.
    pub pending_gap: Option<(CaptureInstant, CaptureInstant)>,
    /// The end of the audio the sink accounted for, delivered or as a gap.
    pub accounted_through: CaptureInstant,
}

/// A gap shorter than this is jitter between chunks, not missing audio.
const GAP_TOLERANCE: Duration = Duration::from_millis(50);

/// The capture sink inside the owner. It never blocks the timeline: a full
/// sink drops audio and records the span as a gap to send once it has room.
pub struct CaptureSink {
    sender: SyncSender<CaptureAudio>,
    /// The capture's start: only audio before it is trimmed.
    since: CaptureInstant,
    accounted_through: CaptureInstant,
    pending_gap: Option<(CaptureInstant, CaptureInstant)>,
}

impl CaptureSink {
    pub fn new(sender: SyncSender<CaptureAudio>, since: CaptureInstant) -> Self {
        Self {
            sender,
            since,
            accounted_through: since,
            pending_gap: None,
        }
    }

    /// Offers one microphone chunk; false once the receiver is gone.
    pub fn offer(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        captured_through: CaptureInstant,
    ) -> bool {
        if captured_through <= self.since || sample_rate == 0 {
            return true;
        }
        let mut from = captured_through.sub_frames(samples.len(), sample_rate);
        let mut samples = samples;
        if from < self.since {
            // The first chunk starts before the capture: keep only its part.
            let overlap = self.since.duration_since(from);
            let frames = (overlap.as_nanos() * u128::from(sample_rate) / 1_000_000_000) as usize;
            samples = &samples[frames.min(samples.len())..];
            from = self.since;
        } else if from > self.accounted_through
            && from.duration_since(self.accounted_through) > GAP_TOLERANCE
        {
            self.extend_gap(self.accounted_through, from);
        }
        // Timestamp jitter can make a chunk overlap the last one; every frame
        // is still real audio, so none is dropped.
        from = from.max(self.accounted_through);
        self.accounted_through = self.accounted_through.max(captured_through);
        if let Some((gap_from, gap_through)) = self.pending_gap {
            match self.sender.try_send(CaptureAudio::Gap {
                from: gap_from,
                through: gap_through,
            }) {
                Ok(()) => self.pending_gap = None,
                Err(TrySendError::Full(_)) => {
                    self.extend_gap(from, captured_through);
                    return true;
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        if samples.is_empty() {
            return true;
        }
        match self.sender.try_send(CaptureAudio::Samples {
            samples: samples.to_vec(),
            sample_rate,
            captured_through,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                if self.pending_gap.is_none() {
                    tracing::warn!("capture sink is full; dropping its audio as a gap");
                }
                self.extend_gap(from, captured_through);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn extend_gap(&mut self, from: CaptureInstant, through: CaptureInstant) {
        self.pending_gap = Some(match self.pending_gap {
            Some((start, _)) => (start, through),
            None => (from, through),
        });
    }

    pub fn close(self) -> CaptureSinkClosed {
        CaptureSinkClosed {
            pending_gap: self.pending_gap,
            accounted_through: self.accounted_through,
        }
    }
}

/// The Linux or Windows listener's timeline: it reads the microphone in its own loop
/// and feeds each chunk here, so the sink sits behind a short lock. The
/// stream's timestamps count from when it opened; each stream is anchored to
/// the monotonic clock at its first chunk after a sink opens, which puts its
/// audio on the same clock as [`CaptureInstant::now`].
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct ListenerTimeline(std::sync::Mutex<ListenerState>);

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct ListenerState {
    sink: Option<CaptureSink>,
    /// Monotonic nanoseconds minus the stream's, for the current stream.
    offset: Option<i128>,
}

#[cfg(not(target_os = "macos"))]
impl ListenerTimeline {
    fn state(&self) -> std::sync::MutexGuard<'_, ListenerState> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// One microphone chunk, stamped on its stream's clock.
    pub fn feed(&self, samples: &[f32], sample_rate: u32, captured_through: CaptureInstant) {
        let mut state = self.state();
        if state.sink.is_none() {
            return;
        }
        let stream = i128::from(captured_through.as_nanos());
        let offset = *state
            .offset
            .get_or_insert_with(|| i128::from(CaptureInstant::now().as_nanos()) - stream);
        let at = CaptureInstant::from_nanos(u64::try_from(stream + offset).unwrap_or(0));
        let open = state
            .sink
            .as_mut()
            .is_some_and(|sink| sink.offer(samples, sample_rate, at));
        if !open {
            state.sink = None;
        }
    }

    /// The microphone stream closed or reopened: its clock starts over.
    pub fn restart(&self) {
        self.state().offset = None;
    }

    /// Whether a capture wants the microphone.
    pub fn wants_audio(&self) -> bool {
        self.state().sink.is_some()
    }
}

#[cfg(not(target_os = "macos"))]
impl super::AudioTimeline for ListenerTimeline {
    fn open_sink(
        &self,
        sink: SyncSender<CaptureAudio>,
        since: CaptureInstant,
    ) -> color_eyre::Result<()> {
        let mut state = self.state();
        state.sink = Some(CaptureSink::new(sink, since));
        state.offset = None;
        Ok(())
    }

    /// The listener feeds chunks on the thread that closes the sink, so the
    /// audio delivered so far is all there is: no wait.
    fn close_sink(
        &self,
        _through: CaptureInstant,
    ) -> color_eyre::Result<Option<CaptureSinkClosed>> {
        Ok(self.state().sink.take().map(CaptureSink::close))
    }
}
