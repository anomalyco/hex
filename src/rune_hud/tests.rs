use std::time::{Duration, Instant};

use super::*;
use crate::dot_scope::Tuning;

#[test]
fn dot_uniform_layout_matches_wgsl() {
    assert_eq!(std::mem::offset_of!(DotUniforms, dark), 60);
    assert_eq!(std::mem::offset_of!(DotUniforms, base), 64);
    assert_eq!(std::mem::offset_of!(DotUniforms, shadow), 128);
    assert_eq!(std::mem::offset_of!(DotUniforms, plate_layout), 144);
    assert_eq!(std::mem::offset_of!(DotUniforms, island), 160);
}

#[test]
fn dot_scope_window_fits_the_largest_plate() {
    let largest = |key| {
        dot_scope::KNOBS
            .iter()
            .find(|knob| knob.key == key)
            .map(|knob| knob.max)
            .unwrap()
    };
    let widest = largest("dot-width").max(largest("dot-processing-width"));
    assert!(widest + 24.0 <= DOT_SCOPE_WINDOW.0);
    // The plate's middle sits in the window's; its shadow reaches about
    // 16 pt below it.
    assert!(largest("dot-height") / 2.0 + 19.0 <= DOT_SCOPE_WINDOW.1 / 2.0);
}

/// The macOS HUD's Dot scope window, which the captures use on every OS so
/// they line up with the Metal reference frames.
const CAPTURE_DOT_WINDOW: (f32, f32) = (320.0, 128.0);

/// A dictation, as the listener reports it, at 60 fps: HUD Lab's synthetic
/// voice for 3 s, then transcribing, processing, and done.
fn schedule(model: &mut IndicatorModel, start: Instant) -> Vec<Drawn> {
    let mut events = vec![
        (0.0, HudEvent::Started),
        (3.0, HudEvent::Submitted { job_id: 1 }),
        (3.6, HudEvent::Processing { job_id: 1 }),
        (4.4, HudEvent::JobCompleted { job_id: 1 }),
    ];
    events.reverse();
    (0..330)
        .map(|n| {
            let t = n as f32 / 60.0;
            let now = start + Duration::from_secs_f32(t);
            while events.last().is_some_and(|(at, _)| *at <= t) {
                model.handle(events.pop().unwrap().1, now, false);
            }
            if t < 3.0 {
                // A microphone's RMS for the demo voice's loudness.
                let rms = 10_f32.powf(dot_scope::demo_loudness(t) / 20.0);
                let meter = HudEvent::Meter {
                    average: rms,
                    peak: (rms * 3.0).min(1.0),
                };
                model.handle(meter, now, false);
            }
            model.step(now, 1.0 / 60.0, false)
        })
        .collect()
}

/// The layout a 14-inch MacBook Pro gives `tuning`'s placement.
fn macbook_layout(tuning: Tuning) -> Layout {
    let screen = crate::hud_placement::Screen {
        frame: crate::hud_placement::Rect {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 982.0,
        },
        visible: crate::hud_placement::Rect {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 950.0,
        },
        safe_top: 32.0,
        aux_left: 663.5,
        aux_right: 663.5,
        bar: 24.0,
    };
    let (width, height) = CAPTURE_DOT_WINDOW;
    crate::hud_placement::place(
        tuning.placement,
        &screen,
        crate::hud_placement::Content {
            window: (f64::from(width), f64::from(height)),
            plate: (f64::from(width) - 24.0, f64::from(dot_scope::PLATE_HEIGHT)),
            island_spread: f64::from(tuning.island_spread),
        },
        false,
    )
    .layout
}

/// Renders a dictation through the real shaders offscreen, as PNG frames
/// at 60 fps over a desktop colour, from a fixed schedule:
///
/// ```sh
/// HEX_HUD_FRAMES=/tmp/frames HEX_HUD_DOT_SCOPE=1 \
///   cargo test --bin hex rune_hud_frames -- --ignored
/// ```
///
/// `HEX_HUD_DOT_SCOPE=1` draws Dot scope (`HEX_HUD_TUNING` overrides its
/// tuning as JSON, such as `{"processing": "pulse", "placement": "notch"}`,
/// placed on a pretend 14-inch MacBook Pro), otherwise the capsule;
/// `HEX_HUD_THEME=light` a light desktop. `HEX_HUD_DUMP=<file>` also writes
/// every frame's shader inputs, for rendering the same frames elsewhere.
#[test]
#[ignore = "writes frames for a review clip"]
fn rune_hud_frames() {
    let out = std::path::PathBuf::from(std::env::var("HEX_HUD_FRAMES").expect("HEX_HUD_FRAMES"));
    std::fs::create_dir_all(&out).unwrap();
    let dots = std::env::var("HEX_HUD_DOT_SCOPE").as_deref() == Ok("1");
    let dark = std::env::var("HEX_HUD_THEME").as_deref() != Ok("light");
    let mut fields = serde_json::to_value(Tuning::default()).unwrap();
    if let Ok(json) = std::env::var("HEX_HUD_TUNING") {
        let overrides: serde_json::Value = serde_json::from_str(&json).unwrap();
        for (key, value) in overrides.as_object().unwrap() {
            fields[key] = value.clone();
        }
    }
    let tuning: Tuning = serde_json::from_value(fields).unwrap();
    let start = Instant::now();
    let mut model = IndicatorModel::new(dots.then_some(tuning), start);
    let frames = schedule(&mut model, start);
    let layout = if dots {
        macbook_layout(tuning)
    } else {
        Layout::default()
    };

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("a GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let gpu = Gpu::new(&device, format);
    let scale = 2.0;
    let window = if dots {
        CAPTURE_DOT_WINDOW
    } else {
        (WINDOW_WIDTH, WINDOW_HEIGHT)
    };
    let size = ((window.0 * scale) as u32, (window.1 * scale) as u32);
    let extent = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let row = (size.0 * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let target = Target { size, scale, dark };
    let mut dump = Vec::new();
    dump.extend(size.0.to_le_bytes());
    dump.extend(size.1.to_le_bytes());
    for (n, drawn) in frames.iter().enumerate() {
        match drawn {
            Drawn::Capsule(uniforms) => {
                let uniforms = Uniforms {
                    resolution: [size.0 as f32, size.1 as f32],
                    ..*uniforms
                };
                dump.push(0);
                dump.extend(bytes(&uniforms));
            }
            Drawn::Dots(frame) => {
                dump.push(1);
                dump.extend(bytes(&DotUniforms::new(frame, layout, target)));
                let mut packed = packed_dots(frame);
                packed.resize(DOT_BYTES as usize, 0);
                dump.extend(packed);
            }
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        gpu.draw(&queue, &mut encoder, &view, drawn, layout, target);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            extent,
        );
        queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = readback.slice(..).get_mapped_range().unwrap();
        let mut rgba = Vec::with_capacity((size.0 * size.1 * 4) as usize);
        for y in 0..size.1 {
            rgba.extend_from_slice(&mapped[(y * row) as usize..][..(size.0 * 4) as usize]);
        }
        drop(mapped);
        readback.unmap();
        write_over_desktop(&out.join(format!("{n:04}.png")), size, &rgba, dark);
    }
    if let Ok(path) = std::env::var("HEX_HUD_DUMP") {
        std::fs::write(path, dump).unwrap();
    }
}

/// Premultiplied RGBA over the desktop colour, as an RGB PNG.
fn write_over_desktop(path: &std::path::Path, size: (u32, u32), rgba: &[u8], dark: bool) {
    let desktop: [f32; 3] = if dark {
        [0.11, 0.11, 0.13]
    } else {
        [0.93, 0.93, 0.94]
    };
    let rgb: Vec<u8> = rgba
        .chunks(4)
        .flat_map(|p| {
            let a = f32::from(p[3]) / 255.0;
            (0..3).map(move |c| {
                let v = f32::from(p[c]) / 255.0 + desktop[c] * (1.0 - a);
                (v * 255.0).round().clamp(0.0, 255.0) as u8
            })
        })
        .collect();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(file, size.0, size.1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgb)
        .unwrap();
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        std::thread::yield_now();
    }
}
