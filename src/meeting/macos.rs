//! The macOS side of meetings: system audio through a Core Audio process
//! tap (`system_audio`: every output, one app, or one output device), with
//! ScreenCaptureKit as the fallback for every output when the tap is not
//! authorized, its consents, and Finder and Trash.

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use block2::RcBlock;
use color_eyre::eyre::{Result, WrapErr, eyre};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2_core_audio::{
    AudioHardwareCreateAggregateDevice, AudioObjectID, kAudioAggregateDeviceClockDeviceKey,
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioSubTapDriftCompensationKey,
    kAudioSubTapUIDKey,
};
use objc2_core_foundation::{CFDictionary, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use screencapturekit::prelude::*;
use screencapturekit::stream::delegate_trait::ErrorHandler;

#[path = "macos/system_audio.rs"]
mod system_audio;

use super::SAMPLE_RATE;
use super::audio_source::effective;
use super::capture::{LiveSource, POLL_INTERVAL, PacketSink, To16k};

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

/// Fails, after asking macOS, until Hex may hear the other participants. A
/// chosen app or output records through a process tap alone, so it needs
/// System Audio Recording consent even when Screen Recording is on.
pub(super) fn ensure_system_audio() -> Result<()> {
    if !effective().is_all() {
        // One prompt per recording, here. The source's open retries check the
        // same consent without asking again, so a recording that outlives
        // many retries never re-prompts.
        if system_audio_authorization() == SystemAudioAuthorization::NotDetermined {
            request_system_audio_access();
        }
        return tap_consent();
    }
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
        tracing::error!(%error, "could not reveal the file");
    }
}

/// Opens the folder `path` itself in Finder, rather than selecting it in its
/// parent.
pub(crate) fn open_folder(path: &Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
    {
        tracing::error!(%error, "could not open meeting folder");
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

pub(super) fn open_system_audio(
    sink: &PacketSink,
    _application: Option<&str>,
) -> Result<Box<dyn LiveSource>> {
    // A source rereads the choice on its route check and reopens when it
    // differs, so turning the lab off, or choosing another source, applies
    // mid-recording.
    let choice = effective();
    if !choice.is_all() {
        // A chosen app or output never falls back to ScreenCaptureKit: that
        // records every app the display plays, not the one chosen. A tap that
        // cannot open fails this stream, and the supervisor retries it.
        tap_consent()?;
        return system_audio::ProcessTapSource::start(system_audio::Scope::of(&choice))
            .map(|source| Box::new(source) as Box<dyn LiveSource>);
    }
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
        match system_audio::ProcessTapSource::start(system_audio::Scope::All) {
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

/// Fails until Hex may tap a chosen app's or output's audio, without asking:
/// only a process tap records just that source, and Screen Recording consent
/// cannot replace it. The recording asked for System Audio Recording once
/// when it started ([ensure_system_audio]), so a retry that finds the prompt
/// unanswered reports it and tries again after the answer.
fn tap_consent() -> Result<()> {
    match system_audio_authorization() {
        SystemAudioAuthorization::Granted | SystemAudioAuthorization::Unknown => Ok(()),
        SystemAudioAuthorization::NotDetermined => Err(eyre!(
            "Allow Hex in System Settings › Privacy & Security › Screen & System Audio Recording, then try again"
        )),
        SystemAudioAuthorization::Denied => Err(eyre!(
            "Allow Hex under System Settings › Privacy & Security › Screen & System Audio Recording"
        )),
    }
}

fn check(status: i32, context: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(eyre!("{context} (OSStatus {status})"))
    }
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
    create_tap_aggregate_at(tap_uid, None, name, uid)
}

/// [`create_tap_aggregate`] clocked by `device_uid`: the device a tap of one
/// output records is the aggregate's main and clock device, so that tap runs
/// on its device's clock rather than the default output's. The device is not
/// a sub-device: a real sub-device stops USB interfaces from delivering to
/// the IOProc, and the tap is already pinned to the device by its UID.
pub(super) fn create_tap_aggregate_at(
    tap_uid: &NSString,
    device_uid: Option<&NSString>,
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
    let description: Retained<NSDictionary<NSString, AnyObject>> = match device_uid {
        Some(device_uid) => NSDictionary::from_retained_objects(
            &[
                &*key(kAudioAggregateDeviceNameKey),
                &*key(kAudioAggregateDeviceUIDKey),
                &*key(kAudioAggregateDeviceIsPrivateKey),
                &*key(kAudioAggregateDeviceTapAutoStartKey),
                &*key(kAudioAggregateDeviceMainSubDeviceKey),
                &*key(kAudioAggregateDeviceClockDeviceKey),
                &*key(kAudioAggregateDeviceTapListKey),
            ],
            &[
                object(NSString::from_str(name)),
                object(NSString::from_str(uid)),
                object(NSNumber::new_bool(true)),
                object(NSNumber::new_bool(true)),
                object(NSString::from_str(&device_uid.to_string())),
                object(NSString::from_str(&device_uid.to_string())),
                object(taps),
            ],
        ),
        None => NSDictionary::from_retained_objects(
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
        ),
    };
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
        // Only every-output capture falls back to this; another choice
        // reopens the source into it, even mid-recording.
        if !effective().is_all() {
            return Err("the meeting audio choice changed".into());
        }
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
