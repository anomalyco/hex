//! Platform-facing traits for Scrawl capture, input telemetry, clock, and word-timed transcription.
//!
//! `scrawl-core` never reaches into ScreenCaptureKit, CoreAudio, EventTap, AppKit, or GPUI.
//! Host platforms implement these traits and feed captured data into [`crate::pipeline::BundleBuilder`].

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::model::{InteractionEvent, RgbaFrame, Stroke, TranscriptData};
use crate::windows::{CursorSample, WindowSnapshot};

pub use crate::windows::WindowProbe;

/// Monotonic or wall-clock time source in milliseconds.
pub trait Clock: Send + Sync {
    /// Current timestamp in milliseconds.
    fn now_ms(&self) -> u64;
}

/// Default wall-clock implementation backed by [`SystemTime`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// Source of candidate screenshots during capture and full-resolution keyframes after capture.
pub trait FrameSource {
    /// Captures a candidate screenshot stamped with `t_ms` (relative to capture start).
    fn capture_frame(&mut self, t_ms: u64) -> Result<Option<RgbaFrame>>;

    /// Decodes video frames from `video_path` at capture-relative `times_ms`, given a movie
    /// whose first frame starts at `video_offset_ms` into the capture.
    /// `max_edge == 0` requests native resolution.
    fn decode_frames(
        &mut self,
        video_path: &Path,
        times_ms: &[u64],
        video_offset_ms: u64,
        max_edge: u32,
    ) -> Result<Vec<RgbaFrame>>;
}

/// Batch of input telemetry collected over a slice of a capture session or at session stop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrawlInputBatch {
    pub events: Vec<InteractionEvent>,
    pub strokes: Vec<Stroke>,
    pub cursor: Vec<CursorSample>,
    pub windows: Vec<WindowSnapshot>,
}

/// Source of clicks, drags, scrolls, modifier shortcuts, ink/laser strokes, cursor samples, and window snapshots.
pub trait InputSource {
    /// Drains any newly observed input telemetry while the capture is live.
    fn poll_events(&mut self) -> ScrawlInputBatch;

    /// Finishes the capture session, flushing any in-progress stroke or laser gesture.
    fn finish(&mut self) -> ScrawlInputBatch;
}

/// Word-timed speech transcriber (implemented in Hex by the loaded Parakeet / Whisper model).
pub trait WordTranscriber {
    /// Transcribes a recorded narration WAV file into text, segments, and word-level timestamps.
    fn transcribe_words(&mut self, wav_path: &Path) -> Result<TranscriptData>;
}
