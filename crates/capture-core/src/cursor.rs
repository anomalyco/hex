//! Cursor telemetry: a ~240 Hz pointer sampler on the host clock, and per-source collectors that
//! map pointer positions and clicks onto each video's own frame times.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};
use screencapturekit::cm::{CMTime, SCFrameStatus};
use serde::{Deserialize, Serialize};

use crate::time::HostTime;

/// The largest [`NormalizedCoordinate`]: the far edge of the captured frame.
pub const NORMALIZED_COORDINATE_MAX: u32 = 1_000_000;

const OUTSIDE_TOLERANCE_POINTS: f64 = 1.0;
const POINTER_SAMPLE_INTERVAL: Duration = Duration::from_micros(4_167);
const POINTER_HISTORY_CAPACITY: usize = 2_048;

/// Whole milliseconds on a source video's own timeline.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Millis(pub u64);

/// A position along one axis of the captured frame, from 0 to [`NORMALIZED_COORDINATE_MAX`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NormalizedCoordinate(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CursorPositionSample {
    pub source_time: Millis,
    pub x: NormalizedCoordinate,
    pub y: NormalizedCoordinate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CursorButtonEvent {
    pub source_time: Millis,
    pub button: CursorButton,
    pub state: CursorButtonState,
}

/// Cursor positions and button transitions on one source's timeline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CursorTelemetry {
    pub positions: Vec<CursorPositionSample>,
    pub button_events: Vec<CursorButtonEvent>,
}

impl CursorTelemetry {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() && self.button_events.is_empty()
    }

    /// Drops samples after `duration` and keeps only valid button transitions.
    #[must_use]
    pub fn sanitized_for_duration(mut self, duration: Millis) -> Self {
        self.positions
            .retain(|sample| sample.source_time <= duration);
        let mut pressed = HashSet::new();
        self.button_events.retain(|event| {
            if event.source_time > duration {
                return false;
            }
            match event.state {
                CursorButtonState::Pressed => pressed.insert(event.button),
                CursorButtonState::Released => pressed.remove(&event.button),
            }
        });
        self
    }
}

/// Whether clicks are being captured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorClickCaptureStatus {
    #[default]
    NotRequested,
    Active,
    PermissionDenied,
    MonitorUnavailable,
}

/// Counters explaining every frame and click the collector kept or skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CursorCaptureDiagnostics {
    pub frames_received: u64,
    pub samples_recorded: u64,
    pub frames_before_attachment: u64,
    pub missing_frame_status: u64,
    pub missing_display_time: u64,
    pub ineligible_frame_status: u64,
    pub invalid_timestamp: u64,
    pub timestamp_epoch_mismatch: u64,
    pub non_monotonic_timestamp: u64,
    pub missing_geometry: u64,
    pub retained_geometry_fallbacks: u64,
    pub pointer_unavailable: u64,
    pub pointer_outside_capture: u64,
    pub click_status: CursorClickCaptureStatus,
    pub click_events_received: u64,
    pub click_events_recorded: u64,
    pub click_events_before_attachment: u64,
    pub click_events_without_anchor: u64,
    pub click_events_without_geometry: u64,
    pub click_events_outside_capture: u64,
    pub invalid_click_transitions: u64,
    pub tap_disabled_by_timeout: u64,
    pub tap_disabled_by_user_input: u64,
    pub tap_reenabled: u64,
    pub click_monitor_failures: u64,
    pub click_monitor_stop_failures: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PointerSample {
    host_time: u64,
    point: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RawCursorButtonEvent {
    pub host_time: u64,
    pub point: Point,
    pub button: CursorButton,
    pub state: CursorButtonState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingCursorButtonEvent {
    host_time: u64,
    position: Option<(NormalizedCoordinate, NormalizedCoordinate)>,
    button: CursorButton,
    state: CursorButtonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameAnchor {
    host_time: u64,
    source_time: Millis,
}

#[derive(Debug)]
pub(crate) struct PointerHistory {
    samples: VecDeque<PointerSample>,
    capacity: usize,
}

impl PointerHistory {
    fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, sample: PointerSample) {
        if self.capacity == 0 {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub(crate) fn point_at(&self, host_time: u64) -> Option<Point> {
        let first = self.samples.front()?;
        if host_time < first.host_time {
            return None;
        }
        let mut after = self.samples.back()?;
        if host_time >= after.host_time {
            return after.point;
        }
        for before in self.samples.iter().rev().skip(1) {
            if host_time >= before.host_time {
                if host_time == before.host_time {
                    return before.point;
                }
                return interpolate(*before, *after, host_time);
            }
            after = before;
        }
        first.point
    }
}

/// Samples the pointer every ~4.2 ms into a short history that frames look up by display time.
/// Its thread lives only as long as the recording.
pub(crate) struct PointerSampler {
    history: Arc<Mutex<PointerHistory>>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PointerSampler {
    pub(crate) fn start() -> std::io::Result<Self> {
        let history = Arc::new(Mutex::new(PointerHistory::new(POINTER_HISTORY_CAPACITY)));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_history = Arc::clone(&history);
        let thread_stopping = Arc::clone(&stopping);
        let thread = thread::Builder::new()
            .name("capture-pointer-sampler".into())
            .spawn(move || {
                let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
                while !thread_stopping.load(Ordering::Acquire) {
                    let sample_started = HostTime::now().ticks();
                    let point = CGEvent::new(source.as_deref()).map(|event| {
                        let location = CGEvent::location(Some(&event));
                        Point {
                            x: location.x,
                            y: location.y,
                        }
                    });
                    let sample_finished = HostTime::now().ticks();
                    let sample = PointerSample {
                        host_time: sample_started
                            .saturating_add(sample_finished.saturating_sub(sample_started) / 2),
                        point,
                    };
                    thread_history
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(sample);
                    thread::sleep(POINTER_SAMPLE_INTERVAL);
                }
            })?;
        Ok(Self {
            history,
            stopping,
            thread: Some(thread),
        })
    }

    pub(crate) fn history(&self) -> Arc<Mutex<PointerHistory>> {
        Arc::clone(&self.history)
    }

    pub(crate) fn stop(&mut self) -> bool {
        self.stopping.store(true, Ordering::Release);
        self.thread
            .take()
            .is_none_or(|thread| thread.join().is_ok())
    }
}

impl Drop for PointerSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

fn interpolate(before: PointerSample, after: PointerSample, host_time: u64) -> Option<Point> {
    let before_point = before.point?;
    let after_point = after.point?;
    let span = after.host_time.checked_sub(before.host_time)?;
    if span == 0 {
        return Some(after_point);
    }
    let offset = host_time.checked_sub(before.host_time)?.min(span);
    #[allow(clippy::cast_precision_loss)]
    let fraction = offset as f64 / span as f64;
    Some(Point {
        x: before_point.x + (after_point.x - before_point.x) * fraction,
        y: before_point.y + (after_point.y - before_point.y) * fraction,
    })
}

impl Rect {
    pub(crate) fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timestamp {
    value: i64,
    timescale: i32,
    epoch: i64,
}

impl Timestamp {
    fn from_cm_time(time: CMTime) -> Option<Self> {
        (time.is_valid()
            && !time.is_indefinite()
            && !time.is_positive_infinity()
            && !time.is_negative_infinity()
            && time.timescale > 0)
            .then_some(Self {
                value: time.value,
                timescale: time.timescale,
                epoch: time.epoch,
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CursorCapture {
    pub telemetry: CursorTelemetry,
    pub diagnostics: CursorCaptureDiagnostics,
}

/// Maps one source's frames, pointer samples and clicks onto that source's timeline.
pub(crate) struct CursorCollector {
    capture_positions: bool,
    last_valid_rect: Option<Rect>,
    attachment_host_time: Option<u64>,
    closed: bool,
    origin: Option<Timestamp>,
    last_timestamp: Option<Timestamp>,
    frame_anchors: Vec<FrameAnchor>,
    pending_clicks: Vec<PendingCursorButtonEvent>,
    capture: CursorCapture,
}

impl CursorCollector {
    #[cfg(test)]
    pub(crate) fn new(startup_rect: Rect) -> Self {
        Self::for_capture(startup_rect, true)
    }

    pub(crate) fn for_capture(startup_rect: Rect, capture_positions: bool) -> Self {
        Self {
            capture_positions,
            last_valid_rect: startup_rect.is_valid().then_some(startup_rect),
            attachment_host_time: None,
            closed: false,
            origin: None,
            last_timestamp: None,
            frame_anchors: Vec::new(),
            pending_clicks: Vec::new(),
            capture: CursorCapture::default(),
        }
    }

    pub(crate) fn arm(&mut self, attachment_host_time: u64) {
        if !self.closed {
            self.attachment_host_time = Some(attachment_host_time);
        }
    }

    pub(crate) fn first_frame_host_time(&self) -> Option<u64> {
        self.frame_anchors.first().map(|anchor| anchor.host_time)
    }

    pub(crate) fn close(&mut self) -> CursorCapture {
        self.finish_clicks();
        self.closed = true;
        std::mem::take(&mut self.capture)
    }

    pub(crate) fn set_click_status(&mut self, status: CursorClickCaptureStatus) {
        self.capture.diagnostics.click_status = status;
    }

    pub(crate) const fn click_status(&self) -> CursorClickCaptureStatus {
        self.capture.diagnostics.click_status
    }

    pub(crate) fn monitor_unavailable(&mut self) {
        self.capture.diagnostics.click_status = CursorClickCaptureStatus::MonitorUnavailable;
        self.capture.diagnostics.click_monitor_failures += 1;
    }

    pub(crate) fn monitor_stop_failed(&mut self) {
        self.capture.diagnostics.click_monitor_stop_failures += 1;
    }

    pub(crate) fn tap_disabled(&mut self, timeout: bool) {
        if timeout {
            self.capture.diagnostics.tap_disabled_by_timeout += 1;
        } else {
            self.capture.diagnostics.tap_disabled_by_user_input += 1;
        }
    }

    pub(crate) fn tap_reenabled(&mut self, enabled: bool) {
        if enabled {
            self.capture.diagnostics.tap_reenabled += 1;
        } else {
            self.monitor_unavailable();
        }
    }

    pub(crate) fn observe_click(&mut self, event: RawCursorButtonEvent) {
        if self.closed {
            return;
        }
        self.capture.diagnostics.click_events_received += 1;
        let position = if !event.point.x.is_finite() || !event.point.y.is_finite() {
            self.capture.diagnostics.click_events_outside_capture += 1;
            None
        } else if let Some(rect) = self.last_valid_rect {
            if let Some((x, y)) = normalize(event.point, rect) {
                Some((NormalizedCoordinate(x), NormalizedCoordinate(y)))
            } else {
                self.capture.diagnostics.click_events_outside_capture += 1;
                None
            }
        } else {
            self.capture.diagnostics.click_events_without_geometry += 1;
            None
        };
        // An outside press has no visible meaning, but an outside release must remain in the
        // transition stream so a preceding inside press cannot leave the button stuck down.
        if position.is_none() && event.state == CursorButtonState::Pressed {
            return;
        }
        self.pending_clicks.push(PendingCursorButtonEvent {
            host_time: event.host_time,
            position,
            button: event.button,
            state: event.state,
        });
    }

    pub(crate) fn observe_cm_frame(
        &mut self,
        status: Option<SCFrameStatus>,
        pts: CMTime,
        display_time: Option<u64>,
        screen_rect: Option<Rect>,
        pointer: Option<Point>,
    ) {
        self.observe_frame(
            status,
            Timestamp::from_cm_time(pts),
            display_time,
            screen_rect,
            pointer,
        );
    }

    fn observe_frame(
        &mut self,
        status: Option<SCFrameStatus>,
        timestamp: Option<Timestamp>,
        display_time: Option<u64>,
        screen_rect: Option<Rect>,
        pointer: Option<Point>,
    ) {
        if self.closed {
            return;
        }
        self.capture.diagnostics.frames_received += 1;
        let Some(display_time) = display_time else {
            self.capture.diagnostics.missing_display_time += 1;
            return;
        };
        if self
            .attachment_host_time
            .is_none_or(|attachment| display_time < attachment)
        {
            self.capture.diagnostics.frames_before_attachment += 1;
            return;
        }
        if status.is_none() {
            self.capture.diagnostics.missing_frame_status += 1;
        } else if !status.is_some_and(timestamp_bearing_status) {
            self.capture.diagnostics.ineligible_frame_status += 1;
            return;
        }
        let Some(timestamp) = timestamp else {
            self.capture.diagnostics.invalid_timestamp += 1;
            return;
        };

        let origin = *self.origin.get_or_insert(timestamp);
        let Some(source_time) = elapsed_ms(origin, timestamp) else {
            if origin.epoch == timestamp.epoch {
                self.capture.diagnostics.non_monotonic_timestamp += 1;
            } else {
                self.capture.diagnostics.timestamp_epoch_mismatch += 1;
            }
            return;
        };
        if self
            .last_timestamp
            .is_some_and(|last| elapsed_ms(last, timestamp).is_none())
        {
            self.capture.diagnostics.non_monotonic_timestamp += 1;
            return;
        }
        self.last_timestamp = Some(timestamp);

        let rect = if let Some(rect) = screen_rect.filter(|rect| rect.is_valid()) {
            self.last_valid_rect = Some(rect);
            rect
        } else {
            self.capture.diagnostics.missing_geometry += 1;
            let Some(rect) = self.last_valid_rect else {
                return;
            };
            self.capture.diagnostics.retained_geometry_fallbacks += 1;
            rect
        };
        self.frame_anchors.push(FrameAnchor {
            host_time: display_time,
            source_time: Millis(source_time),
        });
        if !self.capture_positions {
            return;
        }
        let Some(pointer) = pointer.filter(|point| point.x.is_finite() && point.y.is_finite())
        else {
            self.capture.diagnostics.pointer_unavailable += 1;
            return;
        };
        let Some((x, y)) = normalize(pointer, rect) else {
            self.capture.diagnostics.pointer_outside_capture += 1;
            return;
        };
        self.capture.telemetry.positions.push(CursorPositionSample {
            source_time: Millis(source_time),
            x: NormalizedCoordinate(x),
            y: NormalizedCoordinate(y),
        });
        self.capture.diagnostics.samples_recorded += 1;
    }

    fn finish_clicks(&mut self) {
        let attachment_host_time = self.attachment_host_time;
        let anchors = &self.frame_anchors;
        let mut mapped = self
            .pending_clicks
            .drain(..)
            .filter_map(|event| {
                if attachment_host_time.is_none_or(|attachment| event.host_time < attachment) {
                    self.capture.diagnostics.click_events_before_attachment += 1;
                    return None;
                }
                let Some(anchor) = nearest_anchor(anchors, event.host_time) else {
                    self.capture.diagnostics.click_events_without_anchor += 1;
                    return None;
                };
                Some((anchor.source_time, event))
            })
            .collect::<Vec<_>>();
        // Stable sorting retains callback order when several events map to the same frame time.
        mapped.sort_by_key(|(source_time, _)| *source_time);

        let mut pressed = HashSet::new();
        for (source_time, event) in mapped {
            let valid = match event.state {
                CursorButtonState::Pressed => pressed.insert(event.button),
                CursorButtonState::Released => pressed.remove(&event.button),
            };
            if !valid {
                self.capture.diagnostics.invalid_click_transitions += 1;
                continue;
            }
            if let Some((x, y)) = event.position {
                self.capture
                    .telemetry
                    .positions
                    .push(CursorPositionSample { source_time, x, y });
            }
            self.capture
                .telemetry
                .button_events
                .push(CursorButtonEvent {
                    source_time,
                    button: event.button,
                    state: event.state,
                });
            self.capture.diagnostics.click_events_recorded += 1;
        }
        self.capture
            .telemetry
            .positions
            .sort_by_key(|sample| sample.source_time);
    }
}

/// Every source's collector, so one click reaches each video that shows it.
#[derive(Clone, Default)]
pub(crate) struct Collectors(pub Vec<Arc<Mutex<CursorCollector>>>);

impl Collectors {
    fn each(&self, mut apply: impl FnMut(&mut CursorCollector)) {
        for collector in &self.0 {
            apply(&mut collector.lock().unwrap_or_else(PoisonError::into_inner));
        }
    }

    pub(crate) fn observe_click(&self, event: RawCursorButtonEvent) {
        self.each(|collector| collector.observe_click(event));
    }

    pub(crate) fn set_click_status(&self, status: CursorClickCaptureStatus) {
        self.each(|collector| collector.set_click_status(status));
    }

    pub(crate) fn click_status(&self) -> CursorClickCaptureStatus {
        self.0
            .first()
            .map_or(CursorClickCaptureStatus::NotRequested, |collector| {
                collector
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .click_status()
            })
    }

    pub(crate) fn monitor_unavailable(&self) {
        self.each(CursorCollector::monitor_unavailable);
    }

    pub(crate) fn monitor_stop_failed(&self) {
        self.each(CursorCollector::monitor_stop_failed);
    }

    pub(crate) fn tap_disabled(&self, timeout: bool) {
        self.each(|collector| collector.tap_disabled(timeout));
    }

    pub(crate) fn tap_reenabled(&self, enabled: bool) {
        self.each(|collector| collector.tap_reenabled(enabled));
    }
}

fn nearest_anchor(anchors: &[FrameAnchor], host_time: u64) -> Option<FrameAnchor> {
    anchors
        .iter()
        .min_by_key(|anchor| anchor.host_time.abs_diff(host_time))
        .copied()
}

const fn timestamp_bearing_status(status: SCFrameStatus) -> bool {
    matches!(
        status,
        SCFrameStatus::Complete | SCFrameStatus::Idle | SCFrameStatus::Started
    )
}

fn elapsed_ms(origin: Timestamp, timestamp: Timestamp) -> Option<u64> {
    if origin.epoch != timestamp.epoch {
        return None;
    }
    let origin_scale = i128::from(origin.timescale);
    let timestamp_scale = i128::from(timestamp.timescale);
    if origin_scale <= 0 || timestamp_scale <= 0 {
        return None;
    }
    let numerator = i128::from(timestamp.value)
        .checked_mul(origin_scale)?
        .checked_sub(i128::from(origin.value).checked_mul(timestamp_scale)?)?
        .checked_mul(1_000)?;
    if numerator < 0 {
        return None;
    }
    let denominator = timestamp_scale.checked_mul(origin_scale)?;
    u64::try_from(numerator / denominator).ok()
}

fn normalize(point: Point, rect: Rect) -> Option<(u32, u32)> {
    if !rect.is_valid() {
        return None;
    }
    let x = point.x - rect.x;
    let y = point.y - rect.y;
    if x < -OUTSIDE_TOLERANCE_POINTS
        || y < -OUTSIDE_TOLERANCE_POINTS
        || x > rect.width + OUTSIDE_TOLERANCE_POINTS
        || y > rect.height + OUTSIDE_TOLERANCE_POINTS
    {
        return None;
    }
    Some((
        normalize_axis(x, rect.width),
        normalize_axis(y, rect.height),
    ))
}

fn normalize_axis(offset: f64, extent: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let normalized = ((offset.clamp(0.0, extent) / extent) * f64::from(NORMALIZED_COORDINATE_MAX))
        .round() as u32;
    normalized.min(NORMALIZED_COORDINATE_MAX)
}

#[cfg(test)]
mod tests;
