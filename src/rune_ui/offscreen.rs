//! Renders drawn as Hex's windows and overlays draw them, for tests and the
//! screenshot editor's export: sRGB blending into a non-sRGB target. Theme
//! colours resolve for the blend mode when the view is built, so a runtime
//! turns it on before its first frame.

#![cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]

use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
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
    // As the Linux and Windows apps ask (`rune_ui::window_args`).
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    rt.set_system_scroll_bars(rune::ScrollBars::Always);
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

/// `rt` settled and drawn into `width` × `height` pixels: their RGBA.
pub(crate) fn draw<S: 'static>(gpu: &Gpu, rt: &mut Runtime<S>, width: u32, height: u32) -> Vec<u8> {
    rt.settle_all();
    rt.frame(0.0);
    let mut renderer = renderer(&gpu.device);
    paint(gpu, &mut renderer, &target(&gpu.device, width, height), rt)
}

/// `rt`'s last frame as it stands, mid-animation included, drawn into
/// `target`: its RGBA. Keep `renderer` across a clip's frames; it holds the
/// glyphs the text atlas uploaded on earlier ones.
pub(crate) fn paint<S: 'static>(
    gpu: &Gpu,
    renderer: &mut Renderer,
    target: &Offscreen,
    rt: &mut Runtime<S>,
) -> Vec<u8> {
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (target.width, target.height),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    target.read(&gpu.device)
}

#[cfg(test)]
pub(crate) use golden::compare as compare_golden;

#[cfg(test)]
mod golden {
    use std::path::{Path, PathBuf};

    /// Per-channel delta tolerated between GPU drivers for blending and glyph
    /// coverage differences; a layout or colour change moves far more.
    const CHANNEL_TOLERANCE: u8 = 12;

    /// At most `1 / MISMATCH_DIVISOR` (0.2%) of pixels may exceed
    /// `CHANNEL_TOLERANCE`.
    const MISMATCH_DIVISOR: usize = 500;

    fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(info.buffer_size());
        (info.width, info.height, pixels)
    }

    /// Compare an offscreen RGBA render against its checked-in PNG golden.
    ///
    /// - When `HEX_GOLDEN_RENDERS` is set, writes `{name}.png` there too.
    /// - When `HEX_UPDATE_GOLDENS` is set, writes `golden_path` and returns `Ok(())`.
    /// - Otherwise compares against `golden_path` (tolerance `CHANNEL_TOLERANCE`,
    ///   limit `width * height / MISMATCH_DIVISOR`); on missing golden, size change,
    ///   or pixel drift, writes `<temp_dir>/rune-settings-{name}-actual.png` (picked
    ///   up by CI's `golden-actual` and `onboarding-scenes` artifacts) and returns
    ///   the failure line.
    pub(crate) fn compare(
        name: &str,
        golden_path: &Path,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        if let Some(folder) = std::env::var_os("HEX_GOLDEN_RENDERS") {
            let folder = PathBuf::from(folder);
            std::fs::create_dir_all(&folder).unwrap();
            rune::render::write_png(&folder.join(format!("{name}.png")), width, height, pixels)
                .unwrap();
        }
        if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
            if let Some(parent) = golden_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            rune::render::write_png(golden_path, width, height, pixels).unwrap();
            return Ok(());
        }
        let actual_path = std::env::temp_dir().join(format!("rune-settings-{name}-actual.png"));
        compare_with_actual(name, golden_path, &actual_path, width, height, pixels)
    }

    fn compare_with_actual(
        name: &str,
        golden_path: &Path,
        actual_path: &Path,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        if !golden_path.exists() {
            rune::render::write_png(actual_path, width, height, pixels).unwrap();
            return Err(format!(
                "{} is missing; run with HEX_UPDATE_GOLDENS=1 (render at {})",
                golden_path.display(),
                actual_path.display()
            ));
        }
        let (golden_width, golden_height, golden) = read_png(golden_path);
        if (golden_width, golden_height) != (width, height) {
            rune::render::write_png(actual_path, width, height, pixels).unwrap();
            return Err(format!(
                "{name}: size changed ({golden_width}x{golden_height} -> {width}x{height}); actual render at {}",
                actual_path.display()
            ));
        }
        let differing = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(golden.as_chunks::<4>().0)
            .filter(|(a, b)| {
                a.iter()
                    .zip(b.iter())
                    .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE)
            })
            .count();
        let limit = (width as usize * height as usize) / MISMATCH_DIVISOR;
        if differing > limit {
            rune::render::write_png(actual_path, width, height, pixels).unwrap();
            return Err(format!(
                "{name}: {differing} pixels differ (limit {limit}); actual render at {}",
                actual_path.display()
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
            rgba.into_iter()
                .cycle()
                .take((width * height * 4) as usize)
                .collect()
        }

        struct TempDir(PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn compare_enforces_tolerance_limit_size_and_missing_golden() {
            let dir = TempDir(
                std::env::temp_dir().join(format!("hex-golden-test-{}", std::process::id())),
            );
            let _ = std::fs::remove_dir_all(&dir.0);
            std::fs::create_dir_all(&dir.0).unwrap();
            let golden = dir.0.join("rune-sample-dark.png");
            let actual = dir.0.join("actual.png");
            let (w, h) = (50, 20); // 1,000 pixels -> limit = 2
            let base = solid(w, h, [100, 120, 140, 255]);

            // Missing golden writes actual and uses the message format check-onboarding.sh expects.
            let missing = compare_with_actual("sample-dark", &golden, &actual, w, h, &base)
                .expect_err("missing golden fails");
            assert!(
                missing.contains("rune-sample-dark.png is missing"),
                "{missing}"
            );
            assert!(actual.exists());
            std::fs::remove_file(&actual).unwrap();

            // Exact match passes without writing actual.
            rune::render::write_png(&golden, w, h, &base).unwrap();
            assert_eq!(
                compare_with_actual("sample-dark", &golden, &actual, w, h, &base),
                Ok(())
            );
            assert!(!actual.exists());

            // Within per-channel tolerance (<= 12) passes.
            let close = solid(w, h, [112, 108, 152, 255]);
            assert_eq!(
                compare_with_actual("sample-dark", &golden, &actual, w, h, &close),
                Ok(())
            );

            // Exactly at the mismatch limit (2 pixels) passes.
            let mut at_limit = base.clone();
            at_limit[0] = 200;
            at_limit[4] = 200;
            assert_eq!(
                compare_with_actual("sample-dark", &golden, &actual, w, h, &at_limit),
                Ok(())
            );

            // Over the limit (3 pixels) fails with the expected prefix and writes actual.
            at_limit[8] = 200;
            let drift = compare_with_actual("sample-dark", &golden, &actual, w, h, &at_limit)
                .expect_err("drift over limit fails");
            assert!(
                drift.starts_with("sample-dark: 3 pixels differ (limit 2);"),
                "{drift}"
            );
            assert!(actual.exists());
            std::fs::remove_file(&actual).unwrap();

            // Size mismatch fails and writes actual.
            let resized = solid(40, 20, [100, 120, 140, 255]);
            let size_err = compare_with_actual("sample-dark", &golden, &actual, 40, 20, &resized)
                .expect_err("size mismatch fails");
            assert!(
                size_err.contains("size changed (50x20 -> 40x20)"),
                "{size_err}"
            );
            assert!(actual.exists());
        }
    }
}
