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
        rt.update(|c, _| c.live.entries = call());
        landed(&mut rt);
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
                if let Some(key @ ("pill" | "elapsed" | "copy-prompt" | "meeting-listening-field")) =
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
        hold_clock(&mut rt);
        let first = paint_recording(&gpu, &mut renderer, &target, &mut rt, &mut ink);
        // Opaque interior, above the waveform, outside the antialiased rim.
        let i = 4 * (15 * w as usize + 365);
        assert!(first[i..i + 3].iter().all(|c| *c < 40) && first[i + 3] == 255);
        rt.set_system_dark(true);
        for _ in 0..16 {
            hold_clock(&mut rt);
            let frame = paint_recording(&gpu, &mut renderer, &target, &mut rt, &mut ink);
            assert!(
                frame == first,
                "host change recoloured the recording (open={open})"
            );
        }
    }
}

/// Puts the elapsed label back on the fixture's 04:18 and rebuilds, so no
/// paint measures the wall clock. The first paint of a software renderer
/// compiles its pipelines and takes over a second on Windows' WARP: holding
/// the clock after a frame instead would let the next one tick to 04:19, and
/// the comparison above would fail on the timer's glyphs rather than a
/// recolour.
fn hold_clock(rt: &mut Runtime<Capsule>) {
    rt.update(|capsule, tx| {
        if let Some(Kind::Recording(shown)) = &mut capsule.kind {
            shown.started = Instant::now() - Duration::from_secs(258);
        }
        tx.instant();
    });
    rt.frame(1.0 / 60.0);
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
            let [x, y, w, h] = laid_out(rt, "pill");
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
            if corner.is_right() {
                assert!(
                    (x - timer.0).abs() < 0.02 && (y - timer.1).abs() < 0.02,
                    "{corner:?} timer at {i}"
                );
            } else {
                // In a left corner the time rides the wall, 14 pt inside it.
                let [px, _, pw, _] = laid_out(&rt, "pill");
                let [tx, _, tw, _] = laid_out(&rt, "elapsed");
                assert!(
                    (px + pw - REST_PX - (tx + tw)).abs() < 0.03
                        && x > timer.0 - 0.02
                        && (y - timer.1).abs() < 0.02,
                    "{corner:?} timer at {i}"
                );
            }
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

#[test]
fn stop_hits_its_center_edges_and_corners_but_not_its_neighbors() {
    use MeetingIndicatorCorner::*;
    for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
        let (mut rt, commands, _) = runtime(live_recording("hit"), false, true, RECORDING);
        rt.update(move |c, tx| {
            c.corner = corner;
            tx.instant();
        });
        rt.frame(0.);
        open_recording(&mut rt);
        let ([x, y, w, h], _) = rt.presented("stop").unwrap();
        assert!(w >= 44. && h >= 44., "{corner:?}: {w}×{h}");
        let ([_, cy, _, ch], _) = rt.presented("copy-prompt").unwrap();
        assert!(y + h <= cy || y >= cy + ch);
        let click = |rt: &mut Runtime<Capsule>, px, py| {
            rt.pointer_down(px, py);
            rt.pointer_up(px, py);
            rt.frame(1. / 60.);
        };
        for (px, py) in [
            (x - 0.5, y + h / 2.),
            (x + w + 0.5, y + h / 2.),
            (x + w / 2., y - 0.5),
            (x + w / 2., y + h + 0.5),
        ] {
            click(&mut rt, px, py);
            assert!(commands.try_recv().is_err());
        }
        for px in [x + 0.25, x + w / 2., x + w - 0.25] {
            for py in [y + 0.25, y + h / 2., y + h - 0.25] {
                click(&mut rt, px, py);
                assert!(
                    matches!(commands.try_recv(), Ok(ControllerCommand::Stop)),
                    "{corner:?} {px},{py} in {x},{y} {w}×{h}"
                );
                click(&mut rt, px, py);
                assert!(commands.try_recv().is_err());
                assert!(rt.find("elapsed").is_some());
                rt.update(|c, tx| {
                    c.stopping = false;
                    c.hovered = true;
                    tx.instant();
                });
                rt.frame(0.);
            }
        }
        rt.pointer_leave();
        rt.frame(0.);
        landed(&mut rt);
        click(&mut rt, x + w / 2., y + h / 2.);
        assert!(rt.find("stop").is_none() && commands.try_recv().is_err());
    }
}

#[test]
fn stop_is_one_named_action_for_keyboard_and_accessibility() {
    use rune::accesskit::Action;
    for keyboard in [true, false] {
        let (mut rt, commands, _) = runtime(live_recording("keys"), false, true, RECORDING);
        assert!(!rt.accessibility_action("stop", Action::Click, None));
        open_recording(&mut rt);
        if keyboard {
            assert!(rt.accessibility_action("stop", Action::Focus, None));
            rt.frame(0.);
            let space = rune::KeyEvent {
                key: rune::Key::Text(" ".into()),
                mods: Default::default(),
            };
            rt.key(space.clone());
            rt.key_up(space);
        } else {
            assert!(rt.accessibility_action("stop", Action::Click, None));
        }
        rt.frame(0.);
        assert!(matches!(commands.try_recv(), Ok(ControllerCommand::Stop)));
        assert!(!rt.accessibility_action("stop", Action::Click, None));
        assert!(commands.try_recv().is_err());
    }
}

// Every enabled row is a button under the pointer, the band drags with the
// grab hand, and a row with nothing to do keeps the arrow.
#[test]
fn localized_action_rows_share_their_geometry_and_pointer_in_every_corner() {
    use crate::desktop::i18n;
    use MeetingIndicatorCorner::*;
    for &(code, _) in i18n::LANGUAGE_CHOICES.iter().filter(|(c, _)| c.is_some()) {
        i18n::apply(code); // thread-local in tests, never a settings write
        for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
            let (mut rt, _, _) = runtime(live_recording("rows"), false, true, RECORDING);
            rt.update(move |c, tx| {
                c.corner = corner;
                c.live.entries = call();
                tx.instant()
            });
            rt.frame(0.);
            let ([_, _, w, h], _) = rt.presented("pill").unwrap();
            assert_eq!((w, h), (REST_WIDTH, REST_HEIGHT));
            assert!(rt.find("stop").is_none());
            open_recording(&mut rt);
            for (key, name) in [
                ("stop", "Stop"),
                ("copy-prompt", "Copy agent prompt"),
                ("copy-all", "Copy all"),
            ] {
                let nodes = rt.elements();
                let (idx, row) = nodes
                    .iter()
                    .enumerate()
                    .find(|(_, e)| e.key.as_deref() == Some(key))
                    .unwrap();
                let [x, y, w, h] = row.rect;
                assert_eq!(
                    (x, w, h),
                    (CAPSULE_INSET + PANEL_PAD, 242., 44.),
                    "{code:?} {corner:?} {key}"
                );
                let child = |key| {
                    nodes
                        .iter()
                        .find(|e| e.parent == Some(idx) && e.key.as_deref() == Some(key))
                        .unwrap()
                };
                let [gx, gy, gw, gh] = child("glyph").rect;
                assert_eq!((gx - x, gw, gh), (9., 16., 16.));
                assert_eq!(gy + gh / 2., y + h / 2.);
                let label = child("label");
                assert_eq!(label.text.as_deref(), Some(tr(name)));
                assert_eq!(label.rect[0] - x, 33.);
                for px in [gx + gw / 2., label.rect[0] + 1., x + w - 1.] {
                    rt.pointer_move(px, y + h / 2.);
                    rt.frame(1. / 60.);
                    assert_eq!(
                        rt.cursor(),
                        Cursor::Pointer,
                        "{code:?} {corner:?} {key} {px}"
                    );
                    assert!(
                        rt.elements()
                            .iter()
                            .any(|e| e.key.as_deref() == Some(key) && e.hover > 0.)
                    );
                }
            }
        }
    }
    i18n::apply(Some("en"));
}

#[test]
fn the_meter_band_grabs_and_a_row_with_nothing_to_do_keeps_the_arrow() {
    let (mut rt, _, _) = runtime(live_recording("cursors"), false, true, RECORDING);
    rt.frame(0.);
    // At rest the band is the whole pill, and dragging it moves the capsule.
    let (x, y) = rt.find("pill").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1. / 60.);
    assert_eq!(rt.cursor(), Cursor::Grab);
    open_recording(&mut rt);
    let [x, y, w, h] = laid_out(&rt, "meta");
    rt.pointer_move(x + w / 2., y + h / 2.);
    rt.frame(1. / 60.);
    assert_eq!(rt.cursor(), Cursor::Grab);
    // Copy all has no transcript yet, so it keeps the arrow.
    let (x, y) = rt.find("copy-all").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1. / 60.);
    assert_eq!(rt.cursor(), Cursor::Default, "copy-all");
    // Copy agent prompt always works, and so does Stop.
    for key in ["copy-prompt", "stop"] {
        let (x, y) = rt.find(key).unwrap();
        rt.pointer_move(x, y);
        rt.frame(1. / 60.);
        assert_eq!(rt.cursor(), Cursor::Pointer, "{key}");
    }
    // The finished meeting's ✕ is a button too, in the band that drags.
    rt.update(|c, _| {
        let Kind::Ready(m) = ready_transcript() else {
            unreachable!()
        };
        c.published(m);
    });
    landed(&mut rt);
    let (x, y) = rt.find("dismiss").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1. / 60.);
    assert_eq!(rt.cursor(), Cursor::Pointer, "dismiss");
}

#[test]
fn row_swipes_do_not_move_the_panel_or_fire_actions_but_small_clicks_work() {
    let (mut rt, commands, _) = runtime(live_recording("ready-snap"), false, true, RECORDING);
    let (send, shifts) = mpsc::channel();
    rt.update(move |c, _| {
        c.live.entries = call();
        c.shift = Some(Arc::new(move |_, dx, dy| send.send((dx, dy)).unwrap()));
    });
    rt.frame(0.);
    open_recording(&mut rt);
    for phase in 0..4 {
        rt.update(|c, tx| {
            match phase {
                1 => c.capture_stopped(),
                2 => {
                    let Kind::Ready(m) = ready_transcript() else {
                        unreachable!()
                    };
                    c.published(m)
                }
                3 => {
                    let Kind::Ready(m) = ready_summarized() else {
                        unreachable!()
                    };
                    c.refresh_ready(m)
                }
                _ => (),
            };
            tx.instant()
        });
        rt.frame(0.);
        for key in ["stop", "copy-prompt", "copy-all"] {
            let Some(([x, y, w, h], _)) = rt.presented(key) else {
                continue;
            };
            for px in [x + 16., x + 35., x + w - 20.] {
                let py = y + h / 2.;
                rt.pointer_move(px, py);
                rt.frame(0.);
                // An enabled row is a button; a disabled one is not a target
                // and falls through to the panel's arrow.
                let focusable = rt
                    .elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some(key) && e.focusable);
                assert_eq!(
                    rt.cursor(),
                    if focusable {
                        Cursor::Pointer
                    } else {
                        Cursor::Default
                    },
                    "{key} {phase}"
                );
                rt.pointer_down(px, py);
                rt.pointer_move(px + 10., py);
                rt.frame(0.);
                rt.pointer_up(px + 1., py);
                rt.frame(0.);
                assert!(
                    shifts.try_recv().is_err() && commands.try_recv().is_err(),
                    "{key} {phase}"
                );
                assert!(
                    rt.state.live.copied.is_none(),
                    "{key} {phase}"
                );
            }
        }
        if phase == 0 {
            for key in ["copy-prompt", "stop"] {
                let (x, y) = rt.find(key).unwrap();
                rt.pointer_down(x, y);
                rt.pointer_move(x + 2., y + 1.);
                rt.frame(0.);
                rt.pointer_up(x + 2., y + 1.);
                rt.frame(0.);
                if key == "stop" {
                    assert!(matches!(commands.try_recv(), Ok(ControllerCommand::Stop)));
                    assert!(commands.try_recv().is_err());
                } else {
                    assert!(rt.state.live.copied(LiveCopy::Prompt));
                    rt.state.live.copied = None;
                }
                assert!(shifts.try_recv().is_err());
            }
        }
    }
    tap(&mut rt, "dismiss");
    assert!(rt.state.kind.is_none() && commands.try_recv().is_err());
}

#[test]
fn tab_focus_tracks_the_whole_action_row_without_changing_its_bounds() {
    let (mut rt, commands, _) = runtime(live_recording("tab-rows"), false, true, RECORDING);
    rt.update(|c, _| c.live.entries = call());
    rt.frame(0.);
    open_recording(&mut rt);
    for key in ["stop", "copy-prompt", "copy-all"] {
        rt.key(rune::KeyEvent {
            key: rune::Key::Tab,
            mods: Default::default(),
        });
        rt.frame(0.);
        let focused = rt
            .elements()
            .into_iter()
            .find(|e| e.focus_visible && e.focused)
            .unwrap();
        assert_eq!(focused.key.as_deref(), Some(key));
        assert_eq!(focused.rect[2..], [242., 44.]);
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn capture_end_and_publication_retarget_one_anchored_surface() {
    use MeetingIndicatorCorner::*;
    for (delay, reduced) in [(0, false), (90, false), (0, true), (90, true)] {
        for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
            let (mut rt, _, _) = runtime(live_recording("ready-snap"), false, true, RECORDING);
            rt.reduced_motion = reduced;
            rt.update(move |c, tx| {
                c.corner = corner;
                tx.instant();
            });
            rt.frame(0.);
            let anchor = |rt: &Runtime<Capsule>| {
                let ([x, y, w, h], opacity) = rt.presented("pill").unwrap();
                assert!(w >= REST_WIDTH && h >= REST_HEIGHT && opacity > 0.99);
                (
                    if corner.is_right() { x + w } else { x },
                    if corner.is_top() { y } else { y + h },
                )
            };
            let (x, y) = anchor(&rt);
            let (px, py) = rt.find("elapsed").unwrap();
            rt.pointer_move(px, py);
            for frame in 0..90 + delay {
                if frame == 5 {
                    rt.update(|c, _| c.mark_stopping());
                }
                if frame == 7 {
                    rt.pointer_leave();
                }
                if frame == 12 {
                    rt.update(|c, _| c.capture_stopped());
                }
                if frame == 12 + delay {
                    rt.update(|c, _| {
                        let Kind::Ready(m) = ready_transcript() else {
                            unreachable!()
                        };
                        c.published(m);
                    });
                }
                if frame == 25 + delay {
                    rt.update(|c, _| {
                        let Kind::Ready(m) = ready_summarized() else {
                            unreachable!()
                        };
                        c.refresh_ready(m);
                    });
                }
                rt.frame(1. / 60.);
                let (ax, ay) = anchor(&rt);
                assert!(
                    (x - ax).abs() < 0.02 && (y - ay).abs() < 0.02,
                    "{corner:?} {frame}: {ax},{ay} vs {x},{y}"
                );
                assert_eq!(rt.find("meeting-listening-field").is_some(), frame < 12);
                assert!(rt.find("elapsed").is_some());
                assert_eq!(rt.find("dismiss").is_some(), frame >= 12 + delay);
            }
        }
    }
}

/// Measure what Rune presented, not just Taffy's destination. In particular,
/// a transient but perfectly anchored 18 pt dip used to pass the anchor test.
#[test]
fn notes_grow_past_the_copies_without_a_height_dip_or_a_clock_hop() {
    use MeetingIndicatorCorner::*;
    for corner in [TopRight, BottomLeft, BottomRight, TopLeft] {
        for (delay, reduced, long) in [
            (1, false, false),
            (60, false, false),
            (2, false, true),
            (1, true, true),
        ] {
            let (mut rt, _, _) = runtime(live_recording("ready-snap"), false, true, RECORDING);
            rt.reduced_motion = reduced;
            rt.update(move |c, tx| {
                c.corner = corner;
                tx.instant();
            });
            rt.frame(0.);
            open_recording(&mut rt);
            let rect = |rt: &Runtime<Capsule>, k| rt.presented(k).unwrap().0;
            let clock = rect(&rt, "elapsed");
            let rows = ["copy-prompt", "copy-all"];
            let at = rows.map(|k| rect(&rt, k));
            let mut height = rect(&rt, "pill")[3];
            let mut title_width = None;
            rt.update(|c, _| c.mark_stopping());
            for frame in 0..delay + 100 {
                if frame == 1 {
                    rt.update(|c, _| c.capture_stopped());
                }
                if frame == 1 + delay {
                    rt.update(|c,_| {
                        let Kind::Ready(mut m) = ready_transcript() else { unreachable!() };
                        m.duration_ms = Some(258_000);
                        if long { m.title = "Planification trimestrielle — lancement et préparation des équipes".into(); }
                        c.published(m);
                    });
                }
                if frame == 4 + delay {
                    rt.update(|c,_| {
                        let Kind::Ready(mut m) = ready_summarized() else { unreachable!() };
                        m.duration_ms = Some(258_000);
                        if long { m.notes.as_mut().unwrap().title = "Planification trimestrielle — lancement et préparation des équipes".into(); }
                        c.refresh_ready(m);
                    });
                }
                rt.frame(1. / 60.);
                let now = rect(&rt, "pill")[3];
                assert!(
                    now >= height - 0.02,
                    "{corner:?} f{frame}: {height} → {now}"
                );
                height = now;
                let t = rect(&rt, "elapsed");
                assert!(
                    (t[1] - clock[1]).abs() < 0.02 && t[2..] == clock[2..],
                    "{corner:?} f{frame} timer {t:?} / {clock:?}"
                );
                assert!((t[0] - clock[0]).abs() < 0.02, "timer moved at {frame}");
                if frame > delay {
                    let w = rect(&rt, "title")[2];
                    assert!(
                        (w - *title_width.get_or_insert(w)).abs() < 0.02,
                        "title wrapped at a moving width"
                    );
                }
                for (key, original) in rows.into_iter().zip(at) {
                    let row = rect(&rt, key);
                    assert_eq!(row[2..], original[2..]);
                    if !long {
                        assert!(
                            (row[1] - original[1]).abs() < 0.02,
                            "{corner:?} f{frame} {key}: {row:?} / {original:?}"
                        );
                    }
                }
            }
            let header = rect(&rt, "header")[3];
            assert!(if long {
                header > OPEN_HEADER
            } else {
                (header - OPEN_HEADER).abs() < 0.02
            });
        }
    }
}

#[test]
fn a_long_recording_keeps_its_timer_and_stop_inside_each_corner() {
    use MeetingIndicatorCorner::*;
    for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
        for (sec, warning, width) in [
            (258, false, REST_WIDTH),
            (123 * 60 + 45, false, REST_WIDTH + 8.),
            (123 * 60 + 45, true, PANEL_WIDTH),
        ] {
            let (mut rt, received, _) = runtime(live_recording("fixture"), false, true, RECORDING);
            rt.update(move |c, tx| {
                c.corner = corner;
                if let Some(Kind::Recording(shown)) = &mut c.kind {
                    shown.started = Instant::now() - Duration::from_secs(sec);
                    shown.warning = warning.then_some(meeting::TrackWarning::DigitalSilence);
                }
                tx.instant();
            });
            landed(&mut rt);
            let [px, _, pw, ph] = laid_out(&rt, "pill");
            let [sx, _, sw, _] = laid_out(&rt, "meeting-listening-field");
            let [tx, _, tw, _] = laid_out(&rt, "elapsed");
            assert_eq!((pw, ph), (width, REST_HEIGHT));
            assert!(sx - px >= 9. && px + pw - tx - tw >= 9. && tx - (sx + sw) >= 5.);
            if warning {
                let [x, _, w, _] = laid_out(&rt, "notice");
                assert!(w > 0. && (x + w <= sx || x >= tx + tw));
            }
            let timer = rt.find("elapsed").unwrap();
            rt.pointer_move(timer.0, timer.1);
            for _ in 0..48 {
                rt.frame(1. / 60.);
            }
            if corner.is_right() {
                assert_eq!(rt.find("elapsed"), Some(timer));
            } else {
                // In a left corner the time ends at the open panel's inset;
                // a sixth digit is 7.8 pt wide and counted as 8.
                let [px, _, pw, _] = laid_out(&rt, "pill");
                let [tx, _, tw, _] = laid_out(&rt, "elapsed");
                let inset = px + pw - (tx + tw);
                assert!((inset - REST_PX).abs() < 0.25, "{corner:?}: {inset}");
            }
            let [_, y, w, h] = laid_out(&rt, "stop");
            let [_, my, _, mh] = laid_out(&rt, "meta");
            assert_eq!((w, h), (PANEL_WIDTH - 2. * PANEL_PAD, ROW_HEIGHT));
            assert!(y + h <= my || y >= my + mh);
            tap(&mut rt, "stop");
            assert!(matches!(received.try_recv(), Ok(ControllerCommand::Stop)));
        }
    }
}
