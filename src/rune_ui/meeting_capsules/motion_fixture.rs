// A silent, deterministic replay through the overlay's actual view and pointer handler.
// Only the two level readings, the elapsed clock and the pointer are synthetic.
fn fixture_levels(frame: usize) -> [f32; 2] {
    let t = frame as f32 / 60.0;
    let phrase = |speed: f32, beat: f32| ((t * speed).sin().abs() * (t * beat).cos().abs()).sqrt();
    [
        if (18..108).contains(&frame) || (216..300).contains(&frame) {
            0.12 * phrase(13.0, 2.8).powi(3)
        } else {
            0.0
        },
        if (120..300).contains(&frame) {
            0.09 * phrase(6.3, 4.1).powi(3)
        } else {
            0.0
        },
    ]
}

#[test]
fn recording_motion_frames() {
    use crate::rune_ui::offscreen;
    use std::fmt::Write;
    let Some(folder) = std::env::var_os("HEX_PILL_FRAMES") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
    let (w, h) = ((2.0 * RECORDING.0) as u32, (2.0 * RECORDING.1) as u32);
    for dark in [true, false] {
        let theme = if dark { "dark" } else { "light" };
        let dir = folder.join(theme);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut rt, _, _) = runtime(live_recording("fixture"), false, dark, RECORDING);
        rt.transparent = true;
        rt.update(|c, _| {
            c.live.entries = call();
            c.live.pending = meeting::PendingDelta {
                turns: 2,
                span_ms: 64_000,
            };
        });
        rt.settle_all();
        rt.frame(0.0);
        let mut renderer = offscreen::renderer(&gpu.device);
        let target = offscreen::target(&gpu.device, w, h);
        use crate::{
            dictation::indicator::{DictationIndicatorEvent as Event, IndicatorModel},
            rune_hud::{Painter, Target, dot_scope::Tuning, hud_placement::Layout},
        };
        let start = Instant::now();
        let mut hud = IndicatorModel::new(Tuning::default(), start);
        hud.handle(Event::Started, start, false);
        rt.state.waveform = Some(Box::new(waveform::Listening::new(start)));
        let mut ink = waveform::Ink::default();
        let hud_painter = Painter::new(&gpu.device, offscreen::format());
        let hud_target = offscreen::target(&gpu.device, w, h);
        let target_rect = Target {
            viewport: [0., 0., w as f32, h as f32],
            clip: [0, 0, w, h],
            scale: 2.,
        };
        let mut trace = String::from("frame,hovered,key,x,y,w,h\n");
        for frame in 0..366 {
            // Put the fixture on the same second at each event, independent of GPU speed.
            if let Some(Kind::Recording(shown)) = &mut rt.state.kind {
                shown.started = Instant::now() - Duration::from_secs(258 + frame as u64 / 60);
            }
            if frame % 60 == 0 {
                rt.update(|_, _| {});
            }
            let now = start + Duration::from_secs_f64(frame as f64 / 60.0);
            let levels = fixture_levels(frame);
            let rms = levels[0].max(levels[1]);
            hud.handle(
                Event::Meter {
                    average: rms,
                    peak: rms,
                },
                now,
                false,
            );
            let hud_frame = hud.step(now, 1. / 60., false);
            rt.state.waveform.as_mut().unwrap().sample(now, levels);
            match frame {
                66 | 83 => rt.pointer_move(RECORDING.0 - 22.0, 22.0),
                72 | 249 => rt.pointer_leave(),
                _ => (),
            }
            rt.frame(1.0 / 60.0);
            for elem in rt.elements() {
                if let Some(key @ ("pill" | "elapsed" | "copy-new" | "meeting-listening-field")) =
                    elem.key.as_deref()
                {
                    let [x, y, w, h] = elem.rect;
                    writeln!(trace, "{frame},{},{key},{x},{y},{w},{h}", rt.state.hovered).unwrap();
                }
            }
            {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                let uniforms =
                    hud_painter.write(&hud_frame, Layout::default(), target_rect, |b, d| {
                        gpu.queue.write_buffer(b, 0, d)
                    });
                hud_painter.draw(&mut encoder, &hud_target.view, &uniforms);
                hud_target.copy(&mut encoder);
                gpu.queue.submit([encoder.finish()]);
                rune::render::write_png(
                    &dir.join(format!("hud-{frame:03}.png")),
                    w,
                    h,
                    &hud_target.read(&gpu.device),
                )
                .unwrap();
                let rgba = paint_recording(&gpu, &mut renderer, &target, &mut rt, &mut ink);
                rune::render::write_png(&dir.join(format!("{frame:03}.png")), w, h, &rgba).unwrap();
            }
        }
        std::fs::write(dir.join("trace.csv"), trace).unwrap();
    }
}

fn fixture_scope(capsule: &mut Capsule) {
    let start = Instant::now();
    let mut wave = waveform::Listening::new(start);
    for frame in 0..=54 {
        wave.sample(
            start + Duration::from_secs_f64(frame as f64 / 60.),
            fixture_levels(frame),
        );
    }
    capsule.waveform = Some(Box::new(wave));
}

#[test]
fn a_light_host_draws_the_dark_recording_from_the_first_frame() {
    use crate::rune_ui::offscreen;
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        return;
    };
    let (w, h) = ((2.0 * RECORDING.0) as u32, (2.0 * RECORDING.1) as u32);
    for open in [false, true] {
        let (mut rt, _, _) = runtime(live_recording("fixture"), false, false, RECORDING);
        if open {
            open_recording(&mut rt);
        }
        let mut renderer = offscreen::renderer(&gpu.device);
        let target = offscreen::target(&gpu.device, w, h);
        let mut ink = waveform::Ink::default();
        let first = paint_recording(&gpu, &mut renderer, &target, &mut rt, &mut ink);
        // Opaque interior, above the waveform, outside the antialiased rim.
        let i = 4 * (15 * w as usize + 365);
        assert!(first[i..i + 3].iter().all(|c| *c < 40) && first[i + 3] == 255);
        rt.set_system_dark(true);
        for _ in 0..16 {
            rt.frame(1.0 / 60.0);
            // The elapsed label reads the wall clock, and 17 software renders
            // can straddle a second on a slow runner: hold the clock so only
            // the host change can alter the pixels.
            if let Some(Kind::Recording(shown)) = &mut rt.state.kind {
                shown.started = Instant::now() - Duration::from_secs(258);
            }
            let frame = paint_recording(&gpu, &mut renderer, &target, &mut rt, &mut ink);
            assert!(
                frame == first,
                "host change recoloured the recording (open={open})"
            );
        }
    }
}

#[test]
fn the_timer_and_saved_corner_hold_through_a_hover_reversal() {
    use MeetingIndicatorCorner::*;
    for corner in [TopRight, TopLeft, BottomRight, BottomLeft] {
        let (mut rt, _, _) = runtime(live_recording("fixture"), false, true, RECORDING);
        rt.update(move |c, tx| {
            c.corner = corner;
            tx.instant();
        });
        rt.frame(0.0);
        let anchor = |rt: &Runtime<Capsule>| {
            let [x, y, w, h] = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("pill"))
                .unwrap()
                .rect;
            (
                if corner.is_right() { x + w } else { x },
                if corner.is_top() { y } else { y + h },
            )
        };
        let initial = anchor(&rt);
        let timer = rt.find("elapsed").unwrap();
        rt.pointer_move(timer.0, timer.1);
        for i in 0..32 {
            if i == 6 {
                rt.pointer_leave();
            }
            if i == 14 {
                rt.pointer_move(timer.0, timer.1);
            }
            rt.frame(1.0 / 60.0);
            let (x, y) = anchor(&rt);
            assert!(
                (x - initial.0).abs() < 0.02 && (y - initial.1).abs() < 0.02,
                "{corner:?} at {i}"
            );
            let (x, y) = rt.find("elapsed").unwrap();
            assert!(
                (x - timer.0).abs() < 0.02 && (y - timer.1).abs() < 0.02,
                "{corner:?} timer at {i}"
            );
        }
    }
}

fn paint_recording(
    gpu: &rune::render::Gpu,
    ui: &mut rune::render::Renderer,
    target: &rune::render::Offscreen,
    rt: &mut Runtime<Capsule>,
    ink: &mut waveform::Ink,
) -> Vec<u8> {
    use rune::compose::{Compositor, Frame};
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ink.render(&mut Frame {
        device: &gpu.device,
        queue: &gpu.queue,
        encoder: &mut encoder,
        target: &target.view,
        format: crate::rune_ui::offscreen::format(),
        size: (target.width, target.height),
        dt: 1. / 60.,
        update_ms: 0.,
        rt,
        ui,
    });
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    target.read(&gpu.device)
}
