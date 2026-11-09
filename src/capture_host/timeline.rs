//! A capture's audio on its own 16 kHz clock.

use std::time::Duration;

use scrawl_capture::audio::{GapSpan, RecordedAudio};

use super::{CaptureSinkClosed, GAP_TOLERANCE, MAX_CAPTURE, SAMPLE_RATE};
use crate::audio::CaptureInstant;
use crate::dictation::StreamingResampler;

/// A capture's audio at 16 kHz on its own clock: resampled as it arrives,
/// with missing spans padded with silence so positions stay on the clock.
pub(super) struct Timeline {
    t0: CaptureInstant,
    /// Where `samples[0]` sits on the clock, once audio arrived.
    first: Option<CaptureInstant>,
    pub(super) samples: Vec<f32>,
    resampler: Option<(u32, StreamingResampler)>,
    gaps: Vec<GapSpan>,
    /// The most samples kept (`MAX_CAPTURE`).
    limit: usize,
    /// Where audio past the limit started being dropped.
    truncated_at: Option<CaptureInstant>,
}

impl Timeline {
    pub(super) fn new(t0: CaptureInstant) -> Self {
        Self::with_limit(t0, MAX_CAPTURE)
    }

    pub(super) fn with_limit(t0: CaptureInstant, limit: Duration) -> Self {
        Self {
            t0,
            first: None,
            samples: Vec::new(),
            resampler: None,
            gaps: Vec::new(),
            limit: (limit.as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize,
            truncated_at: None,
        }
    }

    pub(super) fn push(&mut self, samples: &[f32], sample_rate: u32, through: CaptureInstant) {
        if samples.is_empty() || sample_rate == 0 {
            return;
        }
        let from = through.sub_frames(samples.len(), sample_rate);
        self.first.get_or_insert(from.max(self.t0));
        if self
            .resampler
            .as_ref()
            .is_none_or(|(rate, _)| *rate != sample_rate)
        {
            self.flush();
            match StreamingResampler::new(sample_rate) {
                Ok(resampler) => self.resampler = Some((sample_rate, resampler)),
                Err(error) => {
                    tracing::warn!(%error, sample_rate, "capture audio cannot be resampled");
                    self.gap(from, through);
                    return;
                }
            }
        }
        if let Some((_, resampler)) = &mut self.resampler {
            let resampled = resampler.push(samples);
            self.append(&resampled, through);
        }
    }

    pub(super) fn gap(&mut self, from: CaptureInstant, through: CaptureInstant) {
        if through.duration_since(from) <= GAP_TOLERANCE {
            return;
        }
        let Some(first) = self.first else {
            // Before any audio: the recording simply starts later.
            return;
        };
        if self.truncated_at.is_some() {
            // Past the limit: `finish` records everything after it as one gap.
            return;
        }
        self.flush();
        let limit_at = first + duration_for(self.limit);
        self.gaps.push(GapSpan {
            start_ms: self.clock_ms(from.max(first).min(limit_at)),
            end_ms: self.clock_ms(through.min(limit_at)),
        });
        // Padding stops one sample past the limit, which marks the truncation.
        let end = position(first, through).min(self.limit + 1);
        if end > self.samples.len() {
            let padding = vec![0.0; end - self.samples.len()];
            self.append(&padding, through);
        }
    }

    pub(super) fn finish(
        mut self,
        through: CaptureInstant,
        closed: Option<CaptureSinkClosed>,
    ) -> RecordedAudio {
        if let Some(closed) = closed {
            if let Some((from, until)) = closed.pending_gap {
                self.gap(from, until);
            }
            if closed.accounted_through < through {
                self.gap(closed.accounted_through.max(self.t0), through);
            }
        }
        self.flush();
        if let Some(truncated_at) = self.truncated_at {
            self.gaps.push(GapSpan {
                start_ms: self.clock_ms(truncated_at),
                end_ms: self.clock_ms(through),
            });
        }
        let Some(first) = self.first else {
            return RecordedAudio::new(Vec::new(), SAMPLE_RATE, self.clock_ms(through), Vec::new());
        };
        // Audio delivered after the stop belongs to no capture.
        self.samples.truncate(position(first, through));
        let start_offset_ms = self.clock_ms(first);
        RecordedAudio::new(self.samples, SAMPLE_RATE, start_offset_ms, self.gaps)
    }

    fn flush(&mut self) {
        if let Some((_, resampler)) = self.resampler.take() {
            let rest = resampler.finish();
            let through = self
                .first
                .map(|first| first + duration_for(self.samples.len() + rest.len()))
                .unwrap_or(self.t0);
            self.append(&rest, through);
        }
    }

    fn append(&mut self, samples: &[f32], through: CaptureInstant) {
        let limit = self.limit;
        let room = limit.saturating_sub(self.samples.len());
        if samples.len() > room && self.truncated_at.is_none() {
            tracing::warn!("capture reached its length limit; dropping the rest of its audio");
            self.truncated_at = Some(
                self.first
                    .map_or(through, |first| first + duration_for(limit)),
            );
        }
        self.samples
            .extend_from_slice(&samples[..samples.len().min(room)]);
    }

    fn clock_ms(&self, at: CaptureInstant) -> u64 {
        at.duration_since(self.t0).as_millis() as u64
    }
}

fn position(first: CaptureInstant, at: CaptureInstant) -> usize {
    (at.duration_since(first).as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize
}

fn duration_for(samples: usize) -> Duration {
    crate::audio::duration_for_frames(samples, SAMPLE_RATE)
}
