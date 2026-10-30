//! Test renders drawn as Hex's windows and overlays draw them: sRGB
//! blending into a non-sRGB target. Theme colours resolve for the blend mode
//! when the view is built, so a runtime turns it on before its first frame.

use rune::render::{OFFSCREEN_FORMAT, Offscreen, Renderer};
use rune::text::TextSystem;
use rune::{Cx, El, Env, Runtime};

pub(crate) fn format() -> wgpu::TextureFormat {
    OFFSCREEN_FORMAT.remove_srgb_suffix()
}

pub(crate) fn runtime<S: 'static>(
    state: S,
    view: fn(&S, &mut Cx<S>) -> El,
    size: (f32, f32),
    env: Env,
    text: TextSystem,
) -> Runtime<S> {
    let mut rt = Runtime::with_text(state, view, size, env, text);
    rt.set_srgb_blend(true);
    rt
}

pub(crate) fn renderer(device: &wgpu::Device) -> Renderer {
    let mut renderer = Renderer::new(device, format());
    renderer.set_srgb_blend(true);
    renderer
}

pub(crate) fn target(device: &wgpu::Device, width: u32, height: u32) -> Offscreen {
    Offscreen::with_format(device, width, height, format())
}
