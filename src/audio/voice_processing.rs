//! Apple's voice processing as dictation's microphone (macOS, Labs › Voice
//! isolation). AVAudioEngine's input runs as a duplex voice-processing unit,
//! so macOS's Control Center offers Voice Isolation for Hex, and its tap
//! delivers mono float samples on the same capture clock as cpal's streams.
//!
//! The Swift half is `src/voice_processing.swift`. It resolves the cpal
//! device's UID to a CoreAudio device, sets it on the input unit, enables voice
//! processing, and installs the tap; Rust owns the stream, its channels, and
//! the context the tap writes into, and stops the engine on drop. The
//! Objective-C guard in `native/voice_processing_guard.m` turns an exception
//! from AVAudioEngine into an error the raw cpal stream can stand in for.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use color_eyre::eyre::{Result, eyre};

use super::hex_microphone::{CaptureFeed, CaptureFeedSwitch, capture_feed};
use super::instant::{CaptureInstant, duration_for_frames};
use super::stream::StreamError;

/// One tap buffer: mono float samples, the host time they were captured at
/// (Mach ticks), and the rate they were captured at.
type TapCallback = unsafe extern "C" fn(*mut c_void, *const f32, usize, u64, u32);

/// An engine configuration change, delivered on the same context as the tap.
type ErrorCallback = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" {
    /// Builds the engine for `device_uid` with voice processing on, its graph
    /// connected, and its tap installed, without starting the engine yet.
    /// Answers a retained handle, the capture rate and channel count, or null
    /// with a message in `error`.
    fn hex_voice_processing_start(
        device_uid: *const c_char,
        callback: TapCallback,
        error_callback: ErrorCallback,
        context: *mut c_void,
        sample_rate: *mut f64,
        channels: *mut u32,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    /// Starts the engine behind `handle` and verifies its device; 0 on
    /// success, else a message in `error`.
    fn hex_voice_processing_begin(handle: *mut c_void, error: *mut *mut c_char) -> i32;
    /// Removes the tap, stops the engine, and releases `handle`.
    fn hex_voice_processing_stop(handle: *mut c_void);
    fn hex_voice_processing_free_string(string: *mut c_char);
}

/// A running voice-processing stream; dropping it stops the engine.
pub(super) struct VoiceProcessingStream {
    handle: *mut c_void,
    context: *mut VoiceProcessingContext,
}

// SAFETY: the stream owns both pointers. AVAudioEngine calls the tap and the
// configuration observer on its own threads with the context alone; neither
// this struct nor its fields is touched there, and Drop stops the engine
// before freeing either.
unsafe impl Send for VoiceProcessingStream {}

/// What the tap and the configuration observer write into. The feed is
/// installed before the engine starts, so no callback sees it empty.
struct VoiceProcessingContext {
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    errors: SyncSender<StreamError>,
    /// Filled once the bridge reports its capture rate and before the engine
    /// starts. Swift's gate serializes callbacks, but the Rust callback gets
    /// a shared context pointer, so the uncontended mutex supplies the `&mut`
    /// the feed needs; the rate isn't known when the context is built.
    feed: Mutex<Option<CaptureFeed>>,
}

/// A voice-processing stream and the channels its tap feeds.
pub(super) struct StartedVoiceProcessing {
    pub(super) stream: VoiceProcessingStream,
    pub(super) chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub(super) stream_errors: Receiver<StreamError>,
    pub(super) hex_microphone: CaptureFeedSwitch,
    pub(super) sample_rate: u32,
    pub(super) channels: u32,
}

/// Starts voice processing on the device with `device_uid`. The engine sets
/// that device itself, so cpal's handle is only used for the UID.
pub(super) fn start(device_uid: &str) -> Result<StartedVoiceProcessing> {
    let uid = CString::new(device_uid)?;
    let (sender, chunks) = mpsc::channel();
    let (errors, stream_errors) = mpsc::sync_channel(1);
    let context = Box::into_raw(Box::new(VoiceProcessingContext {
        sender,
        errors,
        feed: Mutex::new(None),
    }));
    let mut sample_rate = 0.0_f64;
    let mut channels = 0_u32;
    let mut error = ptr::null_mut();
    // SAFETY: `uid` is a NUL-terminated device UID, `deliver` and
    // `report_error` have the signatures the bridge expects, and `context`
    // stays alive until the stream stops; the rate, channel, and error
    // pointers are valid for the call.
    let handle = unsafe {
        hex_voice_processing_start(
            uid.as_ptr(),
            deliver,
            report_error,
            context.cast(),
            &mut sample_rate,
            &mut channels,
            &mut error,
        )
    };
    if handle.is_null() {
        // SAFETY: nothing holds the context when start fails, so this is its
        // only owner.
        drop(unsafe { Box::from_raw(context) });
        // SAFETY: start answered an owned message, or null.
        let message = unsafe { take_error(error) };
        return Err(eyre!("voice processing would not start: {message}"));
    }
    let rate = sample_rate.round() as u32;
    if !sample_rate.is_finite() || !(8_000..=384_000).contains(&rate) || channels == 0 {
        // SAFETY: the handle owns the Swift capture, and the engine has not
        // started, so its tap cannot be running; both can be released here.
        unsafe {
            hex_voice_processing_stop(handle);
            drop(Box::from_raw(context));
        }
        return Err(eyre!(
            "voice processing reported an unusable format: {sample_rate} Hz, {channels} channels"
        ));
    }
    let (feed, hex_microphone) = capture_feed(rate);
    // SAFETY: the engine has not started, so its installed tap cannot be
    // reading the context; and the context is the one this call owns.
    unsafe {
        if let Ok(mut installed) = (*context).feed.lock() {
            *installed = Some(feed);
        }
    }
    // SAFETY: `handle` is the retained capture this call owns, and `error`
    // answers an owned message when the begin fails.
    let started = unsafe { hex_voice_processing_begin(handle, &mut error) };
    if started != 0 {
        // SAFETY: the begin may have failed after starting the engine, so the
        // retained handle still owns its tap and engine to clean up.
        let message = unsafe { take_error(error) };
        // SAFETY: the handle and context are this call's, and no tap can run
        // after the stop removes it.
        unsafe {
            hex_voice_processing_stop(handle);
            drop(Box::from_raw(context));
        }
        return Err(eyre!("voice processing would not start: {message}"));
    }
    Ok(StartedVoiceProcessing {
        stream: VoiceProcessingStream { handle, context },
        chunks,
        stream_errors,
        hex_microphone,
        sample_rate: rate,
        channels,
    })
}

/// Takes the C string behind `error`, freeing it.
unsafe fn take_error(error: *mut c_char) -> String {
    if error.is_null() {
        return "unknown error".into();
    }
    // SAFETY: the bridge answers a NUL-terminated string this call owns.
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the bridge allocated the string with strdup, so free releases
    // it through the same allocator and this call no longer uses it.
    unsafe { hex_voice_processing_free_string(error) };
    message
}

/// Reports an engine configuration change, on AVAudioEngine's notification
/// thread while the Swift gate's lock is held.
unsafe extern "C" fn report_error(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the Box<VoiceProcessingContext> VoiceProcessingStream
    // keeps alive until stop returns, which the gate makes wait for this
    // callback.
    let context = unsafe { &*context.cast::<VoiceProcessingContext>() };
    let _ = context
        .errors
        .try_send(StreamError::VoiceProcessingConfigurationChanged);
}

/// Delivers one tap buffer, on AVAudioEngine's render thread while the Swift
/// gate's lock is held.
unsafe extern "C" fn deliver(
    context: *mut c_void,
    samples: *const f32,
    count: usize,
    host_time: u64,
    sample_rate: u32,
) {
    if context.is_null() || samples.is_null() || count == 0 || sample_rate == 0 {
        return;
    }
    // SAFETY: `context` is the Box<VoiceProcessingContext> VoiceProcessingStream
    // keeps alive until stop returns, `samples` holds `count` floats for this
    // call, and the gate calls this one buffer at a time.
    let context = unsafe { &*context.cast::<VoiceProcessingContext>() };
    let mono = unsafe { std::slice::from_raw_parts(samples, count) };
    let captured_at = if host_time == 0 {
        CaptureInstant::now()
    } else {
        CaptureInstant::from_mach_ticks(host_time)
    };
    let captured_through = captured_at
        .checked_add(duration_for_frames(count, sample_rate))
        .unwrap_or(captured_at);
    if let Ok(mut feed) = context.feed.lock()
        && let Some(feed) = feed.as_mut()
    {
        feed.push(mono);
    }
    if context
        .sender
        .send((mono.to_vec(), captured_through))
        .is_err()
    {
        let _ = context.errors.try_send(StreamError::Failed(
            "voice processing audio channel disconnected".into(),
        ));
    }
}

impl Drop for VoiceProcessingStream {
    fn drop(&mut self) {
        // SAFETY: `handle` is the retained Swift capture this struct owns, and
        // stopping it clears the gate, removes the tap, and stops the engine
        // before it returns. Only then is the context freed, so no tap or
        // configuration callback can be running or start afterwards.
        unsafe {
            hex_voice_processing_stop(self.handle);
            drop(Box::from_raw(self.context));
        }
    }
}
