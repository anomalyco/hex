//! System audio as its own AAC file, written from the first source's stream audio output.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use objc2_av_foundation::AVFileTypeAppleM4A;
use screencapturekit::cm::{CMSampleBuffer, CMTime};

use crate::av_writer::AvWriter;
use crate::time::HostTime;

const FINISH_TIMEOUT: Duration = Duration::from_secs(10);

struct State {
    writer: Option<AvWriter>,
    /// The first sample's presentation time, on the host clock.
    first: Option<CMTime>,
    /// The end of the last appended sample.
    end: Option<CMTime>,
    dropped: u64,
    error: Option<String>,
    /// Where the recording stopped; later audio is not written.
    cutoff: Option<HostTime>,
    closed: bool,
}

/// What the system audio file holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SystemAudioSummary {
    /// Whether the file exists: no file is left when no system audio arrived.
    pub written: bool,
    /// Where the file's time zero sits on the host clock, in nanoseconds.
    pub start_ns: Option<u64>,
    pub dropped: u64,
    pub error: Option<String>,
}

pub(crate) struct SystemAudioWriter {
    path: PathBuf,
    state: Mutex<State>,
}

impl SystemAudioWriter {
    pub(crate) fn create(path: &Path) -> Result<Self, String> {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        // SAFETY: An immutable framework constant.
        let file_type = unsafe { AVFileTypeAppleM4A }.ok_or("AVFileTypeAppleM4A is unavailable")?;
        let writer = AvWriter::audio(path, file_type, true)?;
        Ok(Self {
            path: path.to_path_buf(),
            state: Mutex::new(State {
                writer: Some(writer),
                first: None,
                end: None,
                dropped: 0,
                error: None,
                cutoff: None,
                closed: false,
            }),
        })
    }

    /// Appends one system audio buffer from the stream. Called on the stream's queue.
    pub(crate) fn append(&self, sample: &CMSampleBuffer) {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let state = &mut *guard;
        if state.closed || state.error.is_some() {
            return;
        }
        let pts = sample.presentation_timestamp();
        if !pts.is_valid() || pts.timescale <= 0 {
            return;
        }
        if let Some(cutoff) = state.cutoff
            && host_time_of(pts).is_none_or(|at| at >= cutoff)
        {
            return;
        }
        let Some(writer) = state.writer.as_ref() else {
            return;
        };
        if state.first.is_none() {
            // SAFETY: A valid, positive-timescale time.
            let start = unsafe { objc2_core_media::CMTime::new(pts.value, pts.timescale) };
            if let Err(error) = writer.start(start) {
                state.error = Some(error);
                return;
            }
            state.first = Some(pts);
        }
        // SAFETY: `apple_cf`'s `CMSampleBuffer` wraps a live, retained `CMSampleBufferRef`, the
        // same object `objc2-core-media` describes.
        let native = unsafe { &*sample.as_ptr().cast::<objc2_core_media::CMSampleBuffer>() };
        match writer.append_audio(native) {
            Ok(true) => state.end = end_of(pts, sample.duration()).or(state.end),
            Ok(false) => state.dropped += 1,
            Err(error) => state.error = Some(error),
        }
    }

    /// Ends the file at `at`, the moment the recording stopped. Returns at once.
    pub(crate) fn cut(&self, at: HostTime) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cutoff = Some(at);
    }

    /// Closes the file. Later buffers are ignored.
    pub(crate) fn finish(&self) -> SystemAudioSummary {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let state = &mut *guard;
        state.closed = true;
        let writer = state.writer.take();
        let start_ns = state.first.and_then(host_time_of).map(HostTime::as_nanos);
        let mut error = state.error.take();
        let mut written = false;
        if let Some(writer) = writer {
            if state.first.is_none() {
                writer.cancel();
                let _ = std::fs::remove_file(&self.path);
                tracing::debug!("no system audio arrived");
            } else {
                written = true;
                let end = state.end.map(|end| {
                    match state
                        .cutoff
                        .and_then(|cutoff| in_timescale(cutoff, end.timescale))
                    {
                        Some(cutoff) if cutoff.value < end.value => cutoff,
                        _ => end,
                    }
                });
                // SAFETY: Each time is valid with a positive timescale.
                let end = end
                    .map(|end| unsafe { objc2_core_media::CMTime::new(end.value, end.timescale) });
                if let Err(finish_error) = writer.finish(end, FINISH_TIMEOUT) {
                    error.get_or_insert(finish_error);
                }
            }
        }
        if let Some(error) = &error {
            tracing::warn!(error, "system audio is incomplete");
        }
        SystemAudioSummary {
            written,
            start_ns,
            dropped: state.dropped,
            error,
        }
    }
}

/// `pts + duration`, exact when both share a timescale.
fn end_of(pts: CMTime, duration: CMTime) -> Option<CMTime> {
    if !duration.is_valid() || duration.value < 0 || duration.timescale <= 0 {
        return None;
    }
    if duration.timescale == pts.timescale {
        return Some(CMTime::new(
            pts.value.checked_add(duration.value)?,
            pts.timescale,
        ));
    }
    let scaled = i128::from(duration.value) * i128::from(pts.timescale);
    let divisor = i128::from(duration.timescale);
    let value = i128::from(pts.value) + (scaled + divisor - 1) / divisor;
    Some(CMTime::new(i64::try_from(value).ok()?, pts.timescale))
}

/// A host time as a media time in `timescale`, rounded down.
fn in_timescale(at: HostTime, timescale: i32) -> Option<CMTime> {
    if timescale <= 0 {
        return None;
    }
    let value = i128::from(at.as_nanos()) * i128::from(timescale) / 1_000_000_000;
    Some(CMTime::new(i64::try_from(value).ok()?, timescale))
}

/// `ScreenCaptureKit` stamps audio on the host clock, in seconds.
fn host_time_of(time: CMTime) -> Option<HostTime> {
    if time.value < 0 || time.timescale <= 0 {
        return None;
    }
    let nanos = i128::from(time.value) * 1_000_000_000 / i128::from(time.timescale);
    Some(HostTime::from_nanos(u64::try_from(nanos).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_is_exact_in_the_presentation_timescale() {
        let end = end_of(CMTime::new(96_000, 48_000), CMTime::new(1_024, 48_000)).unwrap();
        assert_eq!((end.value, end.timescale), (97_024, 48_000));
        let end = end_of(CMTime::new(2_000, 1_000), CMTime::new(1, 3)).unwrap();
        assert_eq!((end.value, end.timescale), (2_334, 1_000));
        assert!(end_of(CMTime::new(0, 1_000), CMTime::new(1, 0)).is_none());
    }

    #[test]
    fn a_cutoff_converts_into_the_audio_timescale() {
        let at = HostTime::from_nanos(1_500_000_000);
        let cut = in_timescale(at, 48_000).unwrap();
        assert!((cut.value - 72_000).abs() <= 1, "{}", cut.value);
        assert!(in_timescale(at, 0).is_none());
    }

    #[test]
    fn audio_host_time_is_in_seconds() {
        let at = host_time_of(CMTime::new(3, 2)).unwrap();
        assert!(at.as_nanos().abs_diff(1_500_000_000) < 1_000);
        assert_eq!(host_time_of(CMTime::new(-1, 2)), None);
    }
}
