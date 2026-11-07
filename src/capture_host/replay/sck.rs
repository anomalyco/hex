//! macOS 27 `SCClipBufferingOutput` runtime bindings over `objc2`.
//!
//! `SCClipBufferingOutput` and `SCClipBufferingOutputDelegate` exist only on
//! macOS 27+. Every class, protocol, and selector is resolved dynamically at
//! runtime through the Objective-C runtime (`AnyClass::get`, `AnyProtocol::get`,
//! `sel!`), so builds link no macOS-27-only symbols and older macOS releases
//! run unchanged with [`available`] returning `false`.

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::{ManualBlockEncoding, RcBlock};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, NSObject, Sel};
use objc2::{ClassType, msg_send, sel};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSError, NSString, NSURL};

/// Modest rolling-buffer frame rate: smooth enough to read scrolling UI and
/// motion bugs while keeping `replayd` / `WindowServer` / H.264 encode work low.
pub(super) const BUFFER_FPS: i32 = 15;

/// Cap the buffered display's long edge at 2560 px (matching Scrawl's display
/// capture cap) with even pixel dimensions for Hardware VideoToolbox H.264.
const MAX_LONG_EDGE: u32 = 2_560;

/// Small sample queue depth since no `CMSampleBuffer` leaves `SCStream` into
/// our process.
const QUEUE_DEPTH: isize = 4;

const SETUP_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_millis(1_500);

/// CoreMedia's `CMTime` is an anonymous `typedef struct` in C headers, so its
/// Objective-C `@encode` struct tag is `"?"`, which `objc2` checks in debug
/// builds.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RawCMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

// SAFETY: Matches `CMTime` layout and `@encode(CMTime)` (`{?=qiIq}`) on arm64/x86_64 macOS.
unsafe impl Encode for RawCMTime {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
    );
}

// SAFETY: Pointer encoding to `RawCMTime`.
unsafe impl RefEncode for RawCMTime {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

impl RawCMTime {
    const fn frame_interval(fps: i32) -> Self {
        Self {
            value: 1,
            timescale: fps,
            flags: 1, // kCMTimeFlags_Valid
            epoch: 0,
        }
    }
}

/// Geometry of the display currently feeding the rolling clip buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BufferDisplay {
    pub display_id: u32,
    /// Global top-left display bounds in points (`[x, y, width, height]`).
    pub frame: [f64; 4],
    pub scale: f64,
    /// Encoded output dimensions in pixels.
    pub width: u32,
    pub height: u32,
}

/// Asynchronous events from `SCStreamDelegate` and `SCClipBufferingOutputDelegate`.
#[derive(Clone, Debug)]
pub(super) enum StreamSignal {
    BufferingStarted,
    BufferingFailed(String),
    BufferingStopped,
    StreamStopped(String),
}

/// Whether the running macOS has `SCClipBufferingOutput` and `SCStream`'s
/// clip-buffering selectors (macOS 27+).
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        autoreleasepool(|_| {
            let Some(clip_cls) = AnyClass::get(c"SCClipBufferingOutput") else {
                return false;
            };
            let Some(stream_cls) = AnyClass::get(c"SCStream") else {
                return false;
            };
            let clip_ok: Bool =
                unsafe { msg_send![clip_cls, instancesRespondToSelector: sel!(initWithDelegate:)] };
            let export_ok: Bool = unsafe {
                msg_send![
                    clip_cls,
                    instancesRespondToSelector: sel!(exportClipToURL:duration:completionHandler:)
                ]
            };
            let add_ok: Bool = unsafe {
                msg_send![
                    stream_cls,
                    instancesRespondToSelector: sel!(addClipBufferingOutput:error:)
                ]
            };
            let remove_ok: Bool = unsafe {
                msg_send![
                    stream_cls,
                    instancesRespondToSelector: sel!(removeClipBufferingOutput:error:)
                ]
            };
            clip_ok.as_bool() && export_ok.as_bool() && add_ok.as_bool() && remove_ok.as_bool()
        })
    })
}

struct DelegateBox {
    signals: SyncSender<StreamSignal>,
}

fn format_ns_error(error: *const NSError) -> String {
    if error.is_null() {
        return "unknown ScreenCaptureKit error".into();
    }
    // SAFETY: Non-null `NSError *` valid for the callback's duration.
    let err = unsafe { &*error };
    format!(
        "{} ({} {})",
        err.localizedDescription(),
        err.domain(),
        err.code()
    )
}

fn delegate_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new(c"HexClipBufferingDelegate", NSObject::class())
            .expect("HexClipBufferingDelegate registered once");
        builder.add_ivar::<*mut c_void>(c"_state");
        if let Some(proto) = AnyProtocol::get(c"SCClipBufferingOutputDelegate") {
            builder.add_protocol(proto);
        }
        if let Some(proto) = AnyProtocol::get(c"SCStreamDelegate") {
            builder.add_protocol(proto);
        }

        unsafe extern "C-unwind" fn dealloc(this: *mut AnyObject, _cmd: Sel) {
            if let Some(cls) = AnyClass::get(c"HexClipBufferingDelegate")
                && let Some(ivar) = cls.instance_variable(c"_state")
            {
                let ptr = unsafe { *ivar.load::<*mut c_void>(&*this) };
                if !ptr.is_null() {
                    unsafe { drop(Box::from_raw(ptr.cast::<DelegateBox>())) };
                }
            }
            unsafe {
                let _: () = msg_send![super(this, NSObject::class()), dealloc];
            }
        }

        unsafe extern "C-unwind" fn did_start(
            this: *mut AnyObject,
            _cmd: Sel,
            _output: *mut AnyObject,
        ) {
            send_signal(this, StreamSignal::BufferingStarted);
        }

        unsafe extern "C-unwind" fn did_fail(
            this: *mut AnyObject,
            _cmd: Sel,
            _output: *mut AnyObject,
            error: *const NSError,
        ) {
            send_signal(this, StreamSignal::BufferingFailed(format_ns_error(error)));
        }

        unsafe extern "C-unwind" fn did_stop(
            this: *mut AnyObject,
            _cmd: Sel,
            _output: *mut AnyObject,
        ) {
            send_signal(this, StreamSignal::BufferingStopped);
        }

        unsafe extern "C-unwind" fn stream_did_stop(
            this: *mut AnyObject,
            _cmd: Sel,
            _stream: *mut AnyObject,
            error: *const NSError,
        ) {
            send_signal(this, StreamSignal::StreamStopped(format_ns_error(error)));
        }

        unsafe {
            builder.add_method(
                sel!(dealloc),
                dealloc as unsafe extern "C-unwind" fn(*mut AnyObject, Sel),
            );
            builder.add_method(
                sel!(clipBufferingOutputDidStartBuffering:),
                did_start as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            builder.add_method(
                sel!(clipBufferingOutput:didFailWithError:),
                did_fail
                    as unsafe extern "C-unwind" fn(
                        *mut AnyObject,
                        Sel,
                        *mut AnyObject,
                        *const NSError,
                    ),
            );
            builder.add_method(
                sel!(clipBufferingOutputDidStopBuffering:),
                did_stop as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            builder.add_method(
                sel!(stream:didStopWithError:),
                stream_did_stop
                    as unsafe extern "C-unwind" fn(
                        *mut AnyObject,
                        Sel,
                        *mut AnyObject,
                        *const NSError,
                    ),
            );
        }
        builder.register()
    })
}

fn send_signal(this: *mut AnyObject, signal: StreamSignal) {
    if this.is_null() {
        return;
    }
    let Some(cls) = AnyClass::get(c"HexClipBufferingDelegate") else {
        return;
    };
    let Some(ivar) = cls.instance_variable(c"_state") else {
        return;
    };
    let ptr = unsafe { *ivar.load::<*mut c_void>(&*this) };
    if ptr.is_null() {
        return;
    }
    let state = unsafe { &*(ptr.cast::<DelegateBox>()) };
    let _ = state.signals.try_send(signal);
}

fn make_delegate(signals: SyncSender<StreamSignal>) -> Result<Retained<AnyObject>, String> {
    let cls = delegate_class();
    let obj: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
    let obj = obj.ok_or_else(|| "could not allocate HexClipBufferingDelegate".to_string())?;
    let ivar = cls
        .instance_variable(c"_state")
        .ok_or_else(|| "missing _state ivar on HexClipBufferingDelegate".to_string())?;
    let boxed = Box::into_raw(Box::new(DelegateBox { signals })).cast::<c_void>();
    unsafe {
        let slot = ivar.load::<*mut c_void>(&obj) as *const *mut c_void as *mut *mut c_void;
        slot.write(boxed);
    }
    Ok(obj)
}

/// CoreGraphics display reconfiguration listener (resolution, scale, sleep/wake,
/// mirror, plug/unplug).
pub(super) struct DisplayReconfigWatch {
    context: *mut c_void,
}

// SAFETY: `context` points to a thread-safe `SyncSender<()>` registered with CoreGraphics
// until `Drop` removes the callback before freeing it.
unsafe impl Send for DisplayReconfigWatch {}

const CG_DISPLAY_BEGIN_CONFIGURATION_FLAG: u32 = 1 << 0;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayRegisterReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut c_void),
        user_info: *mut c_void,
    ) -> i32;
    fn CGDisplayRemoveReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut c_void),
        user_info: *mut c_void,
    ) -> i32;
}

unsafe extern "C" fn on_display_reconfigured(display: u32, flags: u32, user_info: *mut c_void) {
    let _ = display;
    if flags & CG_DISPLAY_BEGIN_CONFIGURATION_FLAG != 0 || user_info.is_null() {
        return;
    }
    let sender = unsafe { &*(user_info.cast::<SyncSender<()>>()) };
    let _ = sender.try_send(());
}

impl DisplayReconfigWatch {
    pub fn register(nudge: SyncSender<()>) -> Option<Self> {
        let context = Box::into_raw(Box::new(nudge)).cast::<c_void>();
        let err =
            unsafe { CGDisplayRegisterReconfigurationCallback(on_display_reconfigured, context) };
        if err != 0 {
            unsafe { drop(Box::from_raw(context.cast::<SyncSender<()>>())) };
            tracing::warn!(err, "could not register display reconfiguration callback");
            return None;
        }
        Some(Self { context })
    }
}

impl Drop for DisplayReconfigWatch {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe {
                let _ =
                    CGDisplayRemoveReconfigurationCallback(on_display_reconfigured, self.context);
                drop(Box::from_raw(self.context.cast::<SyncSender<()>>()));
            }
            self.context = ptr::null_mut();
        }
    }
}

/// Active rolling 15 s `SCStream` + `SCClipBufferingOutput` session on one display.
pub(super) struct ClipBufferSession {
    stream: Retained<AnyObject>,
    clip_output: Retained<AnyObject>,
    _delegate: Retained<AnyObject>,
    pub display: BufferDisplay,
    pub started_at: Instant,
}

// SAFETY: `SCStream` and `SCClipBufferingOutput` are thread-safe Objective-C objects managed
// exclusively by the `capture-replay` worker thread.
unsafe impl Send for ClipBufferSession {}

impl ClipBufferSession {
    pub fn open(
        excluded_window_ids: &[u32],
        exclude_own_chrome: bool,
        signals: SyncSender<StreamSignal>,
    ) -> Result<Self, String> {
        if !available() {
            return Err("SCClipBufferingOutput requires macOS 27 or later".into());
        }
        autoreleasepool(|_| {
            let content = fetch_shareable_content()?;
            let preferred_display = preferred_display_id();
            let (sc_display, display_id, rect) = choose_display(&content, preferred_display)?;
            let filter = build_display_filter(
                &content,
                &sc_display,
                excluded_window_ids,
                exclude_own_chrome,
            )?;

            let raw_scale: f32 = unsafe { msg_send![&filter, pointPixelScale] };
            let scale = if raw_scale.is_finite() && raw_scale > 0.0 {
                f64::from(raw_scale)
            } else {
                2.0
            };
            let content_rect: CGRect = unsafe { msg_send![&filter, contentRect] };
            let logical_w = if content_rect.size.width > 0.0 {
                content_rect.size.width
            } else {
                rect[2]
            };
            let logical_h = if content_rect.size.height > 0.0 {
                content_rect.size.height
            } else {
                rect[3]
            };
            let (width, height) = encoded_dimensions(logical_w, logical_h, scale)?;

            let config = build_stream_configuration(width, height)?;
            let delegate = make_delegate(signals)?;

            let stream_cls =
                AnyClass::get(c"SCStream").ok_or_else(|| "SCStream class missing".to_string())?;
            let stream: Option<Retained<AnyObject>> = unsafe {
                let alloc: Allocated<AnyObject> = msg_send![stream_cls, alloc];
                msg_send![
                    alloc,
                    initWithFilter: &*filter,
                    configuration: &*config,
                    delegate: &*delegate
                ]
            };
            let stream = stream.ok_or_else(|| "could not initialize SCStream".to_string())?;

            let clip_cls = AnyClass::get(c"SCClipBufferingOutput")
                .ok_or_else(|| "SCClipBufferingOutput class missing".to_string())?;
            let clip_output: Option<Retained<AnyObject>> = unsafe {
                let alloc: Allocated<AnyObject> = msg_send![clip_cls, alloc];
                msg_send![alloc, initWithDelegate: &*delegate]
            };
            let clip_output = clip_output
                .ok_or_else(|| "could not initialize SCClipBufferingOutput".to_string())?;

            // Attach before startCapture so ScreenCaptureKit serializes
            // `clientOutputType |= 0x10` in the initial XPC stream start; keep
            // a post-start fallback in case a future OS revision requires an
            // already-capturing stream.
            let attached_pre_start = match add_clip_output(&stream, &clip_output) {
                Ok(()) => true,
                Err(pre_err) => {
                    tracing::debug!(
                        error = %pre_err,
                        "pre-start addClipBufferingOutput declined; retrying after startCapture"
                    );
                    false
                }
            };

            if let Err(err) = start_capture(&stream) {
                if attached_pre_start {
                    let _ = remove_clip_output(&stream, &clip_output);
                }
                return Err(err);
            }

            if !attached_pre_start && let Err(err) = add_clip_output(&stream, &clip_output) {
                let _ = stop_capture(&stream);
                return Err(err);
            }

            Ok(Self {
                stream,
                clip_output,
                _delegate: delegate,
                display: BufferDisplay {
                    display_id,
                    frame: rect,
                    scale,
                    width,
                    height,
                },
                started_at: Instant::now(),
            })
        })
    }

    /// Updates Hex's excluded overlay windows in place without resetting the
    /// 15 s rolling clip buffer.
    pub fn update_exclusions(
        &self,
        excluded_window_ids: &[u32],
        exclude_own_chrome: bool,
    ) -> Result<(), String> {
        autoreleasepool(|_| {
            let content = fetch_shareable_content()?;
            let (sc_display, _, _) = choose_display(&content, self.display.display_id)?;
            let filter = build_display_filter(
                &content,
                &sc_display,
                excluded_window_ids,
                exclude_own_chrome,
            )?;
            let (block, rx) = error_completion_channel();
            unsafe {
                let _: () = msg_send![
                    &self.stream,
                    updateContentFilter: &*filter,
                    completionHandler: &*block
                ];
            }
            rx.recv_timeout(SETUP_TIMEOUT)
                .map_err(|_| "timed out updating replay stream filter".to_string())?
        })
    }

    /// Begins an asynchronous clip export to `output_mp4` for `duration_secs`
    /// without interrupting ongoing buffering.
    pub fn export(&self, output_mp4: &Path, duration_secs: f64) -> Receiver<Result<(), String>> {
        autoreleasepool(|_| {
            let (block, rx) = error_completion_channel();
            let url = NSURL::fileURLWithPath(&NSString::from_str(&output_mp4.to_string_lossy()));
            unsafe {
                let _: () = msg_send![
                    &self.clip_output,
                    exportClipToURL: &*url,
                    duration: duration_secs,
                    completionHandler: &*block
                ];
            }
            rx
        })
    }
}

impl Drop for ClipBufferSession {
    fn drop(&mut self) {
        autoreleasepool(|_| {
            let _ = remove_clip_output(&self.stream, &self.clip_output);
            let _ = stop_capture(&self.stream);
        });
    }
}

struct ErrorBlockEncoding;

// SAFETY: Matches Clang's `void (^)(NSError * _Nullable)` block signature on
// 64-bit Darwin (`v16@?0@"NSError"8`). `SCClipBufferingOutput` passes
// `exportClipToURL:duration:completionHandler:`'s block directly through
// `RPDaemonProxy` to `NSXPCConnection`, which throws an Objective-C
// `NSInvalidArgumentException` unless `BLOCK_HAS_SIGNATURE` is populated.
unsafe impl ManualBlockEncoding for ErrorBlockEncoding {
    type Arguments = (*mut NSError,);
    type Return = ();
    const ENCODING_CSTR: &'static CStr = cr#"v16@?0@"NSError"8"#;
}

struct ShareableContentBlockEncoding;

// SAFETY: Matches Clang's `void (^)(SCShareableContent * _Nullable, NSError * _Nullable)`
// block signature on 64-bit Darwin.
unsafe impl ManualBlockEncoding for ShareableContentBlockEncoding {
    type Arguments = (*mut AnyObject, *mut NSError);
    type Return = ();
    const ENCODING_CSTR: &'static CStr = cr#"v24@?0@"SCShareableContent"8@"NSError"16"#;
}

type ErrorCompletionBlock = RcBlock<dyn Fn(*mut NSError)>;
type ErrorCompletionRx = Receiver<Result<(), String>>;

fn error_completion_channel() -> (ErrorCompletionBlock, ErrorCompletionRx) {
    let (tx, rx) = mpsc::sync_channel(1);
    let slot = Mutex::new(Some(tx));
    let block = RcBlock::with_encoding::<_, _, _, ErrorBlockEncoding>(move |err: *mut NSError| {
        if let Some(tx) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let res = if err.is_null() {
                Ok(())
            } else {
                Err(format_ns_error(err))
            };
            let _ = tx.send(res);
        }
    });
    (block, rx)
}

fn fetch_shareable_content() -> Result<Retained<AnyObject>, String> {
    let cls = AnyClass::get(c"SCShareableContent")
        .ok_or_else(|| "SCShareableContent class missing".to_string())?;
    let (tx, rx) = mpsc::sync_channel(1);
    let slot = Mutex::new(Some(tx));
    let block = RcBlock::with_encoding::<_, _, _, ShareableContentBlockEncoding>(
        move |content: *mut AnyObject, err: *mut NSError| {
            if let Some(tx) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let res = if !content.is_null() {
                    // SAFETY: Retain `SCShareableContent *` across threads before callback returns.
                    unsafe { Retained::retain(content) }
                        .ok_or_else(|| "failed to retain SCShareableContent".to_string())
                } else {
                    Err(format_ns_error(err))
                };
                let _ = tx.send(res);
            }
        },
    );
    unsafe {
        let _: () = msg_send![
            cls,
            getShareableContentExcludingDesktopWindows: Bool::NO,
            onScreenWindowsOnly: Bool::YES,
            completionHandler: &*block
        ];
    }
    rx.recv_timeout(SETUP_TIMEOUT)
        .map_err(|_| "timed out enumerating shareable content".to_string())?
}

fn preferred_display_id() -> u32 {
    let screens = scrawl_capture::screens::displays();
    if screens.is_empty() {
        return scrawl_capture::capture::main_display_id();
    }
    let rects: Vec<[f64; 4]> = screens.iter().map(|s| s.frame).collect();
    let pointer = scrawl_capture::screens::pointer();
    let idx = scrawl_capture::capture::pick_display(&rects, pointer, None);
    screens
        .get(idx)
        .map(|s| s.id)
        .unwrap_or_else(scrawl_capture::capture::main_display_id)
}

fn choose_display(
    content: &AnyObject,
    preferred_id: u32,
) -> Result<(Retained<AnyObject>, u32, [f64; 4]), String> {
    let displays: Retained<NSArray<AnyObject>> = unsafe { msg_send![content, displays] };
    let count = displays.count();
    if count == 0 {
        return Err("no shareable display is online".into());
    }
    let mut first = None;
    for i in 0..count {
        let item = displays.objectAtIndex(i);
        let id: u32 = unsafe { msg_send![&item, displayID] };
        let frame: CGRect = unsafe { msg_send![&item, frame] };
        let rect = [
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ];
        if id == preferred_id {
            return Ok((item, id, rect));
        }
        if first.is_none() {
            first = Some((item, id, rect));
        }
    }
    first.ok_or_else(|| "no shareable display is online".into())
}

fn build_display_filter(
    content: &AnyObject,
    display: &AnyObject,
    excluded_window_ids: &[u32],
    exclude_own_chrome: bool,
) -> Result<Retained<AnyObject>, String> {
    let windows: Retained<NSArray<AnyObject>> = unsafe { msg_send![content, windows] };
    let our_pid = std::process::id() as i32;
    let mut excluded: Vec<Retained<AnyObject>> = Vec::new();
    for i in 0..windows.count() {
        let win = windows.objectAtIndex(i);
        let win_id: u32 = unsafe { msg_send![&win, windowID] };
        let mut drop_it = excluded_window_ids.contains(&win_id);
        if !drop_it && exclude_own_chrome {
            let app: *mut AnyObject = unsafe { msg_send![&win, owningApplication] };
            if !app.is_null() {
                let pid: i32 = unsafe { msg_send![app, processID] };
                let layer: isize = unsafe { msg_send![&win, windowLayer] };
                if pid == our_pid && layer != 0 {
                    drop_it = true;
                }
            }
        }
        if drop_it {
            excluded.push(win);
        }
    }
    let refs: Vec<&AnyObject> = excluded.iter().map(|w| &**w).collect();
    let excluded_ns = NSArray::from_slice(&refs);
    let filter_cls = AnyClass::get(c"SCContentFilter")
        .ok_or_else(|| "SCContentFilter class missing".to_string())?;
    let filter: Option<Retained<AnyObject>> = unsafe {
        let alloc: Allocated<AnyObject> = msg_send![filter_cls, alloc];
        msg_send![alloc, initWithDisplay: display, excludingWindows: &*excluded_ns]
    };
    filter.ok_or_else(|| "could not build replay SCContentFilter".into())
}

fn build_stream_configuration(width: u32, height: u32) -> Result<Retained<AnyObject>, String> {
    let cls = AnyClass::get(c"SCStreamConfiguration")
        .ok_or_else(|| "SCStreamConfiguration class missing".to_string())?;
    let config: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
    let config = config.ok_or_else(|| "could not allocate SCStreamConfiguration".to_string())?;
    let interval = RawCMTime::frame_interval(BUFFER_FPS);
    unsafe {
        let _: () = msg_send![&config, setWidth: width as usize];
        let _: () = msg_send![&config, setHeight: height as usize];
        let _: () = msg_send![&config, setMinimumFrameInterval: interval];
        let _: () = msg_send![&config, setQueueDepth: QUEUE_DEPTH];
        let _: () = msg_send![&config, setShowsCursor: Bool::NO];
        let _: () = msg_send![&config, setCapturesAudio: Bool::NO];
        let _: () = msg_send![&config, setExcludesCurrentProcessAudio: Bool::YES];
        let has_mic: Bool = msg_send![&config, respondsToSelector: sel!(setCaptureMicrophone:)];
        if has_mic.as_bool() {
            let _: () = msg_send![&config, setCaptureMicrophone: Bool::NO];
        }
    }
    Ok(config)
}

pub(super) fn encoded_dimensions(
    logical_width: f64,
    logical_height: f64,
    scale: f64,
) -> Result<(u32, u32), String> {
    let raw_w = (logical_width * scale).round();
    let raw_h = (logical_height * scale).round();
    if !raw_w.is_finite() || !raw_h.is_finite() || raw_w < 2.0 || raw_h < 2.0 {
        return Err(format!(
            "invalid display dimensions ({logical_width}x{logical_height} @ {scale}x)"
        ));
    }
    let long = raw_w.max(raw_h);
    let factor = if long > f64::from(MAX_LONG_EDGE) {
        f64::from(MAX_LONG_EDGE) / long
    } else {
        1.0
    };
    let even = |v: f64| ((v * factor).round().max(2.0) as u32) & !1;
    Ok((even(raw_w).max(2), even(raw_h).max(2)))
}

fn add_clip_output(stream: &AnyObject, clip_output: &AnyObject) -> Result<(), String> {
    let mut err: *mut NSError = ptr::null_mut();
    let ok: Bool = unsafe {
        msg_send![
            stream,
            addClipBufferingOutput: clip_output,
            error: &mut err
        ]
    };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(format_ns_error(err))
    }
}

fn remove_clip_output(stream: &AnyObject, clip_output: &AnyObject) -> Result<(), String> {
    let mut err: *mut NSError = ptr::null_mut();
    let ok: Bool = unsafe {
        msg_send![
            stream,
            removeClipBufferingOutput: clip_output,
            error: &mut err
        ]
    };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(format_ns_error(err))
    }
}

fn start_capture(stream: &AnyObject) -> Result<(), String> {
    let (block, rx) = error_completion_channel();
    unsafe {
        let _: () = msg_send![stream, startCaptureWithCompletionHandler: &*block];
    }
    rx.recv_timeout(SETUP_TIMEOUT)
        .map_err(|_| "timed out starting SCStream for clip buffering".to_string())?
}

fn stop_capture(stream: &AnyObject) -> Result<(), String> {
    let (block, rx) = error_completion_channel();
    unsafe {
        let _: () = msg_send![stream, stopCaptureWithCompletionHandler: &*block];
    }
    rx.recv_timeout(STOP_TIMEOUT)
        .map_err(|_| "timed out stopping replay SCStream".to_string())?
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, c_char, c_void};

    use super::error_completion_channel;

    unsafe extern "C" {
        fn _Block_has_signature(block: *const c_void) -> bool;
        fn _Block_signature(block: *const c_void) -> *const c_char;
    }

    #[test]
    fn error_completion_block_carries_xpc_signature() {
        let (block, _rx) = error_completion_channel();
        let raw = block2::RcBlock::as_ptr(&block).cast::<c_void>();
        assert!(unsafe { _Block_has_signature(raw) });
        let sig = unsafe { CStr::from_ptr(_Block_signature(raw)) };
        assert_eq!(sig, cr#"v16@?0@"NSError"8"#);
    }
}
