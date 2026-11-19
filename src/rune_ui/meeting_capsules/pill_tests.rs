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
