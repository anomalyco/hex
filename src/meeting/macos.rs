//! The macOS side of meetings: system audio through a Core Audio process
//! tap, with ScreenCaptureKit as the fallback when the tap is not
//! authorized, its consents, and Finder and Trash.

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Instant;

use block2::RcBlock;
use color_eyre::eyre::{Result, WrapErr, eyre};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress, CATapDescription, CATapMuteBehavior,
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved,
};
use objc2_core_foundation::{CFDictionary, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use screencapturekit::prelude::*;
use screencapturekit::stream::delegate_trait::ErrorHandler;

use super::SAMPLE_RATE;
use super::capture::{LiveSource, POLL_INTERVAL, PacketSink, STALL_TIMEOUT, To16k};

const TAP_QUEUE_CAPACITY: usize = 512;

unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Whether macOS currently allows Screen & System Audio Recording, which
/// meeting capture needs for remote audio.
pub(crate) fn screen_recording_permitted() -> bool {
    // SAFETY: A CoreGraphics query without arguments or side effects.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Asks macOS to list Hex under Screen & System Audio Recording. The system
/// prompts at most once; later requests return the current state.
pub(crate) fn request_screen_recording() -> bool {
    // SAFETY: A CoreGraphics request without arguments.
    unsafe { CGRequestScreenCaptureAccess() }
}

/// Whether meeting capture can hear the other participants: through the
/// System Audio Recording consent process taps need, or Screen Recording for
/// the ScreenCaptureKit fallback.
pub(crate) fn system_audio_permitted() -> bool {
    system_audio_ready() || screen_recording_permitted()
}

/// Whether system audio uses the display-independent process tap.
pub(crate) fn system_audio_ready() -> bool {
    matches!(
        system_audio_authorization(),
        SystemAudioAuthorization::Granted | SystemAudioAuthorization::Unknown
    )
}

/// Fails, after asking macOS, until Hex may hear the other participants.
pub(super) fn ensure_system_audio() -> Result<()> {
    if system_audio_permitted() {
        return Ok(());
    }
    if !request_system_audio_access() {
        request_screen_recording();
    }
    Err(eyre!(
        "Allow Hex in System Settings › Privacy & Security › Screen & System Audio Recording, then try again"
    ))
}

pub(crate) fn reveal(path: &Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        tracing::error!(%error, "could not reveal meeting file");
    }
}

pub(super) fn move_to_trash(path: &Path) -> Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| {
            eyre!(
                "could not move {} to the Trash: {}",
                path.display(),
                error.localizedDescription()
            )
        })
}

pub(super) fn open_system_audio(sink: &PacketSink) -> Result<Box<dyn LiveSource>> {
    let authorization = system_audio_authorization();
    let screen_permitted = screen_recording_permitted();
    let prefer_tap = match authorization {
        SystemAudioAuthorization::Granted | SystemAudioAuthorization::Unknown => true,
        SystemAudioAuthorization::NotDetermined => {
            request_system_audio_access();
            !screen_permitted
        }
        SystemAudioAuthorization::Denied => false,
    };
    if prefer_tap {
        match ProcessTapSource::start() {
            Ok(source) => return Ok(Box::new(source)),
            Err(error) if screen_permitted => {
                tracing::warn!(%error, "could not start the system audio tap; using ScreenCaptureKit");
            }
            Err(error) => return Err(error),
        }
    }
    if !screen_permitted {
        return Err(eyre!(
            "Allow Hex under System Settings › Privacy & Security › Screen & System Audio Recording"
        ));
    }
    ScreenCaptureSource::start(sink).map(|source| Box::new(source) as Box<dyn LiveSource>)
}

/// Whole-system output, minus Hex's own sounds, through a private Core Audio
/// process tap on a private aggregate device. It has no display dependency.
struct ProcessTapSource {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    io_proc: AudioDeviceIOProcID,
    buffers: Receiver<Vec<f32>>,
    callbacks: Arc<AtomicU64>,
    format_error: Arc<AtomicBool>,
    seen_callbacks: u64,
    last_callback: Instant,
    resample: To16k,
}

// SAFETY: The Core Audio object IDs and IOProc handle are plain identifiers
// used only from the owning supervisor thread.
unsafe impl Send for ProcessTapSource {}

impl ProcessTapSource {
    fn start() -> Result<Self> {
        let excluded = own_process_object()
            .map(|object| vec![NSNumber::new_u32(object)])
            .unwrap_or_default();
        let excluded = NSArray::from_retained_slice(&excluded);
        // SAFETY: Standard CATapDescription initialization and setters.
        let description = unsafe {
            let description = CATapDescription::initMonoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            );
            description.setName(&NSString::from_str("Hex meeting system audio"));
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
            description
        };
        let mut tap: AudioObjectID = 0;
        // SAFETY: `description` is a valid tap description; `tap` is writable.
        check(
            unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) },
            "could not create the system audio tap",
        )?;
        let (sender, buffers) = mpsc::sync_channel::<Vec<f32>>(TAP_QUEUE_CAPACITY);
        let mut source = Self {
            tap,
            aggregate: 0,
            io_proc: None,
            buffers,
            callbacks: Arc::new(AtomicU64::new(0)),
            format_error: Arc::new(AtomicBool::new(false)),
            seen_callbacks: 0,
            last_callback: Instant::now(),
            resample: To16k::new(SAMPLE_RATE)?,
        };
        let format = tap_format(tap)?;
        if format.mFormatFlags & kAudioFormatFlagIsFloat == 0 || format.mBitsPerChannel != 32 {
            return Err(eyre!(
                "unsupported system audio tap format (flags {:#x}, {} bits)",
                format.mFormatFlags,
                format.mBitsPerChannel
            ));
        }
        let rate = format.mSampleRate.round() as u32;
        source.resample = To16k::new(rate)?;
        // SAFETY: Reading the UUID of a live description.
        let tap_uid = unsafe { description.UUID() }.UUIDString();
        source.aggregate = create_tap_aggregate(
            &tap_uid,
            "Hex Meeting System Audio",
            &format!("anomaly.hex.meeting-tap.{}", std::process::id()),
        )?;

        let channels = format.mChannelsPerFrame.max(1) as usize;
        let non_interleaved = format.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let callbacks = source.callbacks.clone();
        let format_error = source.format_error.clone();
        let block = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  _output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                callbacks.fetch_add(1, Ordering::Relaxed);
                // SAFETY: Core Audio passes a valid buffer list for this cycle.
                match unsafe { mono_samples(input, channels, non_interleaved) } {
                    Some(samples) => {
                        let _ = sender.try_send(samples);
                    }
                    None => format_error.store(true, Ordering::Relaxed),
                }
            },
        );
        let mut io_proc: AudioDeviceIOProcID = None;
        // SAFETY: The aggregate exists; Core Audio copies the block.
        check(
            unsafe {
                AudioDeviceCreateIOProcIDWithBlock(
                    NonNull::from(&mut io_proc),
                    source.aggregate,
                    None,
                    RcBlock::as_ptr(&block),
                )
            },
            "could not attach to the system audio tap",
        )?;
        source.io_proc = io_proc;
        // SAFETY: Starting the IOProc created above.
        check(
            unsafe { AudioDeviceStart(source.aggregate, source.io_proc) },
            "could not start the system audio tap",
        )?;
        source.last_callback = Instant::now();
        tracing::info!(rate, channels, "system audio tap started");
        Ok(source)
    }
}

impl LiveSource for ProcessTapSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        let mut samples = Vec::new();
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                samples.extend(first);
                while let Ok(next) = self.buffers.try_recv() {
                    samples.extend(next);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("system audio tap disconnected".into());
            }
        }
        if !samples.is_empty() {
            sink.send(SAMPLE_RATE, self.resample.push(&samples), None);
        }
        if self.format_error.load(Ordering::Relaxed) {
            return Err("system audio tap delivered an unsupported buffer".into());
        }
        let callbacks = self.callbacks.load(Ordering::Relaxed);
        if callbacks != self.seen_callbacks {
            self.seen_callbacks = callbacks;
            self.last_callback = Instant::now();
        } else if self.last_callback.elapsed() > STALL_TIMEOUT {
            return Err("system audio tap stopped delivering audio".into());
        }
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "core-audio-tap"
    }
}

impl Drop for ProcessTapSource {
    fn drop(&mut self) {
        // SAFETY: Each identifier was created by this source and is torn down
        // once, in reverse order of creation.
        unsafe {
            if self.aggregate != 0 {
                if self.io_proc.is_some() {
                    AudioDeviceStop(self.aggregate, self.io_proc);
                    AudioDeviceDestroyIOProcID(self.aggregate, self.io_proc);
                }
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
        }
    }
}

fn check(status: i32, context: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(eyre!("{context} (OSStatus {status})"))
    }
}

fn own_process_object() -> Option<AudioObjectID> {
    let pid = std::process::id() as i32;
    let mut object: AudioObjectID = 0;
    let mut size = size_of::<AudioObjectID>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: The qualifier is a pid and the output is one AudioObjectID.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            size_of::<i32>() as u32,
            (&pid as *const i32).cast(),
            NonNull::from(&mut size),
            NonNull::from(&mut object).cast(),
        )
    };
    (status == 0 && object != 0).then_some(object)
}

fn tap_format(tap: AudioObjectID) -> Result<AudioStreamBasicDescription> {
    let mut format = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let mut size = size_of::<AudioStreamBasicDescription>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioTapPropertyFormat,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: The output buffer is one AudioStreamBasicDescription.
    check(
        unsafe {
            AudioObjectGetPropertyData(
                tap,
                NonNull::from(&address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut format).cast(),
            )
        },
        "could not read the system audio tap format",
    )?;
    if format.mSampleRate <= 0.0 {
        return Err(eyre!("system audio tap reported no sample rate"));
    }
    Ok(format)
}

fn object<T: objc2::Message>(value: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: Every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(value) }
}

/// A private aggregate device that reads one process tap. `uid` must differ
/// for each concurrent tap of this process.
pub(crate) fn create_tap_aggregate(
    tap_uid: &NSString,
    name: &str,
    uid: &str,
) -> Result<AudioObjectID> {
    fn key(name: &CStr) -> Retained<NSString> {
        NSString::from_str(name.to_str().unwrap_or_default())
    }
    let sub_tap: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_retained_objects(
        &[
            &*key(kAudioSubTapUIDKey),
            &*key(kAudioSubTapDriftCompensationKey),
        ],
        &[
            object(NSString::from_str(&tap_uid.to_string())),
            object(NSNumber::new_bool(true)),
        ],
    );
    let taps = NSArray::from_retained_slice(&[sub_tap]);
    let description: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_retained_objects(
            &[
                &*key(kAudioAggregateDeviceNameKey),
                &*key(kAudioAggregateDeviceUIDKey),
                &*key(kAudioAggregateDeviceIsPrivateKey),
                &*key(kAudioAggregateDeviceTapAutoStartKey),
                &*key(kAudioAggregateDeviceTapListKey),
            ],
            &[
                object(NSString::from_str(name)),
                object(NSString::from_str(uid)),
                object(NSNumber::new_bool(true)),
                object(NSNumber::new_bool(true)),
                object(taps),
            ],
        );
    let mut aggregate: AudioObjectID = 0;
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
    let dictionary: &CFDictionary =
        unsafe { &*(Retained::as_ptr(&description) as *const CFDictionary) };
    check(
        unsafe { AudioHardwareCreateAggregateDevice(dictionary, NonNull::from(&mut aggregate)) },
        "could not create the system audio device",
    )?;
    Ok(aggregate)
}

/// Mixes one IO cycle's float buffers to mono.
///
/// # Safety
/// `list` must point at a valid `AudioBufferList` of 32-bit float samples.
unsafe fn mono_samples(
    list: NonNull<AudioBufferList>,
    channels: usize,
    non_interleaved: bool,
) -> Option<Vec<f32>> {
    let list = unsafe { list.as_ref() };
    let count = list.mNumberBuffers as usize;
    let buffers: &[AudioBuffer] =
        unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), count) };
    let float_slice = |buffer: &AudioBuffer| -> Option<&[f32]> {
        if buffer.mData.is_null() {
            return Some(&[]);
        }
        let len = buffer.mDataByteSize as usize / size_of::<f32>();
        Some(unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), len) })
    };
    match buffers {
        [] => Some(Vec::new()),
        [buffer] => {
            let interleaved = (buffer.mNumberChannels as usize).max(1);
            let samples = float_slice(buffer)?;
            if interleaved == 1 {
                return Some(samples.to_vec());
            }
            Some(
                samples
                    .chunks_exact(interleaved)
                    .map(|frame| frame.iter().sum::<f32>() / interleaved as f32)
                    .collect(),
            )
        }
        many if non_interleaved || many.len() == channels => {
            let planes: Vec<&[f32]> = many.iter().map(float_slice).collect::<Option<_>>()?;
            let frames = planes.iter().map(|plane| plane.len()).min().unwrap_or(0);
            Some(
                (0..frames)
                    .map(|frame| {
                        planes.iter().map(|plane| plane[frame]).sum::<f32>() / planes.len() as f32
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// ScreenCaptureKit system audio, used only when the tap is unavailable.
/// It depends on an available display, so display sleep can end it; the
/// supervisor then reopens it once a display returns.
struct ScreenCaptureSource {
    stream: SCStream,
    error: Arc<Mutex<Option<String>>>,
}

impl ScreenCaptureSource {
    fn start(sink: &PacketSink) -> Result<Self> {
        let content = SCShareableContent::get().wrap_err("could not enumerate screen content")?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("no display is available for system audio capture"))?;
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build()
            .wrap_err("could not build meeting capture filter")?;
        let config = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(SAMPLE_RATE as i32)
            .with_channel_count(1);
        let error = Arc::new(Mutex::new(None::<String>));
        let delegate_error = error.clone();
        let mut stream = SCStream::new_with_delegate(
            &filter,
            &config,
            ErrorHandler::new(move |stream_error| {
                *delegate_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(stream_error.to_string());
            }),
        )
        .map_err(|error| eyre!(error).wrap_err("could not create meeting capture stream"))?;
        let handler_sink = sink.clone();
        let resamplers = Mutex::new(None::<To16k>);
        stream
            .add_output_handler(
                move |sample: CMSampleBuffer, _| {
                    let pts_us = media_time_us(sample.presentation_timestamp());
                    let Some(rate) = sample
                        .format_description()
                        .and_then(|format| format.audio_sample_rate())
                        .map(|rate| rate.round() as u32)
                    else {
                        handler_sink.drops.fetch_add(1, Ordering::Relaxed);
                        return;
                    };
                    let Some(samples) = decode_audio(&sample) else {
                        handler_sink.drops.fetch_add(1, Ordering::Relaxed);
                        return;
                    };
                    let mut resampler = resamplers
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if resampler
                        .as_ref()
                        .is_none_or(|current| current.rate != rate)
                    {
                        *resampler = To16k::new(rate).ok();
                    }
                    if let Some(resampler) = resampler.as_mut() {
                        handler_sink.send(SAMPLE_RATE, resampler.push(&samples), pts_us);
                    }
                },
                SCStreamOutputType::Audio,
            )
            .map_err(|error| eyre!("ScreenCaptureKit rejected the audio output: {error}"))?;
        stream
            .start_capture()
            .wrap_err("could not start meeting capture")?;
        Ok(Self { stream, error })
    }
}

impl LiveSource for ScreenCaptureSource {
    fn poll(&mut self, _sink: &PacketSink) -> std::result::Result<(), String> {
        thread::sleep(POLL_INTERVAL);
        match self
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn describe(&self) -> &'static str {
        "screencapturekit"
    }
}

impl Drop for ScreenCaptureSource {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

/// macOS "System Audio Recording" consent (TCC `kTCCServiceAudioCapture`),
/// which process taps need. Screen Recording consent does not imply it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemAudioAuthorization {
    Granted,
    Denied,
    NotDetermined,
    /// The private TCC query is unavailable; try the tap and let macOS ask.
    Unknown,
}

type TccPreflight = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
type TccRequest = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void);

struct Tcc {
    preflight: TccPreflight,
    request: TccRequest,
}

fn tcc() -> Option<&'static Tcc> {
    static TCC: OnceLock<Option<Tcc>> = OnceLock::new();
    TCC.get_or_init(|| {
        // SAFETY: dlopen/dlsym of a system framework; symbols are checked.
        unsafe {
            let handle = libc::dlopen(
                c"/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC".as_ptr(),
                libc::RTLD_NOW,
            );
            if handle.is_null() {
                return None;
            }
            let preflight = libc::dlsym(handle, c"TCCAccessPreflight".as_ptr());
            let request = libc::dlsym(handle, c"TCCAccessRequest".as_ptr());
            if preflight.is_null() || request.is_null() {
                return None;
            }
            Some(Tcc {
                preflight: std::mem::transmute::<*mut c_void, TccPreflight>(preflight),
                request: std::mem::transmute::<*mut c_void, TccRequest>(request),
            })
        }
    })
    .as_ref()
}

fn audio_capture_service() -> objc2_core_foundation::CFRetained<CFString> {
    CFString::from_static_str("kTCCServiceAudioCapture")
}

pub fn system_audio_authorization() -> SystemAudioAuthorization {
    let Some(tcc) = tcc() else {
        return SystemAudioAuthorization::Unknown;
    };
    let service = audio_capture_service();
    // SAFETY: A CFString service name and no options.
    let result =
        unsafe { (tcc.preflight)((&*service as *const CFString).cast(), std::ptr::null()) };
    match result {
        0 => SystemAudioAuthorization::Granted,
        1 => SystemAudioAuthorization::Denied,
        _ => SystemAudioAuthorization::NotDetermined,
    }
}

static SYSTEM_AUDIO_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Allows [`request_system_audio_access`] to prompt again after a TCC reset
/// returned System Audio Recording to `NotDetermined`.
pub(crate) fn reset_system_audio_request() {
    SYSTEM_AUDIO_REQUESTED.store(false, Ordering::Relaxed);
}

/// Shows the macOS System Audio Recording prompt when consent is undecided.
/// Returns whether a prompt was requested.
pub fn request_system_audio_access() -> bool {
    if system_audio_authorization() != SystemAudioAuthorization::NotDetermined
        || SYSTEM_AUDIO_REQUESTED.swap(true, Ordering::Relaxed)
    {
        return false;
    }
    let Some(tcc) = tcc() else {
        return false;
    };
    let service = audio_capture_service();
    let completion = RcBlock::new(|granted: Bool| {
        tracing::info!(
            granted = granted.as_bool(),
            "system audio recording consent answered"
        );
    });
    // SAFETY: TCC copies the completion block before returning.
    unsafe {
        (tcc.request)(
            (&*service as *const CFString).cast(),
            std::ptr::null(),
            RcBlock::as_ptr(&completion).cast(),
        );
    }
    true
}

pub(super) fn media_time_us(time: CMTime) -> Option<i64> {
    if !time.is_valid()
        || time.is_indefinite()
        || time.is_positive_infinity()
        || time.is_negative_infinity()
    {
        return None;
    }
    let microseconds = time.as_seconds()? * 1_000_000.0;
    microseconds
        .is_finite()
        .then(|| microseconds.round() as i64)
}

fn decode_audio(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
    let format = sample.format_description()?;
    if format.audio_is_big_endian() {
        return None;
    }
    let channels = format.audio_channel_count()? as usize;
    let bytes_per_frame = format.audio_bytes_per_frame()? as usize;
    let bytes_per_sample = bytes_per_frame.checked_div(channels)?;
    let bits_per_channel = format.audio_bits_per_channel()?;
    let flags = format.audio_format_flags()?;
    let frames = usize::try_from(sample.num_samples()).ok()?;
    let buffers = sample.audio_buffer_list().ok()?;
    if buffers.num_buffers() == channels && channels > 1 {
        let channel_samples: Vec<Vec<f32>> = buffers
            .iter()
            .map(|buffer| {
                decode_samples(
                    buffer.data(),
                    frames,
                    bytes_per_sample,
                    bits_per_channel,
                    flags,
                )
            })
            .collect::<Option<_>>()?;
        return Some(
            (0..frames)
                .map(|frame| {
                    channel_samples
                        .iter()
                        .map(|samples| samples[frame])
                        .sum::<f32>()
                        / channels as f32
                })
                .collect(),
        );
    }
    let buffer = buffers.get(0)?;
    let available_channels = usize::try_from(buffer.number_channels).ok()?.max(1);
    let samples = decode_samples(
        buffer.data(),
        frames.checked_mul(available_channels)?,
        bytes_per_sample,
        bits_per_channel,
        flags,
    )?;
    Some(
        samples
            .chunks_exact(available_channels)
            .map(|frame| frame.iter().sum::<f32>() / available_channels as f32)
            .collect(),
    )
}

pub(super) fn decode_samples(
    bytes: &[u8],
    samples: usize,
    bytes_per_sample: usize,
    bits_per_channel: u32,
    flags: u32,
) -> Option<Vec<f32>> {
    let required = samples.checked_mul(bytes_per_sample)?;
    if bytes.len() < required {
        return None;
    }
    let is_float = flags & 1 != 0;
    let is_signed_integer = flags & 4 != 0;
    bytes[..required]
        .chunks_exact(bytes_per_sample)
        .map(
            |sample| match (is_float, is_signed_integer, bytes_per_sample) {
                (true, _, 4) => Some(f32::from_le_bytes(sample.try_into().ok()?)),
                (_, true, 2) => {
                    Some(f32::from(i16::from_le_bytes(sample.try_into().ok()?)) / 32_768.0)
                }
                (_, true, 3) => {
                    let raw = i32::from_le_bytes([
                        sample[0],
                        sample[1],
                        sample[2],
                        if sample[2] & 0x80 == 0 { 0 } else { 0xff },
                    ]);
                    Some(raw as f32 / 8_388_608.0)
                }
                (_, true, 4) if bits_per_channel <= 32 => {
                    Some(i32::from_le_bytes(sample.try_into().ok()?) as f32 / 2_147_483_648.0)
                }
                _ => None,
            },
        )
        .collect()
}
