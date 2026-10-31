//! The practice game's lightning strike and burn, on the GPU.
//!
//! `game.rs` publishes an [`Fx`] every frame and marks two kinds of effect
//! slot: one inside each burning card ([`BURN_MARK`] plus the card's index),
//! whose own layer the burn erases so the page shows through the hole, and
//! one over the stage ([`STRIKE_MARK`]) for bolts and ash. The window's
//! compositor draws the frame through [`GameFx::render`], which paints both
//! in painter order (`game_fx.wgsl`). Without it the slots draw nothing.

use std::collections::HashMap;
use std::sync::Mutex;

use rune::Color;
use rune::compose::Frame;
use rune::render::EffectSite;

use crate::rune_ui::Ui;

pub(crate) const BURN_MARK: u32 = 0x4800_0100;
pub(crate) const STRIKE_MARK: u32 = 0x4800_0200;

pub(crate) fn is_game_mark(m: u32) -> bool {
    (BURN_MARK..=STRIKE_MARK).contains(&m)
}

/// Width of the glowing band behind the front, in field units.
pub(crate) const EDGE: f32 = 0.04;
/// The front crosses the whole card in this many seconds.
pub(crate) const BURN: f32 = 0.95;
/// Noise cells per point: blobs about 29 pt across.
const GRAIN: f32 = 0.035;
/// The front's ease out: fast where the bolt hit, slowing into the corners.
const SHAPE: f32 = 1.35;

/// One frame of the effect, in stage points.
#[derive(Clone, Default)]
pub(crate) struct Fx {
    pub dark: bool,
    pub tones: Tones,
    pub burns: Vec<Burn>,
    pub bolts: Vec<Bolt>,
    pub motes: Vec<Mote>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Tones {
    /// The white-hot channel and rim.
    pub core: Color,
    /// The glow around them.
    pub corona: Color,
    /// The scorched band ahead of the front.
    pub scorch: Color,
    /// Cold ash.
    pub ash: Color,
}

#[derive(Clone, Copy)]
pub(crate) struct Burn {
    pub card: usize,
    /// Card points.
    pub size: [f32; 2],
    pub impact: [f32; 2],
    /// Seconds since the bolt hit.
    pub age: f32,
    pub seed: u32,
}

#[derive(Clone)]
pub(crate) struct Bolt {
    pub points: Vec<[f32; 2]>,
    /// Core half width.
    pub width: f32,
    /// White-hot brightness, and the cooling glow after it.
    pub core: f32,
    pub glow: f32,
    /// How far along the path it's drawn: a stepped leader feeling its way.
    pub drawn: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct Mote {
    pub at: [f32; 2],
    pub radius: f32,
    /// 1 glowing as it leaves the rim, cooling to 0.
    pub heat: f32,
    pub opacity: f32,
}

static FX: Mutex<Option<Fx>> = Mutex::new(None);

pub(crate) fn publish(fx: Fx) {
    if let Ok(mut g) = FX.lock() {
        *g = Some(fx);
    }
}

fn lattice_hash(value: u32, salt: u32) -> f32 {
    let mut h = value.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

fn lattice_at(cell: [i32; 2], salt: u32) -> f32 {
    lattice_hash(
        (cell[0] as u32).wrapping_add((cell[1] as u32).wrapping_mul(0x27D4_EB2F)),
        salt,
    )
}

fn lattice2(p: [f32; 2], salt: u32) -> f32 {
    let c = [p[0].floor(), p[1].floor()];
    let f = [p[0] - c[0], p[1] - c[1]];
    let i = [c[0] as i32, c[1] as i32];
    let u = f.map(|f| f * f * (3.0 - 2.0 * f));
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let bottom = mix(
        lattice_at(i, salt),
        lattice_at([i[0] + 1, i[1]], salt),
        u[0],
    );
    let top = mix(
        lattice_at([i[0], i[1] + 1], salt),
        lattice_at([i[0] + 1, i[1] + 1], salt),
        u[0],
    );
    mix(bottom, top, u[1])
}

fn fbm(p: [f32; 2], salt: u32) -> f32 {
    let (mut q, mut sum, mut amplitude) = (p, 0.0, 0.5);
    for octave in 0..4 {
        sum += lattice2(q, salt + octave) * amplitude;
        q = [q[0] * 2.03 + 17.0, q[1] * 2.03 + 5.0];
        amplitude *= 0.5;
    }
    sum / 0.9375
}

/// The field the front sweeps, 0..1, at card point `local`: out from the
/// impact, broken up by noise. The shader evaluates the same field, so ash
/// leaves exactly where the card burns.
pub(crate) fn field(local: [f32; 2], size: [f32; 2], impact: [f32; 2], seed: u32) -> f32 {
    let d = |x: f32, y: f32| (local[0] - x).hypot(local[1] - y);
    let far = [[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size]
        .iter()
        .map(|c| (impact[0] - c[0]).hypot(impact[1] - c[1]))
        .fold(1.0f32, f32::max);
    let radial = d(impact[0], impact[1]) / far;
    let noise = ((fbm([local[0] * GRAIN, local[1] * GRAIN], seed) - 0.2) / 0.6).clamp(0.0, 1.0);
    (0.75 * radial + 0.25 * noise).clamp(0.0, 1.0)
}

/// Where the front stands at `age`: field values below it are gone.
#[cfg(test)]
fn front(age: f32) -> f32 {
    let u = (age / BURN).clamp(0.0, 1.0);
    (1.0 - (1.0 - u).powf(SHAPE)) * (1.0 + 2.0 * EDGE) - EDGE
}

/// The age at which the front passes a field value.
pub(crate) fn passes(value: f32) -> f32 {
    let x = ((value + EDGE) / (1.0 + 2.0 * EDGE)).clamp(0.0, 1.0);
    BURN * (1.0 - (1.0 - x).powf(1.0 / SHAPE))
}

const SLOT_STRIDE: u64 = 256;
const SLOTS: u64 = 32;
const ITEM_BYTES: u64 = 48;

/// What `game_fx.wgsl` reads for one slot.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    rect: [f32; 4],
    view: [f32; 4],
    info: [f32; 4],
    core: [f32; 4],
    corona: [f32; 4],
    scorch: [f32; 4],
    ash: [f32; 4],
    burn: [f32; 4],
    burn2: [f32; 4],
}

fn rgba(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

struct Pipes {
    scorch: wgpu::RenderPipeline,
    erase: wgpu::RenderPipeline,
    halo: wgpu::RenderPipeline,
    bolt: wgpu::RenderPipeline,
    mote: wgpu::RenderPipeline,
}

/// The burn and strike pipelines, made per target format as they're met.
pub(crate) struct GameFx {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipes: HashMap<wgpu::TextureFormat, Pipes>,
    slots: wgpu::Buffer,
    items: wgpu::Buffer,
    points: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

const SOURCE_ATOP: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::DstAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

const DESTINATION_OUT: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

impl GameFx {
    pub(crate) fn new(device: &wgpu::Device) -> GameFx {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("game fx"),
            source: wgpu::ShaderSource::Wgsl(include_str!("game_fx.wgsl").into()),
        });
        let visible = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: visible,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("game fx"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: visible,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Slot>() as u64),
                    },
                    count: None,
                },
                storage(1),
                storage(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("game fx"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("game fx slots"),
            size: SLOT_STRIDE * SLOTS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (items, points, bind) = Self::storage(device, &layout, &slots, 256, 1024);
        GameFx {
            module,
            layout,
            pipeline_layout,
            pipes: HashMap::new(),
            slots,
            items,
            points,
            bind,
        }
    }

    fn storage(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: &wgpu::Buffer,
        items: u64,
        points: u64,
    ) -> (wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup) {
        let buffer = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let items = buffer("game fx items", items * ITEM_BYTES);
        let points = buffer("game fx points", points * 8);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("game fx"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: slots,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Slot>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: items.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: points.as_entire_binding(),
                },
            ],
        });
        (items, points, bind)
    }

    fn pipes(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> &Pipes {
        let (module, layout) = (&self.module, &self.pipeline_layout);
        self.pipes.entry(format).or_insert_with(|| {
            let pipe = |vertex: &str, fragment: &str, blend: wgpu::BlendState| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(fragment),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some(vertex),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(fragment),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let over = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
            Pipes {
                scorch: pipe("burn_vertex", "scorch_fragment", SOURCE_ATOP),
                erase: pipe("burn_vertex", "erase_fragment", DESTINATION_OUT),
                halo: pipe("burn_vertex", "halo_fragment", over),
                bolt: pipe("item_vertex", "bolt_fragment", over),
                mote: pipe("item_vertex", "mote_fragment", over),
            }
        })
    }

    /// Draws the frame, painting the game's slots where they fall.
    pub(crate) fn render(&mut self, f: &mut Frame<'_, Ui>) {
        let fx = FX.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
        let (device, queue) = (f.device, f.queue);

        // Bolts, then motes, as items; bolt paths in one point list.
        let mut items: Vec<f32> = Vec::new();
        let mut points: Vec<f32> = Vec::new();
        for bolt in &fx.bolts {
            let first = points.len() / 2;
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for p in &bolt.points {
                points.extend_from_slice(p);
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
            let reach = 34.0;
            items.extend_from_slice(&[
                first as f32,
                bolt.points.len() as f32,
                bolt.width,
                reach,
                bolt.core,
                bolt.glow,
                bolt.drawn,
                7.0,
                lo[0] - reach,
                lo[1] - reach,
                hi[0] + reach,
                hi[1] + reach,
            ]);
        }
        for m in &fx.motes {
            let r = m.radius * 3.0 + 1.5;
            items.extend_from_slice(&[
                m.at[0],
                m.at[1],
                m.radius,
                m.heat,
                m.opacity,
                0.0,
                0.0,
                0.0,
                m.at[0] - r,
                m.at[1] - r,
                m.at[0] + r,
                m.at[1] + r,
            ]);
        }
        let item_count = (items.len() / 12) as u64;
        let point_count = (points.len() / 2) as u64;
        let (have_items, have_points) = (self.items.size() / ITEM_BYTES, self.points.size() / 8);
        if item_count > have_items || point_count > have_points {
            let grown = Self::storage(
                device,
                &self.layout,
                &self.slots,
                item_count.max(have_items).next_power_of_two(),
                point_count.max(have_points).next_power_of_two(),
            );
            (self.items, self.points, self.bind) = grown;
        }
        if !items.is_empty() {
            queue.write_buffer(&self.items, 0, &floats(&items));
        }
        if !points.is_empty() {
            queue.write_buffer(&self.points, 0, &floats(&points));
        }

        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        let marks = &f.rt.marks;
        let bolts = fx.bolts.len() as u32;
        let motes = fx.motes.len() as u32;
        let tones = fx.tones;
        let mut used = 0u64;
        f.ui.draw_with(
            f.encoder,
            f.target,
            clear,
            &mut |pass, site: &EffectSite<'_>| {
                let Some(&mark) = marks.get(site.index as usize) else {
                    return;
                };
                if !is_game_mark(mark) || used >= SLOTS {
                    return;
                }
                let burn = (mark != STRIKE_MARK)
                    .then(|| fx.burns.iter().find(|b| b.card as u32 == mark - BURN_MARK))
                    .flatten();
                if mark != STRIKE_MARK && burn.is_none() {
                    return;
                }
                let bleed = site.instance.params[2] / site.scale.max(1e-3);
                let slot = Slot {
                    rect: site.instance.rect,
                    view: [
                        site.origin[0],
                        site.origin[1],
                        site.size[0] as f32,
                        site.size[1] as f32,
                    ],
                    info: [
                        site.scale,
                        if site.srgb_blend { 1.0 } else { 0.0 },
                        if fx.dark { 1.0 } else { 0.0 },
                        bleed,
                    ],
                    core: rgba(tones.core),
                    corona: rgba(tones.corona),
                    scorch: rgba(tones.scorch),
                    ash: rgba(tones.ash),
                    burn: burn.map_or([0.0; 4], |b| {
                        [b.impact[0], b.impact[1], b.size[0], b.size[1]]
                    }),
                    burn2: burn.map_or([0.0; 4], |b| [b.age, b.seed as f32, 0.0, 0.0]),
                };
                let offset = used * SLOT_STRIDE;
                used += 1;
                let bytes: Vec<f32> = [
                    slot.rect,
                    slot.view,
                    slot.info,
                    slot.core,
                    slot.corona,
                    slot.scorch,
                    slot.ash,
                    slot.burn,
                    slot.burn2,
                ]
                .concat();
                queue.write_buffer(&self.slots, offset, &floats(&bytes));
                let bind = self.bind.clone();
                let pipes = self.pipes(device, site.format);
                pass.set_bind_group(0, &bind, &[offset as u32]);
                if burn.is_some() {
                    for pipeline in [&pipes.scorch, &pipes.erase, &pipes.halo] {
                        pass.set_pipeline(pipeline);
                        pass.draw(0..6, 0..1);
                    }
                } else {
                    if bolts > 0 {
                        pass.set_pipeline(&pipes.bolt);
                        pass.draw(0..6, 0..bolts);
                    }
                    if motes > 0 {
                        pass.set_pipeline(&pipes.mote);
                        pass.draw(0..6, bolts..bolts + motes);
                    }
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_front_clears_the_card_between_its_ends() {
        let (size, impact) = ([404.0, 120.0], [300.0, 70.0]);
        for i in 0..400u32 {
            let local = [lattice_hash(i, 1) * size[0], lattice_hash(i, 2) * size[1]];
            let value = field(local, size, impact, 7);
            assert!((0.0..=1.0).contains(&value));
            assert!(front(0.0) + EDGE <= value + 1e-6, "intact at 0");
            assert!(front(BURN) > value, "gone once burnt");
            assert!((front(passes(value)) - value).abs() < 1e-4);
        }
        // It burns outward: the impact goes before the far corner.
        assert!(field(impact, size, impact, 7) < field([0.0, 0.0], size, impact, 7));
    }
}
