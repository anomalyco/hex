//! Dot scope's Metal pass (experiment `dot-scope`): steps [`Scope`] and draws
//! it with `dot_scope_fragment`. The HUD's renderer owns one when the
//! experiment is on and draws it instead of the capsule.

use std::ffi::c_void;

use metal::{
    DeviceRef, LibraryRef, MTLPixelFormat, MTLPrimitiveType, RenderCommandEncoderRef,
    RenderPipelineDescriptor, RenderPipelineState,
};

use crate::dot_scope::{Frame, MAX_DOTS, Scope, Stage, Tuning, palette};

#[repr(C)]
#[derive(Clone, Copy)]
struct DotUniforms {
    resolution: [f32; 2],
    backing_scale: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    pitch: f32,
    columns: f32,
    rows: f32,
    glow: f32,
    grid: f32,
    live: f32,
    cool: f32,
    dark: f32,
    base: [f32; 4],
    core: [f32; 4],
    plate: [f32; 4],
    rim: [f32; 4],
    shadow: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<DotUniforms>() == 144);

pub(super) struct DotScopeView {
    pipeline: RenderPipelineState,
    scope: Scope,
    stage: Stage,
    dark: bool,
    dots: Vec<[u16; 2]>,
}

impl DotScopeView {
    pub(super) fn new(
        device: &DeviceRef,
        library: &LibraryRef,
        tuning: Tuning,
    ) -> Result<Self, String> {
        let vertex = library
            .get_function("indicator_vertex", None)
            .map_err(|error| format!("could not load indicator vertex shader: {error}"))?;
        let fragment = library
            .get_function("dot_scope_fragment", None)
            .map_err(|error| format!("could not load Dot scope shader: {error}"))?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("Dot scope pipeline has no color attachment")?
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = device
            .new_render_pipeline_state(&descriptor)
            .map_err(|error| format!("could not create Dot scope pipeline: {error}"))?;
        Ok(Self {
            pipeline,
            scope: Scope::new(tuning),
            stage: Stage::Recording,
            dark: true,
            dots: Vec::with_capacity(MAX_DOTS),
        })
    }

    pub(super) fn restart(&mut self, reduce_motion: bool) {
        self.stage = Stage::Recording;
        self.scope.restart(reduce_motion);
    }

    pub(super) fn tune(&mut self, tuning: Tuning) {
        self.scope.tune(tuning);
    }

    pub(super) fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
    }

    /// Advances the scope. `stage` is `None` while the HUD hides or a
    /// capture was cancelled: the last stage fades out.
    pub(super) fn advance(
        &mut self,
        dt: f32,
        stage: Option<Stage>,
        visible: bool,
        level: f32,
        reduce_motion: bool,
    ) {
        if let Some(stage) = stage {
            self.stage = stage;
        }
        self.scope.advance(
            dt,
            Frame {
                stage: self.stage,
                visible,
                level,
                reduce_motion,
            },
        );
    }

    pub(super) fn settled(&self) -> bool {
        self.scope.settled()
    }

    pub(super) fn encode(
        &mut self,
        encoder: &RenderCommandEncoderRef,
        resolution: [f32; 2],
        backing_scale: f32,
    ) {
        let pose = self.scope.pose();
        let tuning = self.scope.tuning();
        let colors = palette(self.dark, &pose, tuning.saturation);
        let rgba = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        let uniforms = DotUniforms {
            resolution,
            backing_scale,
            width: pose.width,
            height: pose.height,
            opacity: pose.opacity,
            scale: pose.scale,
            softness: pose.softness,
            pitch: tuning.spacing,
            columns: self.scope.columns() as f32,
            rows: self.scope.rows() as f32,
            glow: pose.glow,
            grid: pose.grid,
            live: pose.live,
            cool: pose.cool,
            dark: if self.dark { 1.0 } else { 0.0 },
            base: rgba(colors.base),
            core: rgba(colors.core),
            plate: colors.plate,
            rim: colors.rim,
            shadow: colors.shadow,
        };
        self.dots.clear();
        self.dots.extend(
            self.scope
                .dots()
                .iter()
                .map(|value| value.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)),
        );
        encoder.set_render_pipeline_state(&self.pipeline);
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of::<DotUniforms>() as u64,
            (&raw const uniforms).cast::<c_void>(),
        );
        encoder.set_fragment_bytes(
            1,
            std::mem::size_of_val(self.dots.as_slice()) as u64,
            self.dots.as_ptr().cast::<c_void>(),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
    }
}

/// Whether macOS is in dark mode, from the global `AppleInterfaceStyle`
/// default. Main thread only.
pub(super) fn system_is_dark() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    NSUserDefaults::standardUserDefaults()
        .stringForKey(&NSString::from_str("AppleInterfaceStyle"))
        .is_some_and(|style| style.to_string().eq_ignore_ascii_case("dark"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_matches_metal() {
        assert_eq!(std::mem::offset_of!(DotUniforms, dark), 60);
        assert_eq!(std::mem::offset_of!(DotUniforms, base), 64);
        assert_eq!(std::mem::offset_of!(DotUniforms, shadow), 128);
    }

    #[test]
    fn the_largest_field_fits_one_set_bytes_call() {
        assert!(MAX_DOTS * std::mem::size_of::<[u16; 2]>() <= 4096);
    }

    /// Renders the real shader offscreen through a recording of a WAV, then
    /// transcribing and done, as PNG frames at 60 fps:
    ///
    /// ```sh
    /// HEX_DOT_SCOPE_WAV=tests/fixtures/vocabulary/audio/distractor-14.wav \
    /// HEX_DOT_SCOPE_FRAMES=/tmp/frames HEX_DOT_SCOPE_THEME=light \
    ///   cargo test --bin voice-control dot_scope_frames -- --ignored
    /// ```
    #[test]
    #[ignore = "writes frames for a review clip"]
    fn dot_scope_frames() {
        use metal::{
            Device, MTLLoadAction, MTLRegion, MTLStorageMode, MTLStoreAction, MTLTextureUsage,
            RenderPassDescriptor, TextureDescriptor,
        };

        let wav = std::env::var("HEX_DOT_SCOPE_WAV").expect("HEX_DOT_SCOPE_WAV");
        let out = std::path::PathBuf::from(
            std::env::var("HEX_DOT_SCOPE_FRAMES").expect("HEX_DOT_SCOPE_FRAMES"),
        );
        let dark = std::env::var("HEX_DOT_SCOPE_THEME").as_deref() != Ok("light");
        std::fs::create_dir_all(&out).unwrap();
        let mut reader = hound::WavReader::open(&wav).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                let full = (1_i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.unwrap() as f32 / full)
                    .collect()
            }
            hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        };
        let per_frame = (spec.sample_rate / 60) as usize * usize::from(spec.channels);
        let levels: Vec<f32> = samples
            .chunks(per_frame)
            .map(|chunk| {
                let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
                crate::dot_scope::level_from_rms(rms)
            })
            .collect();

        let device = Device::system_default().expect("Metal");
        let library = device
            .new_library_with_data(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/dictation_indicator.metallib"
            )))
            .unwrap();
        let mut view = DotScopeView::new(&device, &library, Tuning::default()).unwrap();
        view.set_dark(dark);
        view.restart(false);
        let geometry = super::super::Geometry::DOT_SCOPE;
        let scale = 2.0;
        let (width, height) = (
            (geometry.width * scale) as u64,
            (geometry.height * scale) as u64,
        );
        let descriptor = TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_storage_mode(MTLStorageMode::Shared);
        descriptor.set_usage(MTLTextureUsage::RenderTarget);
        let texture = device.new_texture(&descriptor);
        let queue = device.new_command_queue();
        let background: [f32; 3] = if dark {
            [0.11, 0.11, 0.13]
        } else {
            [0.93, 0.93, 0.94]
        };

        // Recording through the WAV and half a second after, transcribing,
        // done, then hidden.
        let recording = levels.len() + 30;
        let schedule = (0..recording)
            .map(|i| {
                (
                    Some(Stage::Recording),
                    true,
                    levels.get(i).copied().unwrap_or(0.0),
                )
            })
            .chain((0..72).map(|_| (Some(Stage::Processing), true, 0.0)))
            .chain((0..20).map(|_| (Some(Stage::Done), true, 0.0)))
            .chain((0..30).map(|_| (None, false, 0.0)));
        let mut pixels = vec![0_u8; (width * height * 4) as usize];
        for (frame, (stage, visible, level)) in schedule.enumerate() {
            view.advance(1.0 / 60.0, stage, visible, level, false);
            let pass = RenderPassDescriptor::new();
            let attachment = pass.color_attachments().object_at(0).unwrap();
            attachment.set_texture(Some(&texture));
            attachment.set_load_action(MTLLoadAction::Clear);
            attachment.set_clear_color(metal::MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            attachment.set_store_action(MTLStoreAction::Store);
            let buffer = queue.new_command_buffer();
            let encoder = buffer.new_render_command_encoder(pass);
            view.encode(encoder, [width as f32, height as f32], scale);
            encoder.end_encoding();
            buffer.commit();
            buffer.wait_until_completed();
            texture.get_bytes(
                pixels.as_mut_ptr().cast(),
                width * 4,
                MTLRegion::new_2d(0, 0, width, height),
                0,
            );
            // Premultiplied BGRA over the background, as RGB.
            let rgb: Vec<u8> = pixels
                .chunks(4)
                .flat_map(|p| {
                    let a = f32::from(p[3]) / 255.0;
                    [p[2], p[1], p[0]]
                        .into_iter()
                        .zip(background)
                        .map(move |(c, b)| {
                            ((f32::from(c) / 255.0 + b * (1.0 - a)) * 255.0).round() as u8
                        })
                })
                .collect();
            let file = std::fs::File::create(out.join(format!("{frame:04}.png"))).unwrap();
            let mut encoder = png::Encoder::new(file, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&rgb)
                .unwrap();
        }
    }
}
