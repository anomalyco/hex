//! Dot scope drawn in Rune's effect slots, including a departing page's slot,
//! and `"With Hex"`'s fireworks ([`super::fireworks`]).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::dictation::indicator::DotFrame;
use crate::rune_hud::hud_placement::Layout;
use crate::rune_hud::{Painter, Target, WINDOW};
use crate::rune_ui::Ui;
use rune::compose::Frame;
use rune::render::EffectSite;

use super::fireworks::{self, is_fireworks_mark};

pub(crate) const KEY_COUNT: usize = 64;
const FIRST_MARK: u32 = 0x4800_0008;
const HUD_COUNT: u32 = 5;

pub(crate) fn hud_mark(key: &str) -> u32 {
    FIRST_MARK
        + match key {
            "download-hud" => 0,
            "waiting-hud" => 1,
            "onboard-dot-hud" => 2,
            "try-dot-hud" => 3,
            "game-dot-hud" => 4,
            _ => panic!("unknown onboarding HUD: {key}"),
        }
}

pub(crate) const fn is_onboard_mark(mark: u32) -> bool {
    mark >= FIRST_MARK && mark < FIRST_MARK + HUD_COUNT || is_fireworks_mark(mark)
}

// View-to-compositor handoff only. The compositor owns retained frames, so
// an outgoing page keeps its own waveform after the next page is built.
static UPDATES: Mutex<Vec<(u32, DotFrame)>> = Mutex::new(Vec::new());

pub(crate) fn update_hud(mark: u32, frame: DotFrame) {
    let mut updates = UPDATES.lock().expect("HUD handoff");
    if let Some((_, old)) = updates.iter_mut().find(|(id, _)| *id == mark) {
        *old = frame;
    } else {
        updates.push((mark, frame));
    }
}

struct Hud {
    frame: DotFrame,
    painters: HashMap<wgpu::TextureFormat, Painter>,
    seen: bool,
}

pub(crate) struct OnboardingGpu {
    format: wgpu::TextureFormat,
    huds: HashMap<u32, Hud>,
    fireworks: Option<fireworks::Painter>,
}

/// Builds, once per process and on a thread, the pipelines the first run
/// reaches for later: Rune's layer and backdrop blurs (the Launch Room's
/// opening) and the game's. Built cold, they cost the frame that first needs
/// them most of a second, and the motion on it jumps; built here and
/// dropped, the GPU driver's shader cache has them by then.
pub(crate) fn warm_later_pipelines(device: &wgpu::Device, format: wgpu::TextureFormat) {
    static WARMED: std::sync::Once = std::sync::Once::new();
    WARMED.call_once(|| {
        let device = device.clone();
        // Rune's isolated layers draw into this format.
        const LAYER: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
        let warm = move || {
            let mut renderer = rune::render::Renderer::new(&device, format);
            renderer.add_format(&device, LAYER);
            drop(renderer.first_use_ms(&device));
            drop(Painter::with_color_space(&device, LAYER, true));
            drop(super::game::fx::GameFx::new(&device));
            drop(fireworks::Painter::new(&device));
        };
        if let Err(error) = std::thread::Builder::new()
            .name("onboarding-gpu-warm".into())
            .spawn(warm)
        {
            tracing::debug!(%error, "could not warm the first run's pipelines");
        }
    });
}

impl OnboardingGpu {
    pub(crate) fn new(_: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            huds: HashMap::new(),
            fireworks: None,
        }
    }

    pub(crate) fn matches(&self, format: wgpu::TextureFormat) -> bool {
        self.format == format
    }

    /// The window compositor's one onboarding stage for `f`'s format,
    /// created again when the format changes.
    pub(crate) fn for_frame<'a>(slot: &'a mut Option<Self>, f: &Frame<'_, Ui>) -> &'a mut Self {
        if slot.as_ref().is_none_or(|g| !g.matches(f.format)) {
            *slot = Some(Self::new(f.device, f.format));
        }
        slot.as_mut().expect("created above")
    }

    pub(crate) fn render(&mut self, f: &mut Frame<'_, Ui>) {
        self.begin(f.device, f.queue);
        let clear = f.rt.clear_color();
        f.ui.prepare(
            f.device,
            f.queue,
            &mut f.rt.text.atlas,
            &f.rt.instances,
            f.size,
        );
        let dark = f.rt.env.system_dark;
        let (device, queue) = (f.device, f.queue);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            self.paint(pass, site, device, queue, dark);
        });
        self.end();
    }

    /// Takes this frame's HUD frames and fireworks; call before drawing.
    pub(crate) fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.fireworks
            .get_or_insert_with(|| fireworks::Painter::new(device))
            .begin(device, queue);
        for hud in self.huds.values_mut() {
            hud.seen = false;
        }
        for (mark, frame) in UPDATES.lock().expect("HUD handoff").drain(..) {
            match self.huds.get_mut(&mark) {
                Some(hud) => hud.frame = frame,
                None => {
                    self.huds.insert(
                        mark,
                        Hud {
                            frame,
                            painters: HashMap::new(),
                            seen: false,
                        },
                    );
                }
            }
        }
    }

    /// Paints the HUD at `site`, if it's one of onboarding's; another
    /// compositor's draw hook can call this for the slots it doesn't own.
    pub(crate) fn paint(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        site: &EffectSite<'_>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dark: bool,
    ) {
        if is_fireworks_mark(site.mark) {
            if let Some(fireworks) = self.fireworks.as_mut() {
                fireworks.paint(pass, site, device, queue);
            }
            return;
        }
        let Some(hud) = self.huds.get_mut(&site.mark) else {
            return;
        };
        hud.seen = true;
        let [x, y, w, h] = site.instance.rect;
        let [ox, oy] = site.origin;
        let [tw, th] = site.size.map(|v| v as f32);
        let [cx, cy, right, bottom] = site.instance.clip;
        let left = (cx.max(x) - ox).clamp(0.0, tw).floor();
        let top = (cy.max(y) - oy).clamp(0.0, th).floor();
        let right = (right.min(x + w) - ox).clamp(0.0, tw).ceil();
        let bottom = (bottom.min(y + h) - oy).clamp(0.0, th).ceil();
        if w <= 0.0 || h <= 0.0 || right <= left || bottom <= top {
            return;
        }
        let target = Target {
            viewport: [x - ox, y - oy, w, h],
            clip: [
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            ],
            scale: w / WINDOW.0,
            dark,
        };
        let painter = hud
            .painters
            .entry(site.format)
            .or_insert_with(|| Painter::with_color_space(device, site.format, !site.srgb_blend));
        let uniforms = painter.write(&hud.frame, Layout::default(), target, |buffer, bytes| {
            queue.write_buffer(buffer, 0, bytes)
        });
        painter.draw_slot(pass, target, &uniforms);
    }

    /// Drops the HUDs this frame didn't draw.
    pub(crate) fn end(&mut self) {
        self.huds.retain(|_, hud| hud.seen);
    }
}
