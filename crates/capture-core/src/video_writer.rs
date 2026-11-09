//! [`Encoder::Hevc`](crate::Encoder::Hevc)'s movie: the stream's frames appended to an
//! `AVAssetWriter` as hardware HEVC at a constant quality. `SCRecordingOutput` takes only a codec
//! and picks its own rate, which starves text in a busy picture.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use objc2_core_foundation::CFType;
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_core_video::{CVAttachmentMode, kCVImageBufferCGColorSpaceKey};
use screencapturekit::cm::{CMSampleBuffer, CMTime, SCFrameStatus};

use crate::av_writer::AvWriter;
use crate::session::RecordingEvent;
use crate::system_audio::in_timescale;
use crate::time::{HostTime, MediaTime};

const FINISH_TIMEOUT: Duration = Duration::from_secs(20);

/// The color space the stream is asked for. Each frame is tagged with it, so the writer matches
/// it to the movie's Rec. 709 tags the way Apple's players undo.
pub(crate) const STREAM_COLOR_SPACE: &str = "kCGColorSpaceSRGB";

struct State {
    writer: Option<AvWriter>,
    /// Frames composited before this are left out, as the cursor collector leaves them out.
    attached: Option<HostTime>,
    /// The first written frame's presentation time: the movie's time zero.
    first: Option<CMTime>,
    /// Where the movie ends, once detached.
    end: Option<CMTime>,
    /// Frames the encoder was not ready for.
    dropped: u64,
    failed: bool,
}

pub(crate) struct VideoWriter {
    path: PathBuf,
    events: Sender<RecordingEvent>,
    state: Mutex<State>,
}

impl VideoWriter {
    /// A movie at `path` for `width`×`height` frames arriving at up to `fps`. Results arrive on
    /// `events` as they would from a recording output.
    pub(crate) fn create(
        path: &Path,
        (width, height): (u32, u32),
        fps: u32,
        events: Sender<RecordingEvent>,
    ) -> Result<Arc<Self>, String> {
        let writer = AvWriter::hevc(path, width, height, fps)?;
        Ok(Arc::new(Self {
            path: path.to_path_buf(),
            events,
            state: Mutex::new(State {
                writer: Some(writer),
                attached: None,
                first: None,
                end: None,
                dropped: 0,
                failed: false,
            }),
        }))
    }

    /// Writes frames composited from `at` on.
    pub(crate) fn attach(&self, at: HostTime) {
        self.lock().attached = Some(at);
    }

    /// Appends one stream frame. Called on the stream's queue; never blocks on the encoder: a
    /// frame it is not ready for is dropped, and the previous one lasts until the next.
    pub(crate) fn append(
        &self,
        sample: &CMSampleBuffer,
        status: Option<SCFrameStatus>,
        display_time: Option<u64>,
    ) {
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.failed || state.end.is_some() {
            return;
        }
        let (Some(attached), Some(display_time)) = (state.attached, display_time) else {
            return;
        };
        // Idle, blank and suspended frames carry no new picture.
        if display_time < attached.ticks()
            || !matches!(
                status,
                Some(SCFrameStatus::Complete | SCFrameStatus::Started)
            )
        {
            return;
        }
        let pts = sample.presentation_timestamp();
        if !pts.is_valid() || pts.timescale <= 0 {
            return;
        }
        // SAFETY: `apple_cf`'s `CMSampleBuffer` wraps a live, retained `CMSampleBufferRef`, the
        // same object `objc2-core-media` describes.
        let native = unsafe { &*sample.as_ptr().cast::<objc2_core_media::CMSampleBuffer>() };
        // SAFETY: A live sample buffer; a screen frame's image buffer is a pixel buffer.
        let Some(image) = (unsafe { native.image_buffer() }) else {
            return;
        };
        tag_srgb(&image);
        let Some(writer) = state.writer.as_ref() else {
            return;
        };
        if state.first.is_none() {
            // SAFETY: A valid, positive-timescale time.
            let start = unsafe { objc2_core_media::CMTime::new(pts.value, pts.timescale) };
            if let Err(error) = writer.start(start) {
                state.failed = true;
                let _ = self.events.send(RecordingEvent::Failed(error));
                return;
            }
            state.first = Some(pts);
            let _ = self.events.send(RecordingEvent::Started);
        }
        match writer.append_video(native) {
            Ok(true) => {}
            Ok(false) => state.dropped += 1,
            Err(error) => {
                state.failed = true;
                let _ = self.events.send(RecordingEvent::Failed(error));
            }
        }
    }

    /// Ends the movie at `at` and finishes it on a thread of its own; the result arrives as
    /// [`RecordingEvent::Finished`] or [`RecordingEvent::Failed`].
    pub(crate) fn detach(&self, at: HostTime) -> Result<(), String> {
        let (writer, first, end, dropped) = {
            let mut state = self.lock();
            let writer = state
                .writer
                .take()
                .ok_or("the movie was already detached")?;
            let end = state
                .first
                .and_then(|first| in_timescale(at, first.timescale))
                .filter(|end| state.first.is_some_and(|first| end.value > first.value));
            state.end = end.or(state.first);
            (writer, state.first, end, state.dropped)
        };
        let events = self.events.clone();
        let path = self.path.clone();
        thread::Builder::new()
            .name("capture-video-finish".into())
            .spawn(move || {
                if first.is_none() {
                    writer.cancel();
                    let _ = std::fs::remove_file(&path);
                    let _ = events.send(RecordingEvent::Failed("no frames were recorded".into()));
                    return;
                }
                if dropped > 0 {
                    tracing::info!(dropped, "the encoder skipped frames it was not ready for");
                }
                // SAFETY: Each time is valid with a positive timescale.
                let end = end
                    .map(|end| unsafe { objc2_core_media::CMTime::new(end.value, end.timescale) });
                let _ = events.send(match writer.finish(end, FINISH_TIMEOUT) {
                    Ok(()) => RecordingEvent::Finished,
                    Err(error) => RecordingEvent::Failed(error),
                });
            })
            .map(drop)
            .map_err(|error| error.to_string())
    }

    /// From the first frame to the end, once detached.
    pub(crate) fn recorded_duration(&self) -> Option<MediaTime> {
        let state = self.lock();
        let (first, end) = (state.first?, state.end?);
        MediaTime::new(end.value.checked_sub(first.value)?, first.timescale)
    }

    /// Abandons the movie, for a source that failed to start.
    pub(crate) fn cancel(&self) {
        if let Some(writer) = self.lock().writer.take() {
            writer.cancel();
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Tags a frame as sRGB when the stream left it untagged, so the writer converts what the
/// stream was asked for rather than guessing.
fn tag_srgb(image: &objc2_core_video::CVImageBuffer) {
    // SAFETY: Reading and setting an attachment on a live buffer; the value is a color space.
    unsafe {
        if image.has_attachment(kCVImageBufferCGColorSpaceKey) {
            return;
        }
        if let Some(space) = CGColorSpace::with_name(Some(kCGColorSpaceSRGB)) {
            image.set_attachment(
                kCVImageBufferCGColorSpaceKey,
                <CGColorSpace as AsRef<CFType>>::as_ref(&space),
                CVAttachmentMode::ShouldPropagate,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{AVMediaTypeVideo, AVURLAsset};
    use objc2_core_foundation::{CFRetained, CFString};
    use objc2_core_media::{
        CMSampleTimingInfo, CMVideoFormatDescriptionCreateForImageBuffer,
        kCMFormatDescriptionExtension_ColorPrimaries, kCMTimeInvalid,
    };
    use objc2_foundation::{NSString, NSURL};

    use super::*;
    use crate::av_writer::fixtures::bgra;
    use crate::probe::probe;

    /// A screen frame as `ScreenCaptureKit` would hand it over, untagged.
    fn frame(index: usize) -> CFRetained<objc2_core_media::CMSampleBuffer> {
        let buffer = bgra(640, 360, |x, y| {
            let on = (x / 8 + y / 8 + index) % 2 == 0;
            if on {
                [20, 30, 200, 255]
            } else {
                [235, 235, 240, 255]
            }
        });
        tag_srgb(&buffer);
        let mut format = std::ptr::null();
        // SAFETY: A live pixel buffer; CoreMedia writes a +1 description.
        let status = unsafe {
            CMVideoFormatDescriptionCreateForImageBuffer(None, &buffer, NonNull::from(&mut format))
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 description.
        let format = unsafe { CFRetained::from_raw(NonNull::new(format.cast_mut()).unwrap()) };
        let timing = CMSampleTimingInfo {
            // SAFETY: Exact positive times.
            duration: unsafe { objc2_core_media::CMTime::new(1_000, 60_000) },
            presentationTimeStamp: unsafe {
                objc2_core_media::CMTime::new(i64::try_from(index * 1_000).unwrap(), 60_000)
            },
            // SAFETY: An immutable CoreMedia constant, copied by value.
            decodeTimeStamp: unsafe { kCMTimeInvalid },
        };
        let mut sample = std::ptr::null_mut();
        // SAFETY: The buffer matches its description; CoreMedia writes a +1 sample buffer.
        let status = unsafe {
            objc2_core_media::CMSampleBuffer::create_ready_with_image_buffer(
                None,
                &buffer,
                &format,
                NonNull::from(&timing),
                NonNull::from(&mut sample),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 sample buffer.
        unsafe { CFRetained::from_raw(NonNull::new(sample).unwrap()) }
    }

    #[test]
    fn writes_a_tagged_hevc_movie_from_frames() {
        let dir = crate::test_dir::tempdir().unwrap();
        let path = dir.path().join("recording.mp4");
        let writer = AvWriter::hevc(&path, 640, 360, 60).unwrap();
        // SAFETY: Exact times.
        writer
            .start(unsafe { objc2_core_media::CMTime::new(0, 60_000) })
            .unwrap();
        for index in 0..30 {
            let sample = frame(index);
            while !writer.append_video(&sample).unwrap() {
                thread::sleep(Duration::from_millis(2));
            }
        }
        // SAFETY: Exact times.
        let end = unsafe { objc2_core_media::CMTime::new(30_000, 60_000) };
        writer.finish(Some(end), FINISH_TIMEOUT).unwrap();

        assert_eq!(probe(&path).unwrap().video_end(), MediaTime::new(1, 2));
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        // SAFETY: A local file asset, read synchronously; the format description outlives the
        // track array it came from.
        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            #[allow(deprecated)]
            let tracks = asset.tracksWithMediaType(AVMediaTypeVideo.unwrap());
            let track = tracks.firstObject().unwrap();
            let formats: Retained<objc2_foundation::NSArray> = track.formatDescriptions();
            let format: Retained<AnyObject> = formats.firstObject().unwrap();
            let format =
                &*(Retained::as_ptr(&format).cast::<objc2_core_media::CMFormatDescription>());
            assert_eq!(format.media_sub_type(), u32::from_be_bytes(*b"hvc1"));
            let primaries = format
                .extension(kCMFormatDescriptionExtension_ColorPrimaries)
                .unwrap();
            let primaries = &*(CFRetained::as_ptr(&primaries).as_ptr().cast::<CFString>());
            assert_eq!(primaries.to_string(), "ITU_R_709_2");
        }
    }
}
