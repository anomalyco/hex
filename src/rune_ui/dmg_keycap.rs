//! The installer window's Option keycap (`packaging/macos/dmg`): Hex's own
//! dark keycap, alone, as transparent PNGs. `packaging/macos/dmg/render.sh`
//! runs it; it never runs in the suite.

use super::controls::{KeySize, keycap};
use super::*;
use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
use rune::text::TextSystem;
use rune::{Env, Runtime};

/// The key, centred, with room for its shadow.
const SIZE: (f32, f32) = (56.0, 48.0);

fn on(fill: rune::Color, cx: &mut Cx<Ui>) -> El {
    let p = Paint::new(true, cx.env().scale);
    col()
        .size(SIZE.0, SIZE.1)
        .fill(fill)
        .center()
        .child(keycap(p, 0, "⌥", KeySize::Large, false))
}

fn on_black(_: &Ui, cx: &mut Cx<Ui>) -> El {
    on(rune::Color::BLACK, cx)
}

fn on_white(_: &Ui, cx: &mut Cx<Ui>) -> El {
    on(rune::Color::WHITE, cx)
}

fn draw(gpu: &Gpu, view: fn(&Ui, &mut Cx<Ui>) -> El, scale: f32) -> (u32, u32, Vec<u8>) {
    let state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    let env = Env {
        system_dark: true,
        scale,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = Runtime::with_text(state, view, SIZE, env, text);
    rt.settle_all();
    rt.frame(0.0);
    let (width, height) = ((SIZE.0 * scale) as u32, (SIZE.1 * scale) as u32);
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    let target = Offscreen::new(&gpu.device, width, height);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (width, height),
    );
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    (width, height, target.read(&gpu.device))
}

/// Recovers each pixel's alpha from the key drawn over black and over white.
fn matte(black: &[u8], white: &[u8]) -> Vec<u8> {
    black
        .as_chunks::<4>()
        .0
        .iter()
        .zip(white.as_chunks::<4>().0)
        .flat_map(|(b, w)| {
            let spread = (0..3)
                .map(|i| i32::from(w[i]) - i32::from(b[i]))
                .sum::<i32>()
                / 3;
            let alpha = 255 - spread.clamp(0, 255);
            if alpha == 0 {
                return [0; 4];
            }
            let channel = |c: u8| (i32::from(c) * 255 / alpha).min(255) as u8;
            [channel(b[0]), channel(b[1]), channel(b[2]), alpha as u8]
        })
        .collect()
}

/// Writes `packaging/macos/dmg/keycap@1x.png` and `keycap@2x.png`, drawn at
/// the 1.5× the background shows the key at.
#[test]
#[ignore = "run by packaging/macos/dmg/render.sh"]
fn render() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/macos/dmg");
    let gpu = Gpu::try_new(Gpu::instance(), None).expect("a GPU");
    for (name, scale) in [("keycap@1x.png", 1.5), ("keycap@2x.png", 3.0)] {
        let (width, height, black) = draw(&gpu, on_black, scale);
        let (_, _, white) = draw(&gpu, on_white, scale);
        rune::render::write_png(&out.join(name), width, height, &matte(&black, &white))
            .expect("the keycap PNG is written");
    }
}
