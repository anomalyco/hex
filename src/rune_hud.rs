//! The dictation HUD on every OS: a click-through Rune overlay whose
//! compositor steps `dictation_indicator`'s model on Rune's frame clock (the
//! display's refresh on macOS) and draws Dot scope on the GPU with
//! `rune_hud.wgsl`. The overlay never takes
//! focus or a click: a nonactivating panel on macOS, a layer-shell surface on
//! Wayland, an override-redirect window on X11, and a topmost tool window
//! that never activates on Windows.
//!
//! Listeners report through [`HudSender`], which never waits: each event is
//! an overlay update, applied on the event thread before the next frame.

#[cfg(target_os = "macos")]
mod macos;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rune::compose::{Compositor, Frame};
use rune::{Cx, El, OverlayHandle, OverlayOptions, OverlaySpec, ScreenEdge, el};

use crate::app_settings::IndicatorPosition;
use crate::dictation_indicator::{DictationIndicatorEvent, DotFrame, IndicatorModel};
use crate::dot_scope::{self, MAX_DOTS, PLATE_HEIGHT};
use crate::hud_placement::{ISLAND_INSET, ISLAND_PAD, Island, Layout};
use crate::rune_ui::live_text::Spot;

#[cfg(target_os = "macos")]
pub(crate) use macos::DictationIndicatorUi;

pub(crate) type HudEvent = DictationIndicatorEvent;

#[cfg(not(target_os = "macos"))]
const TOP_OFFSET: f32 = 12.0;
#[cfg(not(target_os = "macos"))]
const BOTTOM_OFFSET: f32 = 24.0;
/// The HUD window: room for Dot scope's widest plate and drop shadow, and
/// on macOS for an island grown out of the widest notch.
#[cfg(target_os = "macos")]
const WINDOW: (f32, f32) = (320.0, 128.0);
#[cfg(not(target_os = "macos"))]
const WINDOW: (f32, f32) = (304.0, 80.0);
/// The notch's own bottom corner radius, which the island starts from.
const NOTCH_RADIUS: f32 = 8.0;
/// The open island's bottom corner radius.
const ISLAND_RADIUS: f32 = 18.0;

/// The live text's gap from the HUD's plate.
const LIVE_TEXT_GAP: f32 = 6.0;

/// Where the live text starts, as of the HUD's last show.
static LIVE_TEXT: Mutex<Option<Spot>> = Mutex::new(None);

/// Where the live text plate starts: just past the HUD's plate, on its
/// screen; with the HUD hidden, where its plate would be.
pub(crate) fn live_text_place() -> Spot {
    let spot = *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner());
    spot.unwrap_or(Spot::Edge(ScreenEdge::Top, 40.0))
}

fn set_live_text(spot: Spot) {
    *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner()) = Some(spot);
}

/// How a listener reports to the HUD; sending never waits.
#[derive(Clone)]
pub(crate) struct HudSender {
    overlay: OverlayHandle<Hud>,
    #[cfg(target_os = "macos")]
    animating: Arc<AtomicBool>,
}

impl HudSender {
    pub(crate) fn send(&self, event: HudEvent) {
        self.overlay.update(move |hud, _| hud.handle(event));
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_position(&self, position: IndicatorPosition) {
        self.overlay
            .update(move |hud, _| hud.set_position(position));
    }

    /// The overlay, to ask whether the platform could show it.
    #[cfg(target_os = "linux")]
    pub(crate) fn availability(&self) -> rune::Availability {
        self.overlay.availability()
    }
}

/// The overlay's state: the model and what it last drew.
pub(crate) struct Hud {
    model: IndicatorModel,
    drawn: Option<DotFrame>,
    /// Where the plate sits in the window (macOS placements).
    layout: Layout,
    position: IndicatorPosition,
    /// The overlay itself, to show and hide it.
    this: Option<OverlayHandle<Hud>>,
    shown: bool,
    /// Rune's Reduce Motion, as of the last frame.
    reduce: bool,
    /// Whether the compositor wants the display's frames.
    animating: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    placed: Option<crate::hud_placement::Placed>,
}

impl Hud {
    fn handle(&mut self, event: HudEvent) {
        #[cfg(target_os = "macos")]
        if let HudEvent::ConfigureDotScope(_) = event {
            self.placed = None;
        }
        self.model.handle(event, Instant::now(), self.reduce);
        self.animating.store(true, Ordering::Release);
        let starts = matches!(event, HudEvent::Started | HudEvent::EditingStarted);
        if starts || (self.shown && !matches!(event, HudEvent::Meter { .. })) {
            self.show();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn set_position(&mut self, position: IndicatorPosition) {
        self.position = position;
        if self.shown {
            self.show();
        }
    }

    /// Shows the overlay where the HUD belongs, unless it is set to Hidden.
    fn show(&mut self) {
        let Some(overlay) = self.this.clone() else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            self.position = crate::app_settings::indicator_position();
        }
        if self.position == IndicatorPosition::Hidden {
            #[cfg(target_os = "macos")]
            self.place_live_text_without_hud();
            #[cfg(not(target_os = "macos"))]
            {
                let plate_start = TOP_OFFSET + (WINDOW.1 - PLATE_HEIGHT) / 2.0;
                set_live_text(Spot::Edge(ScreenEdge::Top, plate_start));
            }
            self.hide();
            return;
        }
        #[cfg(target_os = "macos")]
        self.place_on_pointer_screen(&overlay);
        #[cfg(not(target_os = "macos"))]
        {
            let (edge, margin) = match self.position {
                IndicatorPosition::Bottom => (ScreenEdge::Bottom, BOTTOM_OFFSET),
                _ => (ScreenEdge::Top, TOP_OFFSET),
            };
            overlay.place(edge, margin);
            // The plate sits in the middle of the window.
            let past_plate = margin + (WINDOW.1 + PLATE_HEIGHT) / 2.0 + LIVE_TEXT_GAP;
            set_live_text(Spot::Edge(edge, past_plate));
        }
        if !self.shown {
            overlay.show();
            self.shown = true;
        }
    }

    fn hide(&mut self) {
        if std::mem::take(&mut self.shown)
            && let Some(overlay) = &self.this
        {
            overlay.hide();
        }
    }
}

/// Adds the HUD's overlay to the app; send its events through the returned
/// sender.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> HudSender {
    let animating = Arc::new(AtomicBool::new(false));
    let now = Instant::now();
    let model = IndicatorModel::new(dot_scope::lab(), now);
    let overlay = app.overlay(
        OverlaySpec {
            name: "hex-hud",
            size: WINDOW,
            edge: ScreenEdge::Top,
            margin: 0.0,
            align: rune::Align::Center,
            inset: 0.0,
            interactive: false,
            view,
        }
        .options(OverlayOptions {
            // The first press never waits on the panel or its pipeline.
            prewarm: cfg!(target_os = "macos"),
            ..OverlayOptions::default()
        })
        // The shaders write display (sRGB-encoded) values.
        .srgb_blend(true)
        .compositor(HudCompositor {
            gpu: None,
            animating: animating.clone(),
        }),
        Hud {
            model,
            drawn: None,
            layout: Layout::default(),
            position: IndicatorPosition::Top,
            this: None,
            shown: false,
            reduce: false,
            animating: animating.clone(),
            #[cfg(target_os = "macos")]
            placed: None,
        },
    );
    let this = overlay.clone();
    overlay.update(move |hud, _| hud.this = Some(this));
    HudSender {
        overlay,
        #[cfg(target_os = "macos")]
        animating,
    }
}

fn view(_: &Hud, _: &mut Cx<Hud>) -> El {
    el().key("hud").w_full().h_full()
}

/// `DotUniforms` in `rune_hud.wgsl`.
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
    plate_layout: [f32; 4],
    island: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<DotUniforms>() == 176);
const DOT_BYTES: u64 = (MAX_DOTS * 4) as u64;

impl DotUniforms {
    fn new(frame: &DotFrame, layout: Layout, target: Target) -> Self {
        let pose = &frame.pose;
        // An island is black in either appearance and draws no plate.
        let dark = target.dark || layout.island.is_some();
        let mut colors = dot_scope::palette(dark, pose, frame.tuning.saturation);
        if layout.island.is_some() {
            for color in [&mut colors.plate, &mut colors.rim, &mut colors.shadow] {
                color[3] = 0.0;
            }
        }
        let mut center = layout.center;
        if layout.anchor_left {
            center[0] += pose.width * layout.fit / 2.0;
        }
        let rgba = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        Self {
            resolution: [target.size.0 as f32, target.size.1 as f32],
            backing_scale: target.scale,
            width: pose.width,
            height: pose.height,
            opacity: pose.opacity,
            scale: pose.scale,
            softness: pose.softness,
            pitch: frame.tuning.spacing,
            columns: frame.columns as f32,
            rows: frame.rows as f32,
            glow: pose.glow,
            grid: pose.grid,
            live: pose.live,
            cool: pose.cool,
            dark: if dark { 1.0 } else { 0.0 },
            base: rgba(colors.base),
            core: rgba(colors.core),
            plate: colors.plate,
            rim: colors.rim,
            shadow: colors.shadow,
            plate_layout: [
                center[0],
                center[1],
                layout.fit,
                if layout.island.is_some() { 1.0 } else { 0.0 },
            ],
            island: layout
                .island
                .map_or([0.0; 4], |island| island_shape(island, pose)),
        }
    }
}

/// The island for this pose: the notch itself while hidden, grown to hold
/// the plate as it shows, bouncing with it.
fn island_shape(island: Island, pose: &dot_scope::Pose) -> [f32; 4] {
    let [notch_width, notch_height] = island.notch;
    let pad = ISLAND_PAD as f32;
    let grow = pose.reveal;
    let half_width = notch_width / 2.0;
    let open_half_width = (pose.width / 2.0 + ISLAND_INSET as f32).max(half_width);
    let open_height = notch_height + 2.0 * pad + pose.height;
    let height = notch_height + (open_height - notch_height) * grow;
    [
        half_width + (open_half_width - half_width) * grow,
        height,
        island.top,
        (NOTCH_RADIUS + (ISLAND_RADIUS - NOTCH_RADIUS) * grow.min(1.0)).min(height / 2.0),
    ]
}

fn bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: `T` is a `repr(C)` struct of `f32`s with no padding, so every
    // byte is initialized.
    unsafe { std::slice::from_raw_parts((value as *const T).cast(), std::mem::size_of::<T>()) }
}

fn packed_dots(frame: &DotFrame) -> Vec<u8> {
    frame
        .dots
        .iter()
        .take(MAX_DOTS)
        .flat_map(|[power, fresh]| [power.to_le_bytes(), fresh.to_le_bytes()])
        .flatten()
        .collect()
}

/// What a frame draws into.
#[derive(Clone, Copy)]
struct Target {
    /// In device pixels.
    size: (u32, u32),
    scale: f32,
    dark: bool,
}

struct HudCompositor {
    gpu: Option<Gpu>,
    animating: Arc<AtomicBool>,
}

impl Compositor<Hud> for HudCompositor {
    fn animating(&self) -> bool {
        self.animating.load(Ordering::Acquire)
    }

    /// The view is empty, so Rune's UI pass is skipped: Hex's own pass
    /// clears the target.
    fn render(&mut self, f: &mut Frame<'_, Hud>) {
        let reduce = f.rt.reduced_motion;
        let target = Target {
            size: f.size,
            scale: f.rt.env.scale,
            dark: f.rt.env.system_dark,
        };
        let hud = &mut f.rt.state;
        hud.reduce = reduce;
        if hud.model.active() {
            hud.drawn = Some(hud.model.step(Instant::now(), f.dt as f32, reduce));
        }
        let active = hud.model.active();
        self.animating.store(active, Ordering::Release);
        if !active {
            hud.hide();
            drop(clear_pass(f.encoder, f.target));
            return;
        }
        let (Some(drawn), layout) = (hud.drawn.take(), hud.layout) else {
            drop(clear_pass(f.encoder, f.target));
            return;
        };
        if self.gpu.as_ref().is_none_or(|gpu| gpu.format != f.format) {
            self.gpu = Some(Gpu::new(f.device, f.format));
        }
        let gpu = self.gpu.as_ref().expect("created above");
        gpu.write(&drawn, layout, target, |buffer, data| {
            f.write_buffer(buffer, 0, data)
        });
        gpu.draw(f.encoder, f.target);
        f.rt.state.drawn = Some(drawn);
    }
}

struct Gpu {
    format: wgpu::TextureFormat,
    dot_uniforms: wgpu::Buffer,
    dots: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Gpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hex hud"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rune_hud.wgsl").into()),
        });
        let buffer = |label, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let dot_uniforms = buffer(
            "hex hud dot uniforms",
            std::mem::size_of::<DotUniforms>() as u64,
        );
        let dots = buffer("hex hud dots", DOT_BYTES);
        let entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hex hud"),
            entries: &[entry(0), entry(1)],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hex hud"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: dot_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: dots.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hex hud"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dot_scope_fragment"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("indicator_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("dot_scope_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Gpu {
            format,
            dot_uniforms,
            dots,
            bind,
            pipeline,
        }
    }

    /// Writes `frame`'s buffers through `write`.
    fn write(
        &self,
        frame: &DotFrame,
        layout: Layout,
        target: Target,
        mut write: impl FnMut(&wgpu::Buffer, &[u8]),
    ) {
        write(
            &self.dot_uniforms,
            bytes(&DotUniforms::new(frame, layout, target)),
        );
        write(&self.dots, &packed_dots(frame));
    }

    /// Clears `view` and draws Dot scope into it.
    fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = clear_pass(encoder, view);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A pass over `view` that starts transparent.
fn clear_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("hex hud"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests;
