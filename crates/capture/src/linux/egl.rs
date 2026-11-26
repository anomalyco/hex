//! Reading tiled screen buffers back through EGL. Some desktops hand out the screen only as
//! DMA-BUFs in a GPU's own tiled layout (niri on NVIDIA), which the CPU can't read as rows and
//! a GPU without a VA-API encoder can't encode. EGL imports such a buffer as a GL texture and
//! `glReadPixels` copies it out as plain rows for x264.
//!
//! Nothing here runs unless the screen stream refused the linear and shared-memory formats:
//! only then are `libEGL.so.1` and `libGLESv2.so.2` loaded and the GPUs asked which tiled
//! layouts they import ([`tiled_modifiers`]). Every DRM device EGL lists is asked (on a laptop
//! with two GPUs the compositor may draw on either) and each display is terminated right after.
//! A [`Reader`] opens a display again only once a tiled format was agreed on, keeps the first
//! device that reads the buffer, and terminates it when the recording ends, so no GPU is kept
//! awake by Hex afterwards.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

use anyhow::{Result, anyhow, bail};

use super::pipewire::{self, Frame, PIXELS, PixelFormat, VideoFormat, library};

type Handle = *mut c_void;

library!(Egl {
    eglGetProcAddress: fn(*const c_char) -> *mut c_void;
    eglQueryString: fn(Handle, i32) -> *const c_char;
    eglGetPlatformDisplay: fn(u32, Handle, *const isize) -> Handle;
    eglInitialize: fn(Handle, *mut i32, *mut i32) -> u32;
    eglBindAPI: fn(u32) -> u32;
    eglCreateContext: fn(Handle, Handle, Handle, *const i32) -> Handle;
    eglDestroyContext: fn(Handle, Handle) -> u32;
    eglMakeCurrent: fn(Handle, Handle, Handle, Handle) -> u32;
    eglTerminate: fn(Handle) -> u32;
    eglGetError: fn() -> i32;
});

library!(Gles {
    glGenTextures: fn(i32, *mut u32);
    glDeleteTextures: fn(i32, *const u32);
    glBindTexture: fn(u32, u32);
    glTexParameteri: fn(u32, u32, i32);
    glGenFramebuffers: fn(i32, *mut u32);
    glDeleteFramebuffers: fn(i32, *const u32);
    glBindFramebuffer: fn(u32, u32);
    glFramebufferTexture2D: fn(u32, u32, u32, u32, i32);
    glCheckFramebufferStatus: fn(u32) -> u32;
    glPixelStorei: fn(u32, i32);
    glReadPixels: fn(i32, i32, i32, i32, u32, u32, *mut c_void);
    glGetString: fn(u32) -> *const u8;
    glGetError: fn() -> u32;
});

/// The extension entry points, from `eglGetProcAddress`.
#[allow(non_snake_case)]
struct Ext {
    eglQueryDevicesEXT: unsafe extern "C" fn(i32, *mut Handle, *mut i32) -> u32,
    eglQueryDeviceStringEXT: unsafe extern "C" fn(Handle, i32) -> *const c_char,
    eglQueryDmaBufModifiersEXT:
        unsafe extern "C" fn(Handle, i32, i32, *mut u64, *mut u32, *mut i32) -> u32,
    eglCreateImageKHR: unsafe extern "C" fn(Handle, Handle, u32, Handle, *const i32) -> Handle,
    eglDestroyImageKHR: unsafe extern "C" fn(Handle, Handle) -> u32,
    glEGLImageTargetTexture2DOES: unsafe extern "C" fn(u32, Handle),
}

const EGL_EXTENSIONS: i32 = 0x3055;
const EGL_NONE: i32 = 0x3038;
const EGL_WIDTH: i32 = 0x3057;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_PLATFORM_DEVICE_EXT: u32 = 0x313F;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;
const EGL_DRM_DEVICE_FILE_EXT: i32 = 0x3233;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_NEAREST: i32 = 0x2600;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_RGBA: u32 = 0x1908;
const GL_BGRA_EXT: u32 = 0x80E1;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_EXTENSIONS: u32 = 0x1F03;
const GL_NO_ERROR: u32 = 0;

/// EGL and GLES, loaded once; loading them wakes no GPU.
struct Lib {
    egl: Egl,
    gles: Gles,
    ext: Ext,
}

fn lib() -> Result<&'static Lib> {
    static LIB: OnceLock<std::result::Result<Lib, String>> = OnceLock::new();
    LIB.get_or_init(|| load().map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow!("{e}"))
}

fn load() -> Result<Lib> {
    let egl = unsafe { Egl::load(pipewire::open("libEGL.so.1")?) }?;
    let gles = unsafe { Gles::load(pipewire::open("libGLESv2.so.2")?) }?;
    let proc = |name: &CStr| -> Result<*mut c_void> {
        let symbol = unsafe { (egl.eglGetProcAddress)(name.as_ptr()) };
        if symbol.is_null() {
            bail!("EGL has no {}", name.to_string_lossy());
        }
        Ok(symbol)
    };
    // SAFETY: each symbol is the named entry point, whose C signature the field spells out.
    let ext = unsafe {
        Ext {
            eglQueryDevicesEXT: std::mem::transmute(proc(c"eglQueryDevicesEXT")?),
            eglQueryDeviceStringEXT: std::mem::transmute(proc(c"eglQueryDeviceStringEXT")?),
            eglQueryDmaBufModifiersEXT: std::mem::transmute(proc(c"eglQueryDmaBufModifiersEXT")?),
            eglCreateImageKHR: std::mem::transmute(proc(c"eglCreateImageKHR")?),
            eglDestroyImageKHR: std::mem::transmute(proc(c"eglDestroyImageKHR")?),
            glEGLImageTargetTexture2DOES: std::mem::transmute(proc(
                c"glEGLImageTargetTexture2DOES",
            )?),
        }
    };
    Ok(Lib { egl, gles, ext })
}

/// A GPU EGL may open: a DRM device, or Mesa's surfaceless platform.
struct Device {
    platform: u32,
    native: usize,
    name: String,
}

/// Every DRM device EGL lists, then Mesa's surfaceless platform (its default GPU).
fn devices(lib: &Lib) -> Vec<Device> {
    let (egl, ext) = (&lib.egl, &lib.ext);
    let client = unsafe { text((egl.eglQueryString)(ptr::null_mut(), EGL_EXTENSIONS)) };
    let mut found = Vec::new();
    if has_extension(&client, "EGL_EXT_platform_device") {
        let mut count = 0;
        unsafe { (ext.eglQueryDevicesEXT)(0, ptr::null_mut(), &mut count) };
        let mut handles = vec![ptr::null_mut(); count.max(0) as usize];
        unsafe { (ext.eglQueryDevicesEXT)(count, handles.as_mut_ptr(), &mut count) };
        handles.truncate(count.max(0) as usize);
        for device in handles {
            // A software renderer has no DRM device and imports nothing.
            let extensions = unsafe { text((ext.eglQueryDeviceStringEXT)(device, EGL_EXTENSIONS)) };
            if !has_extension(&extensions, "EGL_EXT_device_drm") {
                continue;
            }
            let name = unsafe {
                text((ext.eglQueryDeviceStringEXT)(
                    device,
                    EGL_DRM_DEVICE_FILE_EXT,
                ))
            };
            found.push(Device {
                platform: EGL_PLATFORM_DEVICE_EXT,
                native: device as usize,
                name,
            });
        }
    }
    if has_extension(&client, "EGL_MESA_platform_surfaceless") {
        found.push(Device {
            platform: EGL_PLATFORM_SURFACELESS_MESA,
            native: 0,
            name: "the surfaceless platform".into(),
        });
    }
    found
}

/// An initialized EGL display that imports DMA-BUFs with modifiers; terminated when dropped.
struct Display {
    lib: &'static Lib,
    display: usize,
    name: String,
}

impl Display {
    fn open(lib: &'static Lib, device: &Device) -> Result<Self> {
        let egl = &lib.egl;
        let which = &device.name;
        let attributes = [EGL_NONE as isize];
        let display = unsafe {
            (egl.eglGetPlatformDisplay)(
                device.platform,
                device.native as Handle,
                attributes.as_ptr(),
            )
        };
        if display.is_null() {
            bail!("{which}: no display");
        }
        let (mut major, mut minor) = (0, 0);
        if unsafe { (egl.eglInitialize)(display, &mut major, &mut minor) } == 0 {
            bail!("{which}: could not initialize");
        }
        let display = Self {
            lib,
            display: display as usize,
            name: which.clone(),
        };
        let extensions = unsafe { text((egl.eglQueryString)(display.handle(), EGL_EXTENSIONS)) };
        let missing: Vec<&str> = [
            "EGL_EXT_image_dma_buf_import_modifiers",
            "EGL_KHR_surfaceless_context",
            "EGL_KHR_no_config_context",
        ]
        .into_iter()
        .filter(|name| !has_extension(&extensions, name))
        .collect();
        if !missing.is_empty() {
            bail!("{which}: no {}", missing.join(", "));
        }
        Ok(display)
    }

    fn handle(&self) -> Handle {
        self.display as Handle
    }

    /// The tiled layouts it imports for `pixel` as a texture GL can render to.
    fn modifiers(&self, pixel: PixelFormat) -> Vec<u64> {
        unsafe { query_modifiers(&self.lib.ext, self.handle(), pixel.drm_fourcc()) }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        unsafe { (self.lib.egl.eglTerminate)(self.handle()) };
    }
}

/// Whether a space-separated extension list names `name`.
fn has_extension(list: &str, name: &str) -> bool {
    list.split_ascii_whitespace().any(|ext| ext == name)
}

/// # Safety
/// `text` is null or a C string.
unsafe fn text(text: *const c_char) -> String {
    if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }
}

/// The tiled layouts `display` imports for `fourcc` as a texture GL can render to (not only
/// sample as an external image), so `glReadPixels` reads them.
///
/// # Safety
/// `display` is an initialized display with `EGL_EXT_image_dma_buf_import_modifiers`.
unsafe fn query_modifiers(ext: &Ext, display: Handle, fourcc: u32) -> Vec<u64> {
    let mut count = 0;
    let format = fourcc as i32;
    let asked = unsafe {
        (ext.eglQueryDmaBufModifiersEXT)(
            display,
            format,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut count,
        )
    };
    if asked == 0 || count <= 0 {
        return Vec::new();
    }
    let mut modifiers = vec![0_u64; count as usize];
    let mut external = vec![0_u32; count as usize];
    let read = unsafe {
        (ext.eglQueryDmaBufModifiersEXT)(
            display,
            format,
            count,
            modifiers.as_mut_ptr(),
            external.as_mut_ptr(),
            &mut count,
        )
    };
    if read == 0 {
        return Vec::new();
    }
    renderable(&modifiers, &external, count.max(0) as usize)
}

/// The modifiers that aren't external-only, without linear (offered on its own already).
fn renderable(modifiers: &[u64], external: &[u32], count: usize) -> Vec<u64> {
    let mut out = Vec::new();
    for (&modifier, &external) in modifiers.iter().zip(external).take(count) {
        if external == 0 && modifier != pipewire::MODIFIER_LINEAR && !out.contains(&modifier) {
            out.push(modifier);
        }
    }
    out
}

/// The tiled layouts some GPU reads back, by pixel layout: every DRM device's, merged in the
/// order EGL lists them. Each display is terminated before this returns. An error says why
/// none could be asked (no EGL, or no device that imports DMA-BUFs with modifiers).
pub fn tiled_modifiers() -> Result<Vec<(PixelFormat, Vec<u64>)>> {
    let lib = lib()?;
    let mut merged: Vec<(PixelFormat, Vec<u64>)> =
        PIXELS.iter().map(|&pixel| (pixel, Vec::new())).collect();
    let mut failures = Vec::new();
    let mut opened = 0;
    for device in devices(lib) {
        let display = match Display::open(lib, &device) {
            Ok(display) => display,
            Err(error) => {
                failures.push(format!("{error:#}"));
                continue;
            }
        };
        opened += 1;
        for (pixel, modifiers) in &mut merged {
            for modifier in display.modifiers(*pixel) {
                if !modifiers.contains(&modifier) {
                    modifiers.push(modifier);
                }
            }
        }
    }
    if opened == 0 {
        if failures.is_empty() {
            bail!("EGL offers no GPU device");
        }
        bail!("{}", failures.join("; "));
    }
    Ok(merged)
}

/// Reads tiled frames back. Opens nothing until the first frame; then it tries each device
/// that imports the frame's layout and keeps the first that reads it.
#[derive(Default)]
pub struct Reader {
    active: Option<Active>,
}

impl Reader {
    /// Copies `frame`, a single-plane DMA-BUF in `format`'s layout, into `out` as
    /// `format.width * 4`-byte rows in the stream's own byte order.
    pub fn read(&mut self, frame: &Frame, format: &VideoFormat, out: &mut [u8]) -> Result<()> {
        if frame.planes != 1 {
            bail!(
                "the screen's GPU buffers have {} planes; only one is read",
                frame.planes
            );
        }
        if out.len() < format.width as usize * format.height as usize * 4 {
            bail!("the frame doesn't fit its copy");
        }
        if let Some(active) = &mut self.active {
            return active.read(frame, format, out);
        }
        let lib = lib()?;
        let mut failures = Vec::new();
        for device in devices(lib) {
            let tried = Display::open(lib, &device).and_then(|display| {
                if !display.modifiers(format.pixel).contains(&format.modifier) {
                    bail!("{}: doesn't import this layout", display.name);
                }
                let name = display.name.clone();
                let mut active = Active::new(display)?;
                active.read(frame, format, out)?;
                crate::log::scrawl_log(format!("Reading the screen back through EGL on {name}"));
                Ok(active)
            });
            match tried {
                Ok(active) => {
                    self.active = Some(active);
                    return Ok(());
                }
                Err(error) => failures.push(format!("{error:#}")),
            }
        }
        if failures.is_empty() {
            bail!("EGL offers no GPU device");
        }
        bail!("{}", failures.join("; "))
    }
}

/// A GL context on one display, current only while it reads a frame.
struct Active {
    context: usize,
    texture: u32,
    framebuffer: u32,
    /// `glReadPixels` writes blue-first bytes itself (`GL_EXT_read_format_bgra`).
    bgra: bool,
    /// Dropped (terminated) after the context, by field order.
    display: Display,
}

// The context is made current only on the thread that reads, for the read, then released.
unsafe impl Send for Active {}

/// Makes a context current until dropped.
struct Current<'a>(&'a Display);

impl<'a> Current<'a> {
    fn new(display: &'a Display, context: Handle) -> Result<Self> {
        let egl = &display.lib.egl;
        unsafe { (egl.eglBindAPI)(EGL_OPENGL_ES_API) };
        let null = ptr::null_mut();
        if unsafe { (egl.eglMakeCurrent)(display.handle(), null, null, context) } == 0 {
            bail!("EGL could not make its context current ({:#x})", unsafe {
                (egl.eglGetError)()
            });
        }
        Ok(Self(display))
    }
}

impl Drop for Current<'_> {
    fn drop(&mut self) {
        let null = ptr::null_mut();
        unsafe { (self.0.lib.egl.eglMakeCurrent)(self.0.handle(), null, null, null) };
    }
}

impl Active {
    fn new(display: Display) -> Result<Self> {
        let egl = &display.lib.egl;
        unsafe { (egl.eglBindAPI)(EGL_OPENGL_ES_API) };
        let attributes = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
        let context = unsafe {
            (egl.eglCreateContext)(
                display.handle(),
                ptr::null_mut(),
                ptr::null_mut(),
                attributes.as_ptr(),
            )
        };
        if context.is_null() {
            bail!("EGL could not make a GL context ({:#x})", unsafe {
                (egl.eglGetError)()
            });
        }
        let mut active = Self {
            context: context as usize,
            texture: 0,
            framebuffer: 0,
            bgra: false,
            display,
        };
        let gl = &active.display.lib.gles;
        let current = Current::new(&active.display, context)?;
        let (mut texture, mut framebuffer) = (0, 0);
        unsafe {
            (gl.glGenTextures)(1, &mut texture);
            (gl.glBindTexture)(GL_TEXTURE_2D, texture);
            (gl.glTexParameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (gl.glTexParameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (gl.glBindTexture)(GL_TEXTURE_2D, 0);
            (gl.glGenFramebuffers)(1, &mut framebuffer);
        }
        let extensions = unsafe { text((gl.glGetString)(GL_EXTENSIONS).cast()) };
        drop(current);
        active.texture = texture;
        active.framebuffer = framebuffer;
        active.bgra = has_extension(&extensions, "GL_EXT_read_format_bgra");
        Ok(active)
    }

    fn read(&mut self, frame: &Frame, format: &VideoFormat, out: &mut [u8]) -> Result<()> {
        let (width, height) = (format.width as usize, format.height as usize);
        let display = &self.display;
        let (egl, gl, ext) = (&display.lib.egl, &display.lib.gles, &display.lib.ext);
        let _current = Current::new(display, self.context as Handle)?;
        let attributes = [
            EGL_WIDTH,
            format.width as i32,
            EGL_HEIGHT,
            format.height as i32,
            EGL_LINUX_DRM_FOURCC_EXT,
            format.pixel.drm_fourcc() as i32,
            EGL_DMA_BUF_PLANE0_FD_EXT,
            frame.fd as i32,
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            frame.offset as i32,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            frame.stride as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            format.modifier as u32 as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            (format.modifier >> 32) as u32 as i32,
            EGL_NONE,
        ];
        let image = unsafe {
            (ext.eglCreateImageKHR)(
                display.handle(),
                ptr::null_mut(),
                EGL_LINUX_DMA_BUF_EXT,
                ptr::null_mut(),
                attributes.as_ptr(),
            )
        };
        if image.is_null() {
            bail!(
                "EGL could not import the screen's GPU buffer ({:#x})",
                unsafe { (egl.eglGetError)() }
            );
        }
        let read = unsafe {
            (gl.glBindTexture)(GL_TEXTURE_2D, self.texture);
            (ext.glEGLImageTargetTexture2DOES)(GL_TEXTURE_2D, image);
            (gl.glBindFramebuffer)(GL_FRAMEBUFFER, self.framebuffer);
            (gl.glFramebufferTexture2D)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.texture,
                0,
            );
            let status = (gl.glCheckFramebufferStatus)(GL_FRAMEBUFFER);
            let read = if status == GL_FRAMEBUFFER_COMPLETE {
                let layout = if self.bgra && format.pixel.bgr() {
                    GL_BGRA_EXT
                } else {
                    GL_RGBA
                };
                (gl.glPixelStorei)(GL_PACK_ALIGNMENT, 4);
                (gl.glReadPixels)(
                    0,
                    0,
                    format.width as i32,
                    format.height as i32,
                    layout,
                    GL_UNSIGNED_BYTE,
                    out.as_mut_ptr().cast(),
                );
                match (gl.glGetError)() {
                    GL_NO_ERROR => Ok(layout),
                    error => Err(anyhow!("GL could not read the screen back ({error:#x})")),
                }
            } else {
                Err(anyhow!(
                    "GL can't read the screen's GPU buffer ({status:#x})"
                ))
            };
            (gl.glBindFramebuffer)(GL_FRAMEBUFFER, 0);
            (gl.glBindTexture)(GL_TEXTURE_2D, 0);
            (ext.eglDestroyImageKHR)(display.handle(), image);
            read
        }?;
        if read == GL_RGBA && format.pixel.bgr() {
            swap_red_blue(&mut out[..width * height * 4]);
        }
        Ok(())
    }
}

/// RGBA bytes to BGRA, in place.
fn swap_red_blue(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        let display = &self.display;
        let gl = &display.lib.gles;
        if let Ok(_current) = Current::new(display, self.context as Handle) {
            unsafe {
                (gl.glDeleteFramebuffers)(1, &self.framebuffer);
                (gl.glDeleteTextures)(1, &self.texture);
            }
        }
        unsafe { (display.lib.egl.eglDestroyContext)(display.handle(), self.context as Handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_names_match_whole_words() {
        let list = "EGL_KHR_no_config_context EGL_EXT_image_dma_buf_import_modifiers";
        assert!(has_extension(list, "EGL_KHR_no_config_context"));
        assert!(!has_extension(list, "EGL_EXT_image_dma_buf_import"));
    }

    #[test]
    fn only_renderable_tiled_layouts_are_offered() {
        // NVIDIA block-linear layouts; the second is external-only, so GL can't read it back.
        let tiled = [0x0300_0000_0060_6014, 0x0300_0000_0060_6015];
        let modifiers = [pipewire::MODIFIER_LINEAR, tiled[0], tiled[1], tiled[0]];
        assert_eq!(renderable(&modifiers, &[0, 0, 1, 0], 4), [tiled[0]]);
        assert_eq!(renderable(&modifiers, &[0, 0, 0, 0], 2), [tiled[0]]);
    }

    #[test]
    fn red_and_blue_trade_places() {
        let mut pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        swap_red_blue(&mut pixels);
        assert_eq!(pixels, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
