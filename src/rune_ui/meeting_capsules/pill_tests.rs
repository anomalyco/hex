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
    landed(&mut rt);
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
        // A copy's ✓ Copied is the rows' one trailing word.
        rt.update(|c, tx| {
            c.live.copied = Some((LiveCopy::Prompt, Instant::now()));
            tx.instant();
        });
        landed(&mut rt);
        let [px, _, pw, _] = laid_out(&rt, "pill");
        assert_eq!(pw, PANEL_WIDTH);
        let [tx, _, tw, _] = laid_out(&rt, "elapsed");
        // On the right the time stays by the wall; on the left it rides the
        // wall across with the field.
        if corner.is_right() {
            assert_eq!((tx, tw), rest, "{corner:?}: the time moved as the pill opened");
        }
        let open = (tx, tw);
        let [gx, _, _, _] = laid_out(&rt, "glyph");
        let [hx, _, hw, _] = laid_out(&rt, "hint");
        // The rows' icons and their trailing words are that far in too.
        assert_eq!((gx - px, px + pw - (hx + hw)), (inset, inset), "{corner:?} open");
        assert_eq!(tx + tw, hx + hw, "{corner:?}: the time and the hint end apart");
        let [fx, _, _, _] = laid_out(&rt, "meeting-listening-field");
        assert_eq!(fx, gx, "{corner:?}: the field and the icons start apart");

        rt.update(|c, tx| {
            c.capture_stopped();
            let Kind::Ready(m) = ready_summarized() else {
                unreachable!()
            };
            c.published(m);
            tx.instant();
        });
        landed(&mut rt);
        let [px, _, pw, _] = laid_out(&rt, "pill");
        let [lx, _, lw, _] = laid_out(&rt, "title");
        assert_eq!((lx - px, px + pw - (lx + lw)), (inset, inset), "{corner:?} finished");
        let [tx, _, tw, _] = laid_out(&rt, "elapsed");
        assert_eq!((tx, tw), open, "{corner:?}: the time moved at Stop");
        assert_eq!(tx + tw, lx + lw, "{corner:?}: the length and the title end apart");
    }
}

/// The 2 pt gap sat only under the first copy. Stop's hover fill met the
/// first copy's fill with none, so their corners pinched, while Copy all's
/// kept its gap.
#[test]
fn rows_that_can_both_be_filled_are_one_gap_apart() {
    use MeetingIndicatorCorner::*;
    for corner in [TopLeft, TopRight, BottomLeft, BottomRight] {
        let mut rt = recording_in(corner);
        open_recording(&mut rt);
        let [_, y, _, h] = laid_out(&rt, "pill");
        assert_eq!(h, panel::open_height_without_notice());
        let [_, sy, _, sh] = laid_out(&rt, "stop");
        let [_, py, _, ph] = laid_out(&rt, "copy-prompt");
        let [_, ay, _, ah] = laid_out(&rt, "copy-all");
        assert_eq!([sh, ph, ah], [ROW_HEIGHT; 3]);
        // Copy agent prompt is always filled; Stop and Copy all fill under
        // the pointer.
        assert_eq!(ay - (py + ph), ROW_GAP, "{corner:?}: the prompt to Copy all");
        if corner.is_top() {
            assert_eq!(py - (sy + sh), ROW_GAP, "{corner:?}: Stop to the prompt");
            assert_eq!(y + h - (ay + ah), PANEL_PAD);
        } else {
            // A bottom corner keeps the copies' order and puts Stop under them.
            assert_eq!(sy - (ay + ah), ROW_GAP, "{corner:?}: the copies to Stop");
            assert_eq!(py - y, PANEL_PAD);
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
        let shots = replayed(name);
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
        let shots = replayed(name);
        (shots[rest].columns.0, shots[open].columns.0)
    };
    assert_eq!(columns("whole", 171, 250), (31, 69));
    assert_eq!(columns("hundred", 19, 85), (31, 66));
}

/// In a left corner the field kept its resting 78 pt in the open header and
/// left 101 pt empty beside the time.
#[test]
fn a_left_corners_field_fills_the_header_and_the_time_rides_the_wall() {
    let pitch = crate::rune_hud::dot_scope::Tuning::default().spacing;
    let wide = PANEL_WIDTH - 2. * REST_PX - METER_GAP - TIMER_WIDTH;
    for name in ["left", "left-stop", "left-notice"] {
        let shots = replayed(name);
        let (mut widest, mut flights, mut was) = (0f32, 0, None::<f32>);
        for (frame, shot) in shots.iter().enumerate() {
            let (Some([px, _, pw, _]), Some([tx, _, tw, _])) = (shot.pill, shot.elapsed) else {
                continue;
            };
            // It stays in the corner the pill rests in.
            assert!((px - CAPSULE_INSET).abs() < 0.03, "{name} f{frame}: {px}");
            let wall = px + pw;
            // The time never passes the inset from a wall on its way, and
            // never moves further in a frame than the wall can.
            assert!(tx + tw < wall - REST_PX + 0.03, "{name} f{frame}: {tx} + {tw}, wall {wall}");
            if let Some(tx_was) = was {
                assert!((tx - tx_was).abs() < 24., "{name} f{frame}: {tx_was} → {tx}");
            }
            was = Some(tx);
            // While the wall is between its rests the time is its motion,
            // whether the pointer opened it or capture ended.
            if pw > REST_WIDTH + 0.5 && pw < PANEL_WIDTH - 0.5 {
                flights += 1;
                assert!((wall - REST_PX - (tx + tw)).abs() < 0.03, "{name} f{frame}");
            }
            let Some([x, _, w, _]) = shot.field_at else {
                continue;
            };
            widest = widest.max(w);
            // The field starts at the rows' inset and ends beside the time.
            assert!((x - px - REST_PX).abs() < 0.02, "{name} f{frame}: {} from the wall", x - px);
            assert!((x + w + METER_GAP - tx).abs() < 0.02, "{name} f{frame}: {x} + {w}, time {tx}");
            let (columns, shift, _) = shot.columns;
            assert_eq!(columns, (w / pitch + 1e-3).floor() as usize + 1, "{name} f{frame}: {w}");
            assert!(shift <= 0. && shift > -pitch / 2. - 1e-3, "{name} f{frame}: {shift}");
            // The dots keep their places from the wall's side.
            assert!((shot.first_dot().unwrap() - x).abs() < 0.02, "{name} f{frame}");
            if let Some((lane, words, shown)) = shot.notice {
                // What shows of a notice starts where the time ends, and its
                // words stay where they were, ending at the inset.
                assert!((lane[0] - (tx + tw)).abs() < 0.02, "{name} f{frame}: {lane:?}");
                assert!((words[0] + words[2] - (wall - REST_PX)).abs() < 0.02, "{name} f{frame}");
                assert!(w < 129. || shown < 0.01, "f{frame}: {shown} beside a field {w} wide");
            }
        }
        assert!((widest - wide).abs() < 0.01, "{name}: {widest}");
        assert!(name == "left-notice" || flights > 12, "{name}: {flights}");
    }
    // Stop pressed in the open panel: the time keeps its place to the end.
    let shots = replayed("left-stop");
    let held = shots[79].elapsed.unwrap()[0];
    assert!(shots[79..].iter().all(|s| (s.elapsed.unwrap()[0] - held).abs() < 0.03));
    let shots = replayed("left-notice");
    assert!(shots[19].notice.is_some_and(|n| n.2 == 1.), "a notice at rest");
    assert!(shots[60].notice.is_none(), "the notice under an open field");
    assert!(shots.last().unwrap().notice.is_some_and(|n| n.2 == 1.));
}

/// The field restarted when its columns changed: every dot went dark and
/// the trace unfolded again from the middle.
#[test]
fn the_field_keeps_its_light_through_the_opening() {
    let shots = replayed("whole");
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

/// With a notice the resting pill is already the panel's width. The field
/// widened across the notice while it was still fading, dots over words,
/// and coming back the words were cut again at every width on the way.
#[test]
fn the_field_wipes_the_headers_notice_and_never_draws_over_it() {
    let shots = replayed("notice");
    let rest = shots[19].notice.expect("a notice at rest");
    assert_eq!(rest.2, 1.);
    let (mut wiped, mut back) = (0, 0);
    for (frame, shot) in shots.iter().enumerate().skip(19) {
        let [x, _, w, _] = shot.field_at.unwrap();
        let Some((lane, words, shown)) = shot.notice else {
            // Gone only once the field is across the header.
            assert!(w > 178.9, "f{frame}: no notice beside a field {w} wide");
            continue;
        };
        // What shows of it ends where the field begins.
        assert!((lane[0] + lane[2] - x).abs() < 0.02, "f{frame}: {lane:?} and a field at {x}");
        // Its words stay where they were, cut where they were.
        assert_eq!((words[0], words[2]), (rest.1[0], rest.1[2]), "f{frame}");
        // It has gone by the time the field is halfway across.
        assert!(w < 129. || shown < 0.01, "f{frame}: {shown} beside a field {w} wide");
        wiped += usize::from(frame < 90 && shown > 0.05 && shown < 0.95);
        back += usize::from(frame >= 90 && shown > 0.05 && shown < 0.95);
    }
    assert!(wiped > 2 && back > 2, "{wiped} frames going, {back} coming back");
    let last = shots.last().unwrap();
    assert_eq!(last.notice.map(|n| (n.0, n.2)), Some((rest.0, 1.)));
}

/// "Stopping…" dissolved on the label line while "Transcribing…" arrived on
/// the icon line, 24 pt to its left on one baseline: together they read as
/// one word slipping left.
#[test]
fn stops_row_has_gone_before_the_heading_arrives_and_neither_moves() {
    for name in ["whole", "interrupted", "close", "titles"] {
        let shots = replayed(name);
        let (mut label, mut heading, mut seen) = (None, None, 0);
        for (frame, shot) in shots.iter().enumerate() {
            if !matches!(shot.phase, "stopping" | "transcribing" | "published" | "summary") {
                continue;
            }
            let stop = match &shot.label {
                Some((rect, _)) => {
                    let x = *label.get_or_insert(rect[0]);
                    assert!((rect[0] - x).abs() < 0.02, "{name} f{frame}: the label moved");
                    shot.shown.2
                }
                None => 0.,
            };
            if let Some(rect) = shot.title_at {
                let x = *heading.get_or_insert(rect[0]);
                assert!((rect[0] - x).abs() < 0.02, "{name} f{frame}: the heading moved");
            }
            // Whichever is arriving waits until the other no longer reads.
            assert!(
                stop.min(shot.title_shown) < 0.13,
                "{name} f{frame}: Stop at {stop} under a heading at {}",
                shot.title_shown
            );
            seen += usize::from(shot.title_shown > 0.9);
        }
        // Two lines 24 pt apart, and no word ever crosses between them.
        let (label, heading) = (label.unwrap(), heading.unwrap());
        assert!((label - heading - 24.).abs() < 0.02, "{name}: {label} and {heading}");
        // "close" dismisses the meeting a few frames after its title lands.
        assert!(seen > if name == "close" { 2 } else { 20 }, "{name}: {seen}");
    }
}

#[test]
fn a_discarded_recording_dismisses_the_pill_and_shows_no_finished_panel() {
    // The pill's model forgets the recording, so its reconcile hides it.
    let mut model = PillModel::default();
    let now = Instant::now();
    model.apply(
        &ControllerEvent::RecordingStarted {
            id: "discarded".into(),
            title: "Design sync".into(),
            automatic: false,
        },
        now,
    );
    assert!(model.recording.is_some());
    model.apply(&ControllerEvent::Transcribing, now);
    assert!(model.recording.is_none());
    model.apply(&ControllerEvent::Discarded("discarded".into()), now);
    assert!(model.pill(true).is_none());

    // The finalizing pill leaves on that dismissal (what clear_finishing does)
    // and never becomes the finished screen.
    let (mut rt, _, _) = runtime(live_recording("discarded"), false, true, RECORDING);
    rt.update(|capsule, tx| {
        capsule.capture_stopped();
        assert!(matches!(capsule.state, State::Finalizing { .. }));
        capsule.dismiss(tx);
        assert!(capsule.state.is_empty());
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("transcript-block").is_none());
    assert!(rt.find("summary").is_none());
}
