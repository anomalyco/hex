//! `"With Hex"`'s fireworks, on the GPU: shells rise on fading trails and
//! burst into a few hundred sparks each, white-hot, then their hue, then
//! embers that crackle out, under gravity and air drag. Each burst lights
//! the page under it for a moment in its colour.
//!
//! The view puts two effect slots on the stage ([`SPARKS_MARK`] behind the
//! conversation, so the card hides what flies behind it and its text stays
//! clean, and [`LIGHT_MARK`] over everything) and [`publish`]es a [`Show`]
//! each frame. The window's compositor paints both through [`Painter`]
//! (`fireworks.wgsl`). The light multiplies what's already drawn, so a
//! surface brightens by its own colour and the near-black page barely does.

use std::collections::HashMap;
use std::sync::Mutex;

use rune::Color;
use rune::render::EffectSite;

use super::super::controls::Paint;

pub(crate) const SPARKS_MARK: u32 = 0x4800_0300;
pub(crate) const LIGHT_MARK: u32 = 0x4800_0301;

pub(crate) const fn is_fireworks_mark(mark: u32) -> bool {
    mark == SPARKS_MARK || mark == LIGHT_MARK
}

/// The area both slots cover, in stage points: the stage and the page
/// around it, out to the window's edges.
pub(crate) const AREA: [f32; 4] = [-250.0, -160.0, 1000.0, 800.0];

/// From the first shell going up to the last ember.
pub(crate) const LIFE: f32 = 3.1;

/// A shell climbs this long, slowing under gravity, before it bursts: its
/// sound's whistle, up to the boom.
const RISE: f32 = 0.38;
/// Where shells leave from: below the stage.
const LAUNCH_Y: f32 = 600.0;
const DRAG: f32 = 2.3;
const GRAVITY: f32 = 90.0;
/// The page light's decay after a burst.
const FLASH: f32 = 0.26;
/// The most lights a frame carries (`fireworks.wgsl`'s `LIGHTS`).
const LIGHTS: usize = 4;

#[derive(Clone, Copy)]
enum Kind {
    /// Round, short streaks.
    Peony,
    /// Long fading tails, crackling as it dies.
    Chrysanthemum,
}

struct Shell {
    delay: f32,
    /// Where it bursts, in stage points.
    burst: [f32; 2],
    /// Where it leaves from, across from the burst.
    drift: f32,
    hue: &'static str,
    second: &'static str,
    sparks: u32,
    /// Launch speed of its stars, points per second.
    speed: f32,
    kind: Kind,
}

/// When each shell goes up after the first, and how loud its sound is, so
/// every burst has its boom.
pub(crate) fn launches() -> impl Iterator<Item = (f64, u8)> {
    SHELLS
        .iter()
        .enumerate()
        .map(|(i, shell)| (f64::from(shell.delay), if i == 0 { 100 } else { 70 }))
}

/// Three shells either side of the conversation, never over its text.
const SHELLS: [Shell; 3] = [
    Shell {
        delay: 0.0,
        burst: [-105.0, 120.0],
        drift: 40.0,
        hue: "amber",
        second: "yuzu",
        sparks: 240,
        speed: 360.0,
        kind: Kind::Peony,
    },
    Shell {
        delay: 0.34,
        burst: [585.0, 75.0],
        drift: -30.0,
        hue: "cobalt",
        second: "cyan",
        sparks: 280,
        speed: 370.0,
        kind: Kind::Chrysanthemum,
    },
    Shell {
        delay: 0.78,
        burst: [600.0, 335.0],
        drift: -45.0,
        hue: "coral",
        second: "lilac",
        sparks: 180,
        speed: 300.0,
        kind: Kind::Peony,
    },
];

/// One spark's streak, a burst's halo, or a rising shell, in [`AREA`]'s
/// points; `color` is linear light, brighter than white when it's hot.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Streak {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub color: [f32; 3],
    /// Core half width and glow radius, in points.
    pub half: f32,
    pub glow: f32,
    /// How bright the tail end is next to the head, `0..=1`.
    pub tail: f32,
}

/// A burst's light on the page, in [`AREA`]'s points.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Light {
    pub at: [f32; 2],
    pub color: [f32; 3],
    pub strength: f32,
    pub reach: f32,
}

/// One frame of the show.
#[derive(Clone, Default)]
pub(crate) struct Show {
    pub dark: bool,
    pub streaks: Vec<Streak>,
    pub lights: Vec<Light>,
}

static SHOW: Mutex<Option<Show>> = Mutex::new(None);

/// The view's handoff: this frame's show, or none once it's over. The
/// slots draw only while the view puts them on the stage.
pub(crate) fn publish(show: Option<Show>) {
    if let Ok(mut g) = SHOW.lock() {
        *g = show;
    }
}

fn hash(n: u32, salt: u32) -> f32 {
    let mut h = n.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

fn rgb(c: Color) -> [f32; 3] {
    [c.r, c.g, c.b]
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn scaled(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| v * k)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Where a star thrown at `v` from `from` is after `t`: drag slows it to
/// gravity's terminal fall.
fn thrown(from: [f32; 2], v: [f32; 2], t: f32) -> [f32; 2] {
    let reach = (1.0 - (-DRAG * t).exp()) / DRAG;
    let terminal = GRAVITY / DRAG;
    [
        from[0] + v[0] * reach,
        from[1] + terminal * t + (v[1] - terminal) * reach,
    ]
}

/// A shell's climb, `0..=1` of the way: fast off the ground, slowing to
/// its apex as a thrown thing does.
fn climb(u: f32) -> f32 {
    1.0 - (1.0 - u) * (1.0 - u)
}

fn local(at: [f32; 2]) -> [f32; 2] {
    [at[0] - AREA[0], at[1] - AREA[1]]
}

/// The show `t` seconds after the first shell goes up. Reduce Motion gets
/// one soft flash of light and no sparks.
pub(crate) fn show(p: Paint, t: f32, reduced: bool) -> Option<Show> {
    if !(0.0..LIFE).contains(&t) {
        return None;
    }
    let dark = p.k.dark;
    let mut show = Show {
        dark,
        ..Show::default()
    };
    let tone = |name: &str| rgb(rune_theme_opencode::scale(name, true, 9));
    let white = [1.0, 0.97, 0.92];
    let ember = [1.0, 0.36, 0.08];
    if reduced {
        let shell = &SHELLS[0];
        let glow = smooth(0.0, 0.25, t) * (1.0 - smooth(0.35, 0.9, t));
        if glow > 0.0 {
            show.lights.push(Light {
                at: local([250.0, 160.0]),
                color: mix3(tone(shell.hue), white, 0.4),
                strength: 0.55 * glow,
                reach: 420.0,
            });
        }
        return Some(show);
    }
    for (n, shell) in SHELLS.iter().enumerate() {
        let t = t - shell.delay;
        if t < 0.0 {
            continue;
        }
        let salt = n as u32 * 7919 + 17;
        let hue = tone(shell.hue);
        let second = tone(shell.second);
        let from = [shell.burst[0] + shell.drift, LAUNCH_Y];
        let at_climb = |t: f32| {
            let u = (t / RISE).clamp(0.0, 1.0);
            let y = from[1] + (shell.burst[1] - from[1]) * climb(u);
            // It weaves a little on the way up.
            let x = from[0]
                + (shell.burst[0] - from[0]) * u
                + 2.0 * (t * 31.0 + n as f32).sin() * (1.0 - u);
            [x, y]
        };
        if t < RISE {
            // The shell's head on one fading streak: where it was a moment
            // ago to where it is.
            let gold = mix3(tone("amber"), white, 0.5);
            show.streaks.push(Streak {
                from: local(at_climb((t - 0.14).max(0.0))),
                to: local(at_climb(t)),
                color: scaled(gold, 1.1),
                half: 0.75,
                glow: 2.2,
                tail: 0.0,
            });
            show.streaks.push(Streak {
                from: local(at_climb(t)),
                to: local(at_climb(t)),
                color: scaled(white, 1.4),
                half: 1.1,
                glow: 3.5,
                tail: 1.0,
            });
            // Glitter shed from the trail, falling and going out.
            for k in 0..14u32 {
                let shed = hash(k, salt + 3) * t;
                let age = t - shed;
                if age > 0.45 {
                    continue;
                }
                let at = at_climb(shed);
                let v = [
                    (hash(k, salt + 4) - 0.5) * 30.0,
                    20.0 + hash(k, salt + 5) * 30.0,
                ];
                let pos = thrown(at, v, age);
                let left = 1.0 - age / 0.45;
                show.streaks.push(Streak {
                    from: local(pos),
                    to: local(pos),
                    color: scaled(gold, 0.55 * left * left),
                    half: 0.4,
                    glow: 1.4,
                    tail: 1.0,
                });
            }
            show.lights.push(Light {
                at: local(at_climb(t)),
                color: gold,
                strength: 0.12,
                reach: 160.0,
            });
            continue;
        }
        let age = t - RISE;
        let burst = shell.burst;
        // The page catches the burst, then falls back to dark.
        let flash = smooth(0.0, 0.025, age) * (-age / FLASH).exp();
        if flash > 0.003 {
            show.lights.push(Light {
                at: local(burst),
                color: mix3(hue, white, 0.35),
                strength: 2.6 * flash,
                reach: 340.0,
            });
        }
        // The bang: a white pop blooming into the shell's colour.
        let bang = (-age / 0.07).exp();
        if bang > 0.02 {
            show.streaks.push(Streak {
                from: local(burst),
                to: local(burst),
                color: scaled(mix3(hue, white, bang), 1.3 * bang),
                half: 2.0 * bang,
                glow: 9.0 + 14.0 * (1.0 - bang),
                tail: 1.0,
            });
        }
        let (trail, life) = match shell.kind {
            Kind::Peony => (0.045, 1.55),
            Kind::Chrysanthemum => (0.24, 1.85),
        };
        for i in 0..shell.sparks {
            let h = |k| hash(i, salt + k);
            let life = life * (0.75 + 0.45 * h(1));
            if age >= life {
                continue;
            }
            // A sphere of stars seen from the side: denser at its rim.
            let z = 2.0 * h(2) - 1.0;
            let phi = std::f32::consts::TAU * h(3);
            let r = (1.0 - z * z).sqrt();
            let speed = shell.speed * (0.92 + 0.16 * h(4));
            let v = [phi.cos() * r * speed, phi.sin() * r * speed];
            let head = thrown(burst, v, age);
            let tail = thrown(burst, v, (age - trail).max(0.0));
            let u = age / life;
            // White-hot, then the shell's colour, then a dying ember.
            let heat = (-age / 0.07).exp();
            let colour = mix3(hue, second, 0.35 * h(5));
            let colour = mix3(colour, white, heat);
            let colour = mix3(colour, ember, smooth(0.55, 1.0, u));
            // Stars light as they leave the break, so the pile of them at
            // its centre is the bang's flash, not a white disc.
            let lit = 0.15 + 0.85 * smooth(0.0, 0.12, age);
            let mut bright = (1.0 - u).powf(1.3) * (1.0 + 0.5 * heat) * lit;
            // The last third crackles: stars strobe and pop out.
            let crackles = matches!(shell.kind, Kind::Chrysanthemum) || h(6) < 0.35;
            if crackles && u > 0.62 {
                let tick = ((age * 24.0) as u32).wrapping_add(i * 13);
                bright *= if hash(tick, salt + 9) > 0.55 {
                    1.9
                } else {
                    0.12
                };
            }
            if bright < 0.01 {
                continue;
            }
            show.streaks.push(Streak {
                from: local(tail),
                to: local(head),
                color: scaled(colour, 1.1 * bright),
                half: 0.35 + 0.5 * (1.0 - u).sqrt(),
                glow: 1.4 + 0.5 * heat,
                tail: match shell.kind {
                    Kind::Peony => 0.5,
                    Kind::Chrysanthemum => 0.0,
                },
            });
        }
    }
    show.lights.truncate(LIGHTS);
    Some(show)
}

// --- GPU ---

const SLOT_STRIDE: u64 = 256;
const ITEM_FLOATS: usize = 16;

/// The spark and light pipelines, made per target format as they're met.
pub(crate) struct Painter {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipes: HashMap<wgpu::TextureFormat, Pipes>,
    slots: wgpu::Buffer,
    items: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// The show this frame draws, read once per frame.
    frame: Option<Show>,
    /// Which slots this frame has written into the uniform buffer.
    used: u64,
}

struct Pipes {
    sparks: wgpu::RenderPipeline,
    /// Light on a dark page: what's drawn brightens by its own colour.
    lit: wgpu::RenderPipeline,
    /// Light on a light page: tinted, as light on paper.
    tinted: wgpu::RenderPipeline,
}

/// `dst + dst · light`: light falls on what's there.
const LIGHT_ON: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// `dst · tint`.
const TINT: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

impl Painter {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fireworks"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fireworks.wgsl").into()),
        });
        let visible = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fireworks"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: visible,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(SLOT_BYTES),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: visible,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fireworks"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fireworks slots"),
            size: SLOT_STRIDE * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (items, bind) = Self::storage(device, &layout, &slots, 1024);
        Self {
            module,
            layout,
            pipeline_layout,
            pipes: HashMap::new(),
            slots,
            items,
            bind,
            frame: None,
            used: 0,
        }
    }

    fn storage(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: &wgpu::Buffer,
        items: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let items = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fireworks items"),
            size: items * (ITEM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fireworks"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: slots,
                        offset: 0,
                        size: wgpu::BufferSize::new(SLOT_BYTES),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: items.as_entire_binding(),
                },
            ],
        });
        (items, bind)
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
            Pipes {
                sparks: pipe(
                    "spark_vertex",
                    "spark_fragment",
                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                ),
                lit: pipe("area_vertex", "lit_fragment", LIGHT_ON),
                tinted: pipe("area_vertex", "tint_fragment", TINT),
            }
        })
    }

    /// Takes this frame's show and uploads its streaks; call before
    /// drawing.
    pub(crate) fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.used = 0;
        self.frame = SHOW.lock().ok().and_then(|g| g.clone());
        let Some(show) = &self.frame else {
            return;
        };
        let mut items: Vec<f32> = Vec::with_capacity(show.streaks.len() * ITEM_FLOATS);
        for s in &show.streaks {
            let r = s.half + s.glow * 3.0 + 1.0;
            items.extend_from_slice(&[
                s.from[0],
                s.from[1],
                s.to[0],
                s.to[1],
                s.color[0],
                s.color[1],
                s.color[2],
                s.tail,
                s.half,
                s.glow,
                0.0,
                0.0,
                s.from[0].min(s.to[0]) - r,
                s.from[1].min(s.to[1]) - r,
                s.from[0].max(s.to[0]) + r,
                s.from[1].max(s.to[1]) + r,
            ]);
        }
        let count = (items.len() / ITEM_FLOATS) as u64;
        let have = self.items.size() / (ITEM_FLOATS * 4) as u64;
        if count > have {
            (self.items, self.bind) =
                Self::storage(device, &self.layout, &self.slots, count.next_power_of_two());
        }
        if !items.is_empty() {
            queue.write_buffer(&self.items, 0, &floats(&items));
        }
    }

    /// Paints the slot at `site`: the sparks, or their light on the page.
    pub(crate) fn paint(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        site: &EffectSite<'_>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        let Some(show) = self.frame.as_ref() else {
            return;
        };
        let sparks = site.mark == SPARKS_MARK;
        let count = show.streaks.len() as u32;
        if (sparks && count == 0) || (!sparks && show.lights.is_empty()) || self.used >= 4 {
            return;
        }
        let mut slot: Vec<f32> = Vec::with_capacity(SLOT_BYTES as usize / 4);
        slot.extend_from_slice(&site.instance.rect);
        slot.extend_from_slice(&[
            site.origin[0],
            site.origin[1],
            site.size[0] as f32,
            site.size[1] as f32,
        ]);
        slot.extend_from_slice(&[
            site.scale,
            if site.srgb_blend { 1.0 } else { 0.0 },
            if show.dark { 1.0 } else { 0.0 },
            show.lights.len() as f32,
        ]);
        for i in 0..LIGHTS {
            match show.lights.get(i) {
                Some(l) => slot.extend_from_slice(&[
                    l.at[0], l.at[1], l.reach, l.strength, l.color[0], l.color[1], l.color[2], 0.0,
                ]),
                None => slot.extend_from_slice(&[0.0; 8]),
            }
        }
        let offset = self.used * SLOT_STRIDE;
        self.used += 1;
        queue.write_buffer(&self.slots, offset, &floats(&slot));
        let bind = self.bind.clone();
        let dark = show.dark;
        let pipes = self.pipes(device, site.format);
        pass.set_bind_group(0, &bind, &[offset as u32]);
        if sparks {
            pass.set_pipeline(&pipes.sparks);
            pass.draw(0..6, 0..count);
        } else {
            pass.set_pipeline(if dark { &pipes.lit } else { &pipes.tinted });
            pass.draw(0..6, 0..1);
        }
    }
}

/// The uniform `fireworks.wgsl` reads per slot: rect, view, info, then
/// [`LIGHTS`] lights of two vectors each.
const SLOT_BYTES: u64 = (3 + 2 * LIGHTS as u64) * 16;

#[cfg(test)]
mod tests {
    use super::*;

    fn paint() -> Paint {
        Paint::new(true, 2.0)
    }

    #[test]
    fn stars_fall_and_slow() {
        let a = thrown([0.0, 0.0], [300.0, -300.0], 0.3);
        let b = thrown([0.0, 0.0], [300.0, -300.0], 1.2);
        let c = thrown([0.0, 0.0], [300.0, -300.0], 2.4);
        assert!(b[0] > a[0] && c[0] - b[0] < b[0] - a[0], "drag slows it");
        assert!(c[1] > b[1], "and it falls");
    }

    #[test]
    fn the_show_bursts_lights_the_page_and_goes_out() {
        let p = paint();
        let rising = show(p, 0.2, false).unwrap();
        assert!(rising.streaks.len() < 50, "one shell on its way up");
        let burst = show(p, RISE + 0.05, false).unwrap();
        assert!(burst.streaks.len() > 200, "{}", burst.streaks.len());
        assert!(burst.lights.iter().any(|l| l.strength > 1.0));
        let last = SHELLS.iter().map(|s| s.delay).fold(0.0, f32::max);
        let after = show(p, last + RISE + 0.9, false).unwrap();
        assert!(
            after.lights.iter().all(|l| l.strength < 0.2),
            "the flash is short"
        );
        assert!(show(p, LIFE + 0.01, false).is_none());
    }

    #[test]
    fn bursts_stay_beside_the_conversation() {
        // The card spans the stage's middle; no burst sits over it.
        for shell in &SHELLS {
            let [x, _] = shell.burst;
            assert!(!(25.0..475.0).contains(&x), "{x}");
        }
    }

    #[test]
    fn reduce_motion_is_one_soft_flash() {
        let p = paint();
        let mut lit = 0.0f32;
        for i in 0..60 {
            let s = show(p, i as f32 * 0.05, true).unwrap_or_default();
            assert!(s.streaks.is_empty());
            lit = lit.max(s.lights.iter().map(|l| l.strength).fold(0.0, f32::max));
        }
        assert!(lit > 0.2 && lit < 1.0, "{lit}");
    }
}
