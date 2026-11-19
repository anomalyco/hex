// The live pill's measurements: one inset on every side, one gap between
// rows that can both be filled, and a field that widens with the wall.

/// The layout rectangle of the first element keyed `key`.
fn laid_out(rt: &Runtime<Capsule>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
        .rect
}

fn recording_in(corner: MeetingIndicatorCorner) -> Runtime<Capsule> {
    let (mut rt, _, _) = runtime(live_recording("pill"), false, true, RECORDING);
    rt.update(move |c, tx| {
        c.corner = corner;
        c.live.entries = call();
        tx.instant();
    });
    settle_stage(&mut rt);
    rt.settle_all();
    rt.frame(0.);
    rt
}

/// The header kept 11 pt on its right where the rows, the title and the
/// summary keep 14, so the time ended 3 pt past "Up to date" in one column,
/// and the resting pill had 14 pt on one side and 11 on the other.
#[test]
fn the_header_keeps_the_rows_inset_at_rest_open_and_finished() {
    use MeetingIndicatorCorner::*;
    let inset = PANEL_PAD + ROW_PX;
    assert_eq!((REST_PX, REST_WIDTH), (14., 151.));
    for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
        let mut rt = recording_in(corner);
        let [px, _, pw, ph] = laid_out(&rt, "pill");
        assert_eq!((pw, ph), (REST_WIDTH, REST_HEIGHT));
        let [fx, _, _, _] = laid_out(&rt, "meeting-listening-field");
        let [tx, _, tw, _] = laid_out(&rt, "elapsed");
        // The pill's contents are as far from one wall as from the other.
        assert_eq!((fx - px, px + pw - (tx + tw)), (inset, inset), "{corner:?} at rest");
        let rest = (tx, tw);

        open_recording(&mut rt);
        let [px, _, pw, _] = laid_out(&rt, "pill");
        assert_eq!(pw, PANEL_WIDTH);
        let [tx, _, tw, _] = laid_out(&rt, "elapsed");
        assert_eq!((tx, tw), rest, "{corner:?}: the time moved as the pill opened");
        let [gx, _, _, _] = laid_out(&rt, "glyph");
        let [hx, _, hw, _] = laid_out(&rt, "hint");
        // The rows' icons and their trailing words are that far in too.
        assert_eq!((gx - px, px + pw - (hx + hw)), (inset, inset), "{corner:?} open");
        if corner.is_right() {
            assert_eq!(tx + tw, hx + hw, "{corner:?}: the time and the hint end apart");
        } else {
            let [fx, _, _, _] = laid_out(&rt, "meeting-listening-field");
            assert_eq!(fx, gx, "{corner:?}: the field and the icons start apart");
        }

        rt.update(|c, tx| {
            c.capture_stopped();
            let Kind::Ready(m) = ready_summarized() else {
                unreachable!()
            };
            c.published(m);
            tx.instant();
        });
        settle_stage(&mut rt);
        rt.settle_all();
        rt.frame(0.);
        let [px, _, pw, _] = laid_out(&rt, "pill");
        let [lx, _, lw, _] = laid_out(&rt, "title");
        assert_eq!((lx - px, px + pw - (lx + lw)), (inset, inset), "{corner:?} finished");
        let [tx, _, tw, _] = laid_out(&rt, "elapsed");
        assert_eq!((tx, tw), rest, "{corner:?}: the time moved at Stop");
        if corner.is_right() {
            assert_eq!(tx + tw, lx + lw, "{corner:?}: the length and the title end apart");
        }
    }
}

/// The 2 pt gap sat only under Copy new. Stop's hover fill met Copy new's
/// fill with none, so their corners pinched, while Copy all's kept its gap.
#[test]
fn rows_that_can_both_be_filled_are_one_gap_apart() {
    use MeetingIndicatorCorner::*;
    for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
        let mut rt = recording_in(corner);
        open_recording(&mut rt);
        let [_, y, _, h] = laid_out(&rt, "pill");
        assert_eq!(h, panel::open_height_without_notice());
        let [_, sy, _, sh] = laid_out(&rt, "stop");
        let [_, ny, _, nh] = laid_out(&rt, "copy-new");
        let [_, ay, _, ah] = laid_out(&rt, "copy-all");
        let [_, py, _, ph] = laid_out(&rt, "copy-prompt");
        assert_eq!([sh, nh, ah, ph], [ROW_HEIGHT; 4]);
        // Copy new is always filled; Stop and Copy all fill under the pointer.
        assert_eq!(ay - (ny + nh), ROW_GAP, "{corner:?}: Copy new to Copy all");
        // Copy all and Copy agent prompt are never filled together.
        assert_eq!(py - (ay + ah), 0., "{corner:?}");
        if corner.is_top() {
            assert_eq!(ny - (sy + sh), ROW_GAP, "{corner:?}: Stop to Copy new");
            assert_eq!(y + h - (py + ph), PANEL_PAD);
        } else {
            // A bottom corner keeps the copies' order and puts Stop under them.
            assert_eq!(sy - (py + ph), ROW_GAP, "{corner:?}: the copies to Stop");
            assert_eq!(ny - y, PANEL_PAD);
        }
    }
}

/// The field jumped to the header's width in the frame the pointer arrived,
/// past a wall that was still opening, and restarted from its new middle.
#[test]
fn the_field_widens_with_the_wall_one_whole_column_at_a_time() {
    let pitch = crate::rune_hud::dot_scope::Tuning::default().spacing;
    let wide = PANEL_WIDTH - 2. * REST_PX - METER_GAP - TIMER_WIDTH;
    assert_eq!((waveform::WIDTH, wide), (78., 179.));
    for name in ["whole", "interrupted", "close", "hundred", "notice", "reduced"] {
        let shots = play(replay(name), |_, _, _| {});
        let (mut widest, mut flights) = (0f32, 0);
        for (frame, pair) in shots.windows(2).enumerate() {
            let (was, shot) = (&pair[0], &pair[1]);
            let frame = frame + 1;
            let (Some([x, _, w, _]), Some([px, _, pw, _]), Some([tx, ..])) =
                (shot.field_at, shot.pill, shot.elapsed)
            else {
                continue;
            };
            if shot.surface < 1. || shot.phase == "starting" || shot.phase == "offer" {
                continue;
            }
            widest = widest.max(w);
            // It ends beside the time, which never moves.
            assert!((x + w + METER_GAP - tx).abs() < 0.02, "{name} f{frame}: {x} + {w}, time {tx}");
            // It starts no nearer the wall than the rows' icons do.
            assert!(x - px > REST_PX - 0.02, "{name} f{frame}: {} from the wall", x - px);
            assert!(
                w > waveform::WIDTH - 0.01 && w < wide + 0.01,
                "{name} f{frame}: {w}"
            );
            let (columns, shift, _) = shot.columns;
            // Whole columns at the dictation HUD's pitch, as many as fit.
            assert_eq!(columns, (w / pitch + 1e-3).floor() as usize + 1, "{name} f{frame}: {w}");
            let first = shot.first_dot().unwrap();
            let last = first + (columns - 1) as f32 * pitch;
            assert!(shift >= 0. && shift < pitch / 2. + 1e-3, "{name} f{frame}: {shift}");
            // The dots keep their places from the time's side, and none is
            // drawn outside the field's place, let alone the pill.
            assert!((last - (x + w)).abs() < 0.02, "{name} f{frame}: last dot at {last}");
            assert!(first > x - 0.02 && first > px + REST_PX - 0.02, "{name} f{frame}: {first}");
            // While the wall is between its rests the field's free end is
            // the wall's motion, frame for frame.
            let flying = name != "notice"
                && name != "reduced"
                && shot.phase == "recording"
                && was.pill.is_some_and(|r| (r[2] - pw).abs() > 0.05)
                && pw > REST_WIDTH + 8.5
                && pw < PANEL_WIDTH - 0.5;
            if flying {
                flights += 1;
                // A sixth digit is 7.8 pt wide and counted as 8.
                let slack = if name == "hundred" { 0.25 } else { 0.03 };
                assert!(
                    (x - px - REST_PX).abs() < slack,
                    "{name} f{frame}: {} from the wall",
                    x - px
                );
            }
        }
        match name {
            "hundred" => assert!((widest - (wide - 8.)).abs() < 0.01, "{widest}"),
            _ => assert!((widest - wide).abs() < 0.01, "{name}: {widest}"),
        }
        assert!(name == "notice" || name == "reduced" || flights > 12, "{name}: {flights}");
    }
    // Another digit on the time costs the open field three columns.
    let columns = |name, rest: usize, open: usize| {
        let shots = play(replay(name), |_, _, _| {});
        (shots[rest].columns.0, shots[open].columns.0)
    };
    assert_eq!(columns("whole", 171, 250), (31, 69));
    assert_eq!(columns("hundred", 19, 85), (31, 66));
}

/// The field restarted when its columns changed: every dot went dark and
/// the trace unfolded again from the middle.
#[test]
fn the_field_keeps_its_light_through_the_opening() {
    let shots = play(replay("whole"), |_, _, _| {});
    // The pointer arrives at 172 over two people talking.
    let lit = |shot: &Shot| shot.columns.2.iter().sum::<f32>();
    let resting = |shot: &Shot| shot.columns.2.iter().rev().take(31).sum::<f32>();
    let before = resting(&shots[171]);
    assert!(before > 3., "the fixture is talking: {before}");
    let mut arrived = 0;
    for frame in 172..232 {
        let (was, shot) = (&shots[frame - 1], &shots[frame]);
        // The columns it had keep their light: no frame loses more than a
        // pause would take from them.
        assert!(
            resting(shot) > 0.55 * resting(was),
            "f{frame}: the resting columns fell {} → {}",
            resting(was),
            resting(shot)
        );
        // A column is lit from the voice's history the frame it arrives.
        if shot.columns.0 > was.columns.0 + 1 {
            arrived += 1;
            let new = shot.columns.0 - was.columns.0;
            let fresh: f32 = shot.columns.2[..new + 4].iter().sum();
            assert!(fresh > 0., "f{frame}: {new} columns arrived dark");
        }
        assert!(lit(shot) > 0.5 * lit(was), "f{frame}");
    }
    assert!(arrived > 6, "{arrived}");
    assert_eq!(shots[231].columns.0, 69);
}
