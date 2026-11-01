//! Hardware encoding through VA-API, loaded at run time like PipeWire: each screen frame (a
//! DMA-BUF) is imported as a surface, the video engine scales and converts it to NV12, and
//! encodes it as H.264 (an IDR every [`GOP`] frames, P frames between). The CPU only moves the
//! encoded bytes and, every so often, a small RGBA copy for the candidate frames.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::ptr;
use std::sync::OnceLock;

use anyhow::{Result, anyhow, bail};

use super::pipewire::{Frame, PixelFormat, library};

type Display = *mut c_void;
type Status = c_int;
type Id = u32;

const INVALID: Id = 0xffff_ffff;
const PROFILE_H264_HIGH: c_int = 7;
const PROFILE_NONE: c_int = -1;
const ENTRYPOINT_ENC_SLICE: c_int = 6;
const ENTRYPOINT_ENC_SLICE_LP: c_int = 8;
const ENTRYPOINT_VIDEO_PROC: c_int = 10;
const ATTRIB_RATE_CONTROL: c_int = 5;
const ATTRIB_RT_FORMAT: c_int = 0;
const ATTRIB_PACKED_HEADERS: c_int = 10;
/// Hex writes the sequence, picture and slice headers; the driver writes the slice data.
const PACKED_HEADERS: u32 = 0x1 | 0x2 | 0x4;
const BUFFER_PACKED_PARAMS: c_int = 25;
const BUFFER_PACKED_DATA: c_int = 26;
const PACKED_SEQUENCE: u32 = 1;
const PACKED_PICTURE: u32 = 2;
const PACKED_SLICE: u32 = 3;
const RC_CQP: u32 = 0x10;
const RT_FORMAT_YUV420: u32 = 0x1;
const RT_FORMAT_RGB32: u32 = 0x20000;
const SURFACE_ATTRIB_MEMORY_TYPE: c_int = 6;
const SURFACE_ATTRIB_EXTERNAL_DESCRIPTOR: c_int = 7;
const SURFACE_ATTRIB_SETTABLE: u32 = 2;
const MEM_TYPE_DRM_PRIME_2: i32 = 0x4000_0000;
const VALUE_INTEGER: c_int = 1;
const VALUE_POINTER: c_int = 3;
const BUFFER_CODED: c_int = 21;
const BUFFER_SEQUENCE: c_int = 22;
const BUFFER_PICTURE: c_int = 23;
const BUFFER_SLICE: c_int = 24;
const BUFFER_PROC_PIPELINE: c_int = 41;
const PROGRESSIVE: c_int = 1;
const COLOR_BT709: c_int = 2;
const COLOR_SRGB: c_int = 8;
const PICTURE_INVALID: u32 = 0x1;
const PICTURE_SHORT_TERM: u32 = 0x8;
const FOURCC_NV12: u32 = u32::from_le_bytes(*b"NV12");

/// Frames from one IDR to the next: two seconds at the recorder's rate.
pub const GOP: u32 = 60;
/// Constant quantiser: sharp text at a few Mbit/s for a mostly still screen.
const QP: u8 = 24;
/// `log2_max_frame_num_minus4` and `log2_max_pic_order_cnt_lsb_minus4`: 256 each, more
/// than a GOP's frames and picture order counts.
const LOG2_MAX_MINUS4: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct Picture {
    picture_id: Id,
    frame_idx: u32,
    flags: u32,
    top_field_order_cnt: i32,
    bottom_field_order_cnt: i32,
    reserved: [u32; 4],
}

const NO_PICTURE: Picture = Picture {
    picture_id: INVALID,
    frame_idx: 0,
    flags: PICTURE_INVALID,
    top_field_order_cnt: 0,
    bottom_field_order_cnt: 0,
    reserved: [0; 4],
};

#[repr(C)]
struct SequenceParams {
    seq_parameter_set_id: u8,
    level_idc: u8,
    intra_period: u32,
    intra_idr_period: u32,
    ip_period: u32,
    bits_per_second: u32,
    max_num_ref_frames: u32,
    picture_width_in_mbs: u16,
    picture_height_in_mbs: u16,
    seq_fields: u32,
    bit_depth_luma_minus8: u8,
    bit_depth_chroma_minus8: u8,
    num_ref_frames_in_pic_order_cnt_cycle: u8,
    offset_for_non_ref_pic: i32,
    offset_for_top_to_bottom_field: i32,
    offset_for_ref_frame: [i32; 256],
    frame_cropping_flag: u8,
    frame_crop: [u32; 4],
    vui_parameters_present_flag: u8,
    vui_fields: u32,
    aspect_ratio_idc: u8,
    sar_width: u32,
    sar_height: u32,
    num_units_in_tick: u32,
    time_scale: u32,
    reserved: [u32; 4],
}

#[repr(C)]
struct PictureParams {
    curr_pic: Picture,
    reference_frames: [Picture; 16],
    coded_buf: Id,
    pic_parameter_set_id: u8,
    seq_parameter_set_id: u8,
    last_picture: u8,
    frame_num: u16,
    pic_init_qp: u8,
    num_ref_idx_l0_active_minus1: u8,
    num_ref_idx_l1_active_minus1: u8,
    chroma_qp_index_offset: i8,
    second_chroma_qp_index_offset: i8,
    pic_fields: u32,
    reserved: [u32; 4],
}

#[repr(C)]
struct SliceParams {
    macroblock_address: u32,
    num_macroblocks: u32,
    macroblock_info: Id,
    slice_type: u8,
    pic_parameter_set_id: u8,
    idr_pic_id: u16,
    pic_order_cnt_lsb: u16,
    delta_pic_order_cnt_bottom: i32,
    delta_pic_order_cnt: [i32; 2],
    direct_spatial_mv_pred_flag: u8,
    num_ref_idx_active_override_flag: u8,
    num_ref_idx_l0_active_minus1: u8,
    num_ref_idx_l1_active_minus1: u8,
    ref_pic_list0: [Picture; 32],
    ref_pic_list1: [Picture; 32],
    luma_log2_weight_denom: u8,
    chroma_log2_weight_denom: u8,
    luma_weight_l0_flag: u8,
    luma_weight_l0: [i16; 32],
    luma_offset_l0: [i16; 32],
    chroma_weight_l0_flag: u8,
    chroma_weight_l0: [[i16; 2]; 32],
    chroma_offset_l0: [[i16; 2]; 32],
    luma_weight_l1_flag: u8,
    luma_weight_l1: [i16; 32],
    luma_offset_l1: [i16; 32],
    chroma_weight_l1_flag: u8,
    chroma_weight_l1: [[i16; 2]; 32],
    chroma_offset_l1: [[i16; 2]; 32],
    cabac_init_idc: u8,
    slice_qp_delta: i8,
    disable_deblocking_filter_idc: u8,
    slice_alpha_c0_offset_div2: i8,
    slice_beta_offset_div2: i8,
    reserved: [u32; 4],
}

#[repr(C)]
struct ProcPipeline {
    surface: Id,
    surface_region: *const c_void,
    surface_color_standard: c_int,
    output_region: *const c_void,
    output_background_color: u32,
    output_color_standard: c_int,
    pipeline_flags: u32,
    filter_flags: u32,
    filters: *mut Id,
    num_filters: u32,
    forward_references: *mut Id,
    num_forward_references: u32,
    backward_references: *mut Id,
    num_backward_references: u32,
    rotation_state: u32,
    blend_state: *const c_void,
    mirror_state: u32,
    additional_outputs: *mut Id,
    num_additional_outputs: u32,
    input_surface_flag: u32,
    output_surface_flag: u32,
    input_color_properties: [u8; 8],
    output_color_properties: [u8; 8],
    processing_mode: c_int,
    output_hdr_metadata: *mut c_void,
    reserved: [u32; 16],
}

#[repr(C)]
struct PackedParams {
    kind: u32,
    bit_length: u32,
    has_emulation_bytes: u8,
    reserved: [u32; 4],
}

#[repr(C)]
struct Rectangle {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

#[repr(C)]
struct PrimeObject {
    fd: c_int,
    size: u32,
    drm_format_modifier: u64,
}

#[repr(C)]
struct PrimeLayer {
    drm_format: u32,
    num_planes: u32,
    object_index: [u32; 4],
    offset: [u32; 4],
    pitch: [u32; 4],
}

#[repr(C)]
struct PrimeDescriptor {
    fourcc: u32,
    width: u32,
    height: u32,
    num_objects: u32,
    objects: [PrimeObject; 4],
    num_layers: u32,
    layers: [PrimeLayer; 4],
}

#[repr(C)]
struct SurfaceAttrib {
    kind: c_int,
    flags: u32,
    value_type: c_int,
    value: usize,
}

#[repr(C)]
struct ConfigAttrib {
    kind: c_int,
    value: u32,
}

#[repr(C)]
struct ImageFormat {
    fourcc: u32,
    byte_order: u32,
    bits_per_pixel: u32,
    depth: u32,
    masks: [u32; 4],
    reserved: [u32; 4],
}

#[repr(C)]
struct Image {
    image_id: Id,
    format: ImageFormat,
    buf: Id,
    width: u16,
    height: u16,
    data_size: u32,
    num_planes: u32,
    pitches: [u32; 3],
    offsets: [u32; 3],
    num_palette_entries: i32,
    entry_bytes: i32,
    component_order: [i8; 4],
    reserved: [u32; 4],
}

#[repr(C)]
struct CodedSegment {
    size: u32,
    bit_offset: u32,
    status: u32,
    reserved: u32,
    buf: *mut c_void,
    next: *mut CodedSegment,
    va_reserved: [u32; 4],
}

// The layouts libva 1.x defines (checked against its headers on x86-64).
const _: () = {
    assert!(size_of::<Picture>() == 36);
    assert!(size_of::<SequenceParams>() == 1132);
    assert!(size_of::<PictureParams>() == 648);
    assert!(size_of::<SliceParams>() == 3140);
    assert!(size_of::<ProcPipeline>() == 224);
    assert!(size_of::<PrimeDescriptor>() == 312);
    assert!(size_of::<SurfaceAttrib>() == 24);
    assert!(size_of::<Image>() == 120);
    assert!(size_of::<CodedSegment>() == 48);
    assert!(size_of::<PackedParams>() == 28);
};

library!(Va {
    vaInitialize: fn(Display, *mut c_int, *mut c_int) -> Status;
    vaTerminate: fn(Display) -> Status;
    vaErrorStr: fn(Status) -> *const c_char;
    vaCreateConfig: fn(Display, c_int, c_int, *mut ConfigAttrib, c_int, *mut Id) -> Status;
    vaDestroyConfig: fn(Display, Id) -> Status;
    vaCreateContext: fn(Display, Id, c_int, c_int, c_int, *mut Id, c_int, *mut Id) -> Status;
    vaDestroyContext: fn(Display, Id) -> Status;
    vaCreateSurfaces: fn(Display, u32, u32, u32, *mut Id, u32, *mut SurfaceAttrib, u32) -> Status;
    vaDestroySurfaces: fn(Display, *mut Id, c_int) -> Status;
    vaCreateBuffer: fn(Display, Id, c_int, u32, u32, *mut c_void, *mut Id) -> Status;
    vaDestroyBuffer: fn(Display, Id) -> Status;
    vaMapBuffer: fn(Display, Id, *mut *mut c_void) -> Status;
    vaUnmapBuffer: fn(Display, Id) -> Status;
    vaBeginPicture: fn(Display, Id, Id) -> Status;
    vaRenderPicture: fn(Display, Id, *mut Id, c_int) -> Status;
    vaEndPicture: fn(Display, Id) -> Status;
    vaSyncSurface: fn(Display, Id) -> Status;
    vaCreateImage: fn(Display, *mut ImageFormat, c_int, c_int, *mut Image) -> Status;
    vaDestroyImage: fn(Display, Id) -> Status;
    vaGetImage: fn(Display, Id, c_int, c_int, u32, u32, Id) -> Status;
});

library!(VaDrm {
    vaGetDisplayDRM: fn(c_int) -> Display;
});

fn libraries() -> Result<(&'static Va, &'static VaDrm)> {
    static LIBS: OnceLock<std::result::Result<(Va, VaDrm), String>> = OnceLock::new();
    LIBS.get_or_init(|| {
        let load = || -> Result<(Va, VaDrm)> {
            let va = unsafe { Va::load(super::pipewire::open("libva.so.2")?)? };
            let drm = unsafe { VaDrm::load(super::pipewire::open("libva-drm.so.2")?)? };
            Ok((va, drm))
        };
        load().map_err(|e| format!("{e:#}"))
    })
    .as_ref()
    .map(|(va, drm)| (va, drm))
    .map_err(|e| anyhow!("{e}"))
}

/// The GPU render nodes (`/dev/dri/renderD128`, …), lowest number first.
fn render_nodes() -> Vec<std::path::PathBuf> {
    let names = std::fs::read_dir("/dev/dri")
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    sorted_render_nodes(names)
        .into_iter()
        .map(|name| std::path::Path::new("/dev/dri").join(name))
        .collect()
}

fn sorted_render_nodes(names: Vec<String>) -> Vec<String> {
    let mut nodes: Vec<(u32, String)> = names
        .into_iter()
        .filter_map(|name| Some((name.strip_prefix("renderD")?.parse().ok()?, name)))
        .collect();
    nodes.sort();
    nodes.into_iter().map(|(_, name)| name).collect()
}

fn check(va: &Va, status: Status, what: &str) -> Result<()> {
    if status == 0 {
        return Ok(());
    }
    let why = unsafe { std::ffi::CStr::from_ptr((va.vaErrorStr)(status)) };
    bail!("VA-API {what}: {}", why.to_string_lossy())
}

/// The H.264 encoder and the video engine's scaler, on one render node.
pub struct Encoder {
    va: &'static Va,
    display: Display,
    _node: File,
    vpp_config: Id,
    vpp: Id,
    enc_config: Id,
    enc: Id,
    /// NV12 at the output size: the frame to encode, then two reconstructions taking turns.
    input: Id,
    recon: [Id; 2],
    coded: Id,
    /// The small RGBX copy for candidate frames, and its image.
    small: Option<(Id, Image, Id)>,
    /// Imported screen buffers, by their fd.
    imported: HashMap<i64, Id>,
    width: u32,
    height: u32,
    pixel: PixelFormat,
    source: (u32, u32),
    frame: u32,
    idr_count: u16,
}

unsafe impl Send for Encoder {}

impl Encoder {
    /// Opens the first render node that encodes H.264, scaling `source` frames to `width` ×
    /// `height` (multiples of 16), with an optional small copy at `small`. Every render node is
    /// tried in order: on a laptop with two GPUs the first may be one the VA driver can't use.
    pub fn new(
        source: (u32, u32),
        pixel: PixelFormat,
        output: (u32, u32),
        small: (u32, u32),
    ) -> Result<Self> {
        let (va, drm) = libraries()?;
        let nodes = render_nodes();
        if nodes.is_empty() {
            bail!("no GPU render node in /dev/dri");
        }
        let mut failures = Vec::new();
        for node in nodes {
            match Self::open(va, drm, &node, source, pixel, output, small) {
                Ok(encoder) => {
                    if !failures.is_empty() {
                        crate::log::scrawl_log(format!(
                            "Hardware encoding on {} ({})",
                            node.display(),
                            failures.join("; ")
                        ));
                    }
                    return Ok(encoder);
                }
                Err(error) => failures.push(format!("{}: {error:#}", node.display())),
            }
        }
        bail!("{}", failures.join("; "))
    }

    fn open(
        va: &'static Va,
        drm: &VaDrm,
        path: &std::path::Path,
        source: (u32, u32),
        pixel: PixelFormat,
        (width, height): (u32, u32),
        small: (u32, u32),
    ) -> Result<Self> {
        let node = File::options()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| anyhow!("could not open the render node: {e}"))?;
        let display = unsafe { (drm.vaGetDisplayDRM)(node.as_raw_fd()) };
        if display.is_null() {
            bail!("VA-API has no display for the render node");
        }
        let (mut major, mut minor) = (0, 0);
        let initialized = check(
            va,
            unsafe { (va.vaInitialize)(display, &mut major, &mut minor) },
            "initialize",
        );
        if let Err(error) = initialized {
            unsafe { (va.vaTerminate)(display) };
            return Err(error);
        }
        let mut encoder = Self {
            va,
            display,
            _node: node,
            vpp_config: INVALID,
            vpp: INVALID,
            enc_config: INVALID,
            enc: INVALID,
            input: INVALID,
            recon: [INVALID; 2],
            coded: INVALID,
            small: None,
            imported: HashMap::new(),
            width,
            height,
            pixel,
            source,
            frame: 0,
            idr_count: 0,
        };
        encoder.setup(small)?;
        Ok(encoder)
    }

    fn setup(&mut self, small: (u32, u32)) -> Result<()> {
        let (va, display) = (self.va, self.display);
        let mut surfaces = [INVALID; 3];
        check(
            va,
            unsafe {
                (va.vaCreateSurfaces)(
                    display,
                    RT_FORMAT_YUV420,
                    self.width,
                    self.height,
                    surfaces.as_mut_ptr(),
                    3,
                    ptr::null_mut(),
                    0,
                )
            },
            "create NV12 surfaces",
        )?;
        self.input = surfaces[0];
        self.recon = [surfaces[1], surfaces[2]];

        let mut rc = [
            ConfigAttrib {
                kind: ATTRIB_RT_FORMAT,
                value: RT_FORMAT_YUV420,
            },
            ConfigAttrib {
                kind: ATTRIB_RATE_CONTROL,
                value: RC_CQP,
            },
            ConfigAttrib {
                kind: ATTRIB_PACKED_HEADERS,
                value: PACKED_HEADERS,
            },
        ];
        let made = [
            (PROFILE_H264_HIGH, ENTRYPOINT_ENC_SLICE),
            (PROFILE_H264_HIGH, ENTRYPOINT_ENC_SLICE_LP),
        ]
        .into_iter()
        .find(|&(profile, entrypoint)| unsafe {
            (va.vaCreateConfig)(
                display,
                profile,
                entrypoint,
                rc.as_mut_ptr(),
                rc.len() as c_int,
                &mut self.enc_config,
            ) == 0
        });
        if made.is_none() {
            bail!("the GPU has no H.264 High encoder");
        }
        check(
            va,
            unsafe {
                (va.vaCreateContext)(
                    display,
                    self.enc_config,
                    self.width as c_int,
                    self.height as c_int,
                    PROGRESSIVE,
                    surfaces.as_mut_ptr(),
                    3,
                    &mut self.enc,
                )
            },
            "create the encoder",
        )?;
        check(
            va,
            unsafe {
                (va.vaCreateConfig)(
                    display,
                    PROFILE_NONE,
                    ENTRYPOINT_VIDEO_PROC,
                    ptr::null_mut(),
                    0,
                    &mut self.vpp_config,
                )
            },
            "find the video processor",
        )?;
        check(
            va,
            unsafe {
                (va.vaCreateContext)(
                    display,
                    self.vpp_config,
                    self.width as c_int,
                    self.height as c_int,
                    PROGRESSIVE,
                    ptr::null_mut(),
                    0,
                    &mut self.vpp,
                )
            },
            "create the video processor",
        )?;
        check(
            va,
            unsafe {
                (va.vaCreateBuffer)(
                    display,
                    self.enc,
                    BUFFER_CODED,
                    self.width * self.height * 3 / 2 + (1 << 16),
                    1,
                    ptr::null_mut(),
                    &mut self.coded,
                )
            },
            "create the output buffer",
        )?;
        self.small = self.small_surface(small).ok();
        Ok(())
    }

    fn small_surface(&self, (width, height): (u32, u32)) -> Result<(Id, Image, Id)> {
        let (va, display) = (self.va, self.display);
        let mut surface = INVALID;
        check(
            va,
            unsafe {
                (va.vaCreateSurfaces)(
                    display,
                    RT_FORMAT_YUV420,
                    width,
                    height,
                    &mut surface,
                    1,
                    ptr::null_mut(),
                    0,
                )
            },
            "create the small surface",
        )?;
        let mut format: ImageFormat = unsafe { std::mem::zeroed() };
        format.fourcc = FOURCC_NV12;
        format.bits_per_pixel = 12;
        let mut image: Image = unsafe { std::mem::zeroed() };
        check(
            va,
            unsafe {
                (va.vaCreateImage)(
                    display,
                    &mut format,
                    width as c_int,
                    height as c_int,
                    &mut image,
                )
            },
            "create the small image",
        )?;
        // A processing context of its own: one is made for its output size.
        let mut context = INVALID;
        check(
            va,
            unsafe {
                (va.vaCreateContext)(
                    display,
                    self.vpp_config,
                    width as c_int,
                    height as c_int,
                    PROGRESSIVE,
                    &mut surface,
                    1,
                    &mut context,
                )
            },
            "create the small processor",
        )?;
        Ok((surface, image, context))
    }

    /// The surface for a screen buffer, imported on first sight.
    fn import(&mut self, frame: &Frame) -> Result<Id> {
        if let Some(&surface) = self.imported.get(&frame.fd) {
            return Ok(surface);
        }
        let (width, height) = self.source;
        let mut descriptor: PrimeDescriptor = unsafe { std::mem::zeroed() };
        descriptor.fourcc = match self.pixel {
            PixelFormat::Bgrx => u32::from_le_bytes(*b"BGRX"),
            PixelFormat::Rgbx => u32::from_le_bytes(*b"RGBX"),
            PixelFormat::Bgra => u32::from_le_bytes(*b"BGRA"),
            PixelFormat::Rgba => u32::from_le_bytes(*b"RGBA"),
        };
        descriptor.width = width;
        descriptor.height = height;
        descriptor.num_objects = 1;
        descriptor.objects[0] = PrimeObject {
            fd: frame.fd as c_int,
            size: frame.size,
            drm_format_modifier: 0,
        };
        descriptor.num_layers = 1;
        descriptor.layers[0].drm_format = self.pixel.drm_fourcc();
        descriptor.layers[0].num_planes = 1;
        descriptor.layers[0].offset[0] = frame.offset;
        descriptor.layers[0].pitch[0] = frame.stride;
        let mut attribs = [
            SurfaceAttrib {
                kind: SURFACE_ATTRIB_MEMORY_TYPE,
                flags: SURFACE_ATTRIB_SETTABLE,
                value_type: VALUE_INTEGER,
                value: MEM_TYPE_DRM_PRIME_2 as u32 as usize,
            },
            SurfaceAttrib {
                kind: SURFACE_ATTRIB_EXTERNAL_DESCRIPTOR,
                flags: SURFACE_ATTRIB_SETTABLE,
                value_type: VALUE_POINTER,
                value: (&raw mut descriptor) as usize,
            },
        ];
        let mut surface = INVALID;
        check(
            self.va,
            unsafe {
                (self.va.vaCreateSurfaces)(
                    self.display,
                    RT_FORMAT_RGB32,
                    width,
                    height,
                    &mut surface,
                    1,
                    attribs.as_mut_ptr(),
                    2,
                )
            },
            "import the screen buffer",
        )?;
        self.imported.insert(frame.fd, surface);
        Ok(surface)
    }

    fn buffer<T>(&self, context: Id, kind: c_int, value: &mut T) -> Result<Id> {
        let mut id = INVALID;
        check(
            self.va,
            unsafe {
                (self.va.vaCreateBuffer)(
                    self.display,
                    context,
                    kind,
                    size_of::<T>() as u32,
                    1,
                    (value as *mut T).cast(),
                    &mut id,
                )
            },
            "create a parameter buffer",
        )?;
        Ok(id)
    }

    /// Adds one header Hex wrote (start code, NAL header, payload) as a packed header.
    fn packed(
        &self,
        buffers: &mut Vec<Id>,
        kind: u32,
        (bytes, bits): &(Vec<u8>, u32),
    ) -> Result<()> {
        let mut params = PackedParams {
            kind,
            bit_length: *bits,
            has_emulation_bytes: 1,
            reserved: [0; 4],
        };
        buffers.push(self.buffer(self.enc, BUFFER_PACKED_PARAMS, &mut params)?);
        let mut id = INVALID;
        check(
            self.va,
            unsafe {
                (self.va.vaCreateBuffer)(
                    self.display,
                    self.enc,
                    BUFFER_PACKED_DATA,
                    bytes.len() as u32,
                    1,
                    bytes.as_ptr().cast_mut().cast(),
                    &mut id,
                )
            },
            "create a header buffer",
        )?;
        buffers.push(id);
        Ok(())
    }

    /// Runs the video processor from `source` (`from`) into `target` (`size`), scaling to fit.
    fn process(
        &self,
        context: Id,
        (source, from): (Id, (u32, u32)),
        (target, size): (Id, (u32, u32)),
        (input, output): (c_int, c_int),
    ) -> Result<()> {
        let rect = |(width, height): (u32, u32)| Rectangle {
            x: 0,
            y: 0,
            width: width as u16,
            height: height as u16,
        };
        let (from, to) = (rect(from), rect(size));
        let mut pipeline: ProcPipeline = unsafe { std::mem::zeroed() };
        pipeline.surface = source;
        pipeline.surface_region = (&raw const from).cast();
        pipeline.output_region = (&raw const to).cast();
        pipeline.surface_color_standard = input;
        pipeline.output_color_standard = output;
        pipeline.output_background_color = 0xff00_0000;
        let buffer = self.buffer(context, BUFFER_PROC_PIPELINE, &mut pipeline)?;
        let (va, display) = (self.va, self.display);
        let mut buffers = [buffer];
        let result = (|| {
            check(
                va,
                unsafe { (va.vaBeginPicture)(display, context, target) },
                "begin scaling",
            )?;
            check(
                va,
                unsafe { (va.vaRenderPicture)(display, context, buffers.as_mut_ptr(), 1) },
                "scale",
            )?;
            check(
                va,
                unsafe { (va.vaEndPicture)(display, context) },
                "end scaling",
            )?;
            check(
                va,
                unsafe { (va.vaSyncSurface)(display, target) },
                "wait for scaling",
            )
        })();
        unsafe { (va.vaDestroyBuffer)(display, buffer) };
        result
    }

    /// Imports `frame` and scales it into the frame to encode.
    pub fn load(&mut self, frame: &Frame) -> Result<()> {
        let source = self.import(frame)?;
        let output = (self.width, self.height);
        self.process(
            self.vpp,
            (source, self.source),
            (self.input, output),
            (COLOR_SRGB, COLOR_BT709),
        )?;
        if let Some((small, image, context)) = &self.small {
            // From the scaled frame: the video engine shrinks by a bounded factor. Only the
            // candidate frames read it; a failure there costs nothing else.
            let size = (u32::from(image.width), u32::from(image.height));
            let _ = self.process(
                *context,
                (self.input, output),
                (*small, size),
                (COLOR_BT709, COLOR_BT709),
            );
        }
        Ok(())
    }

    /// The last loaded frame at the small size, as tight RGBA rows.
    pub fn small_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (surface, image, _) = self.small.as_ref()?;
        let (va, display) = (self.va, self.display);
        let (width, height) = (u32::from(image.width), u32::from(image.height));
        if unsafe { (va.vaGetImage)(display, *surface, 0, 0, width, height, image.image_id) } != 0 {
            return None;
        }
        let mut mapped = ptr::null_mut();
        if unsafe { (va.vaMapBuffer)(display, image.buf, &mut mapped) } != 0 {
            return None;
        }
        let base = mapped.cast::<u8>().cast_const();
        let (luma, chroma) = (image.offsets[0] as usize, image.offsets[1] as usize);
        let (luma_pitch, chroma_pitch) = (image.pitches[0] as usize, image.pitches[1] as usize);
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height as usize {
            for x in 0..width as usize {
                // BT.709 limited range.
                let at = |offset: usize| f32::from(unsafe { *base.add(offset) });
                let l = (at(luma + y * luma_pitch + x) - 16.0) * 1.164;
                let c = chroma + (y / 2) * chroma_pitch + (x / 2) * 2;
                let (u, v) = (at(c) - 128.0, at(c + 1) - 128.0);
                let clamp = |value: f32| value.clamp(0.0, 255.0) as u8;
                rgba.extend_from_slice(&[
                    clamp(l + 1.793 * v),
                    clamp(l - 0.213 * u - 0.533 * v),
                    clamp(l + 2.112 * u),
                    255,
                ]);
            }
        }
        unsafe { (va.vaUnmapBuffer)(display, image.buf) };
        Some((width, height, rgba))
    }

    /// Encodes the loaded frame (again, when nothing new arrived) and writes its Annex B bytes.
    pub fn encode(&mut self, out: &mut impl Write) -> Result<()> {
        let index = self.frame % GOP;
        let idr = index == 0;
        if idr {
            self.idr_count = self.idr_count.wrapping_add(1);
        }
        let recon = self.recon[(self.frame % 2) as usize];
        let reference = self.recon[((self.frame + 1) % 2) as usize];
        let poc = (index * 2) as i32;
        let current = Picture {
            picture_id: recon,
            frame_idx: index,
            flags: 0,
            top_field_order_cnt: poc,
            bottom_field_order_cnt: 0,
            reserved: [0; 4],
        };
        let previous = Picture {
            picture_id: reference,
            frame_idx: index.saturating_sub(1),
            flags: PICTURE_SHORT_TERM,
            top_field_order_cnt: poc - 2,
            bottom_field_order_cnt: 0,
            reserved: [0; 4],
        };
        let (width_mbs, height_mbs) = ((self.width / 16) as u16, (self.height / 16) as u16);

        let mut buffers = Vec::with_capacity(9);
        if idr {
            let mut sequence: SequenceParams = unsafe { std::mem::zeroed() };
            sequence.level_idc = 51;
            sequence.intra_period = GOP;
            sequence.intra_idr_period = GOP;
            sequence.ip_period = 1;
            sequence.max_num_ref_frames = 1;
            sequence.picture_width_in_mbs = width_mbs;
            sequence.picture_height_in_mbs = height_mbs;
            // 4:2:0, frames only, direct 8×8 inference, and the frame number and picture
            // order count sizes; picture order count type 0.
            sequence.seq_fields =
                1 | (1 << 2) | (1 << 5) | (LOG2_MAX_MINUS4 << 6) | (LOG2_MAX_MINUS4 << 12);
            sequence.time_scale = 60;
            sequence.num_units_in_tick = 1;
            buffers.push(self.buffer(self.enc, BUFFER_SEQUENCE, &mut sequence)?);
        }
        let mut picture: PictureParams = unsafe { std::mem::zeroed() };
        picture.curr_pic = current;
        picture.reference_frames = [NO_PICTURE; 16];
        if !idr {
            picture.reference_frames[0] = previous;
        }
        picture.coded_buf = self.coded;
        picture.frame_num = index as u16;
        picture.pic_init_qp = QP;
        // IDR, used for reference, CABAC, 8×8 transforms, deblocking control.
        picture.pic_fields = u32::from(idr) | (1 << 1) | (1 << 3) | (1 << 8) | (1 << 9);
        buffers.push(self.buffer(self.enc, BUFFER_PICTURE, &mut picture)?);
        let mut slice: SliceParams = unsafe { std::mem::zeroed() };
        slice.num_macroblocks = u32::from(width_mbs) * u32::from(height_mbs);
        slice.macroblock_info = INVALID;
        slice.slice_type = if idr { 2 } else { 0 };
        slice.idr_pic_id = self.idr_count;
        slice.pic_order_cnt_lsb = poc as u16;
        slice.ref_pic_list0 = [NO_PICTURE; 32];
        slice.ref_pic_list1 = [NO_PICTURE; 32];
        if !idr {
            slice.ref_pic_list0[0] = previous;
        }
        if idr {
            let headers = Headers {
                width_mbs: u32::from(width_mbs),
                height_mbs: u32::from(height_mbs),
            };
            self.packed(&mut buffers, PACKED_SEQUENCE, &headers.sequence())?;
            self.packed(&mut buffers, PACKED_PICTURE, &headers.picture())?;
        }
        self.packed(
            &mut buffers,
            PACKED_SLICE,
            &slice_header(idr, index, self.idr_count, poc),
        )?;
        buffers.push(self.buffer(self.enc, BUFFER_SLICE, &mut slice)?);

        let (va, display) = (self.va, self.display);
        let rendered = (|| {
            check(
                va,
                unsafe { (va.vaBeginPicture)(display, self.enc, self.input) },
                "begin encoding",
            )?;
            check(
                va,
                unsafe {
                    (va.vaRenderPicture)(
                        display,
                        self.enc,
                        buffers.as_mut_ptr(),
                        buffers.len() as c_int,
                    )
                },
                "encode",
            )?;
            check(
                va,
                unsafe { (va.vaEndPicture)(display, self.enc) },
                "end encoding",
            )?;
            check(
                va,
                unsafe { (va.vaSyncSurface)(display, self.input) },
                "wait for encoding",
            )
        })();
        for buffer in buffers {
            unsafe { (va.vaDestroyBuffer)(display, buffer) };
        }
        rendered?;
        let mut segment = ptr::null_mut::<c_void>();
        check(
            va,
            unsafe { (va.vaMapBuffer)(display, self.coded, &mut segment) },
            "read the output",
        )?;
        let mut written = Ok(());
        let mut at = segment.cast::<CodedSegment>();
        while !at.is_null() && written.is_ok() {
            let piece = unsafe { &*at };
            let bytes =
                unsafe { std::slice::from_raw_parts(piece.buf.cast::<u8>(), piece.size as usize) };
            written = out.write_all(bytes);
            at = piece.next;
        }
        unsafe { (va.vaUnmapBuffer)(display, self.coded) };
        written?;
        self.frame += 1;
        Ok(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let (va, display) = (self.va, self.display);
        unsafe {
            for (_, mut surface) in self.imported.drain() {
                (va.vaDestroySurfaces)(display, &mut surface, 1);
            }
            if let Some((mut surface, image, context)) = self.small.take() {
                (va.vaDestroyContext)(display, context);
                (va.vaDestroyImage)(display, image.image_id);
                (va.vaDestroySurfaces)(display, &mut surface, 1);
            }
            if self.coded != INVALID {
                (va.vaDestroyBuffer)(display, self.coded);
            }
            for context in [self.enc, self.vpp] {
                if context != INVALID {
                    (va.vaDestroyContext)(display, context);
                }
            }
            for config in [self.enc_config, self.vpp_config] {
                if config != INVALID {
                    (va.vaDestroyConfig)(display, config);
                }
            }
            let mut surfaces = [self.input, self.recon[0], self.recon[1]];
            if self.input != INVALID {
                (va.vaDestroySurfaces)(display, surfaces.as_mut_ptr(), 3);
            }
            (va.vaTerminate)(display);
        }
    }
}

// ─── Headers ─────────────────────────────────────────────────────────────────

/// Writes H.264 syntax elements MSB first.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    bits: u32,
}

impl Bits {
    fn bit(&mut self, value: bool) {
        if self.bits.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if value {
            *self.bytes.last_mut().expect("pushed") |= 0x80 >> (self.bits % 8);
        }
        self.bits += 1;
    }

    fn u(&mut self, count: u32, value: u32) {
        for i in (0..count).rev() {
            self.bit((value >> i) & 1 == 1);
        }
    }

    /// Unsigned Exp-Golomb.
    fn ue(&mut self, value: u32) {
        let coded = value + 1;
        let length = 32 - coded.leading_zeros();
        self.u(length - 1, 0);
        self.u(length, coded);
    }

    /// Signed Exp-Golomb.
    fn se(&mut self, value: i32) {
        self.ue(if value > 0 {
            (value as u32) * 2 - 1
        } else {
            value.unsigned_abs() * 2
        });
    }

    fn trailing(&mut self) {
        self.bit(true);
        while !self.bits.is_multiple_of(8) {
            self.bit(false);
        }
    }

    /// A start code, the NAL header byte, and this payload with emulation prevention bytes,
    /// and the whole thing's length in bits (a slice header may end mid-byte).
    fn nal(self, header: u8) -> (Vec<u8>, u32) {
        let tail_bits = self.bits % 8;
        let mut out = vec![0, 0, 0, 1, header];
        let mut zeros = 0;
        for &byte in &self.bytes {
            if zeros >= 2 && byte <= 3 {
                out.push(3);
                zeros = 0;
            }
            zeros = if byte == 0 { zeros + 1 } else { 0 };
            out.push(byte);
        }
        let bits = out.len() as u32 * 8 - if tail_bits == 0 { 0 } else { 8 - tail_bits };
        (out, bits)
    }
}

struct Headers {
    width_mbs: u32,
    height_mbs: u32,
}

impl Headers {
    /// High profile, level 5.1, 4:2:0, frames only, one reference, timing for the muxer.
    fn sequence(&self) -> (Vec<u8>, u32) {
        let mut b = Bits::default();
        b.u(8, 100);
        b.u(8, 0);
        b.u(8, 51);
        b.ue(0);
        b.ue(1);
        b.ue(0);
        b.ue(0);
        b.bit(false);
        b.bit(false);
        b.ue(LOG2_MAX_MINUS4);
        b.ue(0);
        b.ue(LOG2_MAX_MINUS4);
        b.ue(1);
        b.bit(false);
        b.ue(self.width_mbs - 1);
        b.ue(self.height_mbs - 1);
        b.bit(true);
        b.bit(true);
        b.bit(false);
        // VUI: BT.709 limited range, and the frame rate.
        b.bit(true);
        b.bit(false);
        b.bit(false);
        b.bit(true);
        b.u(3, 5);
        b.bit(false);
        b.bit(true);
        b.u(8, 1);
        b.u(8, 1);
        b.u(8, 1);
        b.bit(false);
        b.bit(true);
        b.u(32, 1);
        b.u(32, 2 * super::media::FPS);
        b.bit(true);
        b.bit(false);
        b.bit(false);
        b.bit(false);
        b.bit(false);
        b.trailing();
        b.nal(0x67)
    }

    /// CABAC, a constant QP, deblocking control, 8×8 transforms.
    fn picture(&self) -> (Vec<u8>, u32) {
        let mut b = Bits::default();
        b.ue(0);
        b.ue(0);
        b.bit(true);
        b.bit(false);
        b.ue(0);
        b.ue(0);
        b.ue(0);
        b.bit(false);
        b.u(2, 0);
        b.se(i32::from(QP) - 26);
        b.se(0);
        b.se(0);
        b.bit(true);
        b.bit(false);
        b.bit(false);
        b.bit(true);
        b.bit(false);
        b.se(0);
        b.trailing();
        b.nal(0x68)
    }
}

/// The slice header of one whole-frame slice; the driver appends the slice data.
fn slice_header(idr: bool, frame_num: u32, idr_pic_id: u16, poc: i32) -> (Vec<u8>, u32) {
    let mut b = Bits::default();
    b.ue(0);
    b.ue(if idr { 2 } else { 0 });
    b.ue(0);
    b.u(LOG2_MAX_MINUS4 + 4, frame_num);
    if idr {
        b.ue(u32::from(idr_pic_id));
    }
    b.u(LOG2_MAX_MINUS4 + 4, poc as u32);
    if !idr {
        b.bit(false);
        b.bit(false);
    }
    if idr {
        b.bit(false);
        b.bit(false);
    } else {
        b.bit(false);
        b.ue(0);
    }
    b.se(0);
    b.ue(0);
    b.se(0);
    b.se(0);
    b.nal(if idr { 0x65 } else { 0x41 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_golomb_codes() {
        let mut b = Bits::default();
        b.ue(0);
        b.ue(1);
        b.ue(2);
        b.se(-1);
        // 1 010 011 011
        assert_eq!(b.bits, 10);
        assert_eq!(b.bytes, [0b1010_0110, 0b1100_0000]);
    }

    #[test]
    fn render_nodes_in_order() {
        let names = ["renderD129", "card0", "renderD128", "renderDx", "card1"]
            .map(String::from)
            .to_vec();
        assert_eq!(sorted_render_nodes(names), ["renderD128", "renderD129"]);
    }

    #[test]
    fn emulation_prevention() {
        let mut b = Bits::default();
        b.u(24, 0x000001);
        let (bytes, bits) = b.nal(0x67);
        assert_eq!(bytes, [0, 0, 0, 1, 0x67, 0, 0, 3, 1]);
        assert_eq!(bits, 72);
    }
}
