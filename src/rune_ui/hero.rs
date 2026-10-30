//! The welcome hero: the Hex glyph, backlit, drawn live on the GPU over the
//! tour's stage. Rune lays out an empty keyed stage; the window's compositor
//! draws the UI as usual, then `hero.wgsl` in two passes over a quad around
//! the stage: god rays at half the target's resolution, then the
//! glyph and its light.
//!
//! A cool lamp stands tight behind the glyph, like an eclipse's corona. It
//! rises, the glyph's core lights, then each ring, one at a time; every part
//! that lights glows, lights what is next to it, and swells the lamp. Then
//! the lamp breathes. The lighting follows rune-light's model (its shafts,
//! rim, shoulder and dither), done in the glyph's own distance field:
//! rune-light's occluders are rounded rects. On a light page the lit rings
//! turn to ink over a pale blue corona.
//!
//! It is a pure function of time and theme; Reduce Motion, or a still
//! fixture, shows its settled frame with the lamp at rest.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

use rune::compose::{Compositor, Frame};
use rune::{Color, El, el};

use super::Ui;
use super::onboarding_kit::gpu_stage::{OnboardingGpu, is_onboard_mark};

/// The stage's key, found by the compositor each frame.
const KEY: &str = "hero-stage";
/// The stage's height in points.
pub(super) const HEIGHT: f32 = 220.0;
/// How far the light reaches past the stage, in points: up and to each side
/// from the icon's centre, and down past the stage (short of the title).
const REACH: [f32; 3] = [250.0, 400.0, 24.0];
/// The rays texture's resolution against the target's: a quarter shows
/// blocks between the rings and saves only 0.06 ms a frame.
const RAYS_SCALE: f32 = 0.5;

/// The lamp: its colour (sRGB) and intensity, its glow radius, and how far its
/// disc reaches past the glyph (points).
const LAMP: [f32; 3] = [0.76, 0.86, 1.0];
const INTENSITY: f32 = 1.6;
const GLOW: f32 = 22.0;
const DISC: f32 = 6.0;
/// The lit glyph's colour (sRGB).
const EMIT: [f32; 3] = [0.93, 0.97, 1.0];
/// The haze's streaks around the lamp: strength and sharpness.
const STREAKS: [f32; 2] = [0.85, 2.8];
/// On a light page the light is a tint: its colour (sRGB) and strength.
const TINT: [f32; 3] = [0.78, 0.86, 1.0];
const TINT_STRENGTH: f32 = 2.2;
/// The rays: the glow's weight, the disc's, the lit glyph's, and their reach (points).
const RAYS: [f32; 4] = [0.5, 1.0, 0.3, 85.0];
/// The glyph's outer apothem in points, and the lamp disc's radius around it.
const GLYPH: f32 = 64.0;
const ICON: f32 = GLYPH / 0.866 * 0.93;

/// A critically damped spring (bounce 0) stepped from 0 to 1 at `at`, settled
/// in about `duration` seconds (ω = 2π / duration).
fn settle(t: f32, at: f32, duration: f32) -> f32 {
    let x = (t - at).max(0.0) * TAU / duration;
    1.0 - (1.0 + x) * (-x).exp()
}

/// The same spring kicked at `at`: it swells to 1 and settles back to 0.
fn kick(t: f32, at: f32, duration: f32) -> f32 {
    let x = (t - at).max(0.0) * TAU / duration;
    x * (1.0 - x).exp()
}

/// When the core, the middle ring and the outer ring light, in seconds.
const LIGHTS_AT: [f32; 3] = [0.35, 0.85, 1.35];
/// How much each one swells the lamp.
const SWELL: [f32; 3] = [0.20, 0.24, 0.40];
/// The lamp's breath once everything is lit: depth and period.
const BREATH: (f32, f32) = (0.07, 5.2);

/// The lamp and the glyph at `t` seconds: the lamp's intensity scale and
/// each part's emission.
fn animate(t: f32, still: bool) -> (f32, [f32; 3]) {
    if still {
        return (1.0, [1.0; 3]);
    }
    let rise = settle(t, 0.05, 1.5);
    let swell: f32 = (0..3).map(|k| SWELL[k] * kick(t, LIGHTS_AT[k], 1.6)).sum();
    let breath = 1.0 + BREATH.0 * settle(t, 2.4, 2.0) * ((t - 2.4).max(0.0) * TAU / BREATH.1).sin();
    let parts = LIGHTS_AT.map(|at| settle(t, at, 0.7) + 0.5 * kick(t, at, 0.8));
    (rise * (1.0 + swell) * breath, parts)
}

/// The empty stage the compositor draws into.
pub(super) fn stage() -> El {
    el().key(KEY).w_full().h(HEIGHT)
}

/// Draws the window: the UI, then the hero over its stage while one shows.
#[derive(Default)]
pub(crate) struct HeroCompositor {
    gpu: Option<Gpu>,
    onboarding: Option<OnboardingGpu>,
    /// Rune's clock when the stage appeared: the entrance starts there.
    since: Option<f64>,
    shown: bool,
    gpu_ms: (f64, u32),
    cpu_ms: (f64, u32),
}

impl Compositor<Ui> for HeroCompositor {
    fn features(&self) -> wgpu::Features {
        wgpu::Features::TIMESTAMP_QUERY
    }

    fn animating(&self) -> bool {
        self.shown
    }

    fn summary(&mut self, _device: &wgpu::Device) -> Option<String> {
        let mean = |(sum, n): (f64, u32)| (n > 0).then(|| sum / n as f64);
        let gpu = mean(self.gpu_ms).map_or("no GPU timestamps".to_owned(), |ms| {
            format!("gpu {ms:.3} ms/frame over {} timed frames", self.gpu_ms.1)
        });
        let cpu = mean(self.cpu_ms).map_or(String::new(), |ms| {
            format!(", cpu {ms:.3} ms/frame encoding")
        });
        Some(format!("hero: {gpu}{cpu}"))
    }

    fn render(&mut self, f: &mut Frame<'_, Ui>) {
        if f.rt.marks.iter().any(|&m| is_onboard_mark(m)) {
            if self
                .onboarding
                .as_ref()
                .is_none_or(|g| !g.matches(f.format))
            {
                self.onboarding = Some(OnboardingGpu::new(f.device, f.format));
            }
            self.onboarding.as_mut().expect("created above").render(f);
        } else {
            f.draw_ui();
        }
        let _ = f.device.poll(wgpu::PollType::Poll);
        let started = Instant::now();
        let fixture_still = f.rt.state.tour.as_ref().map(|tour| tour.hero_still);
        let placed = fixture_still
            .and_then(|_| f.rt.presented(KEY))
            .filter(|(rect, opacity)| rect[2] > 0.0 && rect[3] > 0.0 && *opacity > 0.001);
        let (Some(fixture_still), Some((rect, opacity))) = (fixture_still, placed) else {
            self.shown = false;
            self.since = None;
            return;
        };
        let still = fixture_still || f.rt.reduced_motion;
        self.shown = !still;
        let now = f.rt.t;
        let since = *self.since.get_or_insert(now);
        let t = if still { 12.0 } else { (now - since) as f32 };

        // The quad: around the icon, inside the target.
        let scale = f.rt.env.scale;
        let (w, h) = f.size;
        let centre = [
            (rect[0] + rect[2] * 0.5) * scale,
            (rect[1] + rect[3] * 0.5) * scale,
        ];
        let x0 = (centre[0] - REACH[1] * scale).max(0.0).floor();
        let x1 = (centre[0] + REACH[1] * scale).min(w as f32).ceil();
        let y0 = (centre[1] - REACH[0] * scale).max(0.0).floor();
        let y1 = ((rect[1] + rect[3] + REACH[2]) * scale)
            .min(h as f32)
            .ceil();
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let quad = [x0, y0, x1 - x0, y1 - y0];
        let rays_size = (
            ((quad[2] * RAYS_SCALE).ceil() as u32).max(1),
            ((quad[3] * RAYS_SCALE).ceil() as u32).max(1),
        );

        // The first frame compiles the pipelines; it is left out of the mean.
        let compiled = self.gpu.as_ref().is_none_or(|gpu| gpu.format != f.format);
        if compiled {
            self.gpu = Some(Gpu::new(f.device, f.queue, f.format));
        }
        let gpu = self.gpu.as_mut().expect("created above");
        gpu.size_rays(f.device, rays_size);
        if let Some(ms) = gpu.timer.as_mut().and_then(|timer| timer.begin(f.encoder)) {
            self.gpu_ms.0 += ms as f64;
            self.gpu_ms.1 += 1;
        }

        let dark = f.rt.env.system_dark;
        let (lamp, parts) = animate(t, still);
        let c = &rune_theme_opencode::kit(dark).c;
        let decode = |rgb: [f32; 3]| rgb.map(rune::color::srgb_decode);
        let rgba = |c: Color| [c.r, c.g, c.b, c.a];
        let with = |rgb: [f32; 3], w: f32| [rgb[0], rgb[1], rgb[2], w];
        let uniforms: [[f32; 4]; UNIFORMS] = [
            quad,
            [w as f32, h as f32, scale, t],
            [
                opacity,
                f32::from(u8::from(dark)),
                f32::from(u8::from(still)),
                f32::from(u8::from(!f.format.is_srgb())),
            ],
            [centre[0], centre[1], GLYPH, rect[3] * 0.5],
            rgba(c.bg_base),
            rgba(c.text_base),
            with(decode(LAMP), INTENSITY * lamp),
            [0.0, 0.0, GLOW, ICON + DISC],
            with(decode(EMIT), 0.0),
            // The shader's order: outer ring, middle ring, core.
            [parts[2], parts[1], parts[0], 0.0],
            [STREAKS[0], STREAKS[1], 0.0, 0.0],
            with(decode(TINT), TINT_STRENGTH),
            RAYS,
        ];
        let bytes: Vec<u8> = uniforms
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        f.queue.write_buffer(&gpu.uniforms, 0, &bytes);

        let query = gpu.timer.as_ref().and_then(Timer::query);
        let rays_view = &gpu.rays.as_ref().expect("sized above").1;
        let rays_bind = &gpu.rays.as_ref().expect("sized above").2;
        for k in 0..gpu.repeat {
            let first = query.as_ref().filter(|_| k == 0);
            let last = query.as_ref().filter(|_| k + 1 == gpu.repeat);
            {
                let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hero rays"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: rays_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: first.map(|query_set| wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: None,
                    }),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&gpu.rays_pipeline);
                pass.set_bind_group(0, &gpu.bind, &[]);
                pass.draw(0..4, 0..1);
            }
            let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hero"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: f.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: last.map(|query_set| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(1),
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.composite);
            pass.set_bind_group(0, &gpu.bind, &[]);
            pass.set_bind_group(1, rays_bind, &[]);
            pass.draw(0..4, 0..1);
        }
        if let Some(timer) = gpu.timer.as_mut() {
            timer.end();
        }
        if !compiled {
            self.cpu_ms.0 += started.elapsed().as_secs_f64() * 1000.0;
            self.cpu_ms.1 += 1;
        }
    }
}

/// `U` in the shader: thirteen vec4s.
const UNIFORMS: usize = 13;
const UNIFORM_BYTES: u64 = UNIFORMS as u64 * 16;
const RAYS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

struct Gpu {
    format: wgpu::TextureFormat,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    rays_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The rays texture at its size, its view, and its bind group.
    rays: Option<((u32, u32), wgpu::TextureView, wgpu::BindGroup)>,
    rays_pipeline: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    timer: Option<Timer>,
    /// Draws per frame: `HEX_HERO_REPEAT` measures the passes' own GPU cost
    /// as the slope between repeat counts, free of their fixed overhead.
    repeat: u32,
}

impl Gpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Gpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hero"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hero.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hero uniforms"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hero"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let rays_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hero rays"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: fragment,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: fragment,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hero"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline = |layouts: &[Option<&wgpu::BindGroupLayout>],
                        vertex: &str,
                        fragment: &str,
                        target: wgpu::TextureFormat,
                        blend: Option<wgpu::BlendState>| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(fragment),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let rays_pipeline = pipeline(
            &[Some(&uniform_layout)],
            "vs_rays",
            "fs_rays",
            RAYS_FORMAT,
            None,
        );
        let composite = pipeline(
            &[Some(&uniform_layout), Some(&rays_layout)],
            "vs_quad",
            "fs_composite",
            format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hero rays"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Gpu {
            format,
            uniforms,
            bind,
            rays_layout,
            sampler,
            rays: None,
            rays_pipeline,
            composite,
            timer: Timer::new(device, queue),
            repeat: std::env::var("HEX_HERO_REPEAT")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(1),
        }
    }

    /// Makes the rays texture `size`, keeping it while the size holds.
    fn size_rays(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.rays.as_ref().is_some_and(|(at, _, _)| *at == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hero rays"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RAYS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hero rays"),
            layout: &self.rays_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.rays = Some((size, view, bind));
    }
}

/// The hero pass's GPU time from timestamps at its start and end, read back
/// a few frames later without stalling.
struct Timer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// 0 idle, 4 written, 1 copy recorded, 2 mapping, 3 mapped.
    state: Arc<AtomicU8>,
    period: f32,
    recording: bool,
}

impl Timer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Timer> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("hero timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Timer {
            queries,
            resolve: buffer(
                "hero timestamps resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            read: buffer(
                "hero timestamps read",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            state: Arc::new(AtomicU8::new(0)),
            period: queue.get_timestamp_period(),
            recording: false,
        })
    }

    /// Advances the readback; returns a finished measurement in milliseconds.
    fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<f32> {
        let mut out = None;
        match self.state.load(Ordering::Acquire) {
            4 => {
                encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
                encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 16);
                self.state.store(1, Ordering::Release);
            }
            1 => {
                self.state.store(2, Ordering::Release);
                let state = self.state.clone();
                self.read
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        state.store(if result.is_ok() { 3 } else { 0 }, Ordering::Release);
                    });
            }
            3 => {
                if let Ok(data) = self.read.slice(..).get_mapped_range() {
                    let stamp = |i: usize| {
                        u64::from_ne_bytes(data[i * 8..i * 8 + 8].try_into().unwrap_or_default())
                    };
                    let (start, end) = (stamp(0), stamp(1));
                    out = (end > start).then(|| (end - start) as f32 * self.period / 1e6);
                }
                self.read.unmap();
                self.state.store(0, Ordering::Release);
            }
            _ => {}
        }
        self.recording = self.state.load(Ordering::Acquire) == 0;
        out
    }

    fn query(&self) -> Option<wgpu::QuerySet> {
        self.recording.then(|| self.queries.clone())
    }

    fn end(&mut self) {
        if self.recording {
            self.state.store(4, Ordering::Release);
        }
    }
}
