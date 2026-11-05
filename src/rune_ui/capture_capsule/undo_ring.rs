//! The Undo chip's countdown: the chip's own outline drains clockwise from
//! the top and is gone the moment the Undo window ends.
//!
//! The ring is [`ARCS`] short arcs, each bound to a render-time [`Tween`]
//! of its opacity: an arc fades out as the hand passes it, so the drain is
//! smooth with no producer ticking and no view rebuilds. Under Reduce
//! Motion the arcs go a whole second at a time instead. While the pointer
//! holds the countdown the arcs stand still, and a new ring takes over
//! from what was left once it leaves.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use rune::{Color, El, LineCap, Shape, Tween, canvas, el};

use super::super::controls::Paint;
use super::flow::{Pres, undo_s};
use super::surface::{CHIP_RADIUS, HINT_GAP, medium, undo_label};

/// Arcs in a ring: enough that each one's fade reads as a sweep.
const ARCS: usize = 48;
/// Each arc reaches this far over its neighbours, hiding the seams.
const OVERLAP: f32 = 0.2;
const BORDER_STROKE: f32 = 1.5;

/// The live ring for one Undo window.
pub(crate) struct Ring {
    /// When the window ends, on the flow's clock.
    end: f64,
    /// Held by the pointer, with this long left.
    held: Option<f64>,
    arcs: Vec<Tween<f32>>,
}

impl Ring {
    fn new(end: f64, held: Option<f64>, now: f64, reduced: bool) -> Self {
        let window = undo_s();
        let left = held.unwrap_or(end - now).clamp(0.0, window);
        let past = window - left;
        let arcs = (0..ARCS)
            .map(|k| {
                // When arc `k` is gone, and how fast it fades: over its own
                // share of the window, or at once on a whole second.
                let gone = (k + 1) as f64 / ARCS as f64 * window;
                let (gone, rate) = if reduced {
                    (gone.ceil().min(window), 1e3)
                } else {
                    (gone, ARCS as f64 / window)
                };
                let from = ((gone - past) * rate) as f32;
                if held.is_some() {
                    Tween::linear(from, from, Duration::from_secs(1))
                } else {
                    Tween::linear(
                        from,
                        ((gone - window) * rate) as f32,
                        Duration::from_secs_f64(left),
                    )
                }
            })
            .collect();
        Self { end, held, arcs }
    }
}

/// Keeps `ring` on the window `pres` counts down: built when one begins,
/// kept while it runs, dropped when Undo has none.
pub(crate) fn sync(ring: &mut Option<Ring>, pres: &Pres, now: f64, reduced: bool) {
    let (end, held) = match pres {
        Pres::Done(done) if done.undo => (done.countdown, done.held),
        _ => (None, None),
    };
    match end {
        None => *ring = None,
        Some(end)
            if ring
                .as_ref()
                .is_some_and(|ring| (ring.end, ring.held) == (end, held)) => {}
        Some(end) => *ring = Some(Ring::new(end, held, now, reduced)),
    }
}

/// The Undo chip `size` big, with its countdown when there is one.
pub(crate) fn chip(p: Paint, base: El, size: [f32; 2], ring: Option<&Ring>) -> El {
    let k = p.k;
    let ink = p.muted();
    let track = k.alpha(k.c.text_base, 0.08);
    let mut chip = base
        .radius(CHIP_RADIUS)
        .gap(HINT_GAP)
        .hover(k.c.hover)
        .press(k.c.pressed);
    if let Some(ring) = ring {
        chip = chip.child(border(size, ring, ink, track));
    }
    chip.child(super::label(undo_label(), medium(), ink))
        .role("button")
        .label(undo_label())
        .hit()
}

/// The chip's own outline, from the top centre clockwise.
fn border([w, h]: [f32; 2], ring: &Ring, ink: Color, track: Color) -> El {
    let inset = BORDER_STROKE * 0.5;
    let r = (CHIP_RADIUS - inset).min(h * 0.5 - inset).max(0.01);
    let at = |f: f32| rounded_at(w, h, r, inset, f);
    arcs(
        el().absolute(0.0, 0.0).size(w, h),
        [w, h],
        ring,
        &at,
        BORDER_STROKE,
        ink,
        track,
    )
}

/// The faint track and the ring's arcs along `at` (0…1 around), stacked
/// in `frame`.
fn arcs(
    frame: El,
    [w, h]: [f32; 2],
    ring: &Ring,
    at: &dyn Fn(f32) -> [f32; 2],
    stroke: f32,
    ink: Color,
    track: Color,
) -> El {
    const STEPS: usize = 4;
    let span = |from: f32, to: f32, steps: usize| -> Vec<[f32; 2]> {
        (0..=steps)
            .map(|i| at(from + (to - from) * i as f32 / steps as f32))
            .collect()
    };
    let layer = |shape: Shape| canvas(shape.view_box(w, h)).absolute(0.0, 0.0).size(w, h);
    let whole = Shape::new().stroke_with(
        span(0.0, 1.0, ARCS * STEPS),
        true,
        stroke,
        LineCap::Butt,
        track,
    );
    let mut frame = frame.child(layer(whole));
    for (k, arc) in ring.arcs.iter().enumerate() {
        let n = ARCS as f32;
        let from = ((k as f32 - OVERLAP) / n).max(0.0);
        let to = ((k as f32 + 1.0 + OVERLAP) / n).min(1.0);
        frame = frame.child(
            layer(Shape::new().stroke_with(
                span(from, to, STEPS),
                false,
                stroke,
                LineCap::Butt,
                ink,
            ))
            .live_opacity(arc),
        );
    }
    frame
}

/// The point `f` (0…1) of the way round a `w` × `h` rounded rectangle,
/// `inset` in from its edge with corners `r`, from the top centre
/// clockwise.
fn rounded_at(w: f32, h: f32, r: f32, inset: f32, f: f32) -> [f32; 2] {
    let (x0, y0, x1, y1) = (inset, inset, w - inset, h - inset);
    let across = (x1 - x0 - 2.0 * r).max(0.0);
    let down = (y1 - y0 - 2.0 * r).max(0.0);
    let corner = FRAC_PI_2 * r;
    let half = across * 0.5;
    let mut d = f.clamp(0.0, 1.0) * (2.0 * across + 2.0 * down + 4.0 * corner);
    let turn = |cx: f32, cy: f32, start: f32, d: f32| {
        let a = start + d / r;
        [cx + r * a.cos(), cy + r * a.sin()]
    };
    if d <= half {
        return [x0 + r + half + d, y0];
    }
    d -= half;
    if d <= corner {
        return turn(x1 - r, y0 + r, -FRAC_PI_2, d);
    }
    d -= corner;
    if d <= down {
        return [x1, y0 + r + d];
    }
    d -= down;
    if d <= corner {
        return turn(x1 - r, y1 - r, 0.0, d);
    }
    d -= corner;
    if d <= across {
        return [x1 - r - d, y1];
    }
    d -= across;
    if d <= corner {
        return turn(x0 + r, y1 - r, FRAC_PI_2, d);
    }
    d -= corner;
    if d <= down {
        return [x0, y1 - r - d];
    }
    d -= down;
    if d <= corner {
        return turn(x0 + r, y0 + r, PI, d);
    }
    d -= corner;
    [x0 + r + d.min(half), y0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_starts_at_the_top_centre_and_goes_clockwise() {
        let (w, h, r) = (60.0, 28.0, 10.0);
        let close =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01;
        assert!(close(rounded_at(w, h, r, 0.0, 0.0), [30.0, 0.0]));
        assert!(close(rounded_at(w, h, r, 0.0, 1.0), [30.0, 0.0]));
        // A quarter of the way round is on the right-hand side.
        assert!(rounded_at(w, h, r, 0.0, 0.25)[0] > 50.0);
        // Halfway is the bottom centre.
        assert!(close(rounded_at(w, h, r, 0.0, 0.5), [30.0, 28.0]));
    }

    #[test]
    fn a_ring_follows_its_window_and_rebuilds_only_for_a_new_one() {
        use super::super::flow::{Done, Lead};
        let counting = |end: Option<f64>| {
            Pres::Done(Done {
                undo: true,
                countdown: end,
                ..Done::plain(Lead::Check, "Sent to HOME")
            })
        };
        let mut ring = None;
        sync(&mut ring, &counting(Some(5.0)), 0.0, false);
        assert_eq!(ring.as_ref().map(|r| r.arcs.len()), Some(ARCS));
        // The same window a second on keeps the ring it had.
        sync(&mut ring, &counting(Some(5.0)), 1.0, false);
        assert_eq!(ring.as_ref().map(|r| r.end), Some(5.0));
        sync(&mut ring, &counting(Some(9.0)), 4.0, false);
        assert_eq!(ring.as_ref().map(|r| r.end), Some(9.0));
        sync(&mut ring, &counting(None), 6.0, false);
        assert!(ring.is_none());
    }

    #[test]
    fn a_held_ring_stands_still_where_it_was() {
        use super::super::flow::{Done, Lead};
        let held = Pres::Done(Done {
            undo: true,
            countdown: Some(5.0),
            held: Some(3.5),
            ..Done::plain(Lead::Check, "Sending to SEER")
        });
        let mut ring = None;
        sync(&mut ring, &held, 1.5, false);
        assert_eq!(ring.as_ref().and_then(|r| r.held), Some(3.5));
        // Let go, it drains again from a new ring on the moved window.
        let going = Pres::Done(Done {
            undo: true,
            countdown: Some(7.5),
            ..Done::plain(Lead::Check, "Sending to SEER")
        });
        sync(&mut ring, &going, 4.0, false);
        let ring = ring.expect("a ring again");
        assert_eq!((ring.end, ring.held), (7.5, None));
    }
}
