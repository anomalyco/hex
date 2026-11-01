//! One PipeWire video stream on the screen-cast portal's remote. `libpipewire-0.3` is loaded
//! when a capture starts, so Hex neither builds nor ships against it, and nothing changes for
//! a desktop without it until a capture is asked for.
//!
//! The stream offers two formats: 32-bit RGB as a linear DMA-BUF (what niri and most
//! compositors hand out) and the same as shared memory. The recorder takes the newest buffer
//! on its own clock with [`Stream::newest`], which holds it until the next call so the producer
//! can't reuse it while the GPU or the CPU reads it.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::os::fd::{IntoRawFd, OwnedFd};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};

// ─── libpipewire, loaded at run time ─────────────────────────────────────────

#[repr(C)]
struct SpaPod {
    size: u32,
    kind: u32,
}

#[repr(C)]
struct SpaChunk {
    offset: u32,
    size: u32,
    stride: i32,
    flags: i32,
}

#[repr(C)]
struct SpaData {
    kind: u32,
    flags: u32,
    fd: i64,
    mapoffset: u32,
    maxsize: u32,
    data: *mut c_void,
    chunk: *mut SpaChunk,
}

#[repr(C)]
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut c_void,
    datas: *mut SpaData,
}

#[repr(C)]
struct PwBuffer {
    buffer: *mut SpaBuffer,
}

/// `struct spa_hook`: a list link, the callbacks, `removed` and `priv`.
#[repr(C)]
struct SpaHook([usize; 6]);

type Callback = Option<unsafe extern "C" fn(*mut c_void)>;

#[repr(C)]
struct StreamEvents {
    version: u32,
    destroy: Callback,
    state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
    control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
    param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const SpaPod)>,
    add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    process: Callback,
    drained: Callback,
    command: Option<unsafe extern "C" fn(*mut c_void, *const c_void)>,
    trigger_done: Callback,
}

macro_rules! library {
    ($name:ident { $($f:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)* }) => {
        #[allow(non_snake_case)]
        struct $name {
            $($f: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl $name {
            /// # Safety
            /// `handle` is a `dlopen` handle of a library exporting these C functions.
            unsafe fn load(handle: *mut c_void) -> Result<Self> {
                Ok(Self {
                    $($f: {
                        let name = concat!(stringify!($f), "\0");
                        let symbol = unsafe { libc::dlsym(handle, name.as_ptr().cast()) };
                        if symbol.is_null() {
                            bail!("{} has no {}", stringify!($name), stringify!($f));
                        }
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(symbol) }
                    },)*
                })
            }
        }
    };
}
pub(crate) use library;

library!(Pw {
    pw_init: fn(*mut c_int, *mut *mut *mut c_char);
    pw_thread_loop_new: fn(*const c_char, *const c_void) -> *mut c_void;
    pw_thread_loop_get_loop: fn(*mut c_void) -> *mut c_void;
    pw_thread_loop_start: fn(*mut c_void) -> c_int;
    pw_thread_loop_stop: fn(*mut c_void);
    pw_thread_loop_lock: fn(*mut c_void);
    pw_thread_loop_unlock: fn(*mut c_void);
    pw_thread_loop_destroy: fn(*mut c_void);
    pw_context_new: fn(*mut c_void, *mut c_void, usize) -> *mut c_void;
    pw_context_connect_fd: fn(*mut c_void, c_int, *mut c_void, usize) -> *mut c_void;
    pw_context_destroy: fn(*mut c_void);
    pw_core_disconnect: fn(*mut c_void) -> c_int;
    pw_properties_new_string: fn(*const c_char) -> *mut c_void;
    pw_stream_new: fn(*mut c_void, *const c_char, *mut c_void) -> *mut c_void;
    pw_stream_add_listener: fn(*mut c_void, *mut SpaHook, *const StreamEvents, *mut c_void);
    pw_stream_connect: fn(*mut c_void, u32, u32, u32, *mut *const SpaPod, u32) -> c_int;
    pw_stream_update_params: fn(*mut c_void, *mut *const SpaPod, u32) -> c_int;
    pw_stream_dequeue_buffer: fn(*mut c_void) -> *mut PwBuffer;
    pw_stream_queue_buffer: fn(*mut c_void, *mut PwBuffer) -> c_int;
    pw_stream_disconnect: fn(*mut c_void) -> c_int;
    pw_stream_destroy: fn(*mut c_void);
});

/// Opens a shared library by soname, once per process.
pub(crate) fn open(soname: &str) -> Result<*mut c_void> {
    let name = CString::new(soname)?;
    let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        let why = unsafe { libc::dlerror() };
        let why = if why.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(why) }
                .to_string_lossy()
                .into_owned()
        };
        bail!("could not load {soname}: {why}");
    }
    Ok(handle)
}

fn pw() -> Result<&'static Pw> {
    static PW: OnceLock<std::result::Result<Pw, String>> = OnceLock::new();
    PW.get_or_init(|| {
        let handle = open("libpipewire-0.3.so.0").map_err(|e| format!("{e:#}"))?;
        let pw = unsafe { Pw::load(handle) }.map_err(|e| format!("{e:#}"))?;
        unsafe { (pw.pw_init)(ptr::null_mut(), ptr::null_mut()) };
        Ok(pw)
    })
    .as_ref()
    .map_err(|e| anyhow!("{e}"))
}

// ─── SPA pods ────────────────────────────────────────────────────────────────

const TYPE_ID: u32 = 3;
const TYPE_INT: u32 = 4;
const TYPE_LONG: u32 = 5;
const TYPE_RECTANGLE: u32 = 10;
const TYPE_FRACTION: u32 = 11;
const TYPE_OBJECT: u32 = 15;
const TYPE_CHOICE: u32 = 19;
const OBJECT_FORMAT: u32 = 0x40003;
const OBJECT_PARAM_BUFFERS: u32 = 0x40004;
const PARAM_ENUM_FORMAT: u32 = 3;
const PARAM_FORMAT: u32 = 4;
const PARAM_BUFFERS: u32 = 5;
const FORMAT_MEDIA_TYPE: u32 = 1;
const FORMAT_MEDIA_SUBTYPE: u32 = 2;
const FORMAT_VIDEO_FORMAT: u32 = 0x20001;
const FORMAT_VIDEO_MODIFIER: u32 = 0x20002;
const FORMAT_VIDEO_SIZE: u32 = 0x20003;
const FORMAT_VIDEO_FRAMERATE: u32 = 0x20004;
const MEDIA_TYPE_VIDEO: u32 = 2;
const MEDIA_SUBTYPE_RAW: u32 = 1;
const BUFFERS_DATA_TYPE: u32 = 6;
const CHOICE_RANGE: u32 = 1;
const CHOICE_ENUM: u32 = 3;
const CHOICE_FLAGS: u32 = 4;
const PROP_MANDATORY: u32 = 1 << 3;
const DATA_MEM_PTR: u32 = 1;
const DATA_MEM_FD: u32 = 2;
const DATA_DMA_BUF: u32 = 3;
/// `DRM_FORMAT_MOD_LINEAR`: rows in order, which both the CPU and the video engine read.
const MODIFIER_LINEAR: i64 = 0;
const STREAM_FLAG_AUTOCONNECT: u32 = 1 << 0;
const STREAM_FLAG_MAP_BUFFERS: u32 = 1 << 2;
const DIRECTION_INPUT: u32 = 0;
const STATE_ERROR: c_int = -1;
const STATE_UNCONNECTED: c_int = 0;
const STATE_STREAMING: c_int = 3;

/// 32-bit RGB layouts the recorder takes, by their `spa_video_format`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Rgbx = 7,
    Bgrx = 8,
    Rgba = 11,
    Bgra = 12,
}

impl PixelFormat {
    fn from_spa(value: u32) -> Option<Self> {
        Some(match value {
            7 => Self::Rgbx,
            8 => Self::Bgrx,
            11 => Self::Rgba,
            12 => Self::Bgra,
            _ => return None,
        })
    }

    /// Whether the bytes run blue, green, red.
    pub fn bgr(self) -> bool {
        matches!(self, Self::Bgrx | Self::Bgra)
    }

    /// The DRM fourcc of the same memory layout (little-endian words).
    pub fn drm_fourcc(self) -> u32 {
        let code = match self {
            Self::Bgrx => *b"XR24",
            Self::Rgbx => *b"XB24",
            Self::Bgra => *b"AR24",
            Self::Rgba => *b"AB24",
        };
        u32::from_le_bytes(code)
    }
}

/// Builds pods 8-byte aligned, as SPA reads them.
#[derive(Default)]
struct Pod(Vec<u8>);

impl Pod {
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_ne_bytes());
    }

    fn pad(&mut self) {
        while !self.0.len().is_multiple_of(8) {
            self.0.push(0);
        }
    }

    /// A pod header whose size is filled in by `end`.
    fn begin(&mut self, kind: u32) -> usize {
        let at = self.0.len();
        self.u32(0);
        self.u32(kind);
        at
    }

    fn end(&mut self, at: usize) {
        let size = (self.0.len() - at - 8) as u32;
        self.0[at..at + 4].copy_from_slice(&size.to_ne_bytes());
        self.pad();
    }

    fn object(&mut self, kind: u32, id: u32, props: impl FnOnce(&mut Self)) {
        let at = self.begin(TYPE_OBJECT);
        self.u32(kind);
        self.u32(id);
        props(self);
        self.end(at);
    }

    fn prop(&mut self, key: u32, flags: u32) {
        self.u32(key);
        self.u32(flags);
    }

    fn id(&mut self, value: u32) {
        self.u32(4);
        self.u32(TYPE_ID);
        self.u32(value);
        self.pad();
    }

    /// A choice of `values` (each `child_size` bytes of `child`), the first being the default.
    fn choice(&mut self, choice: u32, child: u32, child_size: u32, values: &[&[u8]]) {
        let at = self.begin(TYPE_CHOICE);
        self.u32(choice);
        self.u32(0);
        self.u32(child_size);
        self.u32(child);
        for value in values {
            self.0.extend_from_slice(value);
        }
        self.end(at);
    }

    fn into_words(self) -> Vec<u64> {
        let mut words = vec![0_u64; self.0.len().div_ceil(8)];
        // SAFETY: `words` holds at least `self.0.len()` bytes.
        unsafe {
            ptr::copy_nonoverlapping(self.0.as_ptr(), words.as_mut_ptr().cast(), self.0.len());
        }
        words
    }
}

fn pair(a: u32, b: u32) -> [u8; 8] {
    let mut out = [0; 8];
    out[..4].copy_from_slice(&a.to_ne_bytes());
    out[4..].copy_from_slice(&b.to_ne_bytes());
    out
}

/// The formats offered: 32-bit RGB at any size, as a linear DMA-BUF when `dmabuf`, else as
/// shared memory.
fn enum_format(dmabuf: bool) -> Vec<u64> {
    let mut pod = Pod::default();
    pod.object(OBJECT_FORMAT, PARAM_ENUM_FORMAT, |pod| {
        pod.prop(FORMAT_MEDIA_TYPE, 0);
        pod.id(MEDIA_TYPE_VIDEO);
        pod.prop(FORMAT_MEDIA_SUBTYPE, 0);
        pod.id(MEDIA_SUBTYPE_RAW);
        pod.prop(FORMAT_VIDEO_FORMAT, 0);
        let formats = [
            PixelFormat::Bgrx,
            PixelFormat::Bgrx,
            PixelFormat::Rgbx,
            PixelFormat::Bgra,
            PixelFormat::Rgba,
        ]
        .map(|f| (f as u32).to_ne_bytes());
        pod.choice(
            CHOICE_ENUM,
            TYPE_ID,
            4,
            &formats.iter().map(|f| &f[..]).collect::<Vec<_>>(),
        );
        if dmabuf {
            pod.prop(FORMAT_VIDEO_MODIFIER, PROP_MANDATORY);
            let linear = MODIFIER_LINEAR.to_ne_bytes();
            pod.choice(CHOICE_ENUM, TYPE_LONG, 8, &[&linear, &linear]);
        }
        pod.prop(FORMAT_VIDEO_SIZE, 0);
        pod.choice(
            CHOICE_RANGE,
            TYPE_RECTANGLE,
            8,
            &[&pair(1920, 1080), &pair(1, 1), &pair(16384, 16384)],
        );
        pod.prop(FORMAT_VIDEO_FRAMERATE, 0);
        pod.choice(
            CHOICE_RANGE,
            TYPE_FRACTION,
            8,
            &[&pair(0, 1), &pair(0, 1), &pair(1000, 1)],
        );
    });
    pod.into_words()
}

fn buffers(dmabuf: bool) -> Vec<u64> {
    let mask = if dmabuf {
        1 << DATA_DMA_BUF
    } else {
        (1 << DATA_MEM_FD) | (1 << DATA_MEM_PTR)
    };
    let mut pod = Pod::default();
    pod.object(OBJECT_PARAM_BUFFERS, PARAM_BUFFERS, |pod| {
        pod.prop(BUFFERS_DATA_TYPE, 0);
        pod.choice(CHOICE_FLAGS, TYPE_INT, 4, &[&(mask as u32).to_ne_bytes()]);
    });
    pod.into_words()
}

/// What a negotiated `Format` param says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoFormat {
    pub pixel: PixelFormat,
    pub width: u32,
    pub height: u32,
    /// Buffers are DMA-BUFs (a modifier was negotiated), not shared memory.
    pub dmabuf: bool,
}

/// Reads a `Format` object: its pixel format, size, and whether it carries a modifier.
///
/// # Safety
/// `pod` points at a whole pod.
unsafe fn parse_format(pod: *const SpaPod) -> Option<VideoFormat> {
    let header = unsafe { &*pod };
    if header.kind != TYPE_OBJECT {
        return None;
    }
    let body = unsafe { std::slice::from_raw_parts(pod.cast::<u8>().add(8), header.size as usize) };
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_ne_bytes(body.get(at..at + 4)?.try_into().ok()?))
    };
    let (mut pixel, mut size, mut dmabuf) = (None, None, false);
    let mut at = 8;
    while at + 16 <= body.len() {
        let key = word(at)?;
        let value_size = word(at + 8)? as usize;
        let value_kind = word(at + 12)?;
        let value = body.get(at + 16..at + 16 + value_size)?;
        // A choice's first value is its default (or only) value.
        let (kind, value) = if value_kind == TYPE_CHOICE && value.len() >= 16 {
            let child_kind = u32::from_ne_bytes(value[12..16].try_into().ok()?);
            (child_kind, &value[16..])
        } else {
            (value_kind, value)
        };
        let first = |n: usize| value.get(..n);
        match key {
            FORMAT_VIDEO_FORMAT if kind == TYPE_ID => {
                pixel = PixelFormat::from_spa(u32::from_ne_bytes(first(4)?.try_into().ok()?));
            }
            FORMAT_VIDEO_SIZE if kind == TYPE_RECTANGLE => {
                let bytes = first(8)?;
                size = Some((
                    u32::from_ne_bytes(bytes[..4].try_into().ok()?),
                    u32::from_ne_bytes(bytes[4..].try_into().ok()?),
                ));
            }
            FORMAT_VIDEO_MODIFIER => dmabuf = true,
            _ => {}
        }
        at += 16 + value_size.next_multiple_of(8);
    }
    let (width, height) = size?;
    Some(VideoFormat {
        pixel: pixel?,
        width,
        height,
        dmabuf,
    })
}

// ─── The stream ──────────────────────────────────────────────────────────────

/// What the loop thread's callbacks tell the recorder.
#[derive(Default)]
struct Shared {
    format: Option<VideoFormat>,
    streaming: bool,
    /// The stream failed, or the portal ended it (the window closed).
    ended: Option<String>,
}

/// The loop thread's callback context.
struct Listener {
    pw: &'static Pw,
    stream: *mut c_void,
    shared: Mutex<Shared>,
    /// CPU views of DMA-BUFs, by fd, made on first read and unmapped with their buffer.
    maps: Mutex<HashMap<i64, (usize, usize)>>,
}

unsafe extern "C" fn on_state(data: *mut c_void, _old: c_int, state: c_int, error: *const c_char) {
    let listener = unsafe { &*data.cast::<Listener>() };
    let mut shared = listener.shared.lock().unwrap_or_else(|e| e.into_inner());
    match state {
        STATE_STREAMING => shared.streaming = true,
        STATE_ERROR => {
            let why = if error.is_null() {
                "the screen stream failed".to_owned()
            } else {
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            shared.ended = Some(why);
        }
        STATE_UNCONNECTED if shared.streaming => {
            shared.ended = Some("the screen stream ended".into());
        }
        _ => {}
    }
}

unsafe extern "C" fn on_param(data: *mut c_void, id: u32, param: *const SpaPod) {
    if id != PARAM_FORMAT || param.is_null() {
        return;
    }
    let listener = unsafe { &*data.cast::<Listener>() };
    let Some(format) = (unsafe { parse_format(param) }) else {
        return;
    };
    listener
        .shared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .format = Some(format);
    let words = buffers(format.dmabuf);
    let mut params = [words.as_ptr().cast::<SpaPod>()];
    unsafe { (listener.pw.pw_stream_update_params)(listener.stream, params.as_mut_ptr(), 1) };
}

unsafe extern "C" fn on_remove_buffer(data: *mut c_void, buffer: *mut PwBuffer) {
    let listener = unsafe { &*data.cast::<Listener>() };
    let Some(data) = (unsafe { first_data(buffer) }) else {
        return;
    };
    if let Some((address, length)) = listener
        .maps
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&data.fd)
    {
        unsafe { libc::munmap(address as *mut c_void, length) };
    }
}

/// # Safety
/// `buffer` is a live `pw_buffer`.
unsafe fn first_data<'a>(buffer: *mut PwBuffer) -> Option<&'a SpaData> {
    let spa = unsafe { (*buffer).buffer };
    if spa.is_null() || unsafe { (*spa).n_datas } == 0 {
        return None;
    }
    Some(unsafe { &*(*spa).datas })
}

static EVENTS: StreamEvents = StreamEvents {
    version: 2,
    destroy: None,
    state_changed: Some(on_state),
    control_info: None,
    io_changed: None,
    param_changed: Some(on_param),
    add_buffer: None,
    remove_buffer: Some(on_remove_buffer),
    process: None,
    drained: None,
    command: None,
    trigger_done: None,
};

/// The newest frame, held until the next [`Stream::newest`].
pub struct Frame {
    /// The buffer's fd: a DMA-BUF, or shared memory.
    pub fd: i64,
    pub dmabuf: bool,
    pub offset: u32,
    pub stride: u32,
    /// The whole buffer object's size, for importing it.
    pub size: u32,
    /// Where the CPU reads its first row, or why it can't (a DMA-BUF that won't map).
    rows: std::result::Result<*const u8, String>,
}

impl Frame {
    /// Rows in memory, `stride` apart, starting at the frame's first pixel.
    pub fn rows(&self) -> Result<*const u8> {
        self.rows.clone().map_err(|why| anyhow!("{why}"))
    }
}

pub struct Stream {
    pw: &'static Pw,
    thread_loop: *mut c_void,
    context: *mut c_void,
    core: *mut c_void,
    listener: Box<Listener>,
    hook: Box<SpaHook>,
    held: *mut PwBuffer,
}

// The raw handles are only used under the thread loop's lock.
unsafe impl Send for Stream {}

impl Stream {
    /// Connects to `node` on the portal's PipeWire remote.
    pub fn connect(remote: OwnedFd, node: u32) -> Result<Self> {
        let pw = pw()?;
        let name = c"hex-capture";
        let thread_loop = unsafe { (pw.pw_thread_loop_new)(name.as_ptr(), ptr::null()) };
        if thread_loop.is_null() {
            bail!("could not make a PipeWire loop");
        }
        let context = unsafe {
            (pw.pw_context_new)(
                (pw.pw_thread_loop_get_loop)(thread_loop),
                ptr::null_mut(),
                0,
            )
        };
        if context.is_null() {
            unsafe { (pw.pw_thread_loop_destroy)(thread_loop) };
            bail!("could not make a PipeWire context");
        }
        let mut stream = Self {
            pw,
            thread_loop,
            context,
            core: ptr::null_mut(),
            listener: Box::new(Listener {
                pw,
                stream: ptr::null_mut(),
                shared: Mutex::default(),
                maps: Mutex::default(),
            }),
            hook: Box::new(SpaHook([0; 6])),
            held: ptr::null_mut(),
        };
        if unsafe { (pw.pw_thread_loop_start)(thread_loop) } < 0 {
            bail!("could not start the PipeWire loop");
        }
        unsafe { (pw.pw_thread_loop_lock)(thread_loop) };
        let connected = stream.connect_locked(remote, node);
        unsafe { (pw.pw_thread_loop_unlock)(thread_loop) };
        connected.map(|()| stream)
    }

    fn connect_locked(&mut self, remote: OwnedFd, node: u32) -> Result<()> {
        let pw = self.pw;
        // The core owns the fd from here.
        self.core = unsafe {
            (pw.pw_context_connect_fd)(self.context, remote.into_raw_fd(), ptr::null_mut(), 0)
        };
        if self.core.is_null() {
            bail!("could not reach the portal's PipeWire remote");
        }
        let props = c"media.type=Video media.category=Capture media.role=Screen";
        let stream = unsafe {
            (pw.pw_stream_new)(
                self.core,
                c"hex-capture".as_ptr(),
                (pw.pw_properties_new_string)(props.as_ptr()),
            )
        };
        if stream.is_null() {
            bail!("could not make a PipeWire stream");
        }
        self.listener.stream = stream;
        unsafe {
            (pw.pw_stream_add_listener)(
                stream,
                &mut *self.hook,
                &EVENTS,
                (&raw const *self.listener).cast_mut().cast(),
            );
        }
        let (dmabuf, shm) = (enum_format(true), enum_format(false));
        let mut params = [dmabuf.as_ptr().cast::<SpaPod>(), shm.as_ptr().cast()];
        let status = unsafe {
            (pw.pw_stream_connect)(
                stream,
                DIRECTION_INPUT,
                node,
                STREAM_FLAG_AUTOCONNECT | STREAM_FLAG_MAP_BUFFERS,
                params.as_mut_ptr(),
                params.len() as u32,
            )
        };
        if status < 0 {
            bail!("could not connect the PipeWire stream ({status})");
        }
        Ok(())
    }

    /// The negotiated format, once the producer agreed to one.
    pub fn format(&self) -> Option<VideoFormat> {
        self.listener
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .format
    }

    /// Why the stream stopped, once it did.
    pub fn ended(&self) -> Option<String> {
        self.listener
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ended
            .clone()
    }

    /// Takes the newest buffer the producer queued, giving back the one held before (and any
    /// older ones). `None` when nothing new arrived; the held frame stays valid.
    pub fn newest(&mut self) -> Option<Frame> {
        let pw = self.pw;
        let stream = self.listener.stream;
        unsafe { (pw.pw_thread_loop_lock)(self.thread_loop) };
        let mut newest: *mut PwBuffer = ptr::null_mut();
        loop {
            let buffer = unsafe { (pw.pw_stream_dequeue_buffer)(stream) };
            if buffer.is_null() {
                break;
            }
            if !newest.is_null() {
                unsafe { (pw.pw_stream_queue_buffer)(stream, newest) };
            }
            newest = buffer;
        }
        if !newest.is_null() && !self.held.is_null() {
            unsafe { (pw.pw_stream_queue_buffer)(stream, self.held) };
            self.held = ptr::null_mut();
        }
        unsafe { (pw.pw_thread_loop_unlock)(self.thread_loop) };
        if newest.is_null() {
            return None;
        }
        self.held = newest;
        let data = unsafe { first_data(newest) }?;
        let chunk = unsafe { data.chunk.as_ref() }?;
        if chunk.flags & 1 != 0 || data.kind == 0 {
            // Corrupted, or no memory at all (a cursor-only update).
            return None;
        }
        let format = self.format()?;
        let stride = u32::try_from(chunk.stride)
            .ok()
            .filter(|&s| s > 0)
            .unwrap_or(format.width * 4);
        // The GPU encoder imports a DMA-BUF without reading it, so one the CPU can't map
        // still records there; only the CPU path needs `rows`.
        let base = if data.data.is_null() {
            self.map(data)
        } else {
            Ok(data.data.cast::<u8>().cast_const())
        };
        Some(Frame {
            fd: data.fd,
            dmabuf: data.kind == DATA_DMA_BUF,
            offset: chunk.offset,
            stride,
            size: data.maxsize + data.mapoffset,
            rows: base.map(|base| unsafe { base.add(chunk.offset as usize) }),
        })
    }

    /// A CPU view of a buffer PipeWire didn't map (a DMA-BUF), kept until it's removed.
    fn map(&self, data: &SpaData) -> std::result::Result<*const u8, String> {
        let mut maps = self.listener.maps.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&(address, _)) = maps.get(&data.fd) {
            return Ok((address + data.mapoffset as usize) as *const u8);
        }
        let length = (data.maxsize + data.mapoffset) as usize;
        let address = unsafe {
            libc::mmap(
                ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                data.fd as c_int,
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(format!(
                "the CPU can't read the screen's GPU buffers ({})",
                std::io::Error::last_os_error()
            ));
        }
        maps.insert(data.fd, (address as usize, length));
        Ok((address as usize + data.mapoffset as usize) as *const u8)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let pw = self.pw;
        unsafe {
            (pw.pw_thread_loop_lock)(self.thread_loop);
            let stream = self.listener.stream;
            if !stream.is_null() {
                if !self.held.is_null() {
                    (pw.pw_stream_queue_buffer)(stream, self.held);
                }
                (pw.pw_stream_disconnect)(stream);
                (pw.pw_stream_destroy)(stream);
            }
            if !self.core.is_null() {
                (pw.pw_core_disconnect)(self.core);
            }
            (pw.pw_thread_loop_unlock)(self.thread_loop);
            (pw.pw_thread_loop_stop)(self.thread_loop);
            (pw.pw_context_destroy)(self.context);
            (pw.pw_thread_loop_destroy)(self.thread_loop);
        }
        for (_, (address, length)) in self
            .listener
            .maps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            unsafe { libc::munmap(address as *mut c_void, length) };
        }
    }
}

/// Brackets CPU reads of a DMA-BUF so the GPU's writes are visible.
pub fn sync_dmabuf(fd: i64, start: bool) {
    #[repr(C)]
    struct DmaBufSync {
        flags: u64,
    }
    const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
    const SYNC_READ: u64 = 1;
    const SYNC_END: u64 = 4;
    let sync = DmaBufSync {
        flags: SYNC_READ | if start { 0 } else { SYNC_END },
    };
    unsafe { libc::ioctl(fd as c_int, DMA_BUF_IOCTL_SYNC, &sync) };
}

/// `Context` for a missing PipeWire library, worded for the person.
pub fn available() -> Result<()> {
    pw().map(|_| ()).context("Linux captures need PipeWire")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_formats_parse_back() {
        for dmabuf in [true, false] {
            let words = enum_format(dmabuf);
            let format = unsafe { parse_format(words.as_ptr().cast()) }.unwrap();
            assert_eq!(format.pixel, PixelFormat::Bgrx);
            assert_eq!((format.width, format.height), (1920, 1080));
            assert_eq!(format.dmabuf, dmabuf);
        }
    }
}
