//! The rail in a Rune runtime: a list far deeper than the card scrolls under
//! the wheel (Shift-turned too, as a held ⌥⇧ turns it) and with the keys,
//! builds only the rows it shows, and a click on any row it scrolled to
//! selects that row's own item.
use super::view::{self, Act, Shown};
use super::*;
use crate::history::paste_source;
use crate::paste::recent::{Entry, Row};
use rune::{Cx, Env, Mods, Runtime, WheelEvent, col, text::TextSystem};
use std::sync::Arc;

const NOW: u64 = 1_795_262_400_000;

struct Doc {
    rows: Arc<[Entry]>,
    cursor: usize,
    picked: Vec<Id>,
}

fn rows(n: u64) -> Arc<[Entry]> {
    (0..n)
        .map(|i| {
            Entry::from(Row {
                ready_at: NOW - i * 60_000,
                id: Id::Dictation(paste_source::Id::Retained(n - i)),
                cancelled: false,
                preview: format!("Note number {i}"),
            })
        })
        .collect()
}

fn view(doc: &Doc, cx: &mut Cx<Doc>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    col().w_full().h_full().center().child(view::card(
        &Shown {
            rows: &doc.rows,
            cursor: doc.cursor,
            now: NOW,
            trigger: &[],
        },
        doc,
        p,
        |_: &Doc, _: &Path, _| None,
        |doc: &mut Doc, act, _| {
            if let Act::Select(id) = act {
                doc.picked.push(id);
            }
        },
    ))
}

fn runtime(n: u64) -> Runtime<Doc> {
    let doc = Doc {
        rows: rows(n),
        cursor: 0,
        picked: Vec::new(),
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let env = Env {
        system_dark: true,
        scale: 1.0,
    };
    let mut rt = crate::rune_ui::offscreen::runtime(doc, view, (760.0, 600.0), env, text);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<Doc>) {
    for _ in 0..4 {
        rt.settle_all();
        rt.frame(1.0 / 60.0);
    }
}

/// Row `i`'s rect, while it is built.
fn rect(rt: &Runtime<Doc>, i: usize) -> Option<[f32; 4]> {
    let key = view::row_name(i);
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key.as_str()))
        .map(|e| e.rect)
}

/// Whether row `i` shows whole inside the rail.
fn shown(rt: &Runtime<Doc>, i: usize) -> bool {
    let rail = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(view::RAIL))
        .expect("the rail")
        .rect;
    rect(rt, i)
        .is_some_and(|row| row[1] >= rail[1] - 0.5 && row[1] + row[3] <= rail[1] + rail[3] + 0.5)
}

fn wheel(rt: &mut Runtime<Doc>, dx: f32, dy: f32, mods: Mods) {
    let (x, y) = rt.find(view::RAIL).expect("the rail");
    rt.pointer_move(x, y);
    rt.wheel_event(WheelEvent::new(dx, dy).at(x, y).mods(mods).precise(true));
    settle(rt);
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Default::default()
    }
}

/// A list of hundreds builds only the rows around the rail's view, and
/// scrolls under a plain wheel or trackpad.
#[test]
fn a_deep_list_builds_only_what_it_shows_and_scrolls_under_the_wheel() {
    let mut rt = runtime(300);
    assert!(shown(&rt, 0) && shown(&rt, view::ROWS - 1));
    assert!(!shown(&rt, view::ROWS));
    assert!(rect(&rt, 299).is_none(), "an unseen row is never built");
    let before = rect(&rt, 10).unwrap()[1];
    wheel(&mut rt, 0.0, -400.0, Mods::default());
    let after = rect(&rt, 10).unwrap()[1];
    assert!(!shown(&rt, 0));
    assert!(
        (before - after - 400.0).abs() < 1.0,
        "moved {}",
        before - after
    );
    assert!((10..20).any(|i| shown(&rt, i)));
}

/// Holding ⌥⇧ turns a mouse wheel sideways (macOS does, before Rune sees
/// it; Rune leaves such a delta alone, and a vertical list ignores it). The
/// rail turns it back, so it scrolls with the keys held; without Shift the
/// same sideways delta (a trackpad's) leaves it where it was. Each runtime
/// is its own wheel gesture: Rune keeps a gesture with whoever took its
/// first delta.
#[test]
fn the_rail_scrolls_under_a_wheel_shift_turned_sideways() {
    let mut rt = runtime(300);
    wheel(&mut rt, -400.0, 0.0, Mods::default());
    assert!(
        shown(&rt, 0),
        "a sideways delta alone doesn't move a vertical list"
    );
    let mut rt = runtime(300);
    let before = rect(&rt, 10).unwrap()[1];
    wheel(&mut rt, -400.0, 0.0, shift());
    assert!((before - rect(&rt, 10).unwrap()[1] - 400.0).abs() < 1.0);
    assert!(!shown(&rt, 0));
    assert!((10..20).any(|i| shown(&rt, i)));
    // Notches between two frames add up, and the way back works too.
    let (x, y) = rt.find(view::RAIL).unwrap();
    for _ in 0..3 {
        rt.wheel_event(
            WheelEvent::new(-1_000.0, 0.0)
                .at(x, y)
                .mods(shift())
                .precise(true),
        );
    }
    settle(&mut rt);
    assert!((60..80).any(|i| shown(&rt, i)));
    wheel(&mut rt, 100_000.0, 0.0, shift());
    assert!(shown(&rt, 0));
    wheel(&mut rt, -100_000.0, 0.0, shift());
    assert!(shown(&rt, 299), "the oldest row at the end");
}

/// A click on a row the rail scrolled to selects that row's own item.
#[test]
fn a_click_on_a_scrolled_row_selects_its_own_item() {
    let mut rt = runtime(300);
    wheel(&mut rt, -2_000.0, 0.0, shift());
    let i = (0..300).find(|&i| shown(&rt, i)).expect("a row in view") + 2;
    let (x, y) = rt.find(&view::row_name(i)).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.picked, [rt.state.rows[i].row.id.clone()]);
}

/// ↑ and ↓ bring the selection into view however deep it goes, and a
/// picker opens from the top.
#[test]
fn the_keys_reveal_the_selection_and_a_picker_opens_from_the_top() {
    let mut rt = runtime(300);
    for cursor in [9, 120, 299, 0] {
        rt.update(|s, tx| {
            let was = s.cursor;
            s.cursor = cursor;
            view::follow(tx, view::reveal(false, was, cursor, s.rows.len()));
        });
        settle(&mut rt);
        assert!(shown(&rt, cursor), "row {cursor} revealed");
    }
    rt.update(|s, tx| {
        s.cursor = 200;
        view::follow(tx, view::reveal(false, 0, 200, s.rows.len()));
    });
    settle(&mut rt);
    rt.update(|s, tx| {
        s.cursor = 1;
        view::follow(tx, view::reveal(true, 200, 1, s.rows.len()));
    });
    settle(&mut rt);
    assert!(shown(&rt, 0) && shown(&rt, 1));
}

#[test]
fn the_rail_follows_the_selection_only_when_it_moves() {
    use view::{Reveal, reveal};
    assert_eq!(reveal(true, 40, 0, 300), Reveal::Top);
    assert_eq!(reveal(true, 0, view::ROWS - 1, 300), Reveal::Top);
    assert_eq!(reveal(true, 0, 30, 300), Reveal::Row(30));
    // Only cancelled rows: nothing selected, the list from its top.
    assert_eq!(reveal(true, 0, 3, 3), Reveal::Top);
    assert_eq!(reveal(false, 4, 5, 300), Reveal::Row(5));
    assert_eq!(reveal(false, 5, 5, 300), Reveal::Stay);
    assert_eq!(reveal(false, 0, 3, 3), Reveal::Stay);
}
