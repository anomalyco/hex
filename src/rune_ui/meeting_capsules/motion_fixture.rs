// A silent, deterministic replay through the overlay's actual view and pointer handler.
// Only the two level readings, the elapsed clock and the pointer are synthetic.
fn fixture_levels(frame: usize) -> [f32; 2] {
    let t = frame as f32 / 60.0;
    let phrase = |speed: f32, beat: f32| ((t * speed).sin().abs() * (t * beat).cos().abs()).sqrt();
    [
        if (18..108).contains(&frame) || (216..300).contains(&frame) {
            phrase(13.0, 2.8)
        } else {
            0.0
        },
        if (120..300).contains(&frame) {
            phrase(6.3, 4.1)
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
        let mut history = History::default();
        let mut trace = String::from("frame,hovered,key,x,y,w,h\n");
        for frame in 0..366 {
            // Put the fixture on the same second at each event, independent of GPU speed.
            if let Some(Kind::Recording(shown)) = &mut rt.state.kind {
                shown.started = Instant::now() - Duration::from_secs(258 + frame as u64 / 60);
            }
            if frame % 60 == 0 {
                rt.update(|_, _| {});
            }
            if frame % 3 == 0 {
                history.push(fixture_levels(frame));
                rt.state.scope.show(&history);
            }
            match frame {
                66 | 83 => rt.pointer_move(RECORDING.0 - 22.0, 22.0),
                72 | 249 => rt.pointer_leave(),
                _ => (),
            }
            rt.frame(1.0 / 60.0);
            for elem in rt.elements() {
                if let Some(key @ ("pill" | "elapsed" | "copy-new")) = elem.key.as_deref() {
                    let [x, y, w, h] = elem.rect;
                    writeln!(trace, "{frame},{},{key},{x},{y},{w},{h}", rt.state.hovered).unwrap();
                }
            }
            if dark || matches!(frame, 54 | 180) {
                let rgba = offscreen::paint(&gpu, &mut renderer, &target, &mut rt);
                rune::render::write_png(&dir.join(format!("{frame:03}.png")), w, h, &rgba).unwrap();
            }
        }
        std::fs::write(dir.join("trace.csv"), trace).unwrap();
    }
}

fn fixture_scope(capsule: &mut Capsule) {
    let mut history = History::default();
    for frame in (216..258).step_by(3) {
        history.push(fixture_levels(frame));
    }
    capsule.scope.show(&history);
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
