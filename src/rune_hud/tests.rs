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
    assert_eq!(std::mem::offset_of!(DotUniforms, field), 176);
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
    assert!(widest + 24.0 <= WINDOW.0);
    // The plate's middle sits in the window's; its shadow reaches about
    // 16 pt below it.
    assert!(largest("dot-height") / 2.0 + 19.0 <= WINDOW.1 / 2.0);
}

/// The macOS HUD's window, which the captures use on every OS so they line
/// up with the Metal reference frames.
const CAPTURE_DOT_WINDOW: (f32, f32) = (320.0, 128.0);

/// A dictation, as the listener reports it, at 60 fps: HUD Lab's synthetic
/// voice for 3 s, then transcribing, processing, and done.
fn schedule(model: &mut IndicatorModel, start: Instant) -> Vec<DotFrame> {
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

/// A 1512 × 982 point 14-inch MacBook Pro: a 32 pt notch 185 pt wide.
fn macbook() -> crate::hud_placement::Screen {
    crate::hud_placement::Screen {
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
    }
}

/// An Apple Studio Display at its default 2560 × 1440 points: no notch, a
/// 24 pt menu bar.
fn studio_display() -> crate::hud_placement::Screen {
    crate::hud_placement::Screen {
        frame: crate::hud_placement::Rect {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
        },
        visible: crate::hud_placement::Rect {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1416.0,
        },
        bar: 24.0,
        ..Default::default()
    }
}

/// Where `screen` puts the capture window for `tuning`'s placement.
fn place_on(screen: &crate::hud_placement::Screen, tuning: Tuning) -> crate::hud_placement::Placed {
    let (width, height) = CAPTURE_DOT_WINDOW;
    crate::hud_placement::place(
        tuning.placement,
        screen,
        crate::hud_placement::Content {
            window: (f64::from(width), f64::from(height)),
            plate: (f64::from(width) - 24.0, f64::from(dot_scope::PLATE_HEIGHT)),
            island_spread: f64::from(tuning.island_spread),
        },
        false,
    )
}

/// What the screen shows behind the window.
#[derive(Clone, Copy)]
struct Backdrop {
    /// How far the window's top sits below the screen's, pt.
    drop: f32,
    /// The menu bar's height.
    bar: f32,
    /// The notch's width and height, if any.
    notch: Option<(f32, f32)>,
}

fn backdrop(
    screen: &crate::hud_placement::Screen,
    placed: &crate::hud_placement::Placed,
) -> Backdrop {
    Backdrop {
        drop: (screen.frame.y + screen.frame.height - placed.top_left.1) as f32,
        bar: screen.bar_height() as f32,
        notch: screen
            .notch()
            .map(|notch| (notch.width as f32, notch.height as f32)),
    }
}

/// Renders a dictation through the real shaders offscreen, as PNG frames
/// at 60 fps over a desktop colour, from a fixed schedule:
///
/// ```sh
/// HEX_HUD_FRAMES=/tmp/frames cargo test --bin hex rune_hud_frames -- --ignored
/// ```
///
/// `HEX_HUD_TUNING` overrides Dot scope's tuning as JSON, such as
/// `{"processing": "pulse", "placement": "notch"}`, placed on a 14-inch
/// MacBook Pro, or with `HEX_HUD_SCREEN=studio` on a Studio Display (no
/// notch); `HEX_HUD_THEME=light` draws over a light desktop. `HEX_HUD_DUMP=<file>` also writes
/// every frame's shader inputs, for rendering the same frames elsewhere.
#[test]
#[ignore = "writes frames for a review clip"]
fn rune_hud_frames() {
    let out = std::path::PathBuf::from(std::env::var("HEX_HUD_FRAMES").expect("HEX_HUD_FRAMES"));
    std::fs::create_dir_all(&out).unwrap();
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
    let screen = if std::env::var("HEX_HUD_SCREEN").as_deref() == Ok("studio") {
        studio_display()
    } else {
        macbook()
    };
    let placed = place_on(&screen, tuning);
    let layout = placed.layout;
    let backdrop = backdrop(&screen, &placed);
    let mut model = IndicatorModel::new(tuning, start);
    model.set_island(layout.island);
    let frames = schedule(&mut model, start);

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("a GPU adapter");
    // `HEX_HUD_BUFFERS=1` draws through the uniform buffer, as a device
    // without immediates does.
    let features = if std::env::var("HEX_HUD_BUFFERS").as_deref() == Ok("1") {
        wgpu::Features::empty()
    } else {
        adapter.features() & Painter::FEATURES
    };
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: wgpu::Limits {
            max_immediate_size: if features.is_empty() {
                0
            } else {
                adapter.limits().max_immediate_size
            },
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let gpu = Painter::new(&device, format);
    let scale = 2.0;
    let window = CAPTURE_DOT_WINDOW;
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
    let target = Target::whole(size, scale, dark);
    let mut dump = Vec::new();
    dump.extend(size.0.to_le_bytes());
    dump.extend(size.1.to_le_bytes());
    for (n, frame) in frames.iter().enumerate() {
        dump.extend(bytes(&DotUniforms::new(frame, layout, target)));
        let mut packed = packed_dots(frame);
        packed.resize(DOT_BYTES as usize, 0);
        dump.extend(packed);
        let mut encoder = device.create_command_encoder(&Default::default());
        let uniforms = gpu.write(frame, layout, target, |buffer, data| {
            queue.write_buffer(buffer, 0, data)
        });
        gpu.draw(&mut encoder, &view, &uniforms);
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
        write_over_desktop(
            &out.join(format!("{n:04}.png")),
            size,
            scale,
            &rgba,
            dark,
            backdrop,
        );
    }
    if let Ok(path) = std::env::var("HEX_HUD_DUMP") {
        std::fs::write(path, dump).unwrap();
    }
}

/// Premultiplied RGBA over the desktop colour, the menu bar, and the notch
/// (`backdrop`), as an RGB PNG.
fn write_over_desktop(
    path: &std::path::Path,
    size: (u32, u32),
    scale: f32,
    rgba: &[u8],
    dark: bool,
    backdrop: Backdrop,
) {
    let (desktop, menu_bar): ([f32; 3], [f32; 3]) = if dark {
        ([0.11, 0.11, 0.13], [0.2, 0.2, 0.22])
    } else {
        ([0.93, 0.93, 0.94], [0.98, 0.98, 0.98])
    };
    let behind = |x: u32, y: u32| {
        // In points from the screen's top middle.
        let px = (x as f32 + 0.5) / scale - size.0 as f32 / scale / 2.0;
        let py = (y as f32 + 0.5) / scale + backdrop.drop;
        if let Some((width, height)) = backdrop.notch {
            // The notch's bottom corners are rounded 8 pt.
            let radius = 8.0;
            let dx = (px.abs() - (width / 2.0 - radius)).max(0.0);
            let dy = (py - (height - radius)).max(0.0);
            if px.abs() <= width / 2.0 && py <= height && dx.hypot(dy) <= radius {
                return [0.0; 3];
            }
        }
        if py < backdrop.bar { menu_bar } else { desktop }
    };
    let rgb: Vec<u8> = rgba
        .chunks(4)
        .enumerate()
        .flat_map(|(i, p)| {
            let under = behind(i as u32 % size.0, i as u32 / size.0);
            let a = f32::from(p[3]) / 255.0;
            (0..3).map(move |c| {
                let v = f32::from(p[c]) / 255.0 + under[c] * (1.0 - a);
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
