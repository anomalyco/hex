//! A small `AVAssetWriter` wrapper: AAC audio files from PCM sample buffers, HEVC movies from
//! screen frames, and (in tests) H.264 MP4 fixtures from BGRA pixel buffers.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus, AVFileType, AVFileTypeMPEG4,
    AVMediaTypeAudio, AVMediaTypeVideo, AVVideoAllowFrameReorderingKey, AVVideoCodecKey,
    AVVideoCodecTypeHEVC, AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPrimariesKey,
    AVVideoColorPropertiesKey, AVVideoCompressionPropertiesKey, AVVideoEncoderSpecificationKey,
    AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalDurationKey,
    AVVideoProfileLevelKey, AVVideoQualityKey, AVVideoTransferFunction_ITU_R_709_2,
    AVVideoTransferFunctionKey, AVVideoWidthKey, AVVideoYCbCrMatrix_ITU_R_709_2,
    AVVideoYCbCrMatrixKey,
};
use objc2_avf_audio::{AVEncoderBitRateKey, AVFormatIDKey, AVNumberOfChannelsKey, AVSampleRateKey};
use objc2_core_audio_types::kAudioFormatMPEG4AAC;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

pub(crate) const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub(crate) const AUDIO_CHANNELS: u32 = 2;
const AAC_BIT_RATE: u32 = 192_000;
/// HEVC's constant quality, from 0 to 1.
const HEVC_QUALITY: f64 = 0.7;
const HEVC_KEYFRAME_SECONDS: f64 = 5.0;
/// Every frame time at up to 120 fps, to a sixtieth of a millisecond.
const VIDEO_TIMESCALE: i32 = 60_000;

pub(crate) struct AvWriter {
    writer: Retained<AVAssetWriter>,
    audio: Option<Retained<AVAssetWriterInput>>,
    /// Screen frames, for [`Self::hevc`].
    screen: Option<Retained<AVAssetWriterInput>>,
    #[cfg(test)]
    video: Option<(
        Retained<AVAssetWriterInput>,
        Retained<objc2_av_foundation::AVAssetWriterInputPixelBufferAdaptor>,
    )>,
}

// SAFETY: `AVAssetWriter` and its inputs may be used from any thread as long as calls are not
// concurrent. Every owner serializes access (a `Mutex` or a single thread).
unsafe impl Send for AvWriter {}

impl AvWriter {
    /// A writer with one AAC input at 48 kHz stereo. `realtime` marks the input as fed live.
    pub(crate) fn audio(
        path: &Path,
        file_type: &AVFileType,
        realtime: bool,
    ) -> Result<Self, String> {
        let writer = create(path, file_type)?;
        let audio = add_aac_input(&writer, realtime)?;
        Ok(Self {
            writer,
            audio: Some(audio),
            screen: None,
            #[cfg(test)]
            video: None,
        })
    }

    /// A writer with one hardware HEVC input for `width`×`height` screen frames arriving at up
    /// to `fps`, sized for crisp text: constant quality 0.7, Rec. 709 tags, no frame reordering,
    /// and a keyframe at least every 5 s.
    pub(crate) fn hevc(path: &Path, width: u32, height: u32, fps: u32) -> Result<Self, String> {
        // SAFETY: An immutable framework constant.
        let file_type = unsafe { AVFileTypeMPEG4 }.ok_or("AVFileTypeMPEG4 is unavailable")?;
        let writer = create(path, file_type)?;
        let input = add_hevc_input(&writer, width, height, fps)?;
        Ok(Self {
            writer,
            audio: None,
            screen: Some(input),
            #[cfg(test)]
            video: None,
        })
    }

    /// Appends one screen frame. Returns `Ok(false)` when the encoder was not ready and the
    /// frame was dropped.
    pub(crate) fn append_video(&self, sample: &CMSampleBuffer) -> Result<bool, String> {
        let input = self.screen.as_ref().ok_or("no video input")?;
        if !self.is_writing() {
            return Err(self.message("append video"));
        }
        // SAFETY: The input belongs to this writer and calls are serialized by the owner.
        if !unsafe { input.isReadyForMoreMediaData() } {
            return Ok(false);
        }
        // SAFETY: As above; the sample buffer is a live, retained screen frame.
        if unsafe { input.appendSampleBuffer(sample) } {
            Ok(true)
        } else {
            Err(self.message("append video"))
        }
    }

    /// Starts writing with the session beginning at `at`.
    pub(crate) fn start(&self, at: CMTime) -> Result<(), String> {
        // SAFETY: Configuration is complete and writing starts exactly once.
        if !unsafe { self.writer.startWriting() } {
            return Err(self.message("start writing"));
        }
        // SAFETY: Writing started; `at` is a numeric time.
        unsafe { self.writer.startSessionAtSourceTime(at) };
        Ok(())
    }

    /// Appends one audio sample buffer. Returns `Ok(false)` when the input was not ready and the
    /// buffer was dropped.
    pub(crate) fn append_audio(&self, sample: &CMSampleBuffer) -> Result<bool, String> {
        let input = self.audio.as_ref().ok_or("no audio input")?;
        if !self.is_writing() {
            return Err(self.message("append audio"));
        }
        // SAFETY: The input belongs to this writer and calls are serialized by the owner.
        if !unsafe { input.isReadyForMoreMediaData() } {
            return Ok(false);
        }
        // SAFETY: As above; the sample buffer is a live, retained PCM buffer.
        if unsafe { input.appendSampleBuffer(sample) } {
            Ok(true)
        } else {
            Err(self.message("append audio"))
        }
    }

    pub(crate) fn is_writing(&self) -> bool {
        // SAFETY: Status is a thread-safe property.
        unsafe { self.writer.status() == AVAssetWriterStatus::Writing }
    }

    /// Marks every input finished, optionally ends the session at `end`, and waits up to
    /// `timeout` for the file to close.
    pub(crate) fn finish(self, end: Option<CMTime>, timeout: Duration) -> Result<(), String> {
        if !self.is_writing() {
            return Err(self.message("finish writing"));
        }
        // SAFETY: All appends returned; inputs are finished once.
        unsafe {
            if let Some(audio) = &self.audio {
                audio.markAsFinished();
            }
            if let Some(screen) = &self.screen {
                screen.markAsFinished();
            }
            #[cfg(test)]
            if let Some((video, _)) = &self.video {
                video.markAsFinished();
            }
            if let Some(end) = end {
                self.writer.endSessionAtSourceTime(end);
            }
        }
        let completion = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = Arc::clone(&completion);
        let callback = RcBlock::new(move || {
            let (lock, wake) = &*signal;
            *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
            wake.notify_all();
        });
        // SAFETY: The callback owns only Send + Sync synchronization state.
        unsafe { self.writer.finishWritingWithCompletionHandler(&callback) };
        let (lock, wake) = &*completion;
        let (done, _) = wake
            .wait_timeout_while(
                lock.lock().unwrap_or_else(PoisonError::into_inner),
                timeout,
                |done| !*done,
            )
            .unwrap_or_else(PoisonError::into_inner);
        let done = *done;
        // SAFETY: Status is a thread-safe property.
        match unsafe { self.writer.status() } {
            AVAssetWriterStatus::Completed => Ok(()),
            _ if !done => {
                // SAFETY: The writer is still finishing; cancelling stops it.
                unsafe { self.writer.cancelWriting() };
                Err("timed out finishing the file".into())
            }
            _ => Err(self.message("finish writing")),
        }
    }

    pub(crate) fn cancel(&self) {
        if self.is_writing() {
            // SAFETY: Calls are serialized by the owner.
            unsafe { self.writer.cancelWriting() };
        }
    }

    fn message(&self, operation: &str) -> String {
        // SAFETY: Error and status are thread-safe properties.
        unsafe {
            self.writer.error().map_or_else(
                || format!("{operation}: writer status {:?}", self.writer.status()),
                |error| format!("{operation}: {}", error.localizedDescription()),
            )
        }
    }
}

fn create(path: &Path, file_type: &AVFileType) -> Result<Retained<AVAssetWriter>, String> {
    let path = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // SAFETY: The URL and file type are valid; the writer retains them.
    let writer = unsafe { AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type) }
        .map_err(|error| error.localizedDescription().to_string())?;
    // A 48 kHz movie timescale represents every audio frame and every millisecond exactly.
    // SAFETY: Configuration is mutable before writing starts.
    unsafe { writer.setMovieTimeScale(AUDIO_SAMPLE_RATE.cast_signed()) };
    Ok(writer)
}

fn add_aac_input(
    writer: &AVAssetWriter,
    realtime: bool,
) -> Result<Retained<AVAssetWriterInput>, String> {
    // SAFETY: Framework constants are immutable process-lifetime objects.
    let (media_type, format_key, rate_key, channels_key, bit_rate_key) = unsafe {
        (
            AVMediaTypeAudio.ok_or("AVMediaTypeAudio is unavailable")?,
            AVFormatIDKey.ok_or("AVFormatIDKey is unavailable")?,
            AVSampleRateKey.ok_or("AVSampleRateKey is unavailable")?,
            AVNumberOfChannelsKey.ok_or("AVNumberOfChannelsKey is unavailable")?,
            AVEncoderBitRateKey.ok_or("AVEncoderBitRateKey is unavailable")?,
        )
    };
    let format = NSNumber::new_u32(kAudioFormatMPEG4AAC);
    let rate = NSNumber::new_f64(f64::from(AUDIO_SAMPLE_RATE));
    let channels = NSNumber::new_u32(AUDIO_CHANNELS);
    let bit_rate = NSNumber::new_u32(AAC_BIT_RATE);
    let settings = NSDictionary::from_slices(
        &[format_key, rate_key, channels_key, bit_rate_key],
        &[
            &*format as &AnyObject,
            &*rate as &AnyObject,
            &*channels as &AnyObject,
            &*bit_rate as &AnyObject,
        ],
    );
    // SAFETY: The settings fully specify AAC-LC at 48 kHz stereo.
    let input = unsafe {
        AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_type,
            Some(&settings),
        )
    };
    // SAFETY: The input is unattached and writing has not started.
    if !unsafe { writer.canAddInput(&input) } {
        return Err("the writer cannot take an AAC input".into());
    }
    // SAFETY: `canAddInput` succeeded and writing has not started.
    unsafe {
        input.setExpectsMediaDataInRealTime(realtime);
        writer.addInput(&input);
    }
    Ok(input)
}

/// Hardware HEVC for screen frames. The constant quality is the knee measured on Retina text:
/// 0.6 loses 4 dB of luma PSNR, 0.8 gains 2 dB for twice the bytes. A live input turns
/// VideoToolbox's real-time mode on, which spends more bytes for less detail on screen content
/// (38.8 dB at 98 MB/min against 43.4 dB at 56 MB/min), so it is turned back off. Without
/// reordering, the final-frame hold never needs a skipped picture.
fn add_hevc_input(
    writer: &AVAssetWriter,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<Retained<AVAssetWriterInput>, String> {
    // SAFETY: Framework constants are immutable process-lifetime objects.
    let (media_type, codec, codec_key, width_key, height_key) = unsafe {
        (
            AVMediaTypeVideo.ok_or("AVMediaTypeVideo is unavailable")?,
            AVVideoCodecTypeHEVC.ok_or("AVVideoCodecTypeHEVC is unavailable")?,
            AVVideoCodecKey.ok_or("AVVideoCodecKey is unavailable")?,
            AVVideoWidthKey.ok_or("AVVideoWidthKey is unavailable")?,
            AVVideoHeightKey.ok_or("AVVideoHeightKey is unavailable")?,
        )
    };
    // SAFETY: As above.
    let (color_key, compression_key, specification_key) = unsafe {
        (
            AVVideoColorPropertiesKey.ok_or("AVVideoColorPropertiesKey is unavailable")?,
            AVVideoCompressionPropertiesKey
                .ok_or("AVVideoCompressionPropertiesKey is unavailable")?,
            AVVideoEncoderSpecificationKey
                .ok_or("AVVideoEncoderSpecificationKey is unavailable")?,
        )
    };
    // SAFETY: As above.
    let color = unsafe {
        NSDictionary::from_slices(
            &[
                AVVideoColorPrimariesKey.ok_or("AVVideoColorPrimariesKey is unavailable")?,
                AVVideoTransferFunctionKey.ok_or("AVVideoTransferFunctionKey is unavailable")?,
                AVVideoYCbCrMatrixKey.ok_or("AVVideoYCbCrMatrixKey is unavailable")?,
            ],
            &[
                AVVideoColorPrimaries_ITU_R_709_2.ok_or("Rec. 709 primaries are unavailable")?
                    as &AnyObject,
                AVVideoTransferFunction_ITU_R_709_2.ok_or("Rec. 709 transfer is unavailable")?
                    as &AnyObject,
                AVVideoYCbCrMatrix_ITU_R_709_2.ok_or("the Rec. 709 matrix is unavailable")?
                    as &AnyObject,
            ],
        )
    };
    // VideoToolbox's keys by value: `objc2-video-toolbox` is not a dependency.
    let realtime_key = NSString::from_str("RealTime");
    let main_profile = NSString::from_str("HEVC_Main_AutoLevel");
    let quality = NSNumber::new_f64(HEVC_QUALITY);
    let rate = NSNumber::new_u32(fps);
    let keyframe = NSNumber::new_f64(HEVC_KEYFRAME_SECONDS);
    let off = NSNumber::new_bool(false);
    // SAFETY: As above.
    let compression = unsafe {
        NSDictionary::from_slices(
            &[
                AVVideoQualityKey.ok_or("AVVideoQualityKey is unavailable")?,
                AVVideoExpectedSourceFrameRateKey
                    .ok_or("AVVideoExpectedSourceFrameRateKey is unavailable")?,
                AVVideoMaxKeyFrameIntervalDurationKey
                    .ok_or("AVVideoMaxKeyFrameIntervalDurationKey is unavailable")?,
                AVVideoAllowFrameReorderingKey
                    .ok_or("AVVideoAllowFrameReorderingKey is unavailable")?,
                AVVideoProfileLevelKey.ok_or("AVVideoProfileLevelKey is unavailable")?,
                &*realtime_key,
            ],
            &[
                &*quality as &AnyObject,
                &*rate as &AnyObject,
                &*keyframe as &AnyObject,
                &*off as &AnyObject,
                &*main_profile as &AnyObject,
                &*off as &AnyObject,
            ],
        )
    };
    let hardware = NSDictionary::from_slices(
        &[&*NSString::from_str(
            "RequireHardwareAcceleratedVideoEncoder",
        )],
        &[&*NSNumber::new_bool(true) as &AnyObject],
    );
    let width = NSNumber::new_u32(width);
    let height = NSNumber::new_u32(height);
    let settings = NSDictionary::from_slices(
        &[
            codec_key,
            width_key,
            height_key,
            color_key,
            compression_key,
            specification_key,
        ],
        &[
            codec as &AnyObject,
            &*width as &AnyObject,
            &*height as &AnyObject,
            &*color as &AnyObject,
            &*compression as &AnyObject,
            &*hardware as &AnyObject,
        ],
    );
    // SAFETY: The settings fully specify HEVC output.
    let input = unsafe {
        AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_type,
            Some(&settings),
        )
    };
    // SAFETY: The input is unattached and writing has not started.
    if !unsafe { writer.canAddInput(&input) } {
        return Err("the writer cannot take an HEVC input".into());
    }
    // SAFETY: `canAddInput` succeeded and writing has not started.
    unsafe {
        input.setExpectsMediaDataInRealTime(true);
        input.setMediaTimeScale(VIDEO_TIMESCALE);
        writer.addInput(&input);
    }
    Ok(input)
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Media fixtures written in-test, so no test needs screen capture or a microphone.

    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_av_foundation::{
        AVAssetWriterInputPixelBufferAdaptor, AVVideoCodecTypeH264,
        AVVideoProfileLevelH264HighAutoLevel,
    };
    use objc2_core_audio_types::{
        AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked,
        kAudioFormatFlagsNativeEndian, kAudioFormatLinearPCM,
    };
    use objc2_core_foundation::CFRetained;
    use objc2_core_media::{
        CMAudioFormatDescriptionCreate, CMBlockBuffer, CMFormatDescription, CMItemCount,
        CMSampleTimingInfo, kCMTimeInvalid,
    };
    use objc2_core_video::{
        CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress,
        CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
    };

    use super::*;
    use crate::time::MediaTime;

    const BYTES_PER_FRAME: u32 = 8;

    pub(crate) fn cm_time(time: MediaTime) -> CMTime {
        // SAFETY: MediaTime guarantees a non-negative value and a positive timescale.
        unsafe { CMTime::new(time.value(), time.timescale()) }
    }

    impl AvWriter {
        /// An H.264 MP4 writer at `width`×`height`, with an AAC input when `with_audio`.
        pub(crate) fn mp4(path: &Path, width: usize, height: usize, with_audio: bool) -> Self {
            Self::mp4_with(path, width, height, with_audio, false)
        }

        /// Like [`Self::mp4`], but High profile with frame reordering (B-frames) allowed.
        pub(crate) fn mp4_reordered(
            path: &Path,
            width: usize,
            height: usize,
            with_audio: bool,
        ) -> Self {
            Self::mp4_with(path, width, height, with_audio, true)
        }

        fn mp4_with(
            path: &Path,
            width: usize,
            height: usize,
            with_audio: bool,
            reordered: bool,
        ) -> Self {
            // SAFETY: Framework constants are immutable process-lifetime objects.
            let file_type = unsafe { AVFileTypeMPEG4 }.expect("AVFileTypeMPEG4");
            let writer = create(path, file_type).expect("create writer");
            // SAFETY: Framework constants are immutable process-lifetime objects.
            let (media_type, codec_key, width_key, height_key, codec) = unsafe {
                (
                    AVMediaTypeVideo.expect("video"),
                    AVVideoCodecKey.expect("codec key"),
                    AVVideoWidthKey.expect("width key"),
                    AVVideoHeightKey.expect("height key"),
                    AVVideoCodecTypeH264.expect("h264"),
                )
            };
            let width_number = NSNumber::new_usize(width);
            let height_number = NSNumber::new_usize(height);
            let mut keys = vec![codec_key, width_key, height_key];
            let mut values = vec![
                codec as &AnyObject,
                &*width_number as &AnyObject,
                &*height_number as &AnyObject,
            ];
            // SAFETY: Framework constants are immutable process-lifetime objects.
            let (compression_key, reorder_key, profile_key, high_profile) = unsafe {
                (
                    AVVideoCompressionPropertiesKey.expect("compression key"),
                    AVVideoAllowFrameReorderingKey.expect("reordering key"),
                    AVVideoProfileLevelKey.expect("profile key"),
                    AVVideoProfileLevelH264HighAutoLevel.expect("high profile"),
                )
            };
            let allow = NSNumber::new_bool(true);
            let compression = NSDictionary::from_slices(
                &[reorder_key, profile_key],
                &[&*allow as &AnyObject, high_profile as &AnyObject],
            );
            if reordered {
                keys.push(compression_key);
                values.push(&*compression as &AnyObject);
            }
            let settings = NSDictionary::from_slices(&keys, &values);
            // SAFETY: The settings fully specify H.264 output.
            let input = unsafe {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                    media_type,
                    Some(&settings),
                )
            };
            // SAFETY: Writing has not started.
            unsafe {
                assert!(writer.canAddInput(&input));
                input.setExpectsMediaDataInRealTime(false);
                input.setMediaTimeScale(30_000);
                writer.addInput(&input);
            }
            let audio = with_audio.then(|| add_aac_input(&writer, false).expect("audio input"));
            let pixel_format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
            let attributes = NSDictionary::from_slices(
                &[
                    &*NSString::from_str("PixelFormatType"),
                    &*NSString::from_str("Width"),
                    &*NSString::from_str("Height"),
                ],
                &[
                    &*pixel_format as &AnyObject,
                    &*width_number as &AnyObject,
                    &*height_number as &AnyObject,
                ],
            );
            // SAFETY: The attributes match every buffer the fixtures append.
            let adaptor = unsafe {
                AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                    &input,
                    Some(&attributes),
                )
            };
            let writer = Self {
                writer,
                audio,
                screen: None,
                video: Some((input, adaptor)),
            };
            writer.start(cm_time(MediaTime::ZERO)).expect("start");
            writer
        }

        pub(crate) fn append_frame(&self, buffer: &CVPixelBuffer, at: MediaTime) {
            let (input, adaptor) = self.video.as_ref().expect("video input");
            // SAFETY: Calls are serialized on the test thread.
            unsafe {
                while !input.isReadyForMoreMediaData() {
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(adaptor.appendPixelBuffer_withPresentationTime(buffer, cm_time(at)));
            }
        }

        /// Appends interleaved stereo float PCM starting at `start_frame`, blocking until the
        /// input takes it.
        pub(crate) fn append_pcm(&self, samples: &[f32], start_frame: u64) {
            let buffer = pcm_sample_buffer(samples, start_frame, AUDIO_SAMPLE_RATE);
            let input = self.audio.as_ref().expect("audio input");
            // SAFETY: Calls are serialized on the test thread.
            unsafe {
                while !input.isReadyForMoreMediaData() {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            assert_eq!(self.append_audio(&buffer), Ok(true));
        }
    }

    /// A solid BGRA pixel buffer.
    pub(crate) fn solid_bgra(
        width: usize,
        height: usize,
        color: [u8; 4],
    ) -> CFRetained<CVPixelBuffer> {
        bgra(width, height, |_, _| color)
    }

    /// A BGRA pixel buffer with each pixel's color from `pixel(x, y)`.
    pub(crate) fn bgra(
        width: usize,
        height: usize,
        pixel: impl Fn(usize, usize) -> [u8; 4],
    ) -> CFRetained<CVPixelBuffer> {
        let mut buffer = std::ptr::null_mut();
        // SAFETY: Valid dimensions and format; CoreVideo writes a +1 buffer to `buffer`.
        let status = unsafe {
            CVPixelBufferCreate(
                None,
                width,
                height,
                kCVPixelFormatType_32BGRA,
                None,
                NonNull::from(&mut buffer),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 buffer.
        let buffer = unsafe { CFRetained::from_raw(NonNull::new(buffer).expect("pixel buffer")) };
        // SAFETY: The buffer is locked while its bytes are written.
        unsafe {
            assert_eq!(
                CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)),
                0
            );
            let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
            let bytes_per_row = CVPixelBufferGetBytesPerRow(&buffer);
            for y in 0..height {
                for x in 0..width {
                    let color = pixel(x, y);
                    base.add(y * bytes_per_row + x * 4)
                        .copy_from_nonoverlapping(color.as_ptr(), color.len());
                }
            }
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0)),
                0
            );
        }
        buffer
    }

    fn pcm_format() -> CFRetained<CMFormatDescription> {
        let mut description = std::ptr::null();
        let asbd = AudioStreamBasicDescription {
            mSampleRate: f64::from(AUDIO_SAMPLE_RATE),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat
                | kAudioFormatFlagIsPacked
                | kAudioFormatFlagsNativeEndian,
            mBytesPerPacket: BYTES_PER_FRAME,
            mFramesPerPacket: 1,
            mBytesPerFrame: BYTES_PER_FRAME,
            mChannelsPerFrame: AUDIO_CHANNELS,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        // SAFETY: The description fully describes packed float stereo PCM.
        let status = unsafe {
            CMAudioFormatDescriptionCreate(
                None,
                NonNull::from(&asbd),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                None,
                NonNull::from(&mut description),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 description.
        unsafe { CFRetained::from_raw(NonNull::new(description.cast_mut()).expect("format")) }
    }

    /// Interleaved stereo float PCM as a sample buffer at `start_frame / timescale`.
    pub(crate) fn pcm_sample_buffer(
        samples: &[f32],
        start_frame: u64,
        timescale: u32,
    ) -> CFRetained<CMSampleBuffer> {
        let format = pcm_format();
        let byte_len = std::mem::size_of_val(samples);
        let mut block = std::ptr::null_mut();
        // SAFETY: A null memory block asks CoreMedia to allocate `byte_len` bytes.
        let status = unsafe {
            CMBlockBuffer::create_with_memory_block(
                None,
                std::ptr::null_mut(),
                byte_len,
                None,
                std::ptr::null(),
                0,
                byte_len,
                0,
                NonNull::from(&mut block),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 block.
        let block = unsafe { CFRetained::from_raw(NonNull::new(block).expect("block")) };
        let source = NonNull::new(samples.as_ptr().cast_mut().cast::<c_void>()).expect("samples");
        // SAFETY: `source` covers `byte_len` initialized bytes; CoreMedia copies them.
        assert_eq!(
            unsafe { CMBlockBuffer::replace_data_bytes(source, &block, 0, byte_len) },
            0
        );
        let timing = CMSampleTimingInfo {
            // SAFETY: Exact positive times.
            duration: unsafe { CMTime::new(1, AUDIO_SAMPLE_RATE.cast_signed()) },
            presentationTimeStamp: unsafe {
                CMTime::new(
                    i64::try_from(start_frame).expect("start"),
                    timescale.cast_signed(),
                )
            },
            // SAFETY: An immutable CoreMedia constant, copied by value.
            decodeTimeStamp: unsafe { kCMTimeInvalid },
        };
        let frames = samples.len() / AUDIO_CHANNELS as usize;
        let sample_size = BYTES_PER_FRAME as usize;
        let mut sample_buffer = std::ptr::null_mut();
        // SAFETY: The block holds `frames` packed PCM frames matching `format`.
        let status = unsafe {
            CMSampleBuffer::create_ready(
                None,
                Some(&block),
                Some(&format),
                CMItemCount::try_from(frames).expect("frames"),
                1,
                &raw const timing,
                1,
                &raw const sample_size,
                NonNull::from(&mut sample_buffer),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: A successful create returned a non-null +1 sample buffer.
        unsafe { CFRetained::from_raw(NonNull::new(sample_buffer).expect("sample buffer")) }
    }

    /// Stereo 440 Hz tone frames `start..start + frames`.
    pub(crate) fn tone(start: u64, frames: u64) -> Vec<f32> {
        (start..start + frames)
            .flat_map(|frame| {
                #[allow(clippy::cast_precision_loss)]
                let phase = frame as f32 * 440.0 / 48_000.0 * std::f32::consts::TAU;
                let sample = phase.sin() * 0.25;
                [sample, sample]
            })
            .collect()
    }
}
