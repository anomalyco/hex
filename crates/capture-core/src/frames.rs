//! The frame tap: low-rate RGBA copies of each source's frames for the host, such as for a
//! contact sheet. Sending never blocks; a full channel drops the frame and counts it.
//!
//! `ScreenCaptureKit` delivers a frame only when the screen changes, so the newest skipped frame
//! is retained and sent the moment the recording stops: the host holds the final picture within
//! milliseconds of `stop()`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferExt, SCFrameStatus};

use crate::time::HostTime;

/// One downscaled frame, tightly packed RGBA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The index of the frame's source in [`Plan::sources`](crate::Plan::sources).
    pub source: usize,
    /// When the frame was composited, on the host clock ([`HostTime::as_nanos`] for nanoseconds).
    pub host_time: HostTime,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where tapped frames go.
#[derive(Clone)]
pub struct FrameTap {
    sender: SyncSender<Frame>,
    interval: Duration,
    max_width: u32,
    dropped: Arc<AtomicU64>,
}

impl FrameTap {
    /// A tap sending at most one frame per source every `interval`, at most `max_width` pixels
    /// wide, through a channel of `capacity` frames.
    #[must_use]
    pub fn new(capacity: usize, interval: Duration, max_width: u32) -> (Self, Receiver<Frame>) {
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        (
            Self {
                sender,
                interval,
                max_width: max_width.max(1),
                dropped: Arc::new(AtomicU64::new(0)),
            },
            receiver,
        )
    }

    /// Frames dropped because the channel was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn send(&self, frame: Frame) {
        match self.sender.try_send(frame) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

struct SamplerState {
    last_sent: Option<u64>,
    /// The newest frame not sent, kept by reference to its pixel buffer.
    latest: Option<(CMSampleBuffer, u64)>,
}

/// One source's view of the tap, called from its stream's frame handler.
pub(crate) struct FrameSampler {
    tap: FrameTap,
    source: usize,
    interval_ticks: u64,
    state: Mutex<SamplerState>,
}

impl FrameSampler {
    pub(crate) fn new(tap: FrameTap, source: usize) -> Self {
        let interval_ticks =
            HostTime::from_nanos(u64::try_from(tap.interval.as_nanos()).unwrap_or(u64::MAX))
                .ticks();
        Self {
            tap,
            source,
            interval_ticks,
            state: Mutex::new(SamplerState {
                last_sent: None,
                latest: None,
            }),
        }
    }

    pub(crate) fn observe(
        &self,
        sample: CMSampleBuffer,
        status: Option<SCFrameStatus>,
        display_time: Option<u64>,
    ) {
        if status != Some(SCFrameStatus::Complete) {
            return;
        }
        let Some(display_time) = display_time else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let due = state
            .last_sent
            .is_none_or(|last| display_time.saturating_sub(last) >= self.interval_ticks);
        if due {
            state.last_sent = Some(display_time);
            state.latest = None;
            drop(state);
            self.send(&sample, display_time);
        } else {
            state.latest = Some((sample, display_time));
        }
    }

    /// Sends the newest frame not yet sent, and releases it.
    pub(crate) fn flush(&self) {
        let latest = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .take();
        if let Some((sample, display_time)) = latest {
            self.send(&sample, display_time);
        }
    }

    fn send(&self, sample: &CMSampleBuffer, display_time: u64) {
        let Some(pixels) = sample.pixel_buffer() else {
            return;
        };
        let Ok(guard) = pixels.lock_read_only() else {
            return;
        };
        // SAFETY: The buffer stays locked for reading while the slice is used.
        let Some(bytes) = (unsafe { guard.as_slice() }) else {
            return;
        };
        let Some((width, height, rgba)) = downscale_bgra(
            bytes,
            guard.width(),
            guard.height(),
            guard.bytes_per_row(),
            self.tap.max_width,
        ) else {
            return;
        };
        drop(guard);
        let host_time = HostTime::from_ticks(display_time);
        self.tap.send(Frame {
            source: self.source,
            host_time,
            width,
            height,
            rgba,
        });
    }
}

/// Nearest-neighbor downscale of BGRA rows to packed RGBA at most `max_width` wide.
fn downscale_bgra(
    bytes: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
    max_width: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || bytes_per_row < width * 4 {
        return None;
    }
    if bytes.len() < bytes_per_row * (height - 1) + width * 4 {
        return None;
    }
    let out_width = width.min(usize::try_from(max_width).unwrap_or(usize::MAX));
    let out_height = (height * out_width).div_ceil(width).max(1);
    let mut rgba = Vec::with_capacity(out_width * out_height * 4);
    for y in 0..out_height {
        let row = (y * height / out_height) * bytes_per_row;
        for x in 0..out_width {
            let pixel = row + (x * width / out_width) * 4;
            rgba.extend_from_slice(&[
                bytes[pixel + 2],
                bytes[pixel + 1],
                bytes[pixel],
                bytes[pixel + 3],
            ]);
        }
    }
    Some((
        u32::try_from(out_width).ok()?,
        u32::try_from(out_height).ok()?,
        rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_swaps_channels_and_skips_row_padding() {
        // 4×2 BGRA with 4 bytes of row padding; left half blue, right half red.
        let mut bytes = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                bytes.extend_from_slice(if x < 2 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
            bytes.extend_from_slice(&[9, 9, 9, 9]);
        }
        let (width, height, rgba) = downscale_bgra(&bytes, 4, 2, 20, 2).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, vec![0, 0, 255, 255, 255, 0, 0, 255]);

        let (width, height, full) = downscale_bgra(&bytes, 4, 2, 20, 640).unwrap();
        assert_eq!((width, height), (4, 2));
        assert_eq!(full.len(), 4 * 2 * 4);
        assert!(downscale_bgra(&bytes[..10], 4, 2, 20, 2).is_none());
    }

    #[test]
    fn a_full_tap_drops_and_counts_instead_of_blocking() {
        let (tap, receiver) = FrameTap::new(1, Duration::from_millis(100), 64);
        let frame = Frame {
            source: 0,
            host_time: HostTime::from_ticks(1),
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        tap.send(frame.clone());
        tap.send(frame.clone());
        tap.send(frame);
        assert_eq!(tap.dropped(), 2);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }
}
