//! Flat UI + real `Dot scope` phosphor HUD compositor for onboarding:
//!
//! Draws the Rune UI flat as usual (`f.draw_ui()`), then composites Hex's real
//! [`crate::rune_hud::Painter`] (`rune_hud.wgsl`) over any mounted
//! [`HUD_SLOT_MARK`] element.

use std::sync::Mutex;

use rune::compose::Frame;
use rune::render::{KIND_EFFECT, KIND_LAYER};

use crate::dictation_indicator::DotFrame;
use crate::hud_placement::Layout;
use crate::rune_hud::{Painter, Target, WINDOW};
use crate::rune_ui::Ui;

pub(crate) const HUD_SLOT_MARK: u32 = 0x4800_0008;
pub(crate) const KEY_COUNT: usize = 64;

pub(crate) const fn is_onboard_mark(m: u32) -> bool {
    m == HUD_SLOT_MARK
}

static PENDING_HUD: Mutex<Option<DotFrame>> = Mutex::new(None);

pub(crate) fn set_dot_hud(frame: Option<DotFrame>) {
    if let Ok(mut g) = PENDING_HUD.lock() {
        *g = frame;
    }
}

pub(crate) struct OnboardingGpu {
    painter: Painter,
}

impl OnboardingGpu {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            painter: Painter::new(device, format),
        }
    }

    pub(crate) fn matches(&self, format: wgpu::TextureFormat) -> bool {
        self.painter.format == format
    }

    pub(crate) fn render(&mut self, f: &mut Frame<'_, Ui>) {
        f.draw_ui();

        let Some(drawn) = PENDING_HUD.lock().ok().and_then(|g| g.clone()) else {
            return;
        };
        let (tw, th) = (f.size.0 as f32, f.size.1 as f32);

        let found =
            f.rt.instances
                .iter()
                .zip(f.rt.marks.iter())
                .enumerate()
                .find(|(_, (ins, m))| **m == HUD_SLOT_MARK && ins.params[1] == KIND_EFFECT);
        let Some((idx, (ins, _))) = found else {
            return;
        };

        let px = ins.rect;
        if px[2] <= 1.0 || px[3] <= 1.0 {
            return;
        }
        let (clip, opacity) = match idx
            .checked_sub(1)
            .and_then(|prev| f.rt.instances.get(prev))
            .filter(|prev| prev.params[1] == KIND_LAYER)
        {
            Some(layer) => (layer.clip, layer.fill[3]),
            None => (ins.clip, ins.fill[3]),
        };
        if opacity <= 0.001 {
            return;
        }
        let left = clip[0].max(px[0]).clamp(0.0, tw);
        let top = clip[1].max(px[1]).clamp(0.0, th);
        let right = clip[2].min(px[0] + px[2]).clamp(0.0, tw);
        let bottom = clip[3].min(px[1] + px[3]).clamp(0.0, th);
        if right - left < 1.0 || bottom - top < 1.0 {
            return;
        }

        let mut frame = drawn;
        frame.pose.opacity *= opacity;
        let target = Target {
            viewport: px,
            clip: [
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            ],
            scale: px[2] / WINDOW.0,
            dark: f.rt.env.system_dark,
        };
        let uniforms = self
            .painter
            .write(&frame, Layout::default(), target, |buffer, data| {
                f.write_buffer(buffer, 0, data)
            });
        self.painter
            .draw_over(f.encoder, f.target, target, &uniforms);
    }
}
