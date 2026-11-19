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
