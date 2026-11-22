//! Apple's voice processing as dictation's microphone (macOS, Labs › Voice
//! isolation). AVAudioEngine's input runs as a duplex voice-processing unit,
//! so macOS's Control Center offers Voice Isolation for Hex, and its tap
//! delivers mono float samples on the same capture clock as cpal's streams.
//!
//! The Swift half is `src/voice_processing.swift`. It resolves the cpal
//! device's UID to a CoreAudio device, sets it on the input unit, enables voice
//! processing, and installs the tap; Rust owns the stream, its channels, and
//! the context the tap writes into, and stops the engine on drop.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use color_eyre::eyre::{Result, eyre};

use super::hex_microphone::{CaptureFeed, CaptureFeedSwitch, capture_feed};
use super::instant::{CaptureInstant, duration_for_frames};

/// One tap buffer: mono float samples, the host time they were captured at
/// (Mach ticks), and the rate they were captured at.
type TapCallback = unsafe extern "C" fn(*mut c_void, *const f32, usize, u64, u32);

unsafe extern "C" {
    /// Builds and starts nothing yet: an engine for `device_uid` with voice
    /// processing on, its tap installed for `context`, and its capture rate
    /// in `sample_rate`. Answers a retained handle, or null with a message in
    /// `error`.
    fn hex_voice_processing_start(
        device_uid: *const c_char,
        callback: TapCallback,
        context: *mut c_void,
        sample_rate: *mut f64,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    /// Starts the engine behind `handle`; 0 on success, else a message in
    /// `error`.
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

// SAFETY: the stream owns both pointers. AVAudioEngine calls the tap on its
// own thread with the context alone; neither this struct nor its fields is
// touched there, and Drop stops the engine before freeing either.
unsafe impl Send for VoiceProcessingStream {}

/// What the tap writes into. The feed is installed once the engine reports
/// its capture rate and before it starts, so no callback sees it empty.
struct VoiceProcessingContext {
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    errors: SyncSender<String>,
    feed: Mutex<Option<CaptureFeed>>,
}

/// A voice-processing stream and the channels its tap feeds.
pub(super) struct StartedVoiceProcessing {
    pub(super) stream: VoiceProcessingStream,
    pub(super) chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub(super) stream_errors: Receiver<String>,
    pub(super) hex_microphone: CaptureFeedSwitch,
    pub(super) sample_rate: u32,
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
    let mut error = ptr::null_mut();
    // SAFETY: `uid` is a NUL-terminated device UID, `deliver` has the
    // signature the bridge expects, and `context` stays alive until the
    // stream stops; the pointers for the rate and error are valid for the
    // call.
    let handle = unsafe {
        hex_voice_processing_start(
            uid.as_ptr(),
            deliver,
            context.cast(),
            &mut sample_rate,
            &mut error,
        )
    };
    if handle.is_null() {
        // SAFETY: nothing holds the context when start fails, so this is its
        // only owner.
        drop(unsafe { Box::from_raw(context) });
        let message = unsafe { take_error(error) };
        return Err(eyre!("voice processing would not start: {message}"));
    }
    let rate = sample_rate.round() as u32;
    if !sample_rate.is_finite() || !(8_000..=384_000).contains(&rate) {
        unsafe {
            hex_voice_processing_stop(handle);
            drop(Box::from_raw(context));
        }
        return Err(eyre!(
            "voice processing reported an unusable sample rate: {sample_rate}"
        ));
    }
    let (feed, hex_microphone) = capture_feed(rate);
    // SAFETY: the engine has not started, so its tap cannot be reading the
    // context yet; and the context is the one this call owns.
    unsafe {
        if let Ok(mut installed) = (*context).feed.lock() {
            *installed = Some(feed);
        }
    }
    let started = unsafe { hex_voice_processing_begin(handle, &mut error) };
    if started != 0 {
        let message = unsafe { take_error(error) };
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
    })
}

/// Takes the C string behind `error`, freeing it.
unsafe fn take_error(error: *mut c_char) -> String {
    if error.is_null() {
        return "unknown error".into();
    }
    // SAFETY: the bridge answers a NUL-terminated string this call owns and
    // frees through the bridge's own allocator.
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { hex_voice_processing_free_string(error) };
    message
}

/// Delivers one tap buffer, on AVAudioEngine's render thread.
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
    // keeps alive until the engine stops, `samples` holds `count` floats for
    // this call, and the bridge calls this one buffer at a time.
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
        let _ = context
            .errors
            .try_send("voice processing audio channel disconnected".into());
    }
}

impl Drop for VoiceProcessingStream {
    fn drop(&mut self) {
        // SAFETY: `handle` is the retained Swift capture this struct owns, and
        // stopping it removes the tap and releases it. The context is freed
        // only afterwards, when no tap can be running it.
        unsafe {
            hex_voice_processing_stop(self.handle);
            drop(Box::from_raw(self.context));
        }
    }
}
