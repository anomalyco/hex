//! A capture's audio on its own 16 kHz clock, streamed to `audio.wav.partial`
//! until the capture's build journals it and gives it its final `audio.wav`
//! name.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use scrawl_capture::audio::{GapSpan, RecordedAudio};
use scrawl_capture::recover::PARTIAL_AUDIO;

use super::wav_file::WavFile;
use super::{CaptureSinkClosed, GAP_TOLERANCE, MAX_CAPTURE, SAMPLE_RATE};
use crate::audio::CaptureInstant;
use crate::dictation::StreamingResampler;

/// A capture's audio at 16 kHz on its own clock: resampled as it arrives and
/// written to the capture's `audio.wav.partial` as it goes, with missing spans
/// padded with silence so positions stay on the clock. Nothing is kept in
/// memory beyond the chunk being written. The file keeps its partial name when
/// the capture stops: only the build that journals a capture may publish
/// `audio.wav`, so a stopped capture waiting its turn never looks recoverable.
pub(super) struct Timeline {
    t0: CaptureInstant,
    /// Where `audio.wav` starts on the clock, once audio arrived.
    first: Option<CaptureInstant>,
    /// Where the capture's audio goes: made when the first sound arrives.
    file: Option<WavFile>,
    /// Where the file grows while the capture records.
    partial: PathBuf,
    /// Set by a write failure: the rest of the capture is one gap.
    failed: bool,
    resampler: Option<(u32, StreamingResampler)>,
    gaps: Vec<GapSpan>,
    /// The most samples written (`MAX_CAPTURE`).
    limit: usize,
    /// Where audio past the limit started being dropped.
    truncated_at: Option<CaptureInstant>,
}

impl Timeline {
    pub(super) fn new(t0: CaptureInstant, path: PathBuf) -> Self {
        Self::with_limit(t0, path, MAX_CAPTURE)
    }

    pub(super) fn with_limit(t0: CaptureInstant, path: PathBuf, limit: Duration) -> Self {
        Self {
            t0,
            first: None,
            file: None,
            partial: path.with_file_name(PARTIAL_AUDIO),
            failed: false,
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
        // Padding stops at the limit; asking past it records the rest of the
        // capture as one gap in `finish`.
        self.pad_to(position(first, through).min(self.limit + 1), through);
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
        let keep = self.lead() + position(first, through).min(self.limit);
        let written = self.written();
        let kept = match self.file.take() {
            Some(file) => match file.finish(keep) {
                Ok(kept) => kept,
                Err(error) => {
                    // The header is final but the trim failed: the file still
                    // holds every sample written, so count those.
                    tracing::error!(%error, "could not finish the capture's audio");
                    written
                }
            },
            None => {
                // No sample reached the disk (the first write failed): there is
                // no file for a recording to point at.
                let stopped = self.truncated_at.map(|at| self.clock_ms(at));
                return RecordedAudio::new(
                    Vec::new(),
                    SAMPLE_RATE,
                    self.clock_ms(first),
                    self.gaps,
                )
                .stopping_audio_at(stopped);
            }
        };
        let stopped_at = self.truncated_at.map(|at| self.clock_ms(at));
        // The file keeps its partial name. `audio.wav` may only appear once the
        // capture's build is journaled, so a stopped capture waiting its turn
        // never looks like a bundle recovery has to rebuild.
        RecordedAudio::on_disk(
            &self.partial,
            kept.saturating_sub(self.lead()),
            SAMPLE_RATE,
            self.clock_ms(first),
            self.gaps,
        )
        .stopping_audio_at(stopped_at)
    }

    /// Ends a capture that will not be written: closes the file and removes the
    /// partial audio, since nothing will give it its final name.
    pub(super) fn discard(self) {
        let partial = self.partial.clone();
        drop(self);
        let _ = std::fs::remove_file(partial);
    }

    fn flush(&mut self) {
        if let Some((_, resampler)) = self.resampler.take() {
            let rest = resampler.finish();
            let through = self
                .first
                .map(|first| first + duration_for(self.since_first() + rest.len()))
                .unwrap_or(self.t0);
            self.append(&rest, through);
        }
    }

    fn append(&mut self, samples: &[f32], through: CaptureInstant) {
        if samples.is_empty() {
            return;
        }
        let room = self.limit.saturating_sub(self.since_first());
        if samples.len() > room {
            self.stop_at_limit();
        }
        let keep = samples.len().min(room);
        if keep > 0 {
            self.write(&samples[..keep], through);
        }
    }

    /// Pads silence up to `end` samples after the first sound, never past the
    /// limit; asking past the limit marks the rest of the capture as a gap.
    fn pad_to(&mut self, end: usize, through: CaptureInstant) {
        if end > self.limit {
            self.stop_at_limit();
        }
        let end = end.min(self.limit);
        let missing = end.saturating_sub(self.since_first());
        if missing > 0 {
            self.write_silence(missing, through);
        }
    }

    fn write(&mut self, samples: &[f32], through: CaptureInstant) {
        if self.failed {
            return;
        }
        if let Err(error) = self.open() {
            self.fail(through, error);
            return;
        }
        let written = match self.file.as_mut() {
            Some(file) => file.write(samples),
            None => Ok(()),
        };
        if let Err(error) = written {
            self.fail(through, error);
        }
    }

    fn write_silence(&mut self, samples: usize, through: CaptureInstant) {
        if self.failed {
            return;
        }
        if let Err(error) = self.open() {
            self.fail(through, error);
            return;
        }
        let written = match self.file.as_mut() {
            Some(file) => file.write_silence(samples),
            None => Ok(()),
        };
        if let Err(error) = written {
            self.fail(through, error);
        }
    }

    /// Makes `audio.wav.partial` on the first sound, with the silence before it.
    fn open(&mut self) -> io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        let Some(first) = self.first else {
            return Ok(());
        };
        let lead = (self.clock_ms(first) * u64::from(SAMPLE_RATE) / 1_000) as usize;
        let mut file = WavFile::create(&self.partial)?;
        file.write_silence(lead)?;
        self.file = Some(file);
        Ok(())
    }

    fn stop_at_limit(&mut self) {
        let Some(first) = self.first else {
            return;
        };
        if self.truncated_at.is_some() {
            return;
        }
        tracing::warn!("capture reached its length limit; dropping the rest of its audio");
        self.truncated_at = Some(first + duration_for(self.limit));
    }

    fn fail(&mut self, through: CaptureInstant, error: io::Error) {
        tracing::error!(
            %error,
            "the capture's audio could not be written; the rest is a gap"
        );
        self.failed = true;
        // The file stops at the last sample that reached it, which is where the
        // failed chunk would have started.
        let stopped = self
            .first
            .map_or(through, |first| first + duration_for(self.since_first()));
        self.truncated_at.get_or_insert(stopped);
    }

    /// Records a failed write where a real one would land, for a test that
    /// needs a disk which fails after a few successful writes.
    #[cfg(test)]
    pub(super) fn write_failed(&mut self, through: CaptureInstant) {
        self.fail(through, io::Error::other("the test's disk refused a write"));
    }

    fn written(&self) -> usize {
        self.file.as_ref().map_or(0, WavFile::written)
    }

    fn lead(&self) -> usize {
        self.first.map_or(0, |first| {
            (self.clock_ms(first) * u64::from(SAMPLE_RATE) / 1_000) as usize
        })
    }

    fn since_first(&self) -> usize {
        self.written().saturating_sub(self.lead())
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
